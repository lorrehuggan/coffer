//! `list_rules`: registered at `standard` only, refused below; the rules in
//! the order they run, with what their actions do, never a private tag,
//! note text or share label; one access-log row per call.

mod support;

use coffer_core::access_log::Outcome;
use coffer_core::category::CategoryKey;
use coffer_core::id::RuleId;
use coffer_core::rule::{
    ActionText, AmountBounds, ExcludeReason, MatchField, MatchKind, Pattern, RuleAction,
    RuleDefinition, Share, SplitLine, SplitTemplate, TagList,
};
use coffer_core::tag::TagName;
use serde_json::{Value, json};
use support::reports::*;
use support::*;

const TOOL: &str = "list_rules";

fn key(text: &str) -> CategoryKey {
    CategoryKey::new(text).unwrap()
}

fn share(basis_points: u64) -> Share {
    Share::new(basis_points).unwrap()
}

/// Three rules, stored in this order (each new rule goes first, so they
/// run in reverse): a merchant rule on every account, a split, and a rule
/// on the bank account with the private text no tool may return.
fn seed_rules(fixture: &Fixture) -> [RuleId; 3] {
    let uber = add_rule(
        fixture,
        &RuleDefinition::new(
            MatchField::Merchant,
            Pattern::new(MatchKind::Contains, "UBER").unwrap(),
            AmountBounds::new(None, Some(-100)).unwrap(),
            None,
            vec![
                RuleAction::SetCategory(key("transport")),
                RuleAction::SetMerchant(ActionText::new("Uber").unwrap()),
            ],
        )
        .unwrap(),
    );
    let costco = add_rule(
        fixture,
        &RuleDefinition::new(
            MatchField::Merchant,
            Pattern::new(MatchKind::Is, "Costco").unwrap(),
            AmountBounds::ANY,
            None,
            vec![RuleAction::SplitTemplate(
                SplitTemplate::new(vec![
                    SplitLine::new(key("food"), share(7_000)),
                    SplitLine::new(key("home"), share(3_000)),
                ])
                .unwrap(),
            )],
        )
        .unwrap(),
    );
    let tags = [PRIVATE_TAG, "work", "Rent-Share"]
        .iter()
        .map(|name| TagName::new(name).unwrap())
        .collect();
    let private = add_rule(
        fixture,
        &RuleDefinition::new(
            MatchField::Description,
            Pattern::new(MatchKind::Regex, "^rent").unwrap(),
            AmountBounds::new(Some(-200_000), Some(-50_000)).unwrap(),
            Some(fixture.seeded.bank.get()),
            vec![
                RuleAction::AddTag(TagList::new(tags).unwrap()),
                RuleAction::SetNote(ActionText::new(USER_NOTE).unwrap()),
                RuleAction::exclude(ExcludeReason::Disputed).unwrap(),
                RuleAction::MyShare(share(5_000)),
            ],
        )
        .unwrap(),
    );
    [uber, costco, private]
}

#[tokio::test]
async fn is_registered_at_standard_only() {
    let fixture = Fixture::new();
    for &level in LEVELS {
        let expected = level == AccessLevel::Standard;
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
async fn is_refused_below_standard() {
    let fixture = Fixture::new();
    seed_rules(&fixture);
    for (n, &level) in [AccessLevel::Aggregate, AccessLevel::Pseudonymous]
        .iter()
        .enumerate()
    {
        let client = fixture.connect("claude-code", level).await;
        let result = client.call(TOOL, json!({})).await;
        assert_eq!(result.is_error, Some(true), "{level}");
        assert!(result.structured_content.is_none());
        assert_no_pii(&serde_json::to_value(&result).unwrap());

        let log = fixture.access_log();
        assert_eq!(log.len(), n + 1, "{log:#?}");
        let call = log.last().unwrap();
        assert_eq!(call.tool, TOOL);
        assert_eq!(call.outcome, Outcome::RefusedLevel);
        assert_eq!(call.access_level, Some(level));
        assert_eq!(call.row_count, 0);
        client.close().await;
    }
}

#[tokio::test]
async fn returns_the_rules_in_the_order_they_run() {
    let fixture = Fixture::new();
    let [uber, costco, private] = seed_rules(&fixture);
    let client = fixture.connect("claude-code", AccessLevel::Standard).await;

    let result = client.call_ok(TOOL, json!({})).await;
    let rules = result["rules"].as_array().unwrap();
    let ids: Vec<i64> = rules.iter().map(|r| r["id"].as_i64().unwrap()).collect();
    assert_eq!(ids, [private.get(), costco.get(), uber.get()]);
    let priorities: Vec<i64> = rules
        .iter()
        .map(|r| r["priority"].as_i64().unwrap())
        .collect();
    assert!(priorities.windows(2).all(|w| w[0] < w[1]), "{priorities:?}");

    // The private text is gone: the private tag's name, the note and any
    // share label.
    assert_eq!(
        rules[0],
        json!({
            "id": private.get(),
            "priority": priorities[0],
            "match_field": "description",
            "match_kind": "regex",
            "pattern": "^rent",
            "amount": {
                "currency_from": "account",
                "min": { "minor": -200_000, "currency": "GBP", "home": null },
                "max": { "minor": -50_000, "currency": "GBP", "home": null },
            },
            "account_id": fixture.seeded.bank.get(),
            "actions": [
                { "kind": "add_tag", "tags": ["work", "Rent-Share"], "private_tags_hidden": 1 },
                { "kind": "set_note" },
                { "kind": "exclude", "reason": "disputed" },
                { "kind": "my_share", "basis_points": 5_000 },
            ],
        })
    );
    assert_eq!(
        rules[1],
        json!({
            "id": costco.get(),
            "priority": priorities[1],
            "match_field": "merchant",
            "match_kind": "is",
            "pattern": "Costco",
            "amount": null,
            "account_id": null,
            "actions": [{
                "kind": "split_template",
                "lines": [
                    { "category": "food", "basis_points": 7_000 },
                    { "category": "home", "basis_points": 3_000 },
                ],
            }],
        })
    );
    // On every account: the bounds are in the matched row's currency.
    assert_eq!(
        rules[2],
        json!({
            "id": uber.get(),
            "priority": priorities[2],
            "match_field": "merchant",
            "match_kind": "contains",
            "pattern": "UBER",
            "amount": {
                "currency_from": "matched_transaction",
                "min_minor": null,
                "max_minor": -100,
            },
            "account_id": null,
            "actions": [
                { "kind": "set_category", "category": "transport" },
                { "kind": "set_merchant", "merchant": "Uber" },
            ],
        })
    );
    client.close().await;
}

#[tokio::test]
async fn no_rules_is_an_empty_list() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Standard).await;
    let result = client.call_ok(TOOL, json!({})).await;
    assert_eq!(result, json!({ "rules": [] }));
    client.close().await;
}

#[tokio::test]
async fn output_has_no_personal_data() {
    let fixture = Fixture::new();
    seed_rules(&fixture);
    let client = fixture.connect("claude-code", AccessLevel::Standard).await;
    // `call_ok` checks the whole response with `assert_no_pii`: the private
    // tag and the user note are both in `PII`.
    let result = client.call_ok(TOOL, json!({})).await;
    assert_no_pii(&result);
    let text = result.to_string();
    assert!(!text.contains(SHARE_LABEL));
    client.close().await;
}

#[tokio::test]
async fn a_call_writes_one_access_log_row() {
    let fixture = Fixture::new();
    seed_rules(&fixture);
    let client = fixture.connect("claude-code", AccessLevel::Standard).await;
    assert!(fixture.access_log().is_empty());

    let result: Value = client
        .call_ok(TOOL, json!({ "purpose": "before proposing a rule" }))
        .await;
    assert_eq!(result["rules"].as_array().unwrap().len(), 3);

    let log = fixture.access_log();
    assert_eq!(log.len(), 1, "{log:#?}");
    let call = &log[0];
    assert_eq!(call.client, "claude-code");
    assert_eq!(call.tool, TOOL);
    assert_eq!(call.access_level, Some(AccessLevel::Standard));
    assert_eq!(call.outcome, Outcome::Ok);
    assert_eq!(call.purpose.as_deref(), Some("before proposing a rule"));
    assert_eq!(call.args_redacted_json, "{}");
    assert_eq!(call.row_count, 3);
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
