//! `coffer mcp --client <name>` as an MCP client starts it: the real binary,
//! JSON-RPC lines over its stdin and stdout, against a scratch data dir
//! (`COFFER_DATA_DIR`). Checks that stdout carries MCP messages only, and
//! that without a database the server fails on stderr and creates nothing,
//! and that a call over stdio is logged.
//!
//! The tools themselves are tested in-process in `crates/mcp/tests`.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use coffer_core::access_log::{self, Outcome};
use coffer_core::db::Database;
use coffer_core::paths::{DATA_DIR_ENV, DATABASE_FILE};
use serde_json::{Value, json};

fn coffer() -> Command {
    Command::new(env!("CARGO_BIN_EXE_coffer"))
}

#[test]
fn without_a_database_it_fails_on_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let output = coffer()
        .args(["mcp", "--client", "claude-code"])
        .env(DATA_DIR_ENV, dir.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "stdout is for MCP messages only");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("there is no Coffer database"), "{stderr}");
    assert!(!dir.path().join(DATABASE_FILE).exists());
}

#[test]
fn a_bad_client_name_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let output = coffer()
        .args(["mcp", "--client", "coffer"])
        .env(DATA_DIR_ENV, dir.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn serves_tools_over_stdio() {
    let dir = tempfile::tempdir().unwrap();
    drop(Database::open(&dir.path().join(DATABASE_FILE)).unwrap());

    let mut child = coffer()
        .args(["mcp", "--client", "claude-code"])
        .env(DATA_DIR_ENV, dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut send = |message: Value| {
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    };
    let mut receive = || -> Value {
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|error| panic!("{error}: {line:?}"))
    };

    send(json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "0" },
        },
    }));
    let initialized = receive();
    assert_eq!(initialized["id"], 1, "{initialized}");
    assert_eq!(initialized["result"]["serverInfo"]["name"], "coffer");

    send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    send(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }));
    let listed = receive();
    assert_eq!(listed["id"], 2, "{listed}");
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"list_categories"), "{names:?}");

    send(json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": { "name": "list_categories", "arguments": { "purpose": "stdio test" } },
    }));
    let called = receive();
    assert_eq!(called["id"], 3, "{called}");
    assert_ne!(called["result"]["isError"], true, "{called}");
    assert!(
        called["result"]["structuredContent"]["groups"]
            .as_array()
            .is_some_and(|groups| !groups.is_empty())
    );

    // Closing stdin ends the session.
    drop(send);
    drop(stdin);
    let status = child.wait().unwrap();
    assert!(status.success(), "{status}");
    let mut rest = String::new();
    std::io::Read::read_to_string(&mut stdout, &mut rest).unwrap();
    assert!(rest.trim().is_empty(), "unexpected stdout: {rest:?}");

    let db = Database::open_existing(&dir.path().join(DATABASE_FILE)).unwrap();
    let log = access_log::recent(&db.reader().unwrap(), 10).unwrap();
    assert_eq!(log.len(), 1, "{log:#?}");
    assert_eq!(
        (log[0].client.as_str(), log[0].tool.as_str(), log[0].outcome),
        ("claude-code", "list_categories", Outcome::Ok)
    );
    assert_eq!(log[0].purpose.as_deref(), Some("stdio test"));
}
