//! A restore while an MCP server runs (decision record "Settings, AI
//! access and backups", point 6, and "M2: MCP server", F.1): the server
//! keeps no connection open between calls, so it never blocks `coffer
//! restore`; it refuses calls while a restore holds the restore lock; a
//! lock file nobody holds (a stopped restore's) doesn't block it; and the
//! next call reads the restored database. The calls it refuses wait in the
//! access log's pending file and go into the log afterwards, at their own
//! times (F.8).

mod support;

use std::fs::{File, OpenOptions};
use std::path::Path;
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use coffer_core::access_log::{LoggedCall, Outcome, pending};
use coffer_core::backup::{self, Reason};
use serde_json::json;
use support::reports::*;
use support::*;

const CLIENT: &str = "claude-code";

fn report_titles(result: &serde_json::Value) -> Vec<String> {
    result["reports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|report| report["title"].as_str().unwrap().to_owned())
        .collect()
}

/// Holds the restore lock as `coffer restore` in another process would,
/// until dropped.
fn hold_restore_lock(db_path: &Path) -> File {
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(backup::restore_lock_path(db_path))
        .unwrap();
    file.lock().unwrap();
    file
}

/// Now, as the access log writes times.
fn now() -> String {
    DateTime::<Utc>::from(SystemTime::now())
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

/// The rows with `outcome`, as (tool, time).
fn with_outcome(log: &[LoggedCall], outcome: Outcome) -> Vec<(String, String)> {
    log.iter()
        .filter(|call| call.outcome == outcome)
        .map(|call| (call.tool.clone(), call.at.clone()))
        .collect()
}

#[tokio::test]
async fn a_restore_succeeds_with_a_server_connected_and_the_next_call_reads_it() {
    let fixture = Fixture::new();
    let client = fixture.connect(CLIENT, AccessLevel::Aggregate).await;
    add_report(&fixture, CLIENT, "Kept", "Before the backup.", None);
    let backup = backup::create(&fixture.open(), Reason::Manual)
        .unwrap()
        .path;
    add_report(&fixture, CLIENT, "Rolled back", "After the backup.", None);

    let before = client.call_ok("list_reports", json!({})).await;
    assert_eq!(report_titles(&before), ["Rolled back", "Kept"]);

    // The connected server holds no connection, so the restore isn't
    // refused as in use.
    backup::restore(fixture.path(), &backup).unwrap();

    let after = client.call_ok("list_reports", json!({})).await;
    assert_eq!(report_titles(&after), ["Kept"]);
    // Logged in the restored database.
    let log = fixture.access_log();
    assert_eq!(log.last().unwrap().tool, "list_reports");
    client.close().await;
}

#[tokio::test]
async fn calls_are_refused_while_the_restore_lock_is_held() {
    let fixture = Fixture::new();
    let client = fixture.connect(CLIENT, AccessLevel::Standard).await;
    client.call_ok("list_categories", json!({})).await;
    let logged = fixture.access_log().len();

    let held = hold_restore_lock(fixture.path());
    let before = now();
    for tool in ["list_categories", "save_report"] {
        let result = client
            .call(
                tool,
                json!({ "period": "2026-11", "title": "t", "body_markdown": "b" }),
            )
            .await;
        assert_eq!(result.is_error, Some(true), "{tool}: {result:?}");
        let text = serde_json::to_string(&result.content).unwrap();
        assert!(text.contains("restoring a backup"), "{text}");
    }
    let refused_by = now();
    // Released, as a finished restore does; the file stays.
    drop(held);
    assert!(backup::restore_lock_path(fixture.path()).exists());

    // Nothing was read or written in the database while it was held: no
    // log rows, no report. The refusals wait outside it.
    assert_eq!(fixture.access_log().len(), logged);
    assert!(
        std::fs::metadata(pending::path(fixture.path()))
            .unwrap()
            .len()
            > 0
    );
    let after = client.call_ok("list_reports", json!({})).await;
    assert_eq!(report_titles(&after), Vec::<String>::new());

    // The next call moved them into the log first, at their own times.
    let log = fixture.access_log();
    assert_eq!(log.len(), logged + 3);
    let refused = with_outcome(&log, Outcome::RefusedRestoring);
    assert_eq!(refused.len(), 2, "{log:#?}");
    for ((tool, at), expected) in refused.iter().zip(["list_categories", "save_report"]) {
        assert_eq!(tool, expected);
        assert!(*at >= before && *at <= refused_by, "{at}");
    }
    let save = log.iter().find(|call| call.tool == "save_report").unwrap();
    assert_eq!(save.client, CLIENT);
    assert_eq!(save.access_level, None);
    // The same redaction as any row: free text only as its length.
    assert_eq!(
        save.args_redacted_json,
        r#"{"body_markdown":{"length":1,"type":"string"},"period":"2026-11","title":{"length":1,"type":"string"}}"#
    );
    assert_eq!(log.last().unwrap().tool, "list_reports");
    assert_eq!(
        std::fs::metadata(pending::path(fixture.path()))
            .unwrap()
            .len(),
        0
    );
    client.close().await;
}

/// A call refused while a restore holds the lock is in the restored
/// database's log once the restore is done: `restore` moves it in.
#[tokio::test]
async fn a_call_refused_during_a_restore_is_logged_in_the_restored_database() {
    let fixture = Fixture::new();
    let client = fixture.connect(CLIENT, AccessLevel::Aggregate).await;
    let backup = backup::create(&fixture.open(), Reason::Manual)
        .unwrap()
        .path;

    let held = hold_restore_lock(fixture.path());
    let before = now();
    let refused = client
        .call(
            "monthly_summary",
            json!({ "from": "2026-10", "to": "2026-11", "purpose": "review" }),
        )
        .await;
    assert_eq!(refused.is_error, Some(true), "{refused:?}");
    let refused_by = now();
    drop(held);
    backup::restore(fixture.path(), &backup).unwrap();

    // Read straight from the file, before any server call could flush.
    let log = fixture.access_log();
    let refused = with_outcome(&log, Outcome::RefusedRestoring);
    assert_eq!(refused.len(), 1, "{log:#?}");
    let (tool, at) = &refused[0];
    assert_eq!(tool, "monthly_summary");
    assert!(*at >= before && *at <= refused_by, "{at}");
    let row = log
        .iter()
        .find(|call| call.outcome == Outcome::RefusedRestoring)
        .unwrap();
    assert_eq!(row.purpose.as_deref(), Some("review"));
    assert_eq!(
        row.args_redacted_json,
        r#"{"from":"2026-10","to":"2026-11"}"#
    );

    // And only once, however many calls follow.
    client.call_ok("list_categories", json!({})).await;
    assert_eq!(
        with_outcome(&fixture.access_log(), Outcome::RefusedRestoring).len(),
        1
    );
    client.close().await;
}

#[tokio::test]
async fn a_lock_file_left_by_a_stopped_restore_doesnt_block_calls() {
    let fixture = Fixture::new();
    let client = fixture.connect(CLIENT, AccessLevel::Standard).await;
    let logged = fixture.access_log().len();

    // What a restore that crashed or was killed leaves: its marker, in a
    // file whose lock went with the process.
    std::fs::write(
        backup::restore_lock_path(fixture.path()),
        "2026-11-12T10:00:00.000Z\n/backups/coffer-20261101T090000.000Z-manual.db\n",
    )
    .unwrap();
    assert!(!backup::restoring(fixture.path()).unwrap());

    client.call_ok("list_categories", json!({})).await;
    assert_eq!(fixture.access_log().len(), logged + 1);

    // A restore goes ahead over it, and the next call reads what it put in.
    add_report(&fixture, CLIENT, "Kept", "Before the backup.", None);
    let backup = backup::create(&fixture.open(), Reason::Manual)
        .unwrap()
        .path;
    add_report(&fixture, CLIENT, "Rolled back", "After the backup.", None);
    backup::restore(fixture.path(), &backup).unwrap();
    let after = client.call_ok("list_reports", json!({})).await;
    assert_eq!(report_titles(&after), ["Kept"]);
    client.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_lock_file_that_cant_be_locked_refuses_calls_and_names_it() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let client = fixture.connect(CLIENT, AccessLevel::Standard).await;
    let logged = fixture.access_log().len();
    let lock = backup::restore_lock_path(fixture.path());
    std::fs::write(&lock, "").unwrap();
    std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o000)).unwrap();
    if File::open(&lock).is_ok() {
        // Running as root: permissions don't stop it.
        return;
    }

    let result = client.call("list_categories", json!({})).await;
    assert_eq!(result.is_error, Some(true), "{result:?}");
    let text = serde_json::to_string(&result.content).unwrap();
    assert!(text.contains("coffer.db-restoring"), "{text}");
    assert!(text.contains("Nothing was read"), "{text}");
    // Only the file's name: the directory can name the user.
    let dir = fixture.path().parent().unwrap().display().to_string();
    assert!(!text.contains(&dir), "{text}");
    assert_eq!(fixture.access_log().len(), logged);

    std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o600)).unwrap();
    client.call_ok("list_categories", json!({})).await;
    // Logged once the lock could be checked again.
    let log = fixture.access_log();
    assert_eq!(log.len(), logged + 2);
    assert_eq!(log[logged].outcome, Outcome::RefusedUnavailable);
    assert_eq!(log[logged + 1].outcome, Outcome::Ok);
    client.close().await;
}
