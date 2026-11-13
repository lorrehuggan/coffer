//! A payment whose bank text is a person's name and nothing else (the
//! user's decision 1, 2026-11-13). The account-details check can't
//! recognise a name, so the name reaches a client only where merchant
//! names do: at `standard`, which is full access by design. At
//! `pseudonymous` it never appears, in any tool, in any field.

mod support;

use serde_json::{Value, json};
use support::*;

const CLIENT: &str = "claude-code";

/// The row tools a `pseudonymous` client has, with arguments that return
/// every row they can.
const ROW_CALLS: &[(&str, &str)] = &[
    ("search_transactions", r#"{"limit": 200}"#),
    ("uncategorized_transactions", r#"{"limit": 200}"#),
    ("recurring_payments", "{}"),
];

fn contains_ignoring_case(value: &Value, needle: &str) -> bool {
    value
        .to_string()
        .to_lowercase()
        .contains(&needle.to_lowercase())
}

fn row_ids(result: &Value) -> Vec<i64> {
    result["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["transaction_id"].as_i64().unwrap())
        .collect()
}

#[tokio::test]
async fn a_name_never_appears_at_pseudonymous() {
    let fixture = Fixture::new();
    let payee = fixture.add_name_only_row();
    // The name is the row's display merchant, so the pseudonym check
    // looks for it.
    assert!(
        display_merchants(&fixture)
            .iter()
            .any(|name| name.eq_ignore_ascii_case(PAYEE_NAME)),
        "{:?}",
        display_merchants(&fixture)
    );
    let client = fixture.connect(CLIENT, AccessLevel::Pseudonymous).await;
    for &(tool, args) in ROW_CALLS {
        let result = client
            .call_ok(tool, serde_json::from_str(args).unwrap())
            .await;
        assert_pseudonymous_ok(&fixture, &result);
        assert!(
            !contains_ignoring_case(&result, PAYEE_NAME),
            "{tool}: {result}"
        );
        assert!(!contains_ignoring_case(&result, "nameonly"), "{tool}");
    }
    // The row is there, under a token.
    let all = client
        .call_ok("search_transactions", json!({ "limit": 200 }))
        .await;
    assert!(row_ids(&all).contains(&payee.get()));
    let row = all["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["transaction_id"] == payee.get())
        .unwrap();
    assert!(is_merchant_token(row["merchant"].as_str().unwrap()), "{row}");
    client.close().await;
}

#[tokio::test]
async fn a_name_may_appear_at_standard() {
    let fixture = Fixture::new();
    let payee = fixture.add_name_only_row();
    let client = fixture.connect(CLIENT, AccessLevel::Standard).await;
    let all = client
        .call_ok("search_transactions", json!({ "limit": 200 }))
        .await;
    let row = all["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["transaction_id"] == payee.get())
        .unwrap()
        .clone();
    assert!(
        contains_ignoring_case(&row["merchant"], "nameonly-payee"),
        "{row}"
    );
    client.close().await;
}
