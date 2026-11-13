//! The inbox tools end to end: `propose_categorizations`, `propose_rule`,
//! `propose_transfer_pair`, `propose_merchant_alias`, `propose_refund_link`
//! and `save_report` (ROADMAP M2.3, M2.14).
//!
//! Each tool is checked for:
//!
//! 1. registration at its level and above, absence below, and
//!    `refused_level` when called below;
//! 2. no personal data in its output, even when the client's own text
//!    holds some (the result never echoes it);
//! 3. exactly one access-log row per call, with no free text in
//!    `args_redacted_json`;
//! 4. writing only `suggestion` or `report`, as the calling client and
//!    `pending`;
//! 5. refusing payloads core refuses, writing nothing and logging `error`;
//! 6. returning the pending suggestion for an identical proposal.

mod support;

use coffer_core::access_log::Outcome;
use serde_json::{Value, json};
use support::inbox::*;
use support::*;

const CLIENT: &str = "claude-code";

/// Free text in every call's arguments, to check it is never echoed or
/// logged.
const MARKER: &str = "Zebra-marker-7Q4 client text";

/// A tool and valid arguments for it.
struct Case {
    tool: &'static str,
    level: AccessLevel,
    /// The suggestion kind it files, or `None` for a report.
    kind: Option<&'static str>,
    /// Valid arguments, with `text` in every free-text field.
    args: fn(&InboxIds, &str) -> Value,
}

const CASES: &[Case] = &[
    Case {
        tool: "propose_categorizations",
        level: AccessLevel::Standard,
        kind: Some("categorization"),
        args: |ids, text| {
            json!({
                "items": [
                    { "transaction_id": ids.money_out.get(), "category": "food.groceries" },
                    { "split_id": ids.split_line.get(), "category": "food.takeaway" },
                ],
                "rationale": text,
            })
        },
    },
    Case {
        tool: "propose_rule",
        level: AccessLevel::Standard,
        kind: Some("rule"),
        args: |_, text| {
            json!({
                "match_field": "description",
                "match_kind": "contains",
                "pattern": text,
                "amount_min": -5000,
                "amount_max": 0,
                "actions": [
                    { "kind": "set_category", "value": "food.groceries" },
                    { "kind": "add_tag", "value": ["groceries"] },
                    { "kind": "set_merchant", "value": text },
                ],
                "rationale": text,
            })
        },
    },
    Case {
        tool: "propose_transfer_pair",
        level: AccessLevel::Standard,
        kind: Some("transfer_pair"),
        args: |ids, text| {
            json!({
                "outflow_id": ids.transfer_out.get(),
                "inflow_id": ids.transfer_in.get(),
                "rationale": text,
            })
        },
    },
    Case {
        tool: "propose_merchant_alias",
        level: AccessLevel::Standard,
        kind: Some("merchant_alias"),
        args: |_, text| {
            json!({
                "pattern": text,
                "display_name": text,
                "match_kind": "prefix",
                "rationale": text,
            })
        },
    },
    Case {
        tool: "propose_refund_link",
        level: AccessLevel::Standard,
        kind: Some("refund_link"),
        args: |ids, text| {
            json!({
                "refund_id": ids.money_in.get(),
                "purchase_id": ids.money_out.get(),
                "link_kind": "refund",
                "rationale": text,
            })
        },
    },
    Case {
        tool: "save_report",
        level: AccessLevel::Aggregate,
        kind: None,
        args: |_, text| {
            json!({
                "period": "2026-10",
                "title": text,
                "body_markdown": format!("# Review\n\n{text}"),
            })
        },
    },
];

fn proposals() -> impl Iterator<Item = &'static Case> {
    CASES.iter().filter(|case| case.kind.is_some())
}

fn case(tool: &str) -> &'static Case {
    CASES.iter().find(|case| case.tool == tool).unwrap()
}

/// The error text of a failed call.
fn error_text(result: &rmcp::model::CallToolResult) -> String {
    assert_eq!(result.is_error, Some(true), "expected an error: {result:?}");
    serde_json::to_string(&result.content).unwrap()
}

#[tokio::test]
async fn each_tool_is_registered_at_its_level_and_above() {
    let fixture = Fixture::new();
    let ids = seed(&fixture);
    let before = outside_inbox(&fixture);
    for case in CASES {
        for &level in LEVELS {
            let listed = level >= case.level;
            assert_eq!(
                registered_tools(level).iter().any(|name| name == case.tool),
                listed,
                "{} in the registry at {level}",
                case.tool
            );
            let client = fixture.connect(CLIENT, level).await;
            assert_eq!(
                client
                    .tool_names()
                    .await
                    .iter()
                    .any(|name| name == case.tool),
                listed,
                "{} listed at {level}",
                case.tool
            );
            if !listed {
                let result = client.call(case.tool, (case.args)(&ids, MARKER)).await;
                assert_eq!(result.is_error, Some(true), "{} ran at {level}", case.tool);
                let log = fixture.access_log();
                let last = log.last().unwrap();
                assert_eq!(last.tool, case.tool);
                assert_eq!(last.outcome, Outcome::RefusedLevel);
                assert_eq!(last.access_level, Some(level));
                assert_eq!(last.row_count, 0);
            }
            client.close().await;
        }
    }
    // Nothing was refused into the inbox.
    assert_eq!(inbox_counts(&fixture), (0, 0));
    assert_eq!(outside_inbox(&fixture), before);
}

#[tokio::test]
async fn writing_tools_say_so_and_take_purpose() {
    let fixture = Fixture::new();
    let client = fixture.connect(CLIENT, AccessLevel::Standard).await;
    let tools = client.tools().await;
    for case in CASES {
        let tool = tools.iter().find(|tool| tool.name == case.tool).unwrap();
        assert!(tool.input_schema["properties"]["purpose"].is_object());
        assert!(
            tool.input_schema["properties"]
                .get("source_client")
                .is_none(),
            "{}: the source is the server's, never an argument",
            case.tool
        );
        assert_eq!(tool.output_schema.as_ref().unwrap()["type"], "object");
        assert_eq!(
            tool.annotations.as_ref().unwrap().read_only_hint,
            Some(false)
        );
    }
    let rule = tools
        .iter()
        .find(|tool| tool.name == "propose_rule")
        .unwrap();
    let description = rule.description.as_deref().unwrap();
    for action in ["set_category", "split_template", "set_merchant", "add_tag"] {
        assert!(description.contains(action), "{description}");
    }
    assert!(description.contains("approve"), "{description}");
    client.close().await;
}

#[tokio::test]
async fn outputs_have_no_personal_data_and_never_echo_the_client() {
    let fixture = Fixture::new();
    let ids = seed(&fixture);
    for case in CASES {
        for &level in LEVELS.iter().filter(|&&level| level >= case.level) {
            let client = fixture.connect(CLIENT, level).await;
            // The client's own text holds a name: `call_ok` fails if the
            // result repeats it.
            let result = client
                .call_ok(case.tool, (case.args)(&ids, HOLDER_NAME))
                .await;
            assert_no_pii(&result);
            let text = result.to_string();
            assert!(!text.contains("Robin"), "{}: {text}", case.tool);
            match case.kind {
                Some(_) => {
                    assert!(result["suggestion_id"].is_i64(), "{result}");
                    assert_eq!(result["status"], "pending");
                    assert!(result["already_pending"].is_boolean());
                }
                None => assert!(result["report_id"].is_i64(), "{result}"),
            }
            client.close().await;
        }
    }
}

#[tokio::test]
async fn a_call_writes_one_access_log_row_without_free_text() {
    let fixture = Fixture::new();
    let ids = seed(&fixture);
    let client = fixture.connect(CLIENT, AccessLevel::Standard).await;
    for case in CASES {
        let before = fixture.access_log().len();
        let mut args = (case.args)(&ids, MARKER);
        args["purpose"] = json!("weekly tidy-up");
        client.call_ok(case.tool, args.clone()).await;

        let log = fixture.access_log();
        assert_eq!(log.len(), before + 1, "{log:#?}");
        let call = log.last().unwrap();
        assert_eq!(call.client, CLIENT);
        assert_eq!(call.tool, case.tool);
        assert_eq!(call.access_level, Some(AccessLevel::Standard));
        assert_eq!(call.outcome, Outcome::Ok);
        assert_eq!(call.purpose.as_deref(), Some("weekly tidy-up"));
        assert_eq!(call.row_count, 1);
        assert_eq!(call.withheld_count, 0);
        assert!(
            !call.args_redacted_json.contains("Zebra"),
            "{}: {}",
            case.tool,
            call.args_redacted_json
        );
        // Each argument, by its type only.
        let redacted: Value = serde_json::from_str(&call.args_redacted_json).unwrap();
        let redacted = redacted.as_object().unwrap();
        let sent = args.as_object().unwrap();
        assert_eq!(redacted.len(), sent.len() - 1, "{redacted:?}");
        for (key, kind) in redacted {
            assert!(sent.contains_key(key));
            assert!(
                ["string", "number", "array", "object", "boolean", "null"]
                    .contains(&kind.as_str().unwrap()),
                "{key}: {kind}"
            );
        }
    }
    client.close().await;
}

#[tokio::test]
async fn each_tool_writes_only_the_inbox_as_the_calling_client() {
    let fixture = Fixture::new();
    let ids = seed(&fixture);
    let client = fixture.connect("codex", AccessLevel::Standard).await;
    for case in CASES {
        let before = outside_inbox(&fixture);
        let (suggestions_before, reports_before) = inbox_counts(&fixture);

        let result = client.call_ok(case.tool, (case.args)(&ids, MARKER)).await;

        assert_eq!(
            outside_inbox(&fixture),
            before,
            "{} wrote outside the inbox",
            case.tool
        );
        match case.kind {
            Some(kind) => {
                let stored = suggestions(&fixture);
                assert_eq!(stored.len(), suggestions_before + 1);
                assert_eq!(reports(&fixture).len(), reports_before);
                let suggestion = stored.last().unwrap();
                assert_eq!(result["suggestion_id"], suggestion.id);
                assert_eq!(result["already_pending"], false);
                assert_eq!(suggestion.kind, kind);
                assert_eq!(suggestion.source_client, "codex");
                assert_eq!(suggestion.status, "pending");
                assert_eq!(suggestion.rationale.as_deref(), Some(MARKER));
            }
            None => {
                let stored = reports(&fixture);
                assert_eq!(stored.len(), reports_before + 1);
                assert_eq!(suggestions(&fixture).len(), suggestions_before);
                let (id, source) = stored.last().unwrap();
                assert_eq!(result["report_id"], *id);
                assert_eq!(source, "codex");
            }
        }
    }
    client.close().await;
}

#[tokio::test]
async fn payloads_are_stored_in_cores_shape() {
    let fixture = Fixture::new();
    let ids = seed(&fixture);
    let client = fixture.connect(CLIENT, AccessLevel::Standard).await;
    for case in proposals() {
        client.call_ok(case.tool, (case.args)(&ids, "Tesco")).await;
    }
    let stored: Vec<Value> = suggestions(&fixture)
        .iter()
        .map(|suggestion| serde_json::from_str(&suggestion.payload_json).unwrap())
        .collect();
    // Core's canonical order: transactions, then split lines.
    assert_eq!(
        stored[0],
        json!({
            "items": [
                { "transaction_id": ids.money_out.get(), "category": "food.groceries" },
                { "split_id": ids.split_line.get(), "category": "food.takeaway" },
            ],
        })
    );
    assert_eq!(
        stored[1],
        json!({
            "match_field": "description",
            "match_kind": "contains",
            "pattern": "Tesco",
            "amount_min": -5000,
            "amount_max": 0,
            "actions": [
                { "kind": "set_category", "value": "food.groceries" },
                { "kind": "set_merchant", "value": "Tesco" },
                { "kind": "add_tag", "value": ["groceries"] },
            ],
        })
    );
    assert_eq!(
        stored[2],
        json!({ "outflow_id": ids.transfer_out.get(), "inflow_id": ids.transfer_in.get() })
    );
    assert_eq!(
        stored[3],
        json!({ "match_kind": "prefix", "pattern": "Tesco", "display_name": "Tesco" })
    );
    assert_eq!(
        stored[4],
        json!({
            "refund_id": ids.money_in.get(),
            "purchase_id": ids.money_out.get(),
            "link_kind": "refund",
        })
    );

    // Optional fields: an alias defaults to `exact`, a link takes an amount.
    client
        .call_ok(
            "propose_merchant_alias",
            json!({ "pattern": "AMZN MKTP", "display_name": "Amazon" }),
        )
        .await;
    client
        .call_ok(
            "propose_refund_link",
            json!({
                "refund_id": ids.money_in.get(),
                "purchase_id": ids.money_out.get(),
                "link_kind": "reimbursement",
                "amount_minor": 1,
            }),
        )
        .await;
    let stored = suggestions(&fixture);
    let alias: Value = serde_json::from_str(&stored[5].payload_json).unwrap();
    assert_eq!(alias["match_kind"], "exact");
    assert_eq!(stored[5].rationale, None);
    let link: Value = serde_json::from_str(&stored[6].payload_json).unwrap();
    assert_eq!(link["link_kind"], "reimbursement");
    assert_eq!(link["amount_minor"], 1);
    client.close().await;
}

/// A call core (or the arguments' schema) refuses, and part of the message
/// the client should see.
struct Refused {
    tool: &'static str,
    args: Value,
    says: &'static str,
}

fn refusals(ids: &InboxIds) -> Vec<Refused> {
    let refused = |tool, args, says| Refused { tool, args, says };
    let too_many: Vec<Value> = (1..=101)
        .map(|id| json!({ "transaction_id": id, "category": "food.groceries" }))
        .collect();
    vec![
        refused(
            "propose_categorizations",
            json!({ "items": [{ "transaction_id": 999_999, "category": "food.groceries" }] }),
            "there is no transaction 999999",
        ),
        refused(
            "propose_categorizations",
            json!({ "items": [{ "transaction_id": ids.money_out.get(), "category": "nope.nothing" }] }),
            "there is no category nope.nothing",
        ),
        refused(
            "propose_categorizations",
            json!({ "items": [{ "split_id": 999_999, "category": "food.groceries" }] }),
            "there is no split line 999999",
        ),
        refused(
            "propose_categorizations",
            json!({ "items": [{ "transaction_id": ids.split_parent.get(), "category": "food.groceries" }] }),
            "is split",
        ),
        refused(
            "propose_categorizations",
            json!({ "items": too_many }),
            "between 1 and 100 items",
        ),
        refused(
            "propose_categorizations",
            json!({ "items": [] }),
            "between 1 and 100 items",
        ),
        refused(
            "propose_categorizations",
            json!({
                "items": [{
                    "transaction_id": ids.money_out.get(),
                    "split_id": ids.split_line.get(),
                    "category": "food.groceries",
                }],
            }),
            "exactly one of transaction_id or split_id",
        ),
        refused(
            "propose_categorizations",
            json!({
                "items": [{ "transaction_id": ids.money_out.get(), "category": "food.groceries" }],
                "rationale": "x".repeat(1_001),
            }),
            "rationale is longer than 1000",
        ),
        refused(
            "propose_rule",
            json!({
                "match_field": "merchant",
                "match_kind": "is",
                "pattern": "TESCO",
                "actions": [{ "kind": "exclude", "value": "bank_error" }],
            }),
            "set_category",
        ),
        refused(
            "propose_rule",
            json!({
                "match_field": "merchant",
                "match_kind": "is",
                "pattern": "TESCO",
                "actions": [{ "kind": "set_category", "value": "nope.nothing" }],
            }),
            "there is no category nope.nothing",
        ),
        refused(
            "propose_rule",
            json!({
                "match_field": "merchant",
                "match_kind": "is",
                "pattern": "TESCO",
                "account_id": 999_999,
                "actions": [{ "kind": "set_category", "value": "food.groceries" }],
            }),
            "there is no account 999999",
        ),
        refused(
            "propose_rule",
            json!({
                "match_field": "merchant",
                "match_kind": "regex",
                "pattern": "(unclosed",
                "actions": [{ "kind": "set_category", "value": "food.groceries" }],
            }),
            "the rule isn't valid",
        ),
        refused(
            "propose_rule",
            json!({
                "match_field": "merchant",
                "match_kind": "is",
                "pattern": "TESCO",
                "amount_min": -12.5,
                "actions": [{ "kind": "set_category", "value": "food.groceries" }],
            }),
            "invalid arguments",
        ),
        refused(
            "propose_transfer_pair",
            json!({ "outflow_id": ids.transfer_out.get(), "inflow_id": ids.money_in.get() }),
            "they are on the same account",
        ),
        refused(
            "propose_transfer_pair",
            json!({ "outflow_id": ids.transfer_in.get(), "inflow_id": ids.transfer_out.get() }),
            "the outflow isn't money out",
        ),
        refused(
            "propose_transfer_pair",
            json!({ "outflow_id": ids.transfer_out.get(), "inflow_id": ids.transfer_out.get() }),
            "different transactions",
        ),
        refused(
            "propose_merchant_alias",
            json!({ "pattern": "AMZN", "display_name": "x".repeat(101) }),
            "at most 100 characters",
        ),
        refused(
            "propose_merchant_alias",
            json!({ "pattern": "   ", "display_name": "Amazon" }),
            "the merchant name isn't valid",
        ),
        refused(
            "propose_merchant_alias",
            json!({ "pattern": "AMZN", "display_name": "Amazon", "match_kind": "fuzzy" }),
            "invalid arguments",
        ),
        refused(
            "propose_refund_link",
            json!({
                "refund_id": ids.money_out.get(),
                "purchase_id": ids.transfer_out.get(),
                "link_kind": "refund",
            }),
            "isn't money in",
        ),
        refused(
            "propose_refund_link",
            json!({
                "refund_id": ids.money_in.get(),
                "purchase_id": ids.transfer_in.get(),
                "link_kind": "refund",
            }),
            "isn't money out",
        ),
        refused(
            "propose_refund_link",
            json!({
                "refund_id": ids.money_in.get(),
                "purchase_id": ids.money_out.get(),
                "link_kind": "refund",
                "amount_minor": 0,
            }),
            "amount_minor is more than zero",
        ),
        refused(
            "propose_refund_link",
            json!({
                "refund_id": ids.money_in.get(),
                "purchase_id": ids.money_out.get(),
                "link_kind": "settlement",
            }),
            "invalid arguments",
        ),
        refused(
            "save_report",
            json!({ "period": "2026-10", "title": "  ", "body_markdown": "Fine." }),
            "title can't be blank",
        ),
        refused(
            "save_report",
            json!({ "period": "x".repeat(33), "title": "October", "body_markdown": "Fine." }),
            "period is longer than 32",
        ),
    ]
}

#[tokio::test]
async fn refused_payloads_write_nothing_and_log_an_error() {
    let fixture = Fixture::new();
    let ids = seed(&fixture);
    let client = fixture.connect(CLIENT, AccessLevel::Standard).await;
    let before = outside_inbox(&fixture);
    for refused in refusals(&ids) {
        let log_before = fixture.access_log().len();
        let result = client.call(refused.tool, refused.args.clone()).await;
        let text = error_text(&result);
        assert!(
            text.contains(refused.says),
            "{} {}: {text}",
            refused.tool,
            refused.args
        );
        assert_no_pii(&serde_json::to_value(&result).unwrap());

        assert_eq!(inbox_counts(&fixture), (0, 0), "{}", refused.tool);
        let log = fixture.access_log();
        assert_eq!(log.len(), log_before + 1);
        let call = log.last().unwrap();
        assert_eq!(call.tool, refused.tool);
        assert_eq!(call.outcome, Outcome::Error);
        assert_eq!(call.row_count, 0);
    }
    assert_eq!(outside_inbox(&fixture), before);
    client.close().await;
}

#[tokio::test]
async fn an_identical_proposal_returns_the_pending_suggestion() {
    let fixture = Fixture::new();
    let ids = seed(&fixture);
    let client = fixture.connect(CLIENT, AccessLevel::Standard).await;
    for case in proposals() {
        let first = client.call_ok(case.tool, (case.args)(&ids, MARKER)).await;
        assert_eq!(first["already_pending"], false);
        let count = suggestions(&fixture).len();

        // The same payload with another rationale is the same suggestion.
        let again = client.call_ok(case.tool, (case.args)(&ids, MARKER)).await;
        assert_eq!(
            again["suggestion_id"], first["suggestion_id"],
            "{}",
            case.tool
        );
        assert_eq!(again["already_pending"], true);
        assert_eq!(again["status"], "pending");
        let mut reworded = (case.args)(&ids, MARKER);
        reworded["rationale"] = json!("Said differently");
        let reworded = client.call_ok(case.tool, reworded).await;
        assert_eq!(reworded["suggestion_id"], first["suggestion_id"]);
        assert_eq!(reworded["already_pending"], true);
        assert_eq!(suggestions(&fixture).len(), count, "{}", case.tool);
    }

    // Items in another order are the same categorisation.
    let categorize = case("propose_categorizations");
    let mut args = (categorize.args)(&ids, MARKER);
    args["items"].as_array_mut().unwrap().reverse();
    let reordered = client.call_ok(categorize.tool, args).await;
    assert_eq!(reordered["already_pending"], true);

    // Another client proposing the same thing gets the same suggestion.
    client.close().await;
    let other = fixture.connect("codex", AccessLevel::Standard).await;
    let pair = case("propose_transfer_pair");
    let result = other.call_ok(pair.tool, (pair.args)(&ids, MARKER)).await;
    assert_eq!(result["already_pending"], true);
    let stored = suggestions(&fixture);
    assert!(stored.iter().all(|s| s.source_client == CLIENT));
    other.close().await;
}
