//! Impact previews: each kind's counts and line, the wide flag, the
//! "category you set" count, refusals, and that previewing writes nothing.

use rusqlite::params;
use serde_json::json;

use super::super::testing::{Fixture, day};
use super::super::{ApproveOptions, SuggestionKind, approve, get, reject};
use super::*;
use crate::manual;
use crate::money::Currency;

/// Every table an approval can write to, as row counts.
fn snapshot(f: &Fixture) -> Vec<i64> {
    [
        "change_batch",
        "change_item",
        "rule",
        "rule_action",
        "transfer_group",
        "merchant_alias",
        "transaction_link",
        "transaction_split",
        "tag",
        "transaction_tag",
    ]
    .iter()
    .map(|table| f.count(table))
    .chain([f
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM suggestion WHERE status = 'pending'",
            [],
            |row| row.get(0),
        )
        .unwrap()])
    .collect()
}

/// A transaction's ID, category, category source, name and group.
type RowState = (
    i64,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<i64>,
);

/// Every transaction's category, source, name and group.
fn rows(f: &Fixture) -> Vec<RowState> {
    f.conn()
        .prepare(
            "SELECT id, category_id, category_source, merchant_display, transfer_group_id \
             FROM \"transaction\" ORDER BY id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// [`impact`], checking that it wrote nothing.
fn preview(f: &Fixture, id: SuggestionId) -> Impact {
    let (before, data) = (snapshot(f), rows(f));
    let impact = impact(&f.db, id).unwrap();
    assert_eq!(snapshot(f), before, "the preview wrote something");
    assert_eq!(rows(f), data, "the preview changed a transaction");
    assert_eq!(impact.id, id);
    impact
}

fn set_by_preset(f: &Fixture, transaction: TransactionId, category: &str) {
    f.conn()
        .execute(
            "UPDATE \"transaction\" SET category_id = (SELECT id FROM category WHERE key = ?1), \
             category_source = 'preset_map' WHERE id = ?2",
            params![category, transaction],
        )
        .unwrap();
}

fn rule(pattern: &str, category: &str) -> serde_json::Value {
    json!({"match_field": "merchant", "match_kind": "starts_with", "pattern": pattern,
           "actions": [{"kind": "set_category", "value": category}]})
}

// Categorisations -------------------------------------------------------------

#[test]
fn a_categorization_counts_changes_categories_you_set_and_missing_targets() {
    let mut f = Fixture::new();
    let blank = f.row(day(1), -350, "Secret cafe text", None);
    let mine = f.row(day(2), -900, "Lunch", Some("food.groceries"));
    let preset = f.row(day(3), -400, "Bakery", None);
    set_by_preset(&f, preset, "food.groceries");
    let same = f.row(day(4), -400, "Cafe", Some("food.eating_out"));
    let gone = f.row(day(5), -100, "Gone", None);
    let shop = f.row(day(6), -1000, "Market", None);
    let lines = f.split(shop, &[-600, -400]);
    let id = f.file(
        SuggestionKind::Categorization,
        json!({"items": [
            {"transaction_id": blank.get(), "category": "food.eating_out"},
            {"transaction_id": mine.get(), "category": "food.eating_out"},
            {"transaction_id": preset.get(), "category": "food.eating_out"},
            {"transaction_id": same.get(), "category": "food.eating_out"},
            {"transaction_id": gone.get(), "category": "food.eating_out"},
            {"split_id": lines[0].get(), "category": "food.eating_out"},
        ]}),
    );

    let impact = preview(&f, id);
    assert_eq!(impact.kind(), SuggestionKind::Categorization);
    assert_eq!(
        impact.detail,
        Detail::Categorization(CategorizationImpact {
            targets: 6,
            changes: 5,
            user_set: 1,
            unchanged: 1,
            missing: 0,
        })
    );
    assert_eq!(impact.blocked, None);
    assert_eq!(
        impact.to_string(),
        "sets the category of 5 targets; 1 has a category you set, which approving \
         overwrites; 1 already has it"
    );

    manual::delete(&mut f.db, gone).unwrap();
    let impact = preview(&f, id);
    let Detail::Categorization(c) = impact.detail else {
        panic!("expected a categorisation");
    };
    assert_eq!(c.missing, 1);
    let line = impact.to_string();
    assert!(line.contains("; 1 is gone"), "{line}");
    assert!(
        line.contains(&format!(
            "can't be approved now: the suggestion no longer fits: there is no transaction {gone}"
        )),
        "{line}"
    );
    assert!(!line.contains("Secret"));
}

#[test]
fn the_categorization_preview_matches_what_approving_does() {
    let mut f = Fixture::new();
    let a = f.row(day(1), -350, "A", None);
    let b = f.row(day(2), -350, "B", Some("food.groceries"));
    let id = f.file(
        SuggestionKind::Categorization,
        json!({"items": [
            {"transaction_id": a.get(), "category": "food.eating_out"},
            {"transaction_id": b.get(), "category": "food.eating_out"},
        ]}),
    );
    let Detail::Categorization(planned) = preview(&f, id).detail else {
        panic!("expected a categorisation");
    };
    let approved = approve(&mut f.db, id, ApproveOptions::default()).unwrap();
    assert_eq!(
        approved.applied,
        crate::inbox::Applied::Categorized {
            batch: approved_batch(&approved),
            transactions: planned.changes,
        }
    );
}

fn approved_batch(approved: &crate::inbox::Approved) -> Option<crate::id::ChangeBatchId> {
    match approved.applied {
        crate::inbox::Applied::Categorized { batch, .. } => batch,
        _ => None,
    }
}

// Rules ---------------------------------------------------------------------------

#[test]
fn a_rule_counts_matches_recategorisations_and_categories_you_set() {
    let mut f = Fixture::new();
    for d in 1..=3 {
        f.row(day(d), -2000, "Tesco Metro", None);
    }
    let preset = f.row(day(4), -1500, "Tesco Extra", None);
    set_by_preset(&f, preset, "shopping.clothing");
    f.row(day(5), -700, "Tesco Express", Some("home.household"));
    f.row(day(6), -900, "Tesco Metro", Some("food.groceries"));
    let split = f.row(day(7), -1000, "Tesco Metro", None);
    f.split(split, &[-500, -500]);
    f.row(day(8), -300, "Aldi", None);
    let id = f.file(SuggestionKind::Rule, rule("Tesco", "food.groceries"));

    let impact = preview(&f, id);
    let Detail::Rule(r) = &impact.detail else {
        panic!("expected a rule");
    };
    assert_eq!(r.matches, 7);
    assert_eq!(r.recategorises, 4);
    assert_eq!(r.changes, 4);
    assert_eq!(r.user_set, 1);
    assert_eq!(r.split_rows, 1);
    assert_eq!(r.merchants, 3);
    assert!(!r.moves_income);
    assert!(!r.wide);
    assert!(!impact.wide());
    assert_eq!(r.history_blocked, None);
    assert_eq!(impact.blocked, None);
    assert_eq!(
        impact.to_string(),
        "matches 7 past transactions across 3 merchants; recategorises 4 past transactions \
         if applied to history; 1 has a category you set (left alone); 1 split ones are skipped"
    );

    // What the preview counted is what the apply does.
    let approved = approve(
        &mut f.db,
        id,
        ApproveOptions {
            apply_to_history: true,
        },
    )
    .unwrap();
    let crate::inbox::Applied::Rule {
        history: Some(batch),
        ..
    } = approved.applied
    else {
        panic!("expected a history batch");
    };
    assert_eq!(crate::change::get(&f.db, batch).unwrap().transactions, 4);
}

#[test]
fn a_rule_over_more_than_five_merchants_is_wide() {
    let mut f = Fixture::new();
    for (d, name) in ["Shop A", "Shop B", "Shop C", "Shop D", "Shop E"]
        .into_iter()
        .enumerate()
    {
        f.row(day(d as u32 + 1), -100, name, None);
    }
    let id = f.file(SuggestionKind::Rule, rule("Shop", "food.groceries"));
    let Detail::Rule(r) = preview(&f, id).detail else {
        panic!("expected a rule");
    };
    assert_eq!(r.merchants, 5);
    assert!(!r.wide);

    f.row(day(9), -100, "Shop F", None);
    let impact = preview(&f, id);
    assert!(impact.wide());
    let Detail::Rule(r) = &impact.detail else {
        panic!("expected a rule");
    };
    assert_eq!(r.merchants, 6);
    assert!(!r.moves_income);
    assert!(
        impact
            .to_string()
            .ends_with("; wide: more than 5 merchants"),
        "{impact}"
    );
}

#[test]
fn a_rule_that_moves_income_into_spending_is_wide() {
    let mut f = Fixture::new();
    f.row(day(1), -2000, "Acme", None);
    let id = f.file(SuggestionKind::Rule, rule("Acme", "food.groceries"));
    assert!(!preview(&f, id).wide());

    // Money in it matches.
    f.row(day(2), 250_000, "Acme payroll", None);
    let impact = preview(&f, id);
    let Detail::Rule(r) = &impact.detail else {
        panic!("expected a rule");
    };
    assert!(r.moves_income);
    assert!(r.wide);
    assert!(
        impact
            .to_string()
            .ends_with("; wide: it moves income into spending"),
        "{impact}"
    );

    // An income rule doesn't move income into spending.
    reject(&mut f.db, id, None).unwrap();
    let income = f.file(SuggestionKind::Rule, rule("Acme", "income.salary"));
    assert!(!preview(&f, income).wide());
}

#[test]
fn a_rule_matching_a_row_in_an_income_category_moves_income() {
    let mut f = Fixture::new();
    // Money out filed under income (a correction, say).
    f.row(day(1), -100, "Acme", Some("income.salary"));
    let id = f.file(SuggestionKind::Rule, rule("Acme", "food.groceries"));
    let Detail::Rule(r) = preview(&f, id).detail else {
        panic!("expected a rule");
    };
    assert!(r.moves_income);
    assert_eq!(r.user_set, 1);
    assert_eq!(r.recategorises, 0);
}

// Transfers, names and links -------------------------------------------------------

#[test]
fn a_transfer_shows_the_amount_and_both_accounts() {
    let mut f = Fixture::new();
    let out = f.row(day(2), -5000, "To savings", None);
    let inn = f.row_on(f.savings, day(2), 5000, "From current", None);
    let id = f.file(
        SuggestionKind::TransferPair,
        json!({"outflow_id": out.get(), "inflow_id": inn.get()}),
    );
    let impact = preview(&f, id);
    assert_eq!(
        impact.detail,
        Detail::TransferPair(TransferImpact {
            amount: Money::new(5000, Currency::GBP),
            from_account: f.account,
            to_account: f.savings,
            suggested: false,
        })
    );
    assert_eq!(
        impact.to_string(),
        format!(
            "marks 50.00 GBP from account {} to account {} as a transfer: both rows leave \
             spending and income",
            f.account, f.savings
        )
    );
}

#[test]
fn a_transfer_with_a_side_gone_has_no_preview() {
    let mut f = Fixture::new();
    let out = f.row(day(2), -5000, "To savings", None);
    let inn = f.row_on(f.savings, day(2), 5000, "From current", None);
    let id = f.file(
        SuggestionKind::TransferPair,
        json!({"outflow_id": out.get(), "inflow_id": inn.get()}),
    );
    manual::delete(&mut f.db, out).unwrap();
    assert!(matches!(
        impact(&f.db, id),
        Err(ApproveError::Invalid(crate::inbox::Invalid::TransactionNotFound(row))) if row == out
    ));
}

#[test]
fn an_alias_counts_renames_and_says_names_are_cosmetic() {
    let mut f = Fixture::new();
    f.row(day(1), -350, "COFFEE", None);
    f.row(day(2), -350, "coffee", None);
    f.row(day(3), -350, "Tea", None);
    let id = f.file(
        SuggestionKind::MerchantAlias,
        json!({"pattern": "COFFEE", "display_name": "Corner Café"}),
    );
    let impact = preview(&f, id);
    assert_eq!(
        impact.detail,
        Detail::MerchantAlias(AliasImpact { renames: 2 })
    );
    assert_eq!(
        impact.to_string(),
        "renames 2 transactions; names are cosmetic and don't change which rules match"
    );
    assert!(!impact.to_string().contains("Corner"));

    // A duplicate is blocked, and the line says so without the name.
    let Payload::MerchantAlias(alias) = get(&f.db, id).unwrap().payload else {
        unreachable!();
    };
    crate::merchant::add(&mut f.db, &alias).unwrap();
    let impact = preview(&f, id);
    assert_eq!(
        impact.detail,
        Detail::MerchantAlias(AliasImpact { renames: 0 })
    );
    let blocked = impact.blocked.as_deref().unwrap();
    assert!(blocked.contains("already matches this text"), "{blocked}");
    assert!(!impact.to_string().contains("COFFEE"));
}

#[test]
fn a_refund_link_shows_the_amount_and_whats_left() {
    let mut f = Fixture::new();
    let shoes = f.row(day(3), -6000, "Shoes", None);
    let back = f.row(day(9), 4000, "Shoes", None);
    let id = f.file(
        SuggestionKind::RefundLink,
        json!({"refund_id": back.get(), "purchase_id": shoes.get(), "link_kind": "refund"}),
    );
    let impact = preview(&f, id);
    assert_eq!(
        impact.detail,
        Detail::RefundLink(LinkImpact {
            kind: LinkKind::Refund,
            amount: Money::new(4000, Currency::GBP),
            refund_left: Money::new(0, Currency::GBP),
            purchase_left: Money::new(2000, Currency::GBP),
        })
    );
    assert_eq!(
        impact.to_string(),
        "links 40.00 GBP as a refund; 0.00 GBP left on the money in, 20.00 GBP left on the \
         purchase"
    );

    let explicit = f.file(
        SuggestionKind::RefundLink,
        json!({"refund_id": back.get(), "purchase_id": shoes.get(), "link_kind": "refund",
               "amount_minor": 1500}),
    );
    let Detail::RefundLink(l) = preview(&f, explicit).detail else {
        panic!("expected a link");
    };
    assert_eq!(l.amount.minor(), 1500);
    assert_eq!(l.refund_left.minor(), 2500);
    assert_eq!(l.purchase_left.minor(), 4500);
}

#[test]
fn a_decided_suggestion_has_no_preview() {
    let mut f = Fixture::new();
    let coffee = f.row(day(1), -350, "Coffee", None);
    let id = f.file(
        SuggestionKind::Categorization,
        json!({"items": [{"transaction_id": coffee.get(), "category": "food.eating_out"}]}),
    );
    reject(&mut f.db, id, None).unwrap();
    assert!(matches!(
        impact(&f.db, id),
        Err(ApproveError::NotPending { .. })
    ));
}
