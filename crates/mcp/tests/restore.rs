//! A restore while an MCP server runs (decision record "Settings, AI
//! access and backups", point 6): the server keeps no connection open
//! between calls, so it never blocks `coffer restore`; it refuses calls
//! while the restore lock is held; and the next call reads the restored
//! database.

mod support;

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

    let lock = backup::restore_lock_path(fixture.path());
    std::fs::write(&lock, "").unwrap();
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
    std::fs::remove_file(&lock).unwrap();

    // Nothing was read or written while it was held: no log rows, no report.
    assert_eq!(fixture.access_log().len(), logged);
    let after = client.call_ok("list_reports", json!({})).await;
    assert_eq!(report_titles(&after), Vec::<String>::new());
    assert_eq!(fixture.access_log().len(), logged + 1);
    client.close().await;
}
