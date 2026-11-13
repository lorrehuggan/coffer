//! `merchant_history` end to end: levels, personal data, the access log,
//! the summary over every row and the cap.

mod support;

use coffer_core::access_log::Outcome;
use serde_json::{Value, json};
use support::reports::*;
use support::*;

const TOOL: &str = "merchant_history";

fn rows(result: &Value) -> &Vec<Value> {
    result["rows"].as_array().unwrap()
}

#[tokio::test]
async fn is_registered_at_standard_only() {
    let fixture = Fixture::new();
    for &level in LEVELS {
        let expected = level == AccessLevel::Standard;
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
    let client = fixture
        .connect("claude-code", AccessLevel::Pseudonymous)
        .await;
    let result = client.call(TOOL, json!({ "merchant": "Bean There" })).await;
    assert_eq!(result.is_error, Some(true));
    let log = fixture.access_log();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].outcome, Outcome::RefusedLevel);
    assert_eq!(log[0].access_level, Some(AccessLevel::Pseudonymous));
    assert_eq!(log[0].row_count, 0);
    client.close().await;
}

#[tokio::test]
async fn output_has_no_personal_data() {
    let fixture = Fixture::new();
    // The manual row's display name is derived from its bank text, account
    // details and all. Asking for it finds the row, and still returns none
    // of them.
    let display: String = rusqlite::Connection::open(fixture.path())
        .unwrap()
        .query_row(
            "SELECT merchant_display FROM \"transaction\" WHERE id = ?1",
            [fixture.seeded.manual.get()],
            |row| row.get(0),
        )
        .unwrap();
    let client = fixture.connect("claude-code", AccessLevel::Standard).await;
    // `call_ok` checks the whole response with `assert_no_pii`.
    let result = client
        .call_ok(
            TOOL,
            json!({ "merchant": display, "include_description": true }),
        )
        .await;
    assert_eq!(rows(&result).len(), 1, "{result:#}");
    assert_manual_row_guarded(&result, &fixture);
    assert_eq!(result["summary"]["rows"], 1);
    assert_eq!(
        result["summary"]["my_total"],
        json!([{ "minor": -2250, "currency": "GBP", "home": null }])
    );

    client
        .call_ok(
            TOOL,
            json!({ "merchant": "Bean There", "include_description": true }),
        )
        .await;
    let log = fixture.access_log();
    assert!(
        log.iter().all(
            |call| !call.args_redacted_json.to_lowercase().contains("bean")
                && !call.args_redacted_json.contains("77-41-29")
        ),
        "{log:#?}"
    );
    client.close().await;
}

#[tokio::test]
async fn the_summary_covers_every_row_even_when_capped() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Standard).await;

    let all = client
        .call_ok(TOOL, json!({ "merchant": "bean there", "limit": 200 }))
        .await;
    let count = rows(&all).len();
    assert!(
        count >= 2,
        "the fixture has several Bean There rows: {all:#}"
    );
    assert_eq!(all["truncated"], false);
    assert!(rows(&all).iter().all(|row| row["merchant"] == "Bean There"));
    assert!(rows(&all).iter().all(|row| row["tags"].is_array()));
    assert!(
        rows(&all)
            .iter()
            .all(|row| row["description_untrusted"].is_null())
    );
    let total: i64 = rows(&all)
        .iter()
        .map(|row| row["my_amount"]["minor"].as_i64().unwrap())
        .sum();
    let dates: Vec<&str> = rows(&all)
        .iter()
        .map(|row| row["date"].as_str().unwrap())
        .collect();

    let capped = client
        .call_ok(
            TOOL,
            json!({ "merchant": "  BEAN THERE ", "limit": 1, "purpose": "coffee habit" }),
        )
        .await;
    assert_eq!(rows(&capped).len(), 1);
    assert_eq!(capped["truncated"], true);
    let summary = &capped["summary"];
    assert_eq!(summary["rows"], count as u64);
    assert!(summary["transactions"].as_u64().unwrap() <= count as u64);
    assert_eq!(
        summary["my_total"],
        json!([{ "minor": total, "currency": "GBP", "home": null }])
    );
    assert_eq!(summary["last_date"], dates[0]);
    assert_eq!(summary["first_date"], *dates.last().unwrap());

    let log = fixture.access_log();
    let call = log.last().unwrap();
    assert_eq!(call.client, "claude-code");
    assert_eq!(call.tool, TOOL);
    assert_eq!(call.access_level, Some(AccessLevel::Standard));
    assert_eq!(call.outcome, Outcome::Ok);
    assert_eq!(call.purpose.as_deref(), Some("coffee habit"));
    assert_eq!(call.row_count, 1);
    assert_eq!(
        call.args_redacted_json,
        r#"{"limit":"number","merchant":"string"}"#
    );
    assert_eq!(log.len(), 2);
    client.close().await;
}

#[tokio::test]
async fn an_unknown_merchant_has_an_empty_history() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Standard).await;
    let result = client
        .call_ok(TOOL, json!({ "merchant": "Nowhere At All" }))
        .await;
    assert!(rows(&result).is_empty());
    assert_eq!(
        result["summary"],
        json!({
            "rows": 0,
            "transactions": 0,
            "my_total": [],
            "first_date": null,
            "last_date": null,
        })
    );
    client.close().await;
}

#[tokio::test]
async fn a_blank_or_missing_merchant_is_refused() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Standard).await;
    for args in [json!({ "merchant": "  " }), json!({})] {
        let result = client.call(TOOL, args.clone()).await;
        assert_eq!(result.is_error, Some(true), "{args}");
    }
    let log = fixture.access_log();
    assert_eq!(log.len(), 2);
    assert!(log.iter().all(|call| call.outcome == Outcome::Error));
    client.close().await;
}
