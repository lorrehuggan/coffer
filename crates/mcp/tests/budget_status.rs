//! `budget_status` end to end (pattern: `list_categories.rs`).
//!
//! It is an `aggregate` tool, the lowest level, so there is no level that
//! should be refused it: no `refused_level` case here.

mod support;

use coffer_core::access_log::Outcome;
use coffer_core::budget::BudgetGroup;
use coffer_core::money::{Currency, Money};
use serde_json::json;
use support::reports::{add_budget, amounts, assert_redacted, gbp};
use support::*;

const TOOL: &str = "budget_status";

/// The fixture with three budgets that apply to October 2026 and one that
/// doesn't.
fn budgeted() -> Fixture {
    let fixture = Fixture::new();
    let pounds = |minor| Money::new(minor, Currency::GBP);
    add_budget(&fixture, "food", None, pounds(5_000), BudgetGroup::Flex);
    add_budget(
        &fixture,
        "food.eating_out",
        None,
        pounds(10_000),
        BudgetGroup::Flex,
    );
    // October's own budget replaces the standing one.
    add_budget(
        &fixture,
        "food.eating_out",
        Some("2026-10"),
        pounds(3_000),
        BudgetGroup::Fixed,
    );
    add_budget(
        &fixture,
        "bills",
        None,
        pounds(10_000),
        BudgetGroup::NonMonthly,
    );
    add_budget(
        &fixture,
        "health",
        Some("2026-12"),
        pounds(100),
        BudgetGroup::Flex,
    );
    fixture
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
async fn compares_budgets_with_spending() {
    let fixture = budgeted();
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;
    let result = client.call_ok(TOOL, json!({ "month": "2026-10" })).await;
    assert_eq!(result["month"], "2026-10");
    let budgets = result["budgets"].as_array().unwrap();
    let rows: Vec<_> = budgets
        .iter()
        .map(|b| {
            (
                b["category"].as_str().unwrap(),
                b["group_kind"].as_str().unwrap(),
                b["rollover"].as_bool().unwrap(),
                b["applies"].as_str().unwrap(),
                amounts(&json!([b["budget"]])),
                amounts(&json!([b["spent"]])),
                amounts(&json!([b["remaining"]])),
                amounts(&b["spent_other_currencies"]),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            // Category tree order: bills before food.
            (
                "bills",
                "non_monthly",
                true,
                "standing",
                gbp(10_000),
                gbp(6_125),
                gbp(3_875),
                vec![],
            ),
            // The group's budget covers groceries and eating out.
            (
                "food",
                "flex",
                false,
                "standing",
                gbp(5_000),
                gbp(4_017),
                gbp(983),
                vec![],
            ),
            // Over: the refund already netted, 36.02 against 30.00.
            (
                "food.eating_out",
                "fixed",
                false,
                "month",
                gbp(3_000),
                gbp(3_602),
                gbp(-602),
                vec![],
            ),
        ]
    );
    // November falls back to the standing budget; the shared manual row
    // is uncategorised, so it counts against no budget.
    let november = client.call_ok(TOOL, json!({ "month": "2026-11" })).await;
    let eating_out = &november["budgets"][2];
    assert_eq!(eating_out["category"], "food.eating_out");
    assert_eq!(eating_out["applies"], "standing");
    assert_eq!(amounts(&json!([eating_out["spent"]])), gbp(0));
    client.close().await;
}

#[tokio::test]
async fn without_budgets_the_list_is_empty() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;
    let result = client.call_ok(TOOL, json!({ "month": "2026-10" })).await;
    assert!(result["budgets"].as_array().unwrap().is_empty());
    client.close().await;
}

#[tokio::test]
async fn output_has_no_personal_data() {
    let fixture = budgeted();
    for &level in LEVELS {
        let client = fixture.connect("claude-code", level).await;
        // `call_ok` checks the whole response with `assert_no_pii`.
        for month in ["2026-10", "2026-11"] {
            let result = client.call_ok(TOOL, json!({ "month": month })).await;
            assert_no_pii(&result);
        }
        client.close().await;
    }
}

#[tokio::test]
async fn a_call_writes_one_access_log_row() {
    let fixture = budgeted();
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;
    assert!(fixture.access_log().is_empty());

    client
        .call_ok(
            TOOL,
            json!({ "month": "2026-10", "purpose": "budget check" }),
        )
        .await;

    let log = fixture.access_log();
    assert_eq!(log.len(), 1, "{log:#?}");
    let call = &log[0];
    assert_eq!(call.client, "claude-code");
    assert_eq!(call.tool, TOOL);
    assert_eq!(call.access_level, Some(AccessLevel::Aggregate));
    assert_eq!(call.outcome, Outcome::Ok);
    assert_eq!(call.purpose.as_deref(), Some("budget check"));
    assert_eq!(call.row_count, 3, "one row per budget");
    assert_redacted(&call.args_redacted_json, &["month"], &["budget check"]);
    client.close().await;
}

#[tokio::test]
async fn a_bad_month_is_a_tool_error_and_one_log_row() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;
    for args in [
        json!({ "month": "2026-00" }),
        json!({ "month": "Oct 2026" }),
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
    assert!(tool.input_schema["properties"]["month"].is_object());
    assert_eq!(tool.output_schema.unwrap()["type"], "object");
    assert_eq!(tool.annotations.unwrap().read_only_hint, Some(true));
    client.close().await;
}
