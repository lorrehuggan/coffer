//! `net_worth_history` end to end (pattern: `list_categories.rs`).
//!
//! It is an `aggregate` tool, the lowest level, so there is no level that
//! should be refused it: no `refused_level` case here.

mod support;

use coffer_core::access_log::Outcome;
use coffer_core::account::AccountKind;
use coffer_core::money::Currency;
use serde_json::{Value, json};
use support::reports::{add_valuation, add_valuation_account, amounts, assert_redacted, gbp};
use support::*;

const TOOL: &str = "net_worth_history";

/// `(name, kind, balance, counted, unanchored)` of each account in a month.
fn accounts(month: &Value) -> Vec<(String, String, i64, i64, bool)> {
    month["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            assert_eq!(amounts(&json!([a["balance"]]))[0].0, "GBP");
            (
                a["name"].as_str().unwrap().to_owned(),
                a["kind"].as_str().unwrap().to_owned(),
                a["balance"]["minor"].as_i64().unwrap(),
                a["counted"]["minor"].as_i64().unwrap(),
                a["unanchored"].as_bool().unwrap(),
            )
        })
        .collect()
}

fn row(
    name: &str,
    kind: &str,
    balance: i64,
    counted: i64,
    unanchored: bool,
) -> (String, String, i64, i64, bool) {
    (
        name.to_owned(),
        kind.to_owned(),
        balance,
        counted,
        unanchored,
    )
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
async fn sums_month_end_balances() {
    let fixture = Fixture::new();
    let mortgage =
        add_valuation_account(&fixture, "Mortgage", AccountKind::Liability, Currency::GBP);
    add_valuation(&fixture, mortgage, "2026-10-01", -200_000);
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;

    let result = client
        .call_ok(TOOL, json!({ "from": "2026-09", "to": "2026-11" }))
        .await;
    assert_eq!(result["truncated"], false);
    let months = result["months"].as_array().unwrap();
    let dates: Vec<(&str, &str)> = months
        .iter()
        .map(|m| (m["month"].as_str().unwrap(), m["date"].as_str().unwrap()))
        .collect();
    assert_eq!(
        dates,
        [
            ("2026-09", "2026-09-30"),
            ("2026-10", "2026-10-31"),
            ("2026-11", "2026-11-30")
        ]
    );

    // September: the Monzo export has no balances, so Everyday and its
    // pots are summed from zero; Bills is counted back from the OFX
    // statement's closing balance; the mortgage has no valuation yet.
    assert_eq!(
        accounts(&months[0]),
        [
            row("Everyday", "current", 0, 0, true),
            row("Rainy Day", "pot", 0, 0, true),
            row("Holiday Fund", "pot", 0, 0, true),
            row("Bills", "current", 118_010, 118_010, false),
        ]
    );
    assert_eq!(amounts(&months[0]["total"]), gbp(118_010));

    // October: money moved into pots stays in net worth; the mortgage
    // subtracts.
    assert_eq!(
        accounts(&months[1]),
        [
            row("Everyday", "current", 181_058, 181_058, true),
            row("Rainy Day", "pot", 17_500, 17_500, true),
            row("Holiday Fund", "pot", 7_500, 7_500, true),
            row("Bills", "current", 118_010, 118_010, false),
            row("Mortgage", "liability", -200_000, -200_000, false),
        ]
    );
    assert_eq!(
        amounts(&months[1]["total"]),
        gbp(181_058 + 17_500 + 7_500 + 118_010 - 200_000)
    );

    // November: Bills ends on the statement's 2,381.47, gross: the shared
    // manual row counts in full (−45.00) in a balance.
    assert_eq!(
        accounts(&months[2])[3],
        row("Bills", "current", 238_147, 238_147, false)
    );
    client.close().await;
}

#[tokio::test]
async fn output_has_no_personal_data() {
    let fixture = Fixture::new();
    for &level in LEVELS {
        let client = fixture.connect("claude-code", level).await;
        // `call_ok` checks the whole response with `assert_no_pii`.
        let result = client
            .call_ok(TOOL, json!({ "from": "2026-09", "to": "2026-12" }))
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
            json!({ "from": "2026-01", "to": "2026-12", "purpose": "net worth trend" }),
        )
        .await;

    let log = fixture.access_log();
    assert_eq!(log.len(), 1, "{log:#?}");
    let call = &log[0];
    assert_eq!(call.client, "claude-code");
    assert_eq!(call.tool, TOOL);
    assert_eq!(call.access_level, Some(AccessLevel::Aggregate));
    assert_eq!(call.outcome, Outcome::Ok);
    assert_eq!(call.purpose.as_deref(), Some("net worth trend"));
    assert_eq!(call.row_count, 12, "one row per month");
    assert_redacted(
        &call.args_redacted_json,
        &["from", "to"],
        &["net worth trend"],
    );
    client.close().await;
}

#[tokio::test]
async fn a_bad_range_is_a_tool_error_and_one_log_row() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;
    for args in [
        json!({ "from": "2026-1", "to": "2026-12" }),
        json!({ "from": "2026-10", "to": "9999-99" }),
        json!({ "from": "2026-12", "to": "2026-01" }),
        json!({ "to": "2026-12" }),
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
    assert!(tool.input_schema["properties"]["to"].is_object());
    assert_eq!(tool.output_schema.unwrap()["type"], "object");
    assert_eq!(tool.annotations.unwrap().read_only_hint, Some(true));
    client.close().await;
}
