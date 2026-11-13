//! `list_reports`: registered at every level (it is an `aggregate` tool,
//! the lowest level, so there is no level to refuse it at); the saved
//! reports newest first, each marked `ai_authored`, capped with
//! `truncated`; one access-log row per call.

mod support;

use coffer_core::access_log::Outcome;
use serde_json::json;
use support::reports::*;
use support::*;

const TOOL: &str = "list_reports";

/// Two reports: the older one saved by another client.
fn seed_reports(fixture: &Fixture) -> (i64, i64) {
    // Inserted first but saved later: the order is by time, not ID.
    let newer = add_report(
        fixture,
        "claude-code",
        "November so far",
        "Spending on **food** is down.",
        Some("2026-11-12T09:30:00.000Z"),
    );
    let older = add_report(
        fixture,
        "codex",
        "October review",
        "Subscriptions crept up.",
        Some("2026-11-01T08:00:00.000Z"),
    );
    (newer, older)
}

#[tokio::test]
async fn is_registered_at_every_level() {
    let fixture = Fixture::new();
    for &level in LEVELS {
        assert!(registered_tools(level).iter().any(|name| name == TOOL));
        let client = fixture.connect("claude-code", level).await;
        assert!(
            client.tool_names().await.iter().any(|name| name == TOOL),
            "{TOOL} isn't listed at {level}"
        );
        client.close().await;
    }
}

#[tokio::test]
async fn returns_the_reports_newest_first_marked_ai_authored() {
    let fixture = Fixture::new();
    let (newer, older) = seed_reports(&fixture);
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;

    let result = client.call_ok(TOOL, json!({})).await;
    assert_eq!(
        result,
        json!({
            "reports": [
                {
                    "id": newer,
                    "period": "2026-11",
                    "title": "November so far",
                    "body_markdown": "Spending on **food** is down.",
                    "source_client": "claude-code",
                    "created_at": "2026-11-12T09:30:00.000Z",
                    "ai_authored": true,
                },
                {
                    "id": older,
                    "period": "2026-11",
                    "title": "October review",
                    "body_markdown": "Subscriptions crept up.",
                    "source_client": "codex",
                    "created_at": "2026-11-01T08:00:00.000Z",
                    "ai_authored": true,
                },
            ],
            "truncated": false,
        })
    );
    client.close().await;
}

#[tokio::test]
async fn a_limit_cuts_the_list_and_says_so() {
    let fixture = Fixture::new();
    let (newer, _) = seed_reports(&fixture);
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;

    let result = client.call_ok(TOOL, json!({ "limit": 1 })).await;
    let reports = result["reports"].as_array().unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0]["id"], newer);
    assert_eq!(reports[0]["ai_authored"], true);
    assert_eq!(result["truncated"], true);

    let result = client.call_ok(TOOL, json!({ "limit": 2 })).await;
    assert_eq!(result["reports"].as_array().unwrap().len(), 2);
    assert_eq!(result["truncated"], false);

    // Above the maximum is the maximum; zero is one.
    let result = client.call_ok(TOOL, json!({ "limit": 100_000 })).await;
    assert_eq!(result["reports"].as_array().unwrap().len(), 2);
    let result = client.call_ok(TOOL, json!({ "limit": 0 })).await;
    assert_eq!(result["reports"].as_array().unwrap().len(), 1);
    assert_eq!(result["truncated"], true);

    // A limit that isn't a number is refused.
    let result = client.call(TOOL, json!({ "limit": "all" })).await;
    assert_eq!(result.is_error, Some(true));
    client.close().await;
}

#[tokio::test]
async fn no_reports_is_an_empty_list() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;
    let result = client.call_ok(TOOL, json!({})).await;
    assert_eq!(result, json!({ "reports": [], "truncated": false }));
    client.close().await;
}

#[tokio::test]
async fn output_has_no_personal_data() {
    let fixture = Fixture::new();
    seed_reports(&fixture);
    for &level in LEVELS {
        let client = fixture.connect("claude-code", level).await;
        // `call_ok` checks the whole response with `assert_no_pii`.
        let result = client.call_ok(TOOL, json!({})).await;
        assert_no_pii(&result);
        client.close().await;
    }
}

#[tokio::test]
async fn a_call_writes_one_access_log_row() {
    let fixture = Fixture::new();
    seed_reports(&fixture);
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;
    assert!(fixture.access_log().is_empty());

    client
        .call_ok(TOOL, json!({ "limit": 1, "purpose": "last review" }))
        .await;

    let log = fixture.access_log();
    assert_eq!(log.len(), 1, "{log:#?}");
    let call = &log[0];
    assert_eq!(call.client, "claude-code");
    assert_eq!(call.tool, TOOL);
    assert_eq!(call.access_level, Some(AccessLevel::Aggregate));
    assert_eq!(call.outcome, Outcome::Ok);
    assert_eq!(call.purpose.as_deref(), Some("last review"));
    assert_eq!(call.args_redacted_json, r#"{"limit":1}"#);
    assert_eq!(call.row_count, 1);
    assert_eq!(call.withheld_count, 0);
    client.close().await;
}

#[tokio::test]
async fn declares_purpose_limit_and_a_read_only_object_result() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;
    let tool = client
        .tools()
        .await
        .into_iter()
        .find(|tool| tool.name == TOOL)
        .unwrap();
    assert!(tool.input_schema["properties"]["purpose"].is_object());
    assert!(tool.input_schema["properties"]["limit"].is_object());
    let output = tool.output_schema.unwrap();
    assert_eq!(output["type"], "object");
    assert_eq!(tool.annotations.unwrap().read_only_hint, Some(true));
    client.close().await;
}

#[tokio::test]
async fn hidden_characters_are_stripped_from_report_text() {
    let fixture = Fixture::new();
    // Zero-width space, a bidi override and a control character.
    let id = add_report(
        &fixture,
        "claude-code",
        "Oct\u{200B}ober \u{202E}review",
        "Spending\u{0007} fell.\u{2066}",
        None,
    );
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;

    let result = client.call_ok(TOOL, json!({})).await;
    let report = &result["reports"][0];
    assert_eq!(report["id"], id);
    assert_eq!(report["title"], "October review");
    assert_eq!(report["body_markdown"], "Spending fell.");
    assert_eq!(report["ai_authored"], true);
    // The stored report is unchanged.
    let stored: String = rusqlite::Connection::open(fixture.path())
        .unwrap()
        .query_row("SELECT title FROM report WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(stored, "Oct\u{200B}ober \u{202E}review");
    client.close().await;
}
