//! The tool × level matrix (ROADMAP M2.9): every registered tool at every
//! access level, on the fixture database.
//!
//! For each tool and level:
//!
//! - it is registered (and listed over the wire) at its level and above,
//!   and absent below;
//! - called below its level, it is refused with `refused_level`;
//! - called at or above it, its whole response passes [`assert_no_pii`],
//!   and at `pseudonymous` [`assert_no_display_merchants`]; below
//!   `standard` no account name (at `aggregate`) or person's name in bank
//!   text appears;
//! - each call writes exactly one access-log row, for that tool, level and
//!   outcome;
//! - no tool writes outside `suggestion` and `report`, and only the tools
//!   registered as writing add to them, filed as the calling client.
//!
//! [`MATRIX`] must list exactly the registered tools: a new tool fails
//! [`the_matrix_lists_every_registered_tool`] until it is added here with
//! its level and valid arguments.

mod support;

use std::collections::BTreeSet;

use coffer_core::access_log::Outcome;
use coffer_mcp::registry::Writes;
use serde_json::{Value, json};
use support::inbox::{InboxIds, inbox_counts, outside_inbox, seed, suggestions};
use support::*;

const CLIENT: &str = "matrix-client";

/// One registered tool: its level, whether it writes, and valid arguments.
struct Entry {
    tool: &'static str,
    level: AccessLevel,
    writes: Writes,
    args: fn(&InboxIds) -> Value,
}

const fn entry(
    tool: &'static str,
    level: AccessLevel,
    writes: Writes,
    args: fn(&InboxIds) -> Value,
) -> Entry {
    Entry {
        tool,
        level,
        writes,
        args,
    }
}

use AccessLevel::{Aggregate, Pseudonymous, Standard};
use Writes::{Inbox, Nothing};

/// Every tool Coffer registers.
const MATRIX: &[Entry] = &[
    // Reference data.
    entry("list_categories", Aggregate, Nothing, |_| json!({})),
    entry("list_reports", Aggregate, Nothing, |_| json!({})),
    entry("list_accounts", Pseudonymous, Nothing, |_| json!({})),
    entry("list_rules", Standard, Nothing, |_| json!({})),
    // Totals.
    entry(
        "monthly_summary",
        Aggregate,
        Nothing,
        |_| json!({ "from": "2026-09", "to": "2026-11" }),
    ),
    entry(
        "spending_by_category",
        Aggregate,
        Nothing,
        |_| json!({ "period": "2026-10" }),
    ),
    entry(
        "budget_status",
        Aggregate,
        Nothing,
        |_| json!({ "month": "2026-10" }),
    ),
    entry(
        "net_worth_history",
        Aggregate,
        Nothing,
        |_| json!({ "from": "2026-09", "to": "2026-11" }),
    ),
    // Rows.
    entry(
        "search_transactions",
        Pseudonymous,
        Nothing,
        |_| json!({ "limit": 200 }),
    ),
    entry(
        "uncategorized_transactions",
        Pseudonymous,
        Nothing,
        |_| json!({ "limit": 200 }),
    ),
    entry(
        "merchant_history",
        Standard,
        Nothing,
        |_| json!({ "merchant": "Bean There" }),
    ),
    entry("recurring_payments", Pseudonymous, Nothing, |_| json!({})),
    // The inbox.
    entry(
        "save_report",
        Aggregate,
        Inbox,
        |_| json!({ "period": "2026-10", "title": "Matrix", "body_markdown": "A review." }),
    ),
    entry("propose_categorizations", Standard, Inbox, |ids| {
        json!({ "items": [
            { "transaction_id": ids.money_out.get(), "category": "food.groceries" }
        ] })
    }),
    entry("propose_rule", Standard, Inbox, |_| {
        json!({
            "match_field": "merchant",
            "match_kind": "is",
            "pattern": "Bean There",
            "actions": [{ "kind": "set_category", "value": "food.eating_out" }],
        })
    }),
    entry(
        "propose_transfer_pair",
        Standard,
        Inbox,
        |ids| json!({ "outflow_id": ids.transfer_out.get(), "inflow_id": ids.transfer_in.get() }),
    ),
    entry(
        "propose_merchant_alias",
        Standard,
        Inbox,
        |_| json!({ "pattern": "BEAN THERE", "display_name": "Bean There", "match_kind": "prefix" }),
    ),
    entry("propose_refund_link", Standard, Inbox, |ids| {
        json!({
            "refund_id": ids.money_in.get(),
            "purchase_id": ids.money_out.get(),
            "link_kind": "refund",
        })
    }),
];

/// Account names in the fixture database that no other output contains
/// (`Bills` is also a category label).
const ACCOUNT_NAMES: &[&str] = &["Everyday", "Rainy Day", "Holiday Fund"];

fn contains_ignoring_case(value: &Value, needle: &str) -> bool {
    value
        .to_string()
        .to_lowercase()
        .contains(&needle.to_lowercase())
}

/// `value` without account names (`accounts[].name`, in any object): at
/// `pseudonymous` and above a client may see the user's account names
/// (decision record "M2: MCP server (2026-11-13)", point 2), and a pot's
/// name is also the display merchant of the transfers into it.
fn without_account_names(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, item)| {
                    let item = match (key.as_str(), item) {
                        ("accounts", Value::Array(accounts)) => Value::Array(
                            accounts
                                .iter()
                                .map(|account| {
                                    let mut account = without_account_names(account);
                                    if let Some(name) = account.get_mut("name") {
                                        *name = Value::Null;
                                    }
                                    account
                                })
                                .collect(),
                        ),
                        _ => without_account_names(item),
                    };
                    (key.clone(), item)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(without_account_names).collect()),
        other => other.clone(),
    }
}

#[test]
fn the_matrix_lists_every_registered_tool() {
    let registry = coffer_mcp::tools::registry();
    let registered: BTreeSet<&str> = registry.all().map(|tool| tool.spec().name).collect();
    let listed: BTreeSet<&str> = MATRIX.iter().map(|entry| entry.tool).collect();
    assert_eq!(
        registered, listed,
        "every registered tool needs a MATRIX entry, and every entry a tool"
    );
    assert_eq!(listed.len(), MATRIX.len(), "a tool is listed twice");
    for entry in MATRIX {
        let spec = registry.get(entry.tool).unwrap().spec();
        assert_eq!(spec.level, entry.level, "{}'s level", entry.tool);
        assert_eq!(spec.writes, entry.writes, "{}'s writes", entry.tool);
    }
}

#[tokio::test]
async fn every_tool_at_every_level() {
    let fixture = Fixture::new();
    let ids = seed(&fixture);
    fixture.add_name_only_row();
    // Enabling is the user's step, and creates the pseudonym key: compare
    // the database after it.
    fixture.set_client(CLIENT, Aggregate);
    let untouched = outside_inbox(&fixture);

    for &level in LEVELS {
        let client = fixture.connect(CLIENT, level).await;
        let listed = client.tool_names().await;
        let registered = registered_tools(level);
        for entry in MATRIX {
            let expected = entry.level <= level;
            let at = format!("{} at {level}", entry.tool);
            assert_eq!(registered.iter().any(|t| t == entry.tool), expected, "{at}");
            assert_eq!(listed.iter().any(|t| t == entry.tool), expected, "{at}");

            let logged = fixture.access_log().len();
            let (suggestions_before, reports_before) = inbox_counts(&fixture);
            let args = (entry.args)(&ids);

            if expected {
                let result = client.call_ok(entry.tool, args).await;
                if level == Pseudonymous {
                    assert_no_display_merchants(&fixture, &without_account_names(&result));
                }
                if level < Standard {
                    assert!(!contains_ignoring_case(&result, PAYEE_NAME), "{at}");
                }
                if level == Aggregate {
                    for name in ACCOUNT_NAMES {
                        assert!(!contains_ignoring_case(&result, name), "{name} in {at}");
                    }
                }
            } else {
                let result = client.call(entry.tool, args).await;
                assert_eq!(result.is_error, Some(true), "{at} ran");
                assert_no_pii(&serde_json::to_value(&result).unwrap());
            }

            // Exactly one access-log row, for this call.
            let log = fixture.access_log();
            assert_eq!(log.len(), logged + 1, "{at}: {log:#?}");
            let row = log.last().unwrap();
            assert_eq!(row.tool, entry.tool, "{at}");
            assert_eq!(row.client, CLIENT, "{at}");
            assert_eq!(row.access_level, Some(level), "{at}");
            let outcome = if expected {
                Outcome::Ok
            } else {
                Outcome::RefusedLevel
            };
            assert_eq!(row.outcome, outcome, "{at}");
            if !expected {
                assert_eq!(row.row_count, 0, "{at}");
            }

            // Writes: only the inbox, only by writing tools that ran.
            assert_eq!(
                outside_inbox(&fixture),
                untouched,
                "{at} wrote outside the inbox"
            );
            let (suggestions_after, reports_after) = inbox_counts(&fixture);
            let wrote = (suggestions_after - suggestions_before) + (reports_after - reports_before);
            match (entry.writes, expected) {
                (Inbox, true) => assert_eq!(wrote, 1, "{at} filed nothing"),
                _ => assert_eq!(wrote, 0, "{at} wrote to the inbox"),
            }
        }
        client.close().await;
    }

    // Every suggestion was filed as the calling client.
    let filed = suggestions(&fixture);
    assert_eq!(filed.len(), 5);
    assert!(
        filed.iter().all(|s| s.source_client == CLIENT),
        "{filed:#?}"
    );
}
