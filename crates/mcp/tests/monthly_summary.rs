//! `monthly_summary` end to end (pattern: `list_categories.rs`).
//!
//! It is an `aggregate` tool, the lowest level, so there is no level that
//! should be refused it: no `refused_level` case here.

mod support;

use coffer_core::access_log::Outcome;
use serde_json::json;
use support::reports::{amounts, assert_redacted, gbp};
use support::*;

const TOOL: &str = "monthly_summary";

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
async fn sums_each_month_with_the_users_share() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;

    let result = client
        .call_ok(TOOL, json!({ "from": "2026-09", "to": "2026-12" }))
        .await;
    assert_eq!(result["truncated"], false);
    let months = result["months"].as_array().unwrap();
    let keys: Vec<&str> = months
        .iter()
        .map(|month| month["month"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["2026-09", "2026-10", "2026-11", "2026-12"]);

    // Months without data are there, empty.
    for month in [&months[0], &months[3]] {
        assert!(amounts(&month["income"]).is_empty());
        assert!(amounts(&month["spending"]).is_empty());
        assert!(amounts(&month["net"]).is_empty());
    }
    // October (Monzo): pay and a friend's 12.00 in; the Bean There refund
    // nets against eating out; pot moves are transfers and count for
    // nothing.
    assert_eq!(amounts(&months[1]["income"]), gbp(216_200));
    assert_eq!(amounts(&months[1]["spending"]), gbp(10_142));
    assert_eq!(amounts(&months[1]["net"]), gbp(206_058));
    // November (the OFX statement, uncategorised): 253.63 out, plus the
    // shared manual row at the user's half, 22.50, not 45.00.
    assert_eq!(amounts(&months[2]["income"]), gbp(150_000));
    assert_eq!(amounts(&months[2]["spending"]), gbp(25_363 + 2_250));
    assert_eq!(amounts(&months[2]["net"]), gbp(150_000 - 27_613));
    client.close().await;
}

#[tokio::test]
async fn output_has_no_personal_data() {
    let fixture = Fixture::new();
    for &level in LEVELS {
        let client = fixture.connect("claude-code", level).await;
        // `call_ok` checks the whole response with `assert_no_pii`.
        let result = client
            .call_ok(TOOL, json!({ "from": "2026-10", "to": "2026-11" }))
            .await;
        assert_no_pii(&result);
        client.close().await;
    }
}

#[tokio::test]
async fn a_call_writes_one_access_log_row() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;
    assert!(fixture.access_log().is_empty());

    client
        .call_ok(
            TOOL,
            json!({ "from": "2026-10", "to": "2026-12", "purpose": "monthly review" }),
        )
        .await;

    let log = fixture.access_log();
    assert_eq!(log.len(), 1, "{log:#?}");
    let call = &log[0];
    assert_eq!(call.client, "claude-code");
    assert_eq!(call.tool, TOOL);
    assert_eq!(call.access_level, Some(AccessLevel::Aggregate));
    assert_eq!(call.outcome, Outcome::Ok);
    assert_eq!(call.purpose.as_deref(), Some("monthly review"));
    assert_eq!(call.row_count, 3, "one row per month");
    assert_redacted(
        &call.args_redacted_json,
        &["from", "to"],
        &["monthly review"],
    );
    client.close().await;
}

#[tokio::test]
async fn a_bad_range_is_a_tool_error_and_one_log_row() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;
    for args in [
        json!({ "from": "2026-13", "to": "2026-12" }),
        json!({ "from": "November", "to": "2026-12" }),
        json!({ "from": "2026-12", "to": "2026-10" }),
        json!({ "from": "2026-10" }),
    ] {
        let before = fixture.access_log().len();
        let result = client.call(TOOL, args.clone()).await;
        assert_eq!(result.is_error, Some(true), "{args}");
        let log = fixture.access_log();
        assert_eq!(log.len(), before + 1, "{args}");
        assert_eq!(log[before].outcome, Outcome::Error);
        assert_eq!(log[before].row_count, 0);
    }
    client.close().await;
}

#[tokio::test]
async fn declares_purpose_and_a_read_only_object_result() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;
    let tool = client
        .tools()
        .await
        .into_iter()
        .find(|tool| tool.name == TOOL)
        .unwrap();
    assert!(tool.input_schema["properties"]["purpose"].is_object());
    assert!(tool.input_schema["properties"]["from"].is_object());
    assert_eq!(tool.output_schema.unwrap()["type"], "object");
    assert_eq!(tool.annotations.unwrap().read_only_hint, Some(true));
    client.close().await;
}
