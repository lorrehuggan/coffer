//! `search_transactions` end to end: levels, personal data, the access
//! log, filters, the row shape (splits, refunds, shared costs) and the cap.

mod support;

use coffer_core::access_log::Outcome;
use serde_json::{Value, json};
use support::reports::*;
use support::*;

const TOOL: &str = "search_transactions";

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
    assert_no_pii(&serde_json::to_value(&result).unwrap());
    let log = fixture.access_log();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].outcome, Outcome::RefusedLevel);
    assert_eq!(log[0].access_level, Some(AccessLevel::Aggregate));
    assert_eq!(log[0].row_count, 0);
    client.close().await;
}

#[tokio::test]
async fn output_has_no_personal_data() {
    let fixture = Fixture::new();
    tag_row(&fixture, fixture.seeded.manual, "flat", false);
    tag_row(&fixture, fixture.seeded.noted, "flat", false);

    let client = fixture
        .connect("claude-code", AccessLevel::Pseudonymous)
        .await;
    let all = client.call_ok(TOOL, json!({ "limit": 200 })).await;
    assert!(!rows_of(&all, fixture.seeded.manual).is_empty());
    client.call_ok(TOOL, json!({ "query": "transfer" })).await;
    client.close().await;

    let client = fixture.connect("claude-code", AccessLevel::Standard).await;
    // `call_ok` checks the whole response with `assert_no_pii`.
    let described = client
        .call_ok(TOOL, json!({ "limit": 200, "include_description": true }))
        .await;
    assert_manual_row_guarded(&described, &fixture);
    let tagged = client
        .call_ok(
            TOOL,
            json!({ "tags": ["flat"], "include_description": true }),
        )
        .await;
    assert_eq!(rows(&tagged).len(), 2, "{tagged:#}");
    assert_manual_row_guarded(&tagged, &fixture);
    // Matching the bank text finds the row; the output still holds none of it.
    let matched = client
        .call_ok(
            TOOL,
            json!({ "query": "fixture-holder", "include_description": true }),
        )
        .await;
    assert_eq!(rows(&matched).len(), 1);
    assert_manual_row_guarded(&matched, &fixture);
    client.close().await;
}

#[tokio::test]
async fn tags_and_descriptions_are_for_standard_only() {
    let fixture = Fixture::new();
    tag_row(&fixture, fixture.seeded.noted, "birthday", false);

    let client = fixture.connect("claude-code", AccessLevel::Standard).await;
    let result = client.call_ok(TOOL, json!({ "limit": 200 })).await;
    let noted = rows_of(&result, fixture.seeded.noted);
    assert_eq!(noted.len(), 1);
    // The private tag on the same row never appears.
    assert_eq!(noted[0]["tags"], json!(["birthday"]));
    assert!(rows(&result).iter().all(|row| row["tags"].is_array()));
    assert!(
        rows(&result)
            .iter()
            .all(|row| row["description_untrusted"].is_null())
    );
    let filtered = client.call_ok(TOOL, json!({ "tags": ["BIRTHDAY"] })).await;
    assert_eq!(rows(&filtered).len(), 1);
    // A private tag is refused as if it didn't exist.
    let private = client.call(TOOL, json!({ "tags": [PRIVATE_TAG] })).await;
    assert_eq!(private.is_error, Some(true));
    assert_no_pii(&serde_json::to_value(&private).unwrap());
    client.close().await;

    let client = fixture
        .connect("claude-code", AccessLevel::Pseudonymous)
        .await;
    let result = client.call_ok(TOOL, json!({ "limit": 200 })).await;
    assert!(!rows(&result).is_empty());
    for row in rows(&result) {
        assert!(row["tags"].is_null(), "{row}");
        assert!(row["description_untrusted"].is_null(), "{row}");
    }
    for args in [
        json!({ "tags": ["birthday"] }),
        json!({ "include_description": true }),
    ] {
        let refused = client.call(TOOL, args.clone()).await;
        assert_eq!(refused.is_error, Some(true), "{args}");
    }
    let log = fixture.access_log();
    assert!(
        log.iter()
            .rev()
            .take(2)
            .all(|call| call.outcome == Outcome::Error)
    );
    client.close().await;
}

#[tokio::test]
async fn a_call_writes_one_access_log_row() {
    let fixture = Fixture::new();
    let client = fixture
        .connect("claude-code", AccessLevel::Pseudonymous)
        .await;
    assert!(fixture.access_log().is_empty());

    let result = client
        .call_ok(
            TOOL,
            json!({
                "purpose": "coffee spend",
                "query": "bean there",
                "from": "2026-10-01",
                "limit": 3,
            }),
        )
        .await;

    let log = fixture.access_log();
    assert_eq!(log.len(), 1, "{log:#?}");
    let call = &log[0];
    assert_eq!(call.client, "claude-code");
    assert_eq!(call.tool, TOOL);
    assert_eq!(call.access_level, Some(AccessLevel::Pseudonymous));
    assert_eq!(call.outcome, Outcome::Ok);
    assert_eq!(call.purpose.as_deref(), Some("coffee spend"));
    assert_eq!(call.row_count, rows(&result).len() as u64);
    assert!(call.row_count > 0);
    assert!(
        !call.args_redacted_json.to_lowercase().contains("bean"),
        "{}",
        call.args_redacted_json
    );
    let args: Value = serde_json::from_str(&call.args_redacted_json).unwrap();
    assert!(args.get("query").is_some());
    assert!(args.get("purpose").is_none());
    client.close().await;
}

#[tokio::test]
async fn limit_caps_the_rows() {
    let fixture = Fixture::new();
    let client = fixture
        .connect("claude-code", AccessLevel::Pseudonymous)
        .await;
    let capped = client.call_ok(TOOL, json!({ "limit": 2 })).await;
    assert_eq!(rows(&capped).len(), 2);
    assert_eq!(capped["truncated"], true);
    let all = client.call_ok(TOOL, json!({ "limit": 200 })).await;
    assert_eq!(all["truncated"], false);
    assert!(rows(&all).len() > 2);
    // Newest first.
    let dates: Vec<&str> = rows(&all)
        .iter()
        .map(|row| row["date"].as_str().unwrap())
        .collect();
    assert!(dates.windows(2).all(|pair| pair[0] >= pair[1]), "{dates:?}");
    assert_eq!(fixture.access_log()[0].row_count, 2);
    client.close().await;
}

#[tokio::test]
async fn splits_refunds_and_shared_costs_show_up_as_rows() {
    let fixture = Fixture::new();
    let shop = add_merchant_row(
        &fixture,
        fixture.seeded.bank,
        "2026-12-01",
        -5_000,
        None,
        "Corner Shop",
    );
    split_row(
        &fixture,
        shop,
        &[(-3_000, Some("food.groceries")), (-2_000, Some("home"))],
    );
    let back = add_row(
        &fixture,
        fixture.seeded.bank,
        "2026-12-02",
        2_000,
        None,
        "CORNER SHOP REFUND",
    );
    refund_row(&fixture, back, shop, 2_000);

    let client = fixture.connect("claude-code", AccessLevel::Standard).await;
    let result = client
        .call_ok(
            TOOL,
            json!({ "from": "2026-11-15", "accounts": [fixture.seeded.bank.get()] }),
        )
        .await;

    let lines = rows_of(&result, shop);
    assert_eq!(lines.len(), 2, "{result:#}");
    assert_eq!(lines[0]["split_line"], 1);
    assert_eq!(lines[0]["category"], "food.groceries");
    assert_eq!(
        lines[0]["amount"],
        json!({ "minor": -3000, "currency": "GBP", "home": null })
    );
    assert_eq!(lines[1]["split_line"], 2);
    assert_eq!(lines[1]["category"], "home");
    assert_eq!(lines[0]["merchant"], "Corner Shop");
    assert!(lines.iter().all(|line| line["link"].is_null()));

    // A refund of a split purchase nets against each line's category, in
    // proportion.
    let refund = rows_of(&result, back);
    assert_eq!(refund.len(), 2, "{result:#}");
    for part in &refund {
        assert_eq!(
            part["link"],
            json!({ "kind": "refund", "target_transaction_id": shop.get() })
        );
        assert_eq!(part["split_line"], Value::Null);
    }
    let categories: Vec<&str> = refund
        .iter()
        .map(|part| part["category"].as_str().unwrap())
        .collect();
    assert_eq!(categories, ["food.groceries", "home"]);
    let total: i64 = refund
        .iter()
        .map(|part| part["amount"]["minor"].as_i64().unwrap())
        .sum();
    assert_eq!(total, 2000);

    let shared = rows_of(&result, fixture.seeded.manual);
    assert_eq!(shared.len(), 1);
    assert_eq!(shared[0]["amount"]["minor"], -4500);
    assert_eq!(shared[0]["my_amount"]["minor"], -2250);
    assert_eq!(shared[0]["merchant"], Value::Null);
    assert_eq!(shared[0]["account_id"], fixture.seeded.bank.get());

    // Every row has the same fields.
    let keys: Vec<&str> = rows(&result)[0]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let mut keys = keys;
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "account_id",
            "amount",
            "category",
            "date",
            "description_untrusted",
            "link",
            "merchant",
            "my_amount",
            "split_line",
            "tags",
            "transaction_id",
        ]
    );
    client.close().await;
}

#[tokio::test]
async fn filters_narrow_the_rows() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Standard).await;

    let coffee = client
        .call_ok(
            TOOL,
            json!({
                "query": "bean",
                "categories": ["food"],
                "accounts": [fixture.seeded.monzo.get()],
                "amount_min": -1000,
                "amount_max": 0,
                "from": "2026-10-01",
                "to": "2026-10-31",
            }),
        )
        .await;
    assert!(!rows(&coffee).is_empty());
    for row in rows(&coffee) {
        assert_eq!(row["merchant"], "Bean There");
        assert!(row["category"].as_str().unwrap().starts_with("food"));
        let minor = row["amount"]["minor"].as_i64().unwrap();
        assert!((-1000..=0).contains(&minor));
        assert!(row["date"].as_str().unwrap().starts_with("2026-10"));
    }

    // One end alone is open on the other side.
    let later = client
        .call_ok(TOOL, json!({ "from": "2026-11-20", "limit": 200 }))
        .await;
    assert!(
        rows(&later)
            .iter()
            .all(|row| row["date"].as_str().unwrap() >= "2026-11-20")
    );
    assert!(!rows_of(&later, fixture.seeded.manual).is_empty());
    let earlier = client
        .call_ok(TOOL, json!({ "to": "2026-11-19", "limit": 200 }))
        .await;
    assert!(rows_of(&earlier, fixture.seeded.manual).is_empty());
    assert!(!rows(&earlier).is_empty());
    client.close().await;
}

#[tokio::test]
async fn bad_arguments_are_refused() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Standard).await;
    for args in [
        json!({ "from": "2026-13-01" }),
        json!({ "to": "01/11/2026" }),
        json!({ "from": "2026-11-02", "to": "2026-11-01" }),
        json!({ "categories": ["no_such_category"] }),
        json!({ "categories": ["Not A Key"] }),
        json!({ "accounts": [999_999] }),
        json!({ "tags": ["no-such-tag"] }),
        json!({ "tags": [""] }),
        json!({ "amount_min": 10, "amount_max": -10 }),
    ] {
        let result = client.call(TOOL, args.clone()).await;
        assert_eq!(result.is_error, Some(true), "{args}");
        let text = serde_json::to_string(&result).unwrap();
        assert!(text.contains("invalid arguments"), "{args}: {text}");
    }
    assert!(
        fixture
            .access_log()
            .iter()
            .all(|call| call.outcome == Outcome::Error)
    );
    client.close().await;
}
