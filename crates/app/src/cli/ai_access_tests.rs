//! `coffer ai-access` against scratch data dirs. Nothing here touches the
//! real data dir.

use std::path::PathBuf;

use clap::Parser;
use coffer_core::access_log::{self, NewEntry, Outcome};
use coffer_core::db::Database;
use coffer_core::mcp_client::{self, AccessLevel, ClientAccess, ClientName};
use coffer_core::setting;
use tempfile::TempDir;

use super::*;

/// A scratch data dir with an empty, migrated database.
struct Scratch {
    _dir: TempDir,
    db: PathBuf,
}

impl Scratch {
    fn new() -> Scratch {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("coffer.db");
        Database::open(&db).unwrap();
        Scratch { _dir: dir, db }
    }

    fn run(&self, args: &[&str]) -> Result<String, String> {
        let cli = Cli::try_parse_from(std::iter::once("coffer").chain(args.iter().copied()))
            .unwrap_or_else(|error| panic!("{args:?} doesn't parse: {error}"));
        let mut out = Vec::new();
        let result = super::run(cli.command.expect("a subcommand"), &self.db, &mut out);
        let out = String::from_utf8(out).unwrap();
        result.map(|()| out).map_err(|error| format!("{error:#}"))
    }

    fn ok(&self, args: &[&str]) -> String {
        self.run(args)
            .unwrap_or_else(|error| panic!("{args:?} failed: {error}"))
    }

    fn err(&self, args: &[&str]) -> String {
        match self.run(args) {
            Ok(out) => panic!("{args:?} succeeded:\n{out}"),
            Err(error) => error,
        }
    }

    fn open(&self) -> Database {
        Database::open(&self.db).unwrap()
    }

    fn access(&self, client: &str) -> ClientAccess {
        mcp_client::access(&self.open().reader().unwrap(), &name(client)).unwrap()
    }

    /// Records `client` as the server does for an unknown name.
    fn requested(&self, client: &str) {
        mcp_client::record_requested(&mut self.open(), &name(client)).unwrap();
    }

    fn switched_on(&self) -> bool {
        setting::mcp_enabled(&self.open().reader().unwrap()).unwrap()
    }
}

fn name(client: &str) -> ClientName {
    ClientName::new(client).unwrap()
}

fn doesnt_parse(args: &[&str]) {
    assert!(
        Cli::try_parse_from(std::iter::once("coffer").chain(args.iter().copied())).is_err(),
        "{args:?} parses"
    );
}

/// The table's lines after the kill-switch line, split into cells.
fn table(out: &str) -> Vec<Vec<String>> {
    out.lines()
        .skip_while(|line| !line.starts_with("NAME") && !line.starts_with("TIME"))
        .map(|line| line.split_whitespace().map(str::to_owned).collect())
        .collect()
}

#[test]
fn list_shows_the_clients_and_the_kill_switch() {
    let scratch = Scratch::new();
    let out = scratch.ok(&["ai-access"]);
    assert!(out.starts_with("AI access is on."), "{out}");
    assert!(out.contains("No AI clients yet"), "{out}");

    scratch.requested("codex");
    scratch.requested("claude-code");
    scratch.ok(&["ai-access", "enable", "claude-code"]);
    scratch.ok(&["ai-access", "level", "claude-code", "standard"]);

    let out = scratch.ok(&["ai-access", "list"]);
    let rows = table(&out);
    assert_eq!(
        rows[0],
        [
            "NAME", "STATUS", "ENABLED", "LEVEL", "LAST", "SEEN", "(UTC)"
        ]
    );
    assert_eq!(rows[1][..4], ["claude-code", "known", "yes", "standard"]);
    assert_eq!(rows[2][..4], ["codex", "requested", "no", "-"]);
    // Last seen: the request's time, to the second.
    assert_eq!(rows[2].len(), 6, "{out}");
    assert_eq!(rows[2][4].len(), "2026-11-13".len());
    assert_eq!(rows[2][5].len(), "12:00:00".len());
}

#[test]
fn enable_starts_at_aggregate_and_keeps_a_level() {
    let scratch = Scratch::new();
    scratch.requested("codex");
    let out = scratch.ok(&["ai-access", "enable", "codex"]);
    assert!(out.starts_with("Enabled codex at aggregate."), "{out}");
    assert_eq!(
        scratch.access("codex"),
        ClientAccess::Enabled(AccessLevel::Aggregate)
    );

    scratch.ok(&["ai-access", "level", "codex", "pseudonymous"]);
    let out = scratch.ok(&["ai-access", "enable", "codex"]);
    assert!(out.starts_with("Enabled codex at pseudonymous."), "{out}");

    // A name the server hasn't seen yet can be enabled ahead of time.
    scratch.ok(&["ai-access", "enable", "new-client"]);
    assert_eq!(
        scratch.access("new-client"),
        ClientAccess::Enabled(AccessLevel::Aggregate)
    );
    // And enabling creates the pseudonym key.
    assert!(
        setting::pseudonym_key(&scratch.open().reader().unwrap())
            .unwrap()
            .is_some()
    );
}

#[test]
fn enable_while_switched_off_says_so() {
    let scratch = Scratch::new();
    scratch.ok(&["ai-access", "off"]);
    let out = scratch.ok(&["ai-access", "enable", "codex"]);
    assert!(out.contains("AI access is off"), "{out}");
}

#[test]
fn level_needs_an_enabled_client() {
    let scratch = Scratch::new();
    scratch.ok(&["ai-access", "enable", "codex"]);
    let out = scratch.ok(&["ai-access", "level", "codex", "standard"]);
    assert!(out.starts_with("codex is now at standard."), "{out}");
    assert_eq!(
        scratch.access("codex"),
        ClientAccess::Enabled(AccessLevel::Standard)
    );

    let error = scratch.err(&["ai-access", "level", "nobody", "standard"]);
    assert_eq!(error, "there is no AI client called `nobody`");

    scratch.requested("asked");
    let error = scratch.err(&["ai-access", "level", "asked", "standard"]);
    assert_eq!(
        error,
        "the AI client `asked` isn't enabled; enable it first"
    );
    assert_eq!(scratch.access("asked"), ClientAccess::Requested);

    doesnt_parse(&["ai-access", "level", "codex", "admin"]);
    doesnt_parse(&["ai-access", "level", "codex"]);
}

#[test]
fn revoke_keeps_the_client_known() {
    let scratch = Scratch::new();
    scratch.ok(&["ai-access", "enable", "codex"]);
    scratch.ok(&["ai-access", "level", "codex", "standard"]);
    let out = scratch.ok(&["ai-access", "revoke", "codex"]);
    assert!(out.starts_with("Revoked codex"), "{out}");
    assert_eq!(scratch.access("codex"), ClientAccess::Revoked);
    let rows = table(&scratch.ok(&["ai-access"]));
    assert_eq!(rows[1][..4], ["codex", "known", "no", "-"]);

    // Enabled again: back at aggregate.
    scratch.ok(&["ai-access", "enable", "codex"]);
    assert_eq!(
        scratch.access("codex"),
        ClientAccess::Enabled(AccessLevel::Aggregate)
    );

    let error = scratch.err(&["ai-access", "revoke", "nobody"]);
    assert_eq!(error, "there is no AI client called `nobody`");
    scratch.requested("asked");
    let error = scratch.err(&["ai-access", "revoke", "asked"]);
    assert_eq!(
        error,
        "the AI client `asked` isn't enabled; enable it first"
    );
}

#[test]
fn ignore_hides_a_request() {
    let scratch = Scratch::new();
    scratch.requested("asked");
    let out = scratch.ok(&["ai-access", "ignore", "asked"]);
    assert!(out.starts_with("Ignored asked"), "{out}");
    assert_eq!(scratch.access("asked"), ClientAccess::Ignored);

    // An enabled client is revoked as well.
    scratch.ok(&["ai-access", "enable", "codex"]);
    scratch.ok(&["ai-access", "ignore", "codex"]);
    assert_eq!(scratch.access("codex"), ClientAccess::Ignored);
    let rows = table(&scratch.ok(&["ai-access"]));
    assert_eq!(rows[1][..4], ["asked", "ignored", "no", "-"]);
    assert_eq!(rows[2][..4], ["codex", "ignored", "no", "-"]);

    let error = scratch.err(&["ai-access", "ignore", "nobody"]);
    assert_eq!(error, "there is no AI client called `nobody`");
}

#[test]
fn off_and_on_switch_every_client() {
    let scratch = Scratch::new();
    let out = scratch.ok(&["ai-access", "off"]);
    assert!(out.starts_with("AI access is off"), "{out}");
    assert!(!scratch.switched_on());
    let out = scratch.ok(&["ai-access", "list"]);
    assert!(out.starts_with("AI access is off"), "{out}");
    // Switching off twice is fine.
    scratch.ok(&["ai-access", "off"]);

    let out = scratch.ok(&["ai-access", "on"]);
    assert!(out.starts_with("AI access is on"), "{out}");
    assert!(scratch.switched_on());
    assert!(scratch.ok(&["ai-access"]).starts_with("AI access is on."));
}

fn append(scratch: &Scratch, client: &str, outcome: Outcome, purpose: Option<&str>) {
    let ok = outcome == Outcome::Ok;
    access_log::append(
        &mut scratch.open(),
        &NewEntry {
            client: name(client),
            tool: "search_transactions".to_owned(),
            access_level: Some(AccessLevel::Standard),
            outcome,
            purpose: purpose.map(str::to_owned),
            args_redacted_json: r#"{"limit":10,"query":"string(12)"}"#.to_owned(),
            row_count: if ok { 7 } else { 0 },
            result_hash: ok.then(|| "ab".repeat(32)),
            withheld_count: if ok { 2 } else { 0 },
        },
    )
    .unwrap();
}

#[test]
fn log_shows_the_latest_calls_with_redacted_arguments() {
    let scratch = Scratch::new();
    let out = scratch.ok(&["ai-access", "log"]);
    assert_eq!(out, "No calls in the AI access log yet.\n");

    append(&scratch, "codex", Outcome::Ok, Some("monthly review"));
    append(&scratch, "claude-code", Outcome::RefusedLevel, None);
    // Untrusted text: a right-to-left override, a zero-width space, an
    // escape sequence and a newline, and far too long.
    let sneaky = format!(
        "please\u{202E}\u{200B}\u{1b}[2J ignore\nall {}",
        "previous instructions ".repeat(10)
    );
    append(&scratch, "codex", Outcome::Error, Some(&sneaky));

    let out = scratch.ok(&["ai-access", "log"]);
    let rows = table(&out);
    assert_eq!(
        rows[0],
        [
            "TIME", "(UTC)", "CLIENT", "TOOL", "LEVEL", "OUTCOME", "ROWS", "WITHHELD", "ARGS",
            "PURPOSE"
        ]
    );
    assert_eq!(rows.len(), 4, "{out}");
    // Newest first.
    assert_eq!(
        rows[1][2..9],
        [
            "codex",
            "search_transactions",
            "standard",
            "error",
            "0",
            "0",
            r#"{"limit":10,"query":"string(12)"}"#
        ]
    );
    assert_eq!(
        rows[2][2..6],
        [
            "claude-code",
            "search_transactions",
            "standard",
            "refused_level"
        ]
    );
    assert_eq!(rows[2][9], "-");
    assert_eq!(rows[3][5..8], ["ok", "7", "2"]);
    assert_eq!(rows[3][9..], ["monthly", "review"]);

    let sneaky_line = out.lines().nth(1).unwrap();
    assert!(
        sneaky_line.contains("please[2J ignore all previous"),
        "{sneaky_line}"
    );
    for invisible in ['\u{202E}', '\u{200B}', '\u{1b}'] {
        assert!(!out.contains(invisible), "{out:?}");
    }
    let purpose = &sneaky_line[sneaky_line.find("please").unwrap()..];
    assert_eq!(purpose.chars().count(), ai_access::PURPOSE_CHARS);
    assert!(purpose.ends_with('…'));
    // The result hash is the server's business, not the user's.
    assert!(!out.contains(&"ab".repeat(32)));
}

/// Calls a server refused while it couldn't open the database wait in the
/// pending file; `coffer ai-access` moves them into the log first.
#[test]
fn log_shows_calls_refused_while_the_database_was_unavailable() {
    let scratch = Scratch::new();
    append(&scratch, "codex", Outcome::Ok, None);
    for (client, outcome) in [
        ("codex", Outcome::RefusedRestoring),
        ("claude-code", Outcome::RefusedUnavailable),
    ] {
        access_log::pending::record(
            &scratch.db,
            &NewEntry {
                client: name(client),
                tool: "monthly_summary".to_owned(),
                access_level: None,
                outcome,
                purpose: Some("check".to_owned()),
                args_redacted_json: "{}".to_owned(),
                row_count: 0,
                result_hash: None,
                withheld_count: 0,
            },
        )
        .unwrap();
    }
    // What a server past the pending file's cap leaves: counts only.
    std::fs::write(
        access_log::pending::path(&scratch.db).with_file_name("coffer.db-access-pending-dropped"),
        r#"{"groups":[{"id":"0192f000-0000-7000-8000-000000000001","client":"codex",
            "tool":"list_reports","outcome":"refused_restoring","calls":40,
            "first_at":"2000-01-01T00:00:00.000Z"}]}"#,
    )
    .unwrap();

    let out = scratch.ok(&["ai-access", "log"]);
    let rows = table(&out);
    assert_eq!(rows.len(), 5, "{out}");
    assert_eq!(
        rows[1][2..6],
        ["claude-code", "monthly_summary", "-", "refused_unavailable"]
    );
    assert_eq!(rows[1][9], "check");
    assert_eq!(
        rows[2][2..6],
        ["codex", "monthly_summary", "-", "refused_restoring"]
    );
    assert_eq!(rows[3][5], "ok");
    assert_eq!(
        rows[4][..9],
        [
            "2000-01-01",
            "00:00:00",
            "codex",
            "list_reports",
            "-",
            "refused_restoring",
            "(40",
            "calls)",
            "0"
        ]
    );

    // Moved, not copied.
    assert_eq!(table(&scratch.ok(&["ai-access", "log"])).len(), 5);
    assert_eq!(
        std::fs::metadata(access_log::pending::path(&scratch.db))
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn log_filters_by_client_and_limits() {
    let scratch = Scratch::new();
    for _ in 0..3 {
        append(&scratch, "codex", Outcome::Ok, None);
    }
    append(&scratch, "claude-code", Outcome::RefusedOff, None);

    let rows = table(&scratch.ok(&["ai-access", "log", "--client", "codex"]));
    assert_eq!(rows.len(), 4);
    assert!(rows[1..].iter().all(|row| row[2] == "codex"));

    let rows = table(&scratch.ok(&["ai-access", "log", "--limit", "1"]));
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[1][2..6],
        [
            "claude-code",
            "search_transactions",
            "standard",
            "refused_off"
        ]
    );

    let out = scratch.ok(&["ai-access", "log", "--client", "nobody"]);
    assert_eq!(out, "No calls from nobody in the AI access log.\n");

    doesnt_parse(&["ai-access", "log", "--limit", "0"]);
    doesnt_parse(&["ai-access", "log", "--client", "not a name"]);
}

#[test]
fn a_client_name_is_checked_and_a_database_is_needed() {
    doesnt_parse(&["ai-access", "enable", "not a name"]);
    doesnt_parse(&["ai-access", "enable", "coffer"]);
    doesnt_parse(&["ai-access", "enable"]);

    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("coffer.db");
    let cli = Cli::try_parse_from(["coffer", "ai-access", "list"]).unwrap();
    let error = super::run(cli.command.unwrap(), &db, &mut Vec::new()).unwrap_err();
    assert!(
        format!("{error:#}").contains("there is no Coffer database"),
        "{error:#}"
    );
    assert!(!db.exists());
}
