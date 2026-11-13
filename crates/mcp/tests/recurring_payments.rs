//! `recurring_payments` end to end, on the fixture database plus a seeded
//! account with recurring payments and a schedule (`support::recurring`).

mod support;

use std::collections::BTreeSet;

use chrono::NaiveDate;
use coffer_core::access_log::Outcome;
use coffer_core::recurring;
use coffer_core::schedule;
use serde_json::{Value, json};
use support::recurring::*;
use support::*;

const TOOL: &str = "recurring_payments";

/// Every key the result may have, at any depth.
const KEYS: &[&str] = &[
    // The result.
    "as_of",
    "detected",
    "schedules",
    // A detected series.
    "merchant",
    "account_id",
    "direction",
    "cadence",
    "typical",
    "amount_kind",
    "first_date",
    "last_date",
    "next_expected",
    "occurrences",
    "lapsed",
    "category",
    // A schedule.
    "id",
    "amount",
    "tolerance_amount",
    "tolerance_days",
    "status",
    "next",
    "covered_through",
    // An occurrence.
    "date",
    "state",
    "paid",
    "paid_on",
    // An amount.
    "minor",
    "currency",
    "home",
];

fn sees_tool(level: AccessLevel) -> bool {
    level >= AccessLevel::Pseudonymous
}

fn keys(value: &Value, found: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, item) in map {
                found.insert(key.clone());
                keys(item, found);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| keys(item, found)),
        _ => {}
    }
}

/// Fails on a float anywhere in `value`: amounts are integer minor units.
fn assert_integers(value: &Value) {
    match value {
        Value::Number(number) => assert!(
            number.is_i64() || number.is_u64(),
            "a non-integer number: {number}"
        ),
        Value::Object(map) => map.values().for_each(assert_integers),
        Value::Array(items) => items.iter().for_each(assert_integers),
        _ => {}
    }
}

/// Fails on any string that looks like a series key (12 hex digits).
fn assert_no_series_keys(value: &Value) {
    match value {
        Value::String(text) => assert!(
            !(text.len() == 12 && text.bytes().all(|byte| byte.is_ascii_hexdigit())),
            "a series key in the output: {text:?}"
        ),
        Value::Object(map) => map.values().for_each(assert_no_series_keys),
        Value::Array(items) => items.iter().for_each(assert_no_series_keys),
        _ => {}
    }
}

fn find<'a>(items: &'a Value, merchant: &str, account: i64) -> Option<&'a Value> {
    items
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["merchant"] == merchant && item["account_id"] == account)
}

fn amount(minor: i64) -> Value {
    json!({ "minor": minor, "currency": "GBP", "home": null })
}

#[tokio::test]
async fn is_registered_at_pseudonymous_and_above() {
    let fixture = Fixture::new();
    for &level in LEVELS {
        assert_eq!(
            registered_tools(level).iter().any(|name| name == TOOL),
            sees_tool(level),
            "{TOOL} registration at {level}"
        );
        let client = fixture.connect("claude-code", level).await;
        assert_eq!(
            client.tool_names().await.iter().any(|name| name == TOOL),
            sees_tool(level),
            "{TOOL} listing at {level}"
        );
        client.close().await;
    }
}

#[tokio::test]
async fn is_refused_at_aggregate() {
    let fixture = Fixture::new();
    let client = fixture.connect("codex", AccessLevel::Aggregate).await;
    let result = client.call(TOOL, json!({ "purpose": "bills" })).await;
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

#[tokio::test]
async fn output_has_no_personal_data() {
    let fixture = Fixture::new();
    seed(&fixture);
    for &level in LEVELS.iter().filter(|&&level| sees_tool(level)) {
        let client = fixture.connect("claude-code", level).await;
        // `call_ok` checks the whole response with `assert_no_pii`.
        let result = client.call_ok(TOOL, json!({})).await;
        assert_no_pii(&result);
        assert_integers(&result);
        assert_no_series_keys(&result);
        client.close().await;
    }
}

#[tokio::test]
async fn a_call_writes_one_access_log_row() {
    let fixture = Fixture::new();
    seed(&fixture);
    let rows = {
        let db = fixture.open();
        let today = schedule::local_today().unwrap();
        recurring::find(&db).unwrap().len() + schedule::overview(&db, today).unwrap().len()
    };
    let client = fixture
        .connect("claude-code", AccessLevel::Pseudonymous)
        .await;
    assert!(fixture.access_log().is_empty());

    let result = client
        .call_ok(TOOL, json!({ "purpose": "check subscriptions" }))
        .await;
    let returned = result["detected"].as_array().unwrap().len()
        + result["schedules"].as_array().unwrap().len();
    assert_eq!(returned, rows);

    let log = fixture.access_log();
    assert_eq!(log.len(), 1, "{log:#?}");
    let call = &log[0];
    assert_eq!(call.client, "claude-code");
    assert_eq!(call.tool, TOOL);
    assert_eq!(call.access_level, Some(AccessLevel::Pseudonymous));
    assert_eq!(call.outcome, Outcome::Ok);
    assert_eq!(call.purpose.as_deref(), Some("check subscriptions"));
    assert_eq!(call.args_redacted_json, "{}");
    assert_eq!(call.row_count, rows as u64);
    assert_eq!(call.withheld_count, 0);
    client.close().await;
}

#[tokio::test]
async fn returns_the_detected_series() {
    let fixture = Fixture::new();
    let seeded = seed(&fixture);
    let account = seeded.account.get();
    let client = fixture.connect("claude-code", AccessLevel::Standard).await;

    let result = client.call_ok(TOOL, json!({})).await;
    assert_eq!(
        result["as_of"],
        schedule::local_today().unwrap().to_string()
    );
    let detected = &result["detected"];

    let gym = find(detected, GYM, account).expect("the gym series is detected");
    assert_eq!(gym["direction"], "out");
    assert_eq!(gym["cadence"], "monthly");
    assert_eq!(gym["typical"], amount(GYM_MINOR));
    assert_eq!(gym["amount_kind"], "fixed");
    assert_eq!(gym["first_date"], "2026-05-10");
    assert_eq!(gym["last_date"], "2026-10-10");
    assert_eq!(gym["next_expected"], "2026-11-10");
    assert_eq!(gym["occurrences"], 6);
    assert_eq!(gym["lapsed"], false);
    assert_eq!(gym["category"], GYM_CATEGORY);

    let energy = find(detected, ENERGY, account).expect("the energy series is detected");
    assert_eq!(energy["direction"], "out");
    assert_eq!(energy["cadence"], "monthly");
    assert_eq!(energy["amount_kind"], "variable");
    assert_eq!(energy["typical"]["currency"], "GBP");
    let typical = energy["typical"]["minor"].as_i64().unwrap();
    let smallest = *ENERGY_MINORS.iter().max().unwrap();
    let largest = *ENERGY_MINORS.iter().min().unwrap();
    assert!(
        (largest..=smallest).contains(&typical),
        "typical {typical} isn't among the bills' range"
    );
    assert_eq!(energy["first_date"], "2026-05-15");
    assert_eq!(energy["last_date"], "2026-10-15");
    assert_eq!(energy["occurrences"], 6);
    assert_eq!(energy["category"], Value::Null);

    // The scheduled series isn't offered again.
    assert!(find(detected, STREAMING, account).is_none());

    // Exactly the series core finds, nothing more.
    let core = recurring::find(&fixture.open()).unwrap();
    assert_eq!(detected.as_array().unwrap().len(), core.len());
    client.close().await;
}

#[tokio::test]
async fn returns_the_schedule_with_its_occurrences() {
    let fixture = Fixture::new();
    let seeded = seed(&fixture);
    let today = schedule::local_today().unwrap();
    let client = fixture.connect("claude-code", AccessLevel::Standard).await;

    let result = client.call_ok(TOOL, json!({})).await;
    let schedules = result["schedules"].as_array().unwrap();
    assert_eq!(schedules.len(), 1, "{schedules:#?}");
    let streaming = &schedules[0];
    assert_eq!(streaming["id"], seeded.schedule.get());
    assert_eq!(streaming["merchant"], STREAMING);
    assert_eq!(streaming["account_id"], seeded.account.get());
    assert_eq!(streaming["direction"], "out");
    assert_eq!(streaming["cadence"], "monthly");
    assert_eq!(streaming["amount"], amount(STREAMING_MINOR));
    assert_eq!(
        streaming["tolerance_amount"],
        amount(STREAMING_TOLERANCE.amount_minor)
    );
    assert_eq!(streaming["tolerance_days"], STREAMING_TOLERANCE.days);
    assert_eq!(streaming["status"], "active");
    assert_eq!(streaming["category"], Value::Null);
    assert_eq!(streaming["covered_through"], "2026-10-20");

    let occurrences = streaming["occurrences"].as_array().unwrap();
    assert!(occurrences.len() <= 13, "{occurrences:#?}");
    let last = occurrences.last().unwrap();
    assert_eq!(last["state"], "upcoming");
    assert_eq!(streaming["next"], last["date"]);
    assert!(last["date"].as_str().unwrap() >= today.to_string().as_str());
    let dates: Vec<&str> = occurrences
        .iter()
        .map(|occurrence| occurrence["date"].as_str().unwrap())
        .collect();
    assert!(dates.is_sorted(), "{dates:?}");

    // May to October: paid on time, missed in August, late in October.
    let expected = [
        ("2026-05-03", "matched", Some("2026-05-03")),
        ("2026-06-03", "matched", Some("2026-06-03")),
        ("2026-07-03", "matched", Some("2026-07-03")),
        ("2026-08-03", "missed", None),
        ("2026-09-03", "matched", Some("2026-09-03")),
        ("2026-10-03", "late", Some("2026-10-08")),
    ];
    let mut seen = 0;
    for (date, state, paid_on) in expected {
        let Some(occurrence) = occurrences.iter().find(|item| item["date"] == date) else {
            continue;
        };
        seen += 1;
        assert_eq!(occurrence["state"], state, "{occurrence}");
        match paid_on {
            Some(paid_on) => {
                assert_eq!(occurrence["paid"], amount(STREAMING_MINOR));
                assert_eq!(occurrence["paid_on"], paid_on);
            }
            None => {
                assert_eq!(occurrence["paid"], Value::Null);
                assert_eq!(occurrence["paid_on"], Value::Null);
            }
        }
    }
    // Twelve months on, the window starts after May; until then it holds
    // all six.
    if today < NaiveDate::from_ymd_opt(2027, 4, 1).unwrap() {
        assert_eq!(seen, expected.len(), "{occurrences:#?}");
    }
    // Every occurrence without a payment says so.
    for occurrence in occurrences {
        let paid = matches!(occurrence["state"].as_str(), Some("matched" | "late"));
        assert_eq!(occurrence["paid"].is_null(), !paid, "{occurrence}");
        assert_eq!(occurrence["paid_on"].is_null(), !paid, "{occurrence}");
    }
    client.close().await;
}

#[tokio::test]
async fn never_returns_series_keys_or_transaction_ids() {
    let fixture = Fixture::new();
    seed(&fixture);
    let series_keys: Vec<String> = recurring::find(&fixture.open())
        .unwrap()
        .into_iter()
        .map(|series| series.key.to_string())
        .collect();
    for &level in LEVELS.iter().filter(|&&level| sees_tool(level)) {
        let client = fixture.connect("claude-code", level).await;
        let result = client.call_ok(TOOL, json!({})).await;

        let mut found = BTreeSet::new();
        keys(&result, &mut found);
        for key in &found {
            assert!(KEYS.contains(&key.as_str()), "unexpected field {key:?}");
            assert!(
                !key.contains("key") && !key.contains("transaction"),
                "field {key:?}"
            );
        }
        let text = result.to_string();
        for series_key in &series_keys {
            assert!(!text.contains(series_key.as_str()), "series key in {text}");
        }
        assert_no_series_keys(&result);
        client.close().await;
    }
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
    let output = tool.output_schema.unwrap();
    assert_eq!(output["type"], "object");
    for field in ["as_of", "detected", "schedules"] {
        assert!(
            output["properties"][field].is_object(),
            "no {field} in the output schema"
        );
    }
    let annotations = tool.annotations.unwrap();
    assert_eq!(annotations.read_only_hint, Some(true));
    assert_eq!(annotations.destructive_hint, Some(false));
    client.close().await;
}
