//! M2's exit criteria (`docs/ROADMAP.md` → M2), walked end to end on the
//! fixture database:
//!
//! 1. the user enables a client (`coffer ai-access enable`, then `level`);
//! 2. the client asks the totals tools about last month and gets an answer;
//! 3. it proposes categorisations for uncategorised rows;
//! 4. the user approves them (`coffer inbox approve`, which runs core's
//!    `inbox::approve_all`), and they apply;
//! 5. every call is in the access log.
//!
//! "Last month" is October 2026: the fixtures' Monzo export is October's,
//! and this was written in November 2026. A real client asks Claude Code
//! "where did my money go last month?"; this test makes the calls such a
//! client makes.

mod support;

use coffer_core::access_log::Outcome;
use coffer_core::inbox::{self, ApproveOptions, SuggestionId};
use coffer_core::mcp_client::{self, ClientName};
use serde_json::{Value, json};
use support::reports::{amounts, gbp};
use support::*;

const CLIENT: &str = "claude-code";
const LAST_MONTH: &str = "2026-10";

fn category_of(fixture: &Fixture, transaction: i64) -> (Option<String>, Option<String>) {
    rusqlite::Connection::open(fixture.path())
        .unwrap()
        .query_row(
            "SELECT c.key, t.category_source FROM \"transaction\" t \
             LEFT JOIN category c ON c.id = t.category_id WHERE t.id = ?1",
            [transaction],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

#[tokio::test]
async fn where_did_my_money_go_last_month() {
    let fixture = Fixture::new();

    // 1. The user enables the client and raises it to `standard`.
    {
        let mut db = fixture.open();
        let name = ClientName::new(CLIENT).unwrap();
        mcp_client::enable(&mut db, &name).unwrap();
        mcp_client::set_level(&mut db, &name, AccessLevel::Standard).unwrap();
    }
    let client = fixture
        .connect_with(CLIENT, coffer_mcp::tools::registry())
        .await;
    let tools = client.tool_names().await;
    for tool in [
        "monthly_summary",
        "spending_by_category",
        "propose_categorizations",
    ] {
        assert!(tools.iter().any(|t| t == tool), "{tool} isn't listed");
    }
    let mut calls = Vec::new();

    // 2. Totals for last month.
    let summary = client
        .call_ok(
            "monthly_summary",
            json!({ "from": LAST_MONTH, "to": LAST_MONTH, "purpose": "where my money went" }),
        )
        .await;
    calls.push("monthly_summary");
    let month = &summary["months"][0];
    assert_eq!(month["month"], LAST_MONTH);
    assert_eq!(amounts(&month["spending"]), gbp(10_142));
    assert_eq!(amounts(&month["income"]), gbp(216_200));

    let spending = client
        .call_ok(
            "spending_by_category",
            json!({ "period": LAST_MONTH, "purpose": "where my money went" }),
        )
        .await;
    calls.push("spending_by_category");
    let groups: Vec<&str> = spending["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|group| group["key"].as_str().unwrap())
        .collect();
    assert!(!groups.is_empty(), "{spending:#}");

    // 3. Categorisations for rows that have none.
    let uncategorized = client
        .call_ok(
            "uncategorized_transactions",
            json!({ "limit": 200, "purpose": "tidy up categories" }),
        )
        .await;
    calls.push("uncategorized_transactions");
    let targets: Vec<i64> = uncategorized["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["split_line"].is_null() && row["amount"]["minor"].as_i64() < Some(0))
        .map(|row| row["transaction_id"].as_i64().unwrap())
        .take(2)
        .collect();
    assert_eq!(targets.len(), 2, "{uncategorized:#}");
    for &target in &targets {
        assert_eq!(category_of(&fixture, target).0, None);
    }
    let proposed = client
        .call_ok(
            "propose_categorizations",
            json!({
                "items": targets
                    .iter()
                    .map(|id| json!({ "transaction_id": id, "category": "bills.energy" }))
                    .collect::<Vec<Value>>(),
                "rationale": "Both look like the energy supplier's direct debit.",
                "purpose": "tidy up categories",
            }),
        )
        .await;
    calls.push("propose_categorizations");
    assert_eq!(proposed["status"], "pending");
    let id = SuggestionId::new(proposed["suggestion_id"].as_i64().unwrap());
    // Proposing changed nothing yet.
    for &target in &targets {
        assert_eq!(category_of(&fixture, target).0, None);
    }

    // 4. The user approves it in Coffer.
    {
        let mut db = fixture.open();
        inbox::approve_all(&mut db, &[id], ApproveOptions::default()).unwrap();
    }
    for &target in &targets {
        assert_eq!(
            category_of(&fixture, target),
            (
                Some("bills.energy".to_owned()),
                Some("ai_approved".to_owned())
            )
        );
    }
    let after = client
        .call_ok("uncategorized_transactions", json!({ "limit": 200 }))
        .await;
    calls.push("uncategorized_transactions");
    let still: Vec<i64> = after["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["transaction_id"].as_i64().unwrap())
        .collect();
    assert!(targets.iter().all(|target| !still.contains(target)));

    // 5. Every call is in the access log, in order, as this client.
    let log = fixture.access_log();
    let logged: Vec<&str> = log.iter().map(|row| row.tool.as_str()).collect();
    assert_eq!(logged, calls);
    for row in &log {
        assert_eq!(row.client, CLIENT);
        assert_eq!(row.access_level, Some(AccessLevel::Standard));
        assert_eq!(row.outcome, Outcome::Ok);
    }
    assert_eq!(log[0].purpose.as_deref(), Some("where my money went"));
    client.close().await;
}
