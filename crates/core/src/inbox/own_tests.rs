//! Coffer's own rule suggestions after repeated recategorisations (M2.14).

use std::collections::BTreeSet;

use chrono::NaiveDate;
use rusqlite::params;

use super::super::testing::{Fixture, day, key};
use super::super::{
    COFFER_SOURCE, Filter, MAX_PENDING_PER_SOURCE, Payload, Suggestion, SuggestionKind,
    SuggestionStatus, list, reject,
};
use super::{RULE_SUGGESTION_THRESHOLD, RecategorizeError, recategorize};
use crate::change::{self, CategorySource, CategoryValue, ChangeKind, Recorder, Value};
use crate::id::{ChangeBatchId, TransactionId};
use crate::import::{self, Committed};
use crate::money::{Currency, Money};
use crate::rule::{self, AmountBounds, MatchField, MatchKind, Pattern, RuleAction, RuleDefinition};
use crate::statement::{SourceFormat, SourceLocation, Statement, StatementLine};

const COFFEE_RULE: &str = r#"{"match_field":"merchant","match_kind":"is","pattern":"Coffee","actions":[{"kind":"set_category","value":"food.eating_out"}]}"#;

/// Recategorises `rows` to `category` as the user.
fn set(f: &mut Fixture, rows: &[TransactionId], category: &str) -> Option<ChangeBatchId> {
    let items: Vec<_> = rows.iter().map(|row| (*row, key(category))).collect();
    recategorize(&mut f.db, &items).unwrap()
}

/// Records category `category` with `source` on `rows` as a batch of `kind`.
fn record(
    f: &mut Fixture,
    kind: ChangeKind,
    rows: &[TransactionId],
    category: &str,
    source: CategorySource,
) {
    let id: i64 = f
        .conn()
        .query_row(
            "SELECT id FROM category WHERE key = ?1",
            [category],
            |row| row.get(0),
        )
        .unwrap();
    let tx = f.db.conn_mut().transaction().unwrap();
    let mut recorder = Recorder::new(kind);
    for row in rows {
        recorder
            .set(
                &tx,
                *row,
                Value::Category(CategoryValue {
                    category_id: Some(id),
                    source: Some(source),
                    rule_id: None,
                }),
            )
            .unwrap();
    }
    assert!(recorder.finish(&tx).unwrap().is_some());
    tx.commit().unwrap();
}

/// Every `rule` suggestion, oldest first.
fn rule_suggestions(f: &Fixture) -> Vec<Suggestion> {
    list(
        &f.db,
        &Filter {
            status: None,
            kind: Some(SuggestionKind::Rule),
        },
    )
    .unwrap()
}

fn payload_json(suggestion: &Suggestion) -> String {
    suggestion.payload.canonical_json()
}

/// `n` uncategorised manual rows at `merchant`.
fn rows(f: &Fixture, n: u32, merchant: &str) -> Vec<TransactionId> {
    (1..=n)
        .map(|d| f.row(day(d), -350, merchant, None))
        .collect()
}

fn coffee_rule() -> RuleDefinition {
    RuleDefinition::new(
        MatchField::Merchant,
        Pattern::new(MatchKind::Is, "Coffee").unwrap(),
        AmountBounds::ANY,
        None,
        vec![RuleAction::SetCategory(key("food.eating_out"))],
    )
    .unwrap()
}

#[test]
fn the_third_identical_recategorisation_files_one_coffer_rule() {
    assert_eq!(RULE_SUGGESTION_THRESHOLD, 3);
    let mut f = Fixture::new();
    let coffee = rows(&f, 5, "Coffee");

    set(&mut f, &coffee[..1], "food.eating_out");
    set(&mut f, &coffee[1..2], "food.eating_out");
    assert!(rule_suggestions(&f).is_empty());

    set(&mut f, &coffee[2..3], "food.eating_out");
    let filed = rule_suggestions(&f);
    assert_eq!(filed.len(), 1);
    let suggestion = &filed[0];
    assert_eq!(suggestion.source_client, COFFER_SOURCE);
    assert_eq!(suggestion.status, SuggestionStatus::Pending);
    assert_eq!(payload_json(suggestion), COFFEE_RULE);
    assert_eq!(suggestion.payload, Payload::Rule(coffee_rule()));
    assert_eq!(
        suggestion.rationale.as_deref(),
        Some("You set this category on 3 transactions from this merchant.")
    );

    // The fourth is the same suggestion, still pending: nothing new.
    set(&mut f, &coffee[3..4], "food.eating_out");
    assert_eq!(rule_suggestions(&f).len(), 1);
    assert_eq!(f.count("suggestion"), 1);

    // Rejected, it isn't filed again.
    reject(&mut f.db, suggestion.id, Some("Not always")).unwrap();
    set(&mut f, &coffee[4..5], "food.eating_out");
    let after = rule_suggestions(&f);
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].status, SuggestionStatus::Rejected);
}

#[test]
fn three_rows_in_one_edit_file_at_once() {
    let mut f = Fixture::new();
    let coffee = rows(&f, 3, "Coffee");
    let batch = set(&mut f, &coffee, "food.eating_out");
    assert!(batch.is_some());
    let filed = rule_suggestions(&f);
    assert_eq!(filed.len(), 1);
    assert_eq!(payload_json(&filed[0]), COFFEE_RULE);
}

#[test]
fn categories_and_merchants_count_separately() {
    let mut f = Fixture::new();
    let coffee = rows(&f, 5, "Coffee");
    let tesco = rows(&f, 2, "Tesco");

    set(&mut f, &coffee[..2], "food.eating_out");
    set(&mut f, &coffee[2..4], "food.groceries");
    set(&mut f, &tesco, "food.eating_out");
    assert!(rule_suggestions(&f).is_empty());

    set(&mut f, &coffee[4..], "food.eating_out");
    let filed = rule_suggestions(&f);
    assert_eq!(filed.len(), 1);
    assert_eq!(payload_json(&filed[0]), COFFEE_RULE);
}

#[test]
fn setting_a_row_back_stops_it_counting() {
    let mut f = Fixture::new();
    let coffee = rows(&f, 4, "Coffee");

    set(&mut f, &coffee[..2], "food.eating_out");
    set(&mut f, &coffee[1..2], "food.groceries");
    set(&mut f, &coffee[2..3], "food.eating_out");
    assert!(rule_suggestions(&f).is_empty());

    set(&mut f, &coffee[3..4], "food.eating_out");
    assert_eq!(rule_suggestions(&f).len(), 1);
}

#[test]
fn an_undone_recategorisation_stops_counting() {
    let mut f = Fixture::new();
    let coffee = rows(&f, 4, "Coffee");

    let batch = set(&mut f, &coffee[..2], "food.eating_out").unwrap();
    change::undo(&mut f.db, batch).unwrap();
    assert_eq!(f.category_of(coffee[0]), None);

    set(&mut f, &coffee[2..3], "food.eating_out");
    set(&mut f, &coffee[3..4], "food.eating_out");
    assert!(rule_suggestions(&f).is_empty());

    set(&mut f, &coffee[..1], "food.eating_out");
    assert_eq!(rule_suggestions(&f).len(), 1);
}

#[test]
fn rows_whose_bank_merchants_differ_file_nothing() {
    let mut f = Fixture::new();
    let a = f.row(day(1), -350, "COFFEE CO", None);
    let b = f.row(day(2), -350, "BEAN BAR", None);
    let c = f.row(day(3), -350, "COFFEE CO", None);
    f.conn()
        .execute(
            "UPDATE \"transaction\" SET merchant_display = 'Corner Café'",
            [],
        )
        .unwrap();
    set(&mut f, &[a, b, c], "food.eating_out");
    assert!(rule_suggestions(&f).is_empty());
}

#[test]
fn bank_merchants_that_differ_only_in_case_share_one_pattern() {
    let mut f = Fixture::new();
    let a = f.row(day(1), -350, "Coffee", None);
    let b = f.row(day(2), -350, "COFFEE", None);
    let c = f.row(day(3), -350, "coffee  ", None);
    f.conn()
        .execute(
            "UPDATE \"transaction\" SET merchant_display = 'Corner Café'",
            [],
        )
        .unwrap();
    set(&mut f, &[a, b, c], "food.eating_out");
    let filed = rule_suggestions(&f);
    assert_eq!(filed.len(), 1);
    let Payload::Rule(rule) = &filed[0].payload else {
        panic!("expected a rule");
    };
    assert_eq!(rule.pattern().as_str(), "COFFEE");
    // The rationale never names the merchant.
    let rationale = filed[0].rationale.as_deref().unwrap();
    assert!(!rationale.to_lowercase().contains("coffee"));
    assert!(!rationale.contains("Corner"));
}

#[test]
fn the_pattern_is_the_cleaned_bank_merchant() {
    let mut f = Fixture::new();
    let coffee = rows(&f, 3, "CARD PAYMENT TO COFFEE CO 4821");
    set(&mut f, &coffee, "food.eating_out");
    let filed = rule_suggestions(&f);
    assert_eq!(filed.len(), 1);
    let Payload::Rule(rule) = &filed[0].payload else {
        panic!("expected a rule");
    };
    assert_eq!(
        rule.pattern().as_str(),
        crate::merchant::clean("CARD PAYMENT TO COFFEE CO 4821")
    );
    assert_ne!(rule.pattern().as_str(), "CARD PAYMENT TO COFFEE CO 4821");
}

#[test]
fn rows_without_a_display_merchant_or_a_bank_merchant_file_nothing() {
    let mut f = Fixture::new();
    let nameless = rows(&f, 3, "Coffee");
    f.conn()
        .execute("UPDATE \"transaction\" SET merchant_display = NULL", [])
        .unwrap();
    set(&mut f, &nameless, "food.eating_out");

    // A display name from the description alone: no rule can match it.
    let described = rows(&f, 3, "Tesco");
    for row in &described {
        f.conn()
            .execute(
                "UPDATE \"transaction\" SET merchant = NULL WHERE id = ?1",
                [row],
            )
            .unwrap();
    }
    set(&mut f, &described, "food.groceries");
    assert!(rule_suggestions(&f).is_empty());
}

#[test]
fn an_approved_suggestion_or_an_identical_rule_stops_it() {
    let mut f = Fixture::new();
    let coffee = rows(&f, 4, "Coffee");
    set(&mut f, &coffee[..3], "food.eating_out");
    let filed = rule_suggestions(&f);
    assert_eq!(filed.len(), 1);
    f.conn()
        .execute(
            "UPDATE suggestion SET status = 'approved', \
             decided_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
            [filed[0].id],
        )
        .unwrap();
    set(&mut f, &coffee[3..], "food.eating_out");
    assert_eq!(rule_suggestions(&f).len(), 1);

    let mut g = Fixture::new();
    rule::create(&mut g.db, &coffee_rule()).unwrap();
    let coffee = rows(&g, 3, "Coffee");
    set(&mut g, &coffee, "food.eating_out");
    assert!(rule_suggestions(&g).is_empty());
}

#[test]
fn a_pending_identical_suggestion_from_a_client_is_not_doubled() {
    let mut f = Fixture::new();
    let theirs = f.file(
        SuggestionKind::Rule,
        serde_json::from_str(COFFEE_RULE).unwrap(),
    );
    let coffee = rows(&f, 3, "Coffee");
    set(&mut f, &coffee, "food.eating_out");
    let filed = rule_suggestions(&f);
    assert_eq!(filed.len(), 1);
    assert_eq!(filed[0].id, theirs);
    assert_eq!(filed[0].source_client, "claude-code");
}

#[test]
fn a_full_inbox_never_fails_the_users_edit() {
    let mut f = Fixture::new();
    for n in 0..MAX_PENDING_PER_SOURCE {
        let n = i64::try_from(n).unwrap();
        f.conn()
            .execute(
                "INSERT INTO suggestion (kind, payload_json, source_client) \
                 VALUES ('transfer_pair', json_object('outflow_id', ?1, 'inflow_id', ?2), ?3)",
                params![n * 2 + 1, n * 2 + 2, COFFER_SOURCE],
            )
            .unwrap();
    }
    let coffee = rows(&f, 3, "Coffee");
    assert!(set(&mut f, &coffee, "food.eating_out").is_some());
    assert!(rule_suggestions(&f).is_empty());
    assert_eq!(f.category_of(coffee[2]).as_deref(), Some("food.eating_out"));
}

#[test]
fn other_change_kinds_and_sources_never_file() {
    let mut f = Fixture::new();
    let a = rows(&f, 3, "Coffee");
    record(
        &mut f,
        ChangeKind::DuplicateMerge,
        &a,
        "food.eating_out",
        CategorySource::User,
    );
    let b = rows(&f, 3, "Tesco");
    record(
        &mut f,
        ChangeKind::RuleApply,
        &b,
        "food.groceries",
        CategorySource::User,
    );
    let c = rows(&f, 3, "Bakery");
    record(
        &mut f,
        ChangeKind::BulkEdit,
        &c,
        "food.eating_out",
        CategorySource::AiApproved,
    );
    let d = rows(&f, 3, "Cinema");
    record(
        &mut f,
        ChangeKind::BulkEdit,
        &d,
        "food.eating_out",
        CategorySource::Rule,
    );
    assert!(rule_suggestions(&f).is_empty());

    // The rows a merge moved count once the user recategorises another.
    let more = f.row(day(9), -350, "Coffee", None);
    set(&mut f, &[more], "food.eating_out");
    assert_eq!(rule_suggestions(&f).len(), 1);
}

#[test]
fn a_rule_applied_to_history_never_files() {
    let mut f = Fixture::new();
    let coffee = rows(&f, 3, "Coffee");
    // Not the rule Coffer would suggest, so that check isn't what stops it.
    let wider = RuleDefinition::new(
        MatchField::Merchant,
        Pattern::new(MatchKind::StartsWith, "Cof").unwrap(),
        AmountBounds::ANY,
        None,
        vec![RuleAction::SetCategory(key("food.eating_out"))],
    )
    .unwrap();
    let id = rule::create(&mut f.db, &wider).unwrap();
    let preview = rule::preview_apply(&f.db, id).unwrap();
    rule::commit_apply(&mut f.db, &preview, &BTreeSet::new()).unwrap();
    for row in &coffee {
        assert_eq!(f.category_of(*row).as_deref(), Some("food.eating_out"));
    }
    assert!(rule_suggestions(&f).is_empty());
}

#[test]
fn imports_never_file_until_the_user_recategorises() {
    let mut f = Fixture::new();
    let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
    for d in 1..=3u32 {
        let mut line = StatementLine::new(
            NaiveDate::from_ymd_opt(2026, 9, d).unwrap(),
            Money::new(-350, Currency::GBP),
            "TESCO STORES 1234 LONDON",
        );
        line.merchant = Some("TESCO STORES 1234".to_owned());
        line.external_id = Some(format!("tx-{d}"));
        line.category = Some(key("food.groceries"));
        statement.push(SourceLocation::Line(u64::from(d) + 1), Ok(line));
    }
    let preview = import::preview(&f.db, f.account, "statement.csv", statement).unwrap();
    let confirm = preview.needs_confirmation();
    let Committed::Batch(_) = import::commit(&mut f.db, &preview, confirm).unwrap() else {
        panic!("expected a new batch");
    };
    let imported: Vec<TransactionId> = {
        let mut statement = f
            .conn()
            .prepare(
                "SELECT id FROM \"transaction\" WHERE category_source = 'preset_map' ORDER BY id",
            )
            .unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(imported.len(), 3);
    assert!(rule_suggestions(&f).is_empty());

    // The user confirms the preset's category: now it is theirs.
    set(&mut f, &imported, "food.groceries");
    let filed = rule_suggestions(&f);
    assert_eq!(filed.len(), 1);
    let Payload::Rule(rule) = &filed[0].payload else {
        panic!("expected a rule");
    };
    assert_eq!(rule.pattern().as_str(), "TESCO STORES");
}

#[test]
fn recategorize_refuses_an_unknown_category_and_writes_nothing() {
    let mut f = Fixture::new();
    let coffee = rows(&f, 1, "Coffee");
    let err = recategorize(&mut f.db, &[(coffee[0], key("food.nope"))]).unwrap_err();
    assert!(matches!(err, RecategorizeError::UnknownCategory(_)));
    assert_eq!(f.category_of(coffee[0]), None);
    assert_eq!(f.count("change_batch"), 0);
    // The same category again changes nothing and records nothing.
    set(&mut f, &coffee, "food.eating_out");
    assert_eq!(set(&mut f, &coffee, "food.eating_out"), None);
    assert_eq!(f.count("change_batch"), 1);
}
