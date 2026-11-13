//! `spending_by_category` end to end (pattern: `list_categories.rs`).
//!
//! It is an `aggregate` tool, the lowest level, so there is no level that
//! should be refused it: no `refused_level` case here.

mod support;

use coffer_core::access_log::Outcome;
use serde_json::json;
use support::reports::{add_row, amounts, assert_redacted, gbp};
use support::*;

const TOOL: &str = "spending_by_category";

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
async fn nests_spending_under_groups() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;

    // October (Monzo): energy under `bills` itself, groceries and eating
    // out under `food`; the Bean There refund nets against eating out.
    let october = client.call_ok(TOOL, json!({ "period": "2026-10" })).await;
    assert_eq!(october["period"], "2026-10");
    let groups = october["groups"].as_array().unwrap();
    let keys: Vec<&str> = groups.iter().map(|g| g["key"].as_str().unwrap()).collect();
    assert_eq!(keys, ["bills", "food"]);
    let bills = &groups[0];
    assert_eq!(amounts(&bills["spent_directly"]), gbp(6_125));
    assert!(bills["subcategories"].as_array().unwrap().is_empty());
    assert_eq!(amounts(&bills["total"]), gbp(6_125));
    let food = &groups[1];
    assert!(amounts(&food["spent_directly"]).is_empty());
    let subcategories: Vec<(&str, Vec<(String, i64)>)> = food["subcategories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["key"].as_str().unwrap(), amounts(&s["spent"])))
        .collect();
    assert_eq!(
        subcategories,
        [
            ("food.groceries", gbp(415)),
            // 3.40 + 3.40 + 8.62 + 24.00, less the 3.40 refund.
            ("food.eating_out", gbp(3_602)),
        ]
    );
    assert_eq!(amounts(&food["total"]), gbp(4_017));
    assert!(amounts(&october["uncategorized"]).is_empty());
    assert_eq!(amounts(&october["total"]), gbp(10_142));

    // November: the OFX rows are uncategorised money out, and the shared
    // manual row counts as the user's 22.50.
    let november = client.call_ok(TOOL, json!({ "period": "2026-11" })).await;
    assert!(november["groups"].as_array().unwrap().is_empty());
    assert_eq!(amounts(&november["uncategorized"]), gbp(25_363 + 2_250));
    assert_eq!(amounts(&november["total"]), gbp(27_613));
    client.close().await;
}

#[tokio::test]
async fn keeps_currencies_apart() {
    let fixture = Fixture::new();
    let euros = reports::add_account(&fixture, "Euros", coffer_core::money::Currency::EUR);
    add_row(
        &fixture,
        euros,
        "2026-10-12",
        -1_234,
        Some("food.groceries"),
        "TEST ROW",
    );
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;
    let october = client.call_ok(TOOL, json!({ "period": "2026-10" })).await;
    let food = &october["groups"][1];
    assert_eq!(food["key"], "food");
    let expected = vec![("EUR".to_owned(), 1_234), ("GBP".to_owned(), 4_017)];
    assert_eq!(amounts(&food["total"]), expected);
    assert_eq!(
        amounts(&october["total"]),
        [("EUR".to_owned(), 1_234), ("GBP".to_owned(), 10_142)]
    );
    client.close().await;
}

#[tokio::test]
async fn output_has_no_personal_data() {
    let fixture = Fixture::new();
    for &level in LEVELS {
        let client = fixture.connect("claude-code", level).await;
        // `call_ok` checks the whole response with `assert_no_pii`.
        for period in ["2026-10", "2026-11"] {
            let result = client.call_ok(TOOL, json!({ "period": period })).await;
            assert_no_pii(&result);
        }
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
            json!({ "period": "2026-10", "purpose": "where did it go" }),
        )
        .await;

    let log = fixture.access_log();
    assert_eq!(log.len(), 1, "{log:#?}");
    let call = &log[0];
    assert_eq!(call.client, "claude-code");
    assert_eq!(call.tool, TOOL);
    assert_eq!(call.access_level, Some(AccessLevel::Aggregate));
    assert_eq!(call.outcome, Outcome::Ok);
    assert_eq!(call.purpose.as_deref(), Some("where did it go"));
    // Two groups and their two subcategories.
    assert_eq!(call.row_count, 4);
    assert_redacted(&call.args_redacted_json, &["period"], &["where did it go"]);
    client.close().await;
}

#[tokio::test]
async fn a_bad_month_is_a_tool_error_and_one_log_row() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;
    for args in [
        json!({ "period": "2026-13" }),
        json!({ "period": "last month" }),
        json!({ "period": 202610 }),
        json!({}),
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
    assert!(tool.input_schema["properties"]["period"].is_object());
    assert_eq!(tool.output_schema.unwrap()["type"], "object");
    assert_eq!(tool.annotations.unwrap().read_only_hint, Some(true));
    client.close().await;
}
