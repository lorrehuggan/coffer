//! Calls refused because the database couldn't be opened are logged once it
//! can be (decision record "M2: MCP server", F.8): they wait in the access
//! log's pending file, with their own times, and the next call that opens
//! the database moves them in, whichever server makes it. The restore lock
//! cases are in `restore.rs`; the file's own guarantees (crash between
//! insert and truncate, the cap) are tested in `coffer_core`.

mod support;

use std::fs::OpenOptions;
use std::sync::Barrier;

use coffer_core::access_log::{Outcome, pending};
use coffer_core::backup;
use coffer_core::db::{self, Database};
use coffer_core::mcp_client::ClientName;
use coffer_mcp::Server;
use serde_json::json;
use support::*;

/// A call refused because migrations were pending is in the log after the
/// database has been migrated.
#[tokio::test]
async fn a_call_refused_with_migrations_pending_is_logged_after_migrating() {
    let fixture = Fixture::new();
    let client = fixture.connect("codex", AccessLevel::Aggregate).await;
    client.call_ok("list_categories", json!({})).await;

    // As an older Coffer left it: the last migration not applied. V15 adds
    // `account.preset_id`, so undoing it drops the column with its history
    // row.
    let conn = rusqlite::Connection::open(fixture.path()).unwrap();
    assert_eq!(
        Database::supported_schema_version(),
        15,
        "undo the latest migration here"
    );
    conn.execute("ALTER TABLE account DROP COLUMN preset_id", [])
        .unwrap();
    conn.execute(
        "DELETE FROM refinery_schema_history WHERE version = ?1",
        [Database::supported_schema_version()],
    )
    .unwrap();
    drop(conn);
    assert!(matches!(
        Database::open_existing(fixture.path()),
        Err(db::Error::SchemaTooOld { .. })
    ));

    let refused = client
        .call(
            "monthly_summary",
            json!({ "from": "2026-10", "to": "2026-11" }),
        )
        .await;
    assert_eq!(refused.is_error, Some(true), "{refused:?}");
    let text = serde_json::to_string(&refused.content).unwrap();
    assert!(text.contains("couldn't open its database"), "{text}");

    // Any `coffer` command migrates it.
    Database::open(fixture.path()).unwrap();
    client.call_ok("list_categories", json!({})).await;

    let log = fixture.access_log();
    let tools: Vec<(&str, Outcome)> = log
        .iter()
        .map(|call| (call.tool.as_str(), call.outcome))
        .collect();
    assert_eq!(
        tools,
        [
            ("list_categories", Outcome::Ok),
            ("monthly_summary", Outcome::RefusedUnavailable),
            ("list_categories", Outcome::Ok),
        ]
    );
    let refused = &log[1];
    assert_eq!(refused.client, "codex");
    assert_eq!(
        refused.args_redacted_json,
        r#"{"from":"2026-10","to":"2026-11"}"#
    );
    // Timed when it was refused, between the calls around it.
    assert!(
        log[0].at <= refused.at && refused.at <= log[2].at,
        "{log:#?}"
    );
    client.close().await;
}

/// Two servers (one per client, as two `coffer mcp` processes would be)
/// refused at the same time, on two threads, lose nothing; the first call
/// that can open the database moves every refused call in, its own and the
/// other's.
#[test]
fn two_servers_refused_at_once_lose_nothing() {
    const CALLS: usize = 20;
    let fixture = Fixture::new();
    let server = |client: &str| {
        fixture.set_client(client, AccessLevel::Aggregate);
        Server::new(fixture.open(), ClientName::new(client).unwrap()).unwrap()
    };
    let codex = server("codex");
    let claude = server("claude-code");
    let logged = fixture.access_log().len();

    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(backup::restore_lock_path(fixture.path()))
        .unwrap();
    lock.lock().unwrap();
    let start = Barrier::new(2);
    std::thread::scope(|scope| {
        for server in [&codex, &claude] {
            let start = &start;
            scope.spawn(move || {
                start.wait();
                for _ in 0..CALLS {
                    let result = server.dispatch("list_categories", None).unwrap();
                    assert_eq!(result.is_error, Some(true), "{result:?}");
                }
            });
        }
    });
    drop(lock);
    assert_eq!(
        std::fs::read_to_string(pending::path(fixture.path()))
            .unwrap()
            .lines()
            .count(),
        2 * CALLS
    );

    let ok = codex.dispatch("list_categories", None).unwrap();
    assert_ne!(ok.is_error, Some(true), "{ok:?}");
    let log = &fixture.access_log()[logged..];
    assert_eq!(log.len(), 2 * CALLS + 1);
    for client in ["codex", "claude-code"] {
        let refused = log
            .iter()
            .filter(|call| call.client == client && call.outcome == Outcome::RefusedRestoring)
            .count();
        assert_eq!(refused, CALLS, "{client}");
    }
    // Moved once: the other server's next call finds nothing waiting.
    claude.dispatch("list_categories", None).unwrap();
    assert_eq!(fixture.access_log().len(), logged + 2 * CALLS + 2);
}
