//! A restore while an MCP server runs (decision record "Settings, AI
//! access and backups", point 6, and "M2: MCP server", F.1): the server
//! keeps no connection open between calls, so it never blocks `coffer
//! restore`; it refuses calls while a restore holds the restore lock; a
//! lock file nobody holds (a stopped restore's) doesn't block it; and the
//! next call reads the restored database.

mod support;

use std::fs::{File, OpenOptions};
use std::path::Path;

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
    // Released, as a finished restore does; the file stays.
    drop(held);
    assert!(backup::restore_lock_path(fixture.path()).exists());

    // Nothing was read or written while it was held: no log rows, no report.
    assert_eq!(fixture.access_log().len(), logged);
    let after = client.call_ok("list_reports", json!({})).await;
    assert_eq!(report_titles(&after), Vec::<String>::new());
    assert_eq!(fixture.access_log().len(), logged + 1);
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
    client.close().await;
}
