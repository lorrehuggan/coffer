//! `uncategorized_transactions` end to end: levels, personal data, the
//! access log, the cap, and uncategorised split lines.

mod support;

use coffer_core::access_log::Outcome;
use serde_json::{Value, json};
use support::reports::*;
use support::*;

const TOOL: &str = "uncategorized_transactions";

fn rows(result: &Value) -> &Vec<Value> {
    result["rows"].as_array().unwrap()
}

#[tokio::test]
async fn is_registered_at_pseudonymous_and_above() {
    let fixture = Fixture::new();
    for &level in LEVELS {
        let expected = level >= AccessLevel::Pseudonymous;
        assert_eq!(
            registered_tools(level).iter().any(|name| name == TOOL),
            expected
        );
        let client = fixture.connect("claude-code", level).await;
        assert_eq!(
            client.tool_names().await.iter().any(|name| name == TOOL),
            expected,
            "{TOOL} at {level}"
        );
        client.close().await;
    }
}

#[tokio::test]
async fn is_refused_below_its_level() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;
    let result = client.call(TOOL, json!({})).await;
    assert_eq!(result.is_error, Some(true));
    let log = fixture.access_log();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].outcome, Outcome::RefusedLevel);
    assert_eq!(log[0].row_count, 0);
    client.close().await;
}

#[tokio::test]
async fn output_has_no_personal_data() {
    let fixture = Fixture::new();
    tag_row(&fixture, fixture.seeded.manual, "flat", false);

    let client = fixture
        .connect("claude-code", AccessLevel::Pseudonymous)
        .await;
    // `call_ok` checks the whole response with `assert_no_pii`.
    let result = client.call_ok(TOOL, json!({ "limit": 200 })).await;
    let manual = rows_of(&result, fixture.seeded.manual);
    assert_eq!(manual.len(), 1, "the fixture's manual row is uncategorised");
    assert!(manual[0]["tags"].is_null());
    assert!(manual[0]["description_untrusted"].is_null());
    let refused = client
        .call(TOOL, json!({ "include_description": true }))
        .await;
    assert_eq!(refused.is_error, Some(true));
    client.close().await;

    let client = fixture.connect("claude-code", AccessLevel::Standard).await;
    let result = client
        .call_ok(TOOL, json!({ "limit": 200, "include_description": true }))
        .await;
    assert_manual_row_guarded(&result, &fixture);
    assert_eq!(
        rows_of(&result, fixture.seeded.manual)[0]["tags"],
        json!(["flat"])
    );
    client.close().await;
}

#[tokio::test]
async fn every_row_is_uncategorised_and_split_lines_count_alone() {
    let fixture = Fixture::new();
    let shop = add_row(
        &fixture,
        fixture.seeded.bank,
        "2026-12-01",
        -5_000,
        None,
        "CORNER SHOP",
    );
    split_row(
        &fixture,
        shop,
        &[(-3_000, Some("food.groceries")), (-2_000, None)],
    );

    let client = fixture
        .connect("claude-code", AccessLevel::Pseudonymous)
        .await;
    let result = client.call_ok(TOOL, json!({ "limit": 200 })).await;
    assert!(!rows(&result).is_empty());
    assert!(rows(&result).iter().all(|row| row["category"].is_null()));
    let lines = rows_of(&result, shop);
    assert_eq!(lines.len(), 1, "{result:#}");
    assert_eq!(lines[0]["split_line"], 2);
    assert_eq!(lines[0]["amount"]["minor"], -2000);
    // The newest row comes first.
    assert_eq!(rows(&result)[0]["transaction_id"], shop.get());
    client.close().await;
}

#[tokio::test]
async fn limit_caps_the_rows_and_a_call_writes_one_log_row() {
    let fixture = Fixture::new();
    for n in 0..3 {
        add_row(
            &fixture,
            fixture.seeded.bank,
            &format!("2026-12-0{}", n + 1),
            -100,
            None,
            "UNSORTED",
        );
    }
    let client = fixture
        .connect("claude-code", AccessLevel::Pseudonymous)
        .await;
    let capped = client
        .call_ok(TOOL, json!({ "limit": 2, "purpose": "tidy up" }))
        .await;
    assert_eq!(rows(&capped).len(), 2);
    assert_eq!(capped["truncated"], true);

    let log = fixture.access_log();
    assert_eq!(log.len(), 1, "{log:#?}");
    let call = &log[0];
    assert_eq!(call.client, "claude-code");
    assert_eq!(call.tool, TOOL);
    assert_eq!(call.access_level, Some(AccessLevel::Pseudonymous));
    assert_eq!(call.outcome, Outcome::Ok);
    assert_eq!(call.purpose.as_deref(), Some("tidy up"));
    assert_eq!(call.args_redacted_json, r#"{"limit":2}"#);
    assert_eq!(call.row_count, 2);
    assert_eq!(call.withheld_count, 0);

    let all = client.call_ok(TOOL, json!({ "limit": 200 })).await;
    assert_eq!(all["truncated"], false);
    assert!(rows(&all).len() >= 4);
    client.close().await;
}
