use crate::account::AccountKind;
use crate::category::CategoryKey;
use crate::inbox::{self, NewReport};
use crate::mcp_client::ClientName;
use crate::money::Currency;
use crate::reporting::balance::Anchor;
use crate::rule::{
    self, ActionText, AmountBounds, ExcludeReason, MatchField, MatchKind, Pattern, RuleAction,
    RuleDefinition, Share, SplitLine, SplitTemplate, TagList,
};
use crate::tag::{self, TagName};

use super::super::test_ledger::{TestLedger, date};
use super::*;

const NOTE_TEXT: &str = "Secret note for Alex";
const PRIVATE_TAG: &str = "Clinic-private";

fn key(text: &str) -> CategoryKey {
    CategoryKey::new(text).unwrap()
}

fn tags(names: &[&str]) -> RuleAction {
    RuleAction::AddTag(
        TagList::new(names.iter().map(|n| TagName::new(n).unwrap()).collect()).unwrap(),
    )
}

#[test]
fn accounts_come_in_creation_order_with_their_latest_balance() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    ledger.row(current, "2026-10-01", 10_000, None);
    ledger.row(current, "2026-10-20", -2_500, None);
    let pot = ledger.pot(current, "Holiday");
    ledger.row(pot, "2026-10-05", 1_000, None);
    ledger.balance(pot, "2026-10-31", 5_000);
    let house = ledger.valuation_account("House", AccountKind::Asset, Currency::GBP);
    ledger.valuation(house, "2026-09-30", 30_000_000);
    let car = ledger.valuation_account("Car loan", AccountKind::Liability, Currency::GBP);
    let empty = ledger.account("Euro", Currency::EUR);

    let reader = ledger.reader();
    let accounts = accounts(&reader).unwrap();
    let ids: Vec<AccountId> = accounts.iter().map(|a| a.id).collect();
    assert_eq!(ids, [current, pot, house, car, empty]);

    let latest = |a: &AccountSummary| {
        a.latest
            .map(|b| (b.date, b.balance.minor(), b.balance.currency(), b.anchor))
    };
    assert_eq!(accounts[0].name, "Current");
    assert_eq!(accounts[0].kind, AccountKind::Current);
    assert_eq!(accounts[0].parent, None);
    assert_eq!(
        latest(&accounts[0]),
        Some((date("2026-10-20"), 7_500, Currency::GBP, Anchor::Unanchored))
    );

    assert_eq!(accounts[1].kind, AccountKind::Pot);
    assert_eq!(accounts[1].parent, Some(current));
    let entered = Anchor::Stated {
        date: date("2026-10-31"),
    };
    assert_eq!(
        latest(&accounts[1]),
        Some((date("2026-10-31"), 5_000, Currency::GBP, entered))
    );

    let valued = Anchor::Stated {
        date: date("2026-09-30"),
    };
    assert_eq!(
        latest(&accounts[2]),
        Some((date("2026-09-30"), 30_000_000, Currency::GBP, valued))
    );
    // A valuation account without valuations, and an empty account.
    assert_eq!(latest(&accounts[3]), None);
    assert_eq!(accounts[4].currency, Currency::EUR);
    assert_eq!(latest(&accounts[4]), None);
    assert!(accounts.iter().all(|a| a.closed_on.is_none()));
}

#[test]
fn closed_accounts_say_when() {
    let mut ledger = TestLedger::new();
    let old = ledger.account("Old", Currency::GBP);
    ledger
        .db
        .conn()
        .execute(
            "UPDATE account SET closed_on = '2026-06-30' WHERE id = ?1",
            [old],
        )
        .unwrap();
    let accounts = accounts(&ledger.reader()).unwrap();
    assert_eq!(accounts[0].closed_on, Some(date("2026-06-30")));
}

#[test]
fn rules_come_in_priority_order_without_private_text() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    tag::create(&mut ledger.db, &TagName::new(PRIVATE_TAG).unwrap(), true).unwrap();
    tag::create(&mut ledger.db, &TagName::new("work").unwrap(), false).unwrap();

    let first = rule::create(
        &mut ledger.db,
        &RuleDefinition::new(
            MatchField::Merchant,
            Pattern::new(MatchKind::Contains, "UBER").unwrap(),
            AmountBounds::ANY,
            None,
            vec![
                RuleAction::SetCategory(key("transport")),
                RuleAction::SetMerchant(ActionText::new("Uber").unwrap()),
            ],
        )
        .unwrap(),
    )
    .unwrap();
    // Created second, so it goes first (a new rule goes to the top).
    let second = rule::create(
        &mut ledger.db,
        &RuleDefinition::new(
            MatchField::Description,
            Pattern::new(MatchKind::Regex, "^rent").unwrap(),
            AmountBounds::new(Some(-200_000), Some(-50_000)).unwrap(),
            Some(current.get()),
            vec![
                // Upper case: still the private tag, ignoring case.
                tags(&["Work", &PRIVATE_TAG.to_uppercase(), "new-tag"]),
                RuleAction::SetNote(ActionText::new(NOTE_TEXT).unwrap()),
                RuleAction::exclude(ExcludeReason::Disputed).unwrap(),
                RuleAction::MyShare(Share::new(5_000).unwrap()),
            ],
        )
        .unwrap(),
    )
    .unwrap();
    let third = rule::create_at(
        &mut ledger.db,
        &RuleDefinition::new(
            MatchField::Merchant,
            Pattern::new(MatchKind::Is, "Costco").unwrap(),
            AmountBounds::new(None, Some(0)).unwrap(),
            None,
            vec![RuleAction::SplitTemplate(
                SplitTemplate::new(vec![
                    SplitLine::new(key("food"), Share::new(7_000).unwrap()),
                    SplitLine::new(key("home"), Share::new(3_000).unwrap()),
                ])
                .unwrap(),
            )],
        )
        .unwrap(),
        2,
    )
    .unwrap();

    let rules = rules(&ledger.reader()).unwrap();
    let ids: Vec<RuleId> = rules.iter().map(|r| r.id).collect();
    assert_eq!(ids, [second, first, third]);
    assert!(rules.windows(2).all(|w| w[0].priority < w[1].priority));

    let rent = &rules[0];
    assert_eq!(rent.field, MatchField::Description);
    assert_eq!(rent.kind, MatchKind::Regex);
    assert_eq!(rent.pattern, "^rent");
    assert_eq!(
        (rent.amount.min(), rent.amount.max()),
        (Some(-200_000), Some(-50_000))
    );
    assert_eq!(rent.account, Some(current));
    assert_eq!(rent.currency, Some(Currency::GBP));
    assert_eq!(
        rent.actions,
        [
            ActionSummary::AddTag {
                tags: vec![
                    TagName::new("Work").unwrap(),
                    TagName::new("new-tag").unwrap()
                ],
                hidden: 1,
            },
            ActionSummary::SetNote,
            ActionSummary::Exclude(ExcludeReason::Disputed),
            ActionSummary::MyShare(Share::new(5_000).unwrap()),
        ]
    );
    let text = format!("{rules:?}");
    assert!(!text.to_lowercase().contains(&PRIVATE_TAG.to_lowercase()));
    assert!(!text.contains("Alex"));

    let uber = &rules[1];
    assert_eq!((uber.account, uber.currency), (None, None));
    assert!(uber.amount.is_any());
    assert_eq!(
        uber.actions,
        [
            ActionSummary::SetCategory(key("transport")),
            ActionSummary::SetMerchant("Uber".to_owned()),
        ]
    );

    let costco = &rules[2];
    assert_eq!(costco.amount.max(), Some(0));
    assert_eq!(
        costco.actions,
        [ActionSummary::SplitTemplate(vec![
            SplitLine::new(key("food"), Share::new(7_000).unwrap()),
            SplitLine::new(key("home"), Share::new(3_000).unwrap()),
        ])]
    );
    assert_eq!(costco.actions[0].kind(), rule::ActionKind::SplitTemplate);
}

#[test]
fn a_rule_that_no_longer_validates_is_an_error_without_its_text() {
    let mut ledger = TestLedger::new();
    let id = rule::create(
        &mut ledger.db,
        &RuleDefinition::new(
            MatchField::Merchant,
            Pattern::new(MatchKind::Contains, "x").unwrap(),
            AmountBounds::ANY,
            None,
            vec![RuleAction::SetNote(ActionText::new(NOTE_TEXT).unwrap())],
        )
        .unwrap(),
    )
    .unwrap();
    ledger
        .db
        .conn()
        .execute(
            "UPDATE rule_action SET value_json = '[\"Secret note for Alex\"]' WHERE rule_id = ?1",
            [id],
        )
        .unwrap();
    let error = rules(&ledger.reader()).unwrap_err();
    assert!(matches!(
        error,
        Error::Rule(StoreError::Invalid {
            problem: InvalidRule::Action(ActionKind::SetNote),
            ..
        })
    ));
    assert!(!error.is_invalid_input());
    assert!(!error.to_string().contains("Alex"), "{error}");
}

fn save(ledger: &mut TestLedger, title: &str, created_at: Option<&str>) -> i64 {
    let id = inbox::insert_report(
        &mut ledger.db,
        &ClientName::new("claude-code").unwrap(),
        &NewReport {
            period: "2026-10".to_owned(),
            title: title.to_owned(),
            body_markdown: format!("Body of {title}"),
        },
    )
    .unwrap();
    if let Some(at) = created_at {
        ledger
            .db
            .conn()
            .execute(
                "UPDATE report SET created_at = ?1 WHERE id = ?2",
                rusqlite::params![at, id],
            )
            .unwrap();
    }
    id
}

#[test]
fn reports_come_newest_first_and_are_capped() {
    let mut ledger = TestLedger::new();
    assert!(
        reports(&ledger.reader(), Limit::new(None))
            .unwrap()
            .is_empty()
    );

    // Inserted first, saved last.
    let newest = save(&mut ledger, "Newest", Some("2026-11-12T09:00:00.000Z"));
    let oldest = save(&mut ledger, "Oldest", Some("2026-10-01T09:00:00.000Z"));
    // Two in the same millisecond: the later insert first.
    let tie_a = save(&mut ledger, "Tie A", Some("2026-11-01T09:00:00.000Z"));
    let tie_b = save(&mut ledger, "Tie B", Some("2026-11-01T09:00:00.000Z"));

    let all = reports(&ledger.reader(), Limit::new(None)).unwrap();
    assert!(!all.truncated);
    let ids: Vec<i64> = all.items.iter().map(|r| r.id).collect();
    assert_eq!(ids, [newest, tie_b, tie_a, oldest]);
    let first = &all.items[0];
    assert_eq!(
        (
            first.period.as_str(),
            first.title.as_str(),
            first.body_markdown.as_str(),
            first.source_client.as_str(),
            first.created_at.as_str()
        ),
        (
            "2026-10",
            "Newest",
            "Body of Newest",
            "claude-code",
            "2026-11-12T09:00:00.000Z"
        )
    );

    let two = reports(&ledger.reader(), Limit::new(Some(2))).unwrap();
    assert!(two.truncated);
    assert_eq!(
        two.items.iter().map(|r| r.id).collect::<Vec<_>>(),
        [newest, tie_b]
    );
    let exact = reports(&ledger.reader(), Limit::new(Some(4))).unwrap();
    assert!(!exact.truncated);
    assert_eq!(exact.len(), 4);
}

#[test]
fn a_report_saved_now_has_a_timestamp() {
    let mut ledger = TestLedger::new();
    save(&mut ledger, "Now", None);
    let saved = reports(&ledger.reader(), Limit::new(None)).unwrap();
    let at = &saved.items[0].created_at;
    assert_eq!(at.len(), 24, "{at}");
    assert!(at.ends_with('Z'));
}

#[test]
fn a_report_is_read_by_id() {
    let mut ledger = TestLedger::new();
    let id = save(&mut ledger, "October", Some("2026-11-01T09:00:00.000Z"));
    let found = report(&ledger.reader(), id).unwrap().unwrap();
    assert_eq!(found.id, id);
    assert_eq!(found.title, "October");
    assert_eq!(found.body_markdown, "Body of October");
    assert_eq!(found.created_at, "2026-11-01T09:00:00.000Z");
    assert_eq!(report(&ledger.reader(), id + 1).unwrap(), None);
}
