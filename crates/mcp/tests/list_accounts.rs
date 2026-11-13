//! `list_accounts`: registered at `pseudonymous` and above, refused below;
//! the fixture's accounts with their latest balances and anchoring, no
//! identifiers; one access-log row per call.

mod support;

use coffer_core::access_log::Outcome;
use serde_json::{Value, json};
use support::reports::*;
use support::*;

const TOOL: &str = "list_accounts";

/// What every account entry has, and nothing else: no institution, no
/// numbers.
const FIELDS: &[&str] = &[
    "balance",
    "balance_date",
    "balance_unanchored",
    "closed_on",
    "currency",
    "id",
    "kind",
    "name",
    "parent_id",
];

#[tokio::test]
async fn is_registered_at_pseudonymous_and_above() {
    let fixture = Fixture::new();
    for &level in LEVELS {
        let expected = level >= AccessLevel::Pseudonymous;
        assert_eq!(
            registered_tools(level).iter().any(|name| name == TOOL),
            expected,
            "{level}"
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
async fn is_refused_at_aggregate() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;
    let result = client.call(TOOL, json!({ "purpose": "net worth" })).await;
    assert_eq!(result.is_error, Some(true));
    assert!(result.structured_content.is_none());

    let log = fixture.access_log();
    assert_eq!(log.len(), 1, "{log:#?}");
    assert_eq!(log[0].tool, TOOL);
    assert_eq!(log[0].outcome, Outcome::RefusedLevel);
    assert_eq!(log[0].access_level, Some(AccessLevel::Aggregate));
    assert_eq!(log[0].row_count, 0);
    client.close().await;
}

fn by_name<'a>(accounts: &'a [Value], name: &str) -> &'a Value {
    accounts
        .iter()
        .find(|account| account["name"] == name)
        .unwrap_or_else(|| panic!("no account {name}: {accounts:#?}"))
}

#[tokio::test]
async fn returns_the_accounts_with_their_balances() {
    let fixture = Fixture::new();
    let seeded = fixture.seeded;
    let pot = add_pot(&fixture, seeded.bank, "Test Pot");
    add_balance(&fixture, pot, "2026-12-01", 12_345);
    let client = fixture
        .connect("claude-code", AccessLevel::Pseudonymous)
        .await;

    let result = client.call_ok(TOOL, json!({})).await;
    let accounts = result["accounts"].as_array().unwrap();
    for account in accounts {
        let mut keys: Vec<&str> = account
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, FIELDS, "{account}");
    }
    // In the order they were created: IDs ascending.
    let ids: Vec<i64> = accounts.iter().map(|a| a["id"].as_i64().unwrap()).collect();
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ids:?}");
    assert_eq!(ids.first(), Some(&seeded.monzo.get()));
    assert_eq!(ids.last(), Some(&pot.get()));

    // The Monzo export has no balances: its rows summed from zero, up to
    // the last row's day.
    let everyday = by_name(accounts, "Everyday");
    assert_eq!(everyday["id"], seeded.monzo.get());
    assert_eq!(everyday["kind"], "current");
    assert_eq!(everyday["currency"], "GBP");
    assert_eq!(everyday["parent_id"], Value::Null);
    assert_eq!(everyday["closed_on"], Value::Null);
    assert_eq!(
        everyday["balance"],
        json!({ "minor": 181_058, "currency": "GBP", "home": null })
    );
    assert_eq!(everyday["balance_date"], "2026-10-13");
    assert_eq!(everyday["balance_unanchored"], true);

    // The OFX statement's ledger balance anchors it; the manual row on
    // 20 Nov is before it.
    let bills = by_name(accounts, "Bills");
    assert_eq!(bills["id"], seeded.bank.get());
    assert_eq!(bills["kind"], "current");
    assert_eq!(
        bills["balance"],
        json!({ "minor": 238_147, "currency": "GBP", "home": null })
    );
    assert_eq!(bills["balance_date"], "2026-11-30");
    assert_eq!(bills["balance_unanchored"], false);

    // The Monzo import found its pots; they belong to Everyday.
    let pots: Vec<&Value> = accounts.iter().filter(|a| a["kind"] == "pot").collect();
    assert_eq!(pots.len(), 3, "{pots:#?}");
    for pot in &pots[..2] {
        assert_eq!(pot["parent_id"], seeded.monzo.get());
    }
    let test_pot = by_name(accounts, "Test Pot");
    assert_eq!(test_pot["parent_id"], seeded.bank.get());
    assert_eq!(test_pot["balance"]["minor"], 12_345);
    assert_eq!(test_pot["balance_date"], "2026-12-01");
    assert_eq!(test_pot["balance_unanchored"], false);
    client.close().await;
}

#[tokio::test]
async fn an_empty_account_has_no_balance() {
    let fixture = Fixture::new();
    add_account(&fixture, "Euro Cash", coffer_core::money::Currency::EUR);
    let client = fixture.connect("claude-code", AccessLevel::Standard).await;
    let result = client.call_ok(TOOL, json!({})).await;
    let euro = by_name(result["accounts"].as_array().unwrap(), "Euro Cash");
    assert_eq!(euro["currency"], "EUR");
    assert_eq!(euro["balance"], Value::Null);
    assert_eq!(euro["balance_date"], Value::Null);
    assert_eq!(euro["balance_unanchored"], Value::Null);
    client.close().await;
}

#[tokio::test]
async fn output_has_no_personal_data() {
    let fixture = Fixture::new();
    for &level in &[AccessLevel::Pseudonymous, AccessLevel::Standard] {
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
    let client = fixture
        .connect("claude-code", AccessLevel::Pseudonymous)
        .await;
    assert!(fixture.access_log().is_empty());

    let result = client
        .call_ok(TOOL, json!({ "purpose": "net worth check" }))
        .await;
    let count = result["accounts"].as_array().unwrap().len() as u64;

    let log = fixture.access_log();
    assert_eq!(log.len(), 1, "{log:#?}");
    let call = &log[0];
    assert_eq!(call.client, "claude-code");
    assert_eq!(call.tool, TOOL);
    assert_eq!(call.access_level, Some(AccessLevel::Pseudonymous));
    assert_eq!(call.outcome, Outcome::Ok);
    assert_eq!(call.purpose.as_deref(), Some("net worth check"));
    assert_eq!(call.args_redacted_json, "{}");
    assert_eq!(call.row_count, count);
    assert_eq!(count, 4);
    assert_eq!(call.withheld_count, 0);
    client.close().await;
}

#[tokio::test]
async fn declares_purpose_and_a_read_only_object_result() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Standard).await;
    let tool = client
        .tools()
        .await
        .into_iter()
        .find(|tool| tool.name == TOOL)
        .unwrap();
    assert!(tool.input_schema["properties"]["purpose"].is_object());
    assert_eq!(tool.output_schema.unwrap()["type"], "object");
    assert_eq!(tool.annotations.unwrap().read_only_hint, Some(true));
    client.close().await;
}
