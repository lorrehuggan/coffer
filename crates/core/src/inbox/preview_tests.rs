//! The Inbox's row-by-row preview: each kind's lines, the rows left alone,
//! the spending change, what decides a rule's matches today, what a rule
//! settles, a wide rule by merchant, and that previewing writes nothing.

use rusqlite::params;
use serde_json::json;

use super::super::testing::{Fixture, day, key};
use super::super::{ApproveOptions, SuggestionKind, approve, counts, reject, reject_all};
use super::*;
use crate::merchant::DisplayName;
use crate::money::Currency;

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

/// `transaction` filed by stored rule `rule` in `category`.
fn set_by_rule(f: &Fixture, transaction: TransactionId, rule: RuleId, category: &str) {
    f.conn()
        .execute(
            "UPDATE \"transaction\" SET category_id = (SELECT id FROM category WHERE key = ?1), \
             category_source = 'rule', rule_id = ?2 WHERE id = ?3",
            params![category, rule, transaction],
        )
        .unwrap();
}

fn rule_json(field: &str, kind: &str, pattern: &str, category: &str) -> serde_json::Value {
    json!({"match_field": field, "match_kind": kind, "pattern": pattern,
           "actions": [{"kind": "set_category", "value": category}]})
}

/// A transaction's ID, category, category source and name.
type RowState = (i64, Option<i64>, Option<String>, Option<String>);

/// Every transaction's category, source and name: what a preview must not
/// change.
fn state(f: &Fixture) -> Vec<RowState> {
    f.conn()
        .prepare(
            "SELECT id, category_id, category_source, merchant_display FROM \"transaction\" \
             ORDER BY id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// [`preview`], checking that it wrote nothing.
fn checked(f: &Fixture, id: SuggestionId) -> Preview {
    let before = (state(f), f.count("rule"), f.count("change_batch"));
    let preview = preview(&f.db, id).unwrap();
    assert_eq!(
        (state(f), f.count("rule"), f.count("change_batch")),
        before,
        "the preview wrote something"
    );
    preview
}

fn label(after: &After) -> String {
    match after {
        After::Category(category) => category.label.clone(),
        After::Name(name) => name.clone(),
        other => format!("{other:?}"),
    }
}

#[test]
fn a_rule_lists_its_matches_now_and_after_and_keeps_what_you_set() {
    let mut f = Fixture::new();
    let uber = crate::rule::create(
        &mut f.db,
        &crate::rule::RuleDefinition::new(
            MatchField::Merchant,
            Pattern::new(MatchKind::StartsWith, "UBER").unwrap(),
            crate::rule::AmountBounds::default(),
            None,
            vec![crate::rule::RuleAction::SetCategory(key("transport.taxi"))],
        )
        .unwrap(),
    )
    .unwrap();
    let a = f.row(day(1), -1840, "UBER *EATS HELP", None);
    let b = f.row(day(2), -2215, "UBER *EATS HELP", None);
    set_by_rule(&f, a, uber, "transport.taxi");
    set_by_rule(&f, b, uber, "transport.taxi");
    let mine = f.row(day(3), -2310, "UBER *EATS HELP", Some("food.groceries"));
    let fresh = f.row(day(4), -1000, "uber *eats help", None);
    f.row(day(5), -500, "Uber trip", None);
    // A categorisation of `a` that the rule would settle.
    let settled = f.file(
        SuggestionKind::Categorization,
        json!({"items": [{"transaction_id": a.get(), "category": "food.eating_out"}]}),
    );
    let id = f.file(
        SuggestionKind::Rule,
        rule_json("merchant", "contains", "UBER *EATS", "food.eating_out"),
    );

    let p = checked(&f, id);
    assert_eq!(p.matches, 4);
    assert_eq!(p.kept, 1);
    // Newest first.
    let lines: Vec<(TransactionId, String)> = p
        .lines
        .iter()
        .map(|line| (line.transaction.id, label(&line.after)))
        .collect();
    assert_eq!(
        lines,
        vec![
            (fresh, "Eating out".to_owned()),
            (mine, "Stays".to_owned()),
            (b, "Eating out".to_owned()),
            (a, "Eating out".to_owned()),
        ]
    );
    // The match is found in the bank text, whatever its case.
    let fresh_line = &p.lines[0];
    assert_eq!(
        &fresh_line.transaction.bank_text[fresh_line.highlight.clone().unwrap()],
        "uber *eats"
    );
    assert!(matches!(
        &p.lines[2].transaction.now,
        Now::Category { set_by: SetBy::Rule(Some(rule)), .. } if *rule == uber
    ));
    // Only Rule UBER files them today.
    assert!(matches!(&p.deciding, Some(Deciding::Rule(d)) if d.pattern().as_str() == "UBER"));
    assert_eq!(
        p.before_rule.as_ref().map(|d| d.pattern().as_str()),
        Some("UBER")
    );
    // Transport loses two, uncategorised one, eating out gains three.
    let spending: Vec<(Option<&str>, i64)> = p
        .spending
        .iter()
        .map(|c| {
            (
                c.category.as_ref().map(|c| c.key.as_str()),
                c.change.minor(),
            )
        })
        .collect();
    assert_eq!(
        spending,
        vec![
            (Some("transport.taxi"), -4055),
            (None, -1000),
            (Some("food.eating_out"), 5055),
        ]
    );
    assert_eq!(p.settles, vec![settled]);
    assert!(p.by_merchant.is_none());
}

#[test]
fn a_rule_with_many_matches_lists_the_newest_six() {
    let mut f = Fixture::new();
    for d in 1..=11 {
        f.row(day(d), -100, "Tesco Metro", None);
    }
    let id = f.file(
        SuggestionKind::Rule,
        rule_json("merchant", "starts_with", "Tesco", "food.groceries"),
    );
    let p = checked(&f, id);
    assert_eq!(p.matches, 11);
    assert_eq!(p.lines.len(), LIST_NEWEST);
    assert_eq!(p.lines[0].transaction.date, day(11));
    assert_eq!(p.deciding, Some(Deciding::NoRule));
}

#[test]
fn a_wide_rule_is_summed_by_merchant_income_first() {
    let mut f = Fixture::new();
    for (d, name) in ["PAY A", "PAY B", "PAY C", "PAY D", "PAY E", "PAY F"]
        .into_iter()
        .enumerate()
    {
        f.row(day(d as u32 + 1), -100, name, None);
        f.row(day(d as u32 + 1), -100, name, None);
    }
    f.row(day(20), 300_000, "ACME PAYROLL", Some("income.salary"));
    f.row(day(21), -999, "PAY ME", Some("food.groceries"));
    let id = f.file(
        SuggestionKind::Rule,
        rule_json("description", "contains", "PAY", "bills.energy"),
    );
    let p = checked(&f, id);
    assert!(p.impact.wide());
    let by = p.by_merchant.unwrap();
    assert_eq!(by.merchants, 8);
    // The salary is a category you set, so it stays: no income moves.
    let kept = by.kept.unwrap();
    assert_eq!(kept.count, 2);
    assert_eq!(kept.now, CategoryNow::Several);
    assert_eq!(kept.total, Some(gbp(300_000 - 999)));
    assert_eq!(by.shown.len(), MERCHANTS_SHOWN);
    assert_eq!(by.shown[0].merchant, "PAY A");
    assert_eq!(by.shown[0].count, 2);
    assert_eq!(by.shown[0].total, Some(gbp(-200)));
    assert_eq!(by.shown[0].now, CategoryNow::Uncategorised);
    assert!(matches!(
        &by.shown[0].after,
        Some(After::Category(c)) if c.key.as_str() == "bills.energy"
    ));
    assert_eq!(kept.after, Some(After::Stays));
    let (rest, merchants) = by.rest.unwrap();
    assert_eq!(merchants, 3);
    assert_eq!(rest.count, 6);
    assert_eq!(rest.total, Some(gbp(-600)));
}

#[test]
fn income_turning_into_spending_sorts_first() {
    let mut f = Fixture::new();
    for name in ["PAY A", "PAY B", "PAY C", "PAY D", "PAY E", "PAY F"] {
        f.row(day(1), -100, name, None);
        f.row(day(2), -100, name, None);
    }
    f.row(day(3), 250_000, "ACME PAYROLL", None);
    let id = f.file(
        SuggestionKind::Rule,
        rule_json("description", "contains", "PAY", "bills.energy"),
    );
    let by = checked(&f, id).by_merchant.unwrap();
    assert_eq!(by.shown[0].merchant, "ACME PAYROLL");
    assert!(by.shown[0].moves_income);
    assert!(by.kept.is_none());
}

#[test]
fn a_merchant_name_lists_renames_and_keeps_names_you_set() {
    let mut f = Fixture::new();
    let a = f.row(day(1), -345, "LIME*RIDE 3HX9QP", None);
    let b = f.row(day(2), -510, "LIME*RIDE 7KD2LM", None);
    let pass = f.row(day(3), -1499, "LIME*RIDE PASS", None);
    crate::merchant::set_user_name(&mut f.db, pass, &DisplayName::new("Lime pass").unwrap())
        .unwrap();
    f.row(day(4), -900, "Tesco", None);
    let id = f.file(
        SuggestionKind::MerchantAlias,
        json!({"match_kind": "prefix", "pattern": "LIME*RIDE", "display_name": "Lime"}),
    );
    let p = checked(&f, id);
    assert_eq!(p.matches, 3);
    assert_eq!(p.kept, 1);
    let lines: Vec<(TransactionId, String)> = p
        .lines
        .iter()
        .map(|line| (line.transaction.id, label(&line.after)))
        .collect();
    assert_eq!(
        lines,
        vec![
            (pass, "Stays".to_owned()),
            (b, "Lime".to_owned()),
            (a, "Lime".to_owned()),
        ]
    );
    assert_eq!(p.names_now, Some(NamesNow::Several(3)));
    assert_eq!(p.names_merged, 2);
    assert_eq!(
        &p.lines[1].transaction.bank_text[p.lines[1].highlight.clone().unwrap()],
        "LIME*RIDE"
    );
    assert!(p.spending.is_empty());
}

#[test]
fn a_categorisation_lists_its_items_and_the_rule_that_covers_it() {
    let mut f = Fixture::new();
    let uber = f.row(day(4), -1840, "UBER *EATS", Some("transport.taxi"));
    let other = f.row(day(5), -2000, "UBER *EATS", None);
    let done = f.row(day(6), -500, "Pret", Some("food.eating_out"));
    let id = f.file(
        SuggestionKind::Categorization,
        json!({"items": [
            {"transaction_id": uber.get(), "category": "food.eating_out"},
            {"transaction_id": done.get(), "category": "food.eating_out"},
        ]}),
    );
    let p = checked(&f, id);
    assert_eq!(p.matches, 2);
    assert_eq!(label(&p.lines[0].after), "Eating out");
    assert_eq!(p.lines[1].after, After::Unchanged);
    assert!(matches!(
        p.lines[0].transaction.now,
        Now::Category {
            set_by: SetBy::User,
            ..
        }
    ));
    let spending: Vec<i64> = p.spending.iter().map(|c| c.change.minor()).collect();
    assert_eq!(spending, vec![-1840, 1840]);

    // No rule covers it until one is filed; a rule leaves a category you
    // set alone, so it covers only an uncategorised row.
    assert_eq!(covered_by(&f.db, id).unwrap(), None);
    let one = f.file(
        SuggestionKind::Categorization,
        json!({"items": [{"transaction_id": other.get(), "category": "food.eating_out"}]}),
    );
    f.row(day(7), -300, "UBER *EATS", None);
    let rule = f.file(
        SuggestionKind::Rule,
        rule_json("merchant", "starts_with", "UBER", "food.eating_out"),
    );
    let covered = covered_by(&f.db, one).unwrap().unwrap();
    assert_eq!(covered.rule, rule);
    assert_eq!(covered.others, 1);
    assert_eq!(covered_by(&f.db, id).unwrap(), None);
}

#[test]
fn a_transfer_leaves_income_and_spending() {
    let mut f = Fixture::new();
    let out = f.row_on(f.savings, day(4), -25_000, "To current", None);
    let back = f.row(day(4), 25_000, "From savings", None);
    let id = f.file(
        SuggestionKind::TransferPair,
        json!({"outflow_id": out.get(), "inflow_id": back.get()}),
    );
    let p = checked(&f, id);
    let effect = p.transfer.unwrap();
    assert_eq!(effect.month, Period::containing(day(4)));
    assert_eq!(effect.income_less, Some(gbp(25_000)));
    assert_eq!(effect.spending_less, Some(gbp(25_000)));
    let Subject::TransferPair {
        outflow: Some(outflow),
        inflow: Some(inflow),
    } = &p.subject
    else {
        panic!("expected both sides");
    };
    assert_eq!(outflow.account_name, "Savings");
    assert_eq!(inflow.account_name, "Current");
}

#[test]
fn a_refund_link_moves_money_from_income_to_the_purchase() {
    let mut f = Fixture::new();
    let shirt = f.row(day(1), -5980, "Uniqlo", Some("shopping.clothing"));
    let back = f.row(day(14), 2990, "Uniqlo", None);
    let id = f.file(
        SuggestionKind::RefundLink,
        json!({"refund_id": back.get(), "purchase_id": shirt.get(), "link_kind": "refund"}),
    );
    let effect = checked(&f, id).link.unwrap();
    assert_eq!(effect.refunded, gbp(2990));
    assert_eq!(effect.income_less, Some(gbp(2990)));
    let (category, less) = effect.spending_less.unwrap();
    assert_eq!(category.unwrap().key.as_str(), "shopping.clothing");
    assert_eq!(less, gbp(2990));
}

#[test]
fn describe_works_for_decided_suggestions_and_missing_rows() {
    let mut f = Fixture::new();
    let coffee = f.row(day(1), -350, "Pret", None);
    let id = f.file(
        SuggestionKind::Categorization,
        json!({"items": [{"transaction_id": coffee.get(), "category": "food.eating_out"}]}),
    );
    approve(&mut f.db, id, ApproveOptions::default()).unwrap();
    let suggestion = crate::inbox::get(&f.db, id).unwrap();
    let Subject::Categorization(items) = describe(&f.db, &suggestion).unwrap() else {
        panic!("expected items");
    };
    assert_eq!(items[0].transaction.as_ref().unwrap().name(), "Pret");
    assert_eq!(items[0].category.as_ref().unwrap().label, "Eating out");
    // Decided: no preview.
    assert!(matches!(
        preview(&f.db, id),
        Err(ApproveError::NotPending { .. })
    ));
}

#[test]
fn counts_by_status_kind_and_source() {
    let mut f = Fixture::new();
    let a = f.row(day(1), -100, "A", None);
    let b = f.row(day(2), -100, "B", None);
    let one = f.file(
        SuggestionKind::Categorization,
        json!({"items": [{"transaction_id": a.get(), "category": "food.groceries"}]}),
    );
    let two = f.file(
        SuggestionKind::Categorization,
        json!({"items": [{"transaction_id": b.get(), "category": "food.groceries"}]}),
    );
    f.file_from(
        "codex",
        SuggestionKind::Rule,
        rule_json("merchant", "is", "A", "food.groceries"),
    )
    .unwrap();
    reject(&mut f.db, two, None).unwrap();
    let counts = counts(&f.db).unwrap();
    assert_eq!(
        (counts.pending, counts.approved, counts.rejected),
        (2, 0, 1)
    );
    assert_eq!(counts.pending_by_kind[&SuggestionKind::Categorization], 1);
    assert_eq!(counts.pending_by_kind[&SuggestionKind::Rule], 1);
    assert_eq!(counts.by_source["claude-code"], 1);
    assert_eq!(counts.by_source["codex"], 1);
    let _ = one;
}

#[test]
fn reject_all_is_all_or_nothing() {
    let mut f = Fixture::new();
    let a = f.row(day(1), -100, "A", None);
    let b = f.row(day(2), -100, "B", None);
    let one = f.file(
        SuggestionKind::Categorization,
        json!({"items": [{"transaction_id": a.get(), "category": "food.groceries"}]}),
    );
    let two = f.file(
        SuggestionKind::Categorization,
        json!({"items": [{"transaction_id": b.get(), "category": "food.groceries"}]}),
    );
    reject(&mut f.db, two, None).unwrap();
    let error = reject_all(&mut f.db, &[one, two], None).unwrap_err();
    assert!(matches!(error, ApproveError::InList { id, .. } if id == two));
    assert_eq!(
        crate::inbox::get(&f.db, one).unwrap().status,
        SuggestionStatus::Pending
    );
    assert!(matches!(
        reject_all(&mut f.db, &[one, one], None),
        Err(ApproveError::Repeated(_))
    ));
    reject_all(&mut f.db, &[one], Some("Not mine")).unwrap();
    let rejected = crate::inbox::get(&f.db, one).unwrap();
    assert_eq!(rejected.status, SuggestionStatus::Rejected);
    assert_eq!(rejected.decision_reason.as_deref(), Some("Not mine"));
}

#[test]
fn a_wide_split_rule_says_it_splits_and_counts_the_lines_spending() {
    let mut f = Fixture::new();
    for name in ["PAY A", "PAY B", "PAY C", "PAY D", "PAY E", "PAY F"] {
        f.row(day(1), -1000, name, None);
    }
    let id = f.file(
        SuggestionKind::Rule,
        json!({"match_field": "description", "match_kind": "contains", "pattern": "PAY",
               "actions": [{"kind": "split_template", "value": [
                   {"category": "food", "basis_points": 7000},
                   {"category": "home", "basis_points": 3000}]}]}),
    );
    let p = checked(&f, id);
    let by = p.by_merchant.unwrap();
    // Not "stays": every merchant is split.
    assert!(matches!(&by.shown[0].after, Some(After::Split(lines)) if lines.len() == 2));
    assert!(by.kept.is_none());
    // Uncategorised loses all 6 rows; the two lines gain 70% and 30%.
    let spending: Vec<(Option<&str>, i64)> = p
        .spending
        .iter()
        .map(|c| {
            (
                c.category.as_ref().map(|c| c.key.as_str()),
                c.change.minor(),
            )
        })
        .collect();
    assert_eq!(
        spending,
        vec![(None, -6000), (Some("food"), 4200), (Some("home"), 1800)]
    );
}

#[test]
fn a_rule_that_only_renames_says_so() {
    let mut f = Fixture::new();
    f.row(day(1), -500, "UBER TRIP", Some("transport.taxi"));
    let id = f.file(
        SuggestionKind::Rule,
        json!({"match_field": "merchant", "match_kind": "contains", "pattern": "UBER",
               "actions": [{"kind": "set_merchant", "value": "Uber"}]}),
    );
    let p = checked(&f, id);
    assert_eq!(p.lines.len(), 1);
    assert_eq!(p.lines[0].after, After::Other);
    assert!(p.lines[0].after.changes());
    assert!(p.spending.is_empty());
}
