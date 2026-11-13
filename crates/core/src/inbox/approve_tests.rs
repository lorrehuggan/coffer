//! Approving suggestions: each kind's effect and decision record, settled
//! categorisations, and refusals that leave the suggestion pending with
//! nothing written.

use rusqlite::params;
use serde_json::json;

use super::super::testing::{Fixture, day};
use super::super::{SuggestionKind, SuggestionStatus, get};
use super::*;
use crate::change::{self, BatchStatus};
use crate::id::TransactionId;
use crate::link::LinkStatus;
use crate::manual;

const HISTORY: ApproveOptions = ApproveOptions {
    apply_to_history: true,
};

/// Everything approving can write to, as one comparable snapshot.
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
    ]
    .iter()
    .map(|table| f.count(table))
    .chain([f
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM suggestion WHERE status <> 'pending'",
            [],
            |row| row.get(0),
        )
        .unwrap()])
    .collect()
}

fn source_of(f: &Fixture, transaction: TransactionId) -> Option<String> {
    f.conn()
        .query_row(
            "SELECT category_source FROM \"transaction\" WHERE id = ?1",
            [transaction],
            |row| row.get(0),
        )
        .unwrap()
}

/// Sets `transaction`'s category as a rule (or the preset) would have.
fn set_by_rule(f: &Fixture, transaction: TransactionId, category: &str) {
    f.conn()
        .execute(
            "UPDATE \"transaction\" SET category_id = (SELECT id FROM category WHERE key = ?1), \
             category_source = 'preset_map' WHERE id = ?2",
            params![category, transaction],
        )
        .unwrap();
}

fn assert_pending(f: &Fixture, id: SuggestionId) {
    let stored = get(&f.db, id).unwrap();
    assert_eq!(stored.status, SuggestionStatus::Pending);
    assert_eq!(stored.decided_at, None);
    assert_eq!(stored.change_batch, None);
    assert_eq!(stored.applied, None);
}

fn categorization(items: serde_json::Value) -> serde_json::Value {
    json!({ "items": items })
}

fn grocery_rule() -> serde_json::Value {
    json!({"match_field": "merchant", "match_kind": "is", "pattern": "Tesco",
           "actions": [{"kind": "set_category", "value": "food.groceries"}]})
}

// Categorisations -------------------------------------------------------------

#[test]
fn approving_a_categorization_records_one_undoable_batch() {
    let mut f = Fixture::new();
    let coffee = f.row(day(1), -350, "Coffee", None);
    let lunch = f.row(day(2), -900, "Lunch", Some("food.groceries"));
    let already = f.row(day(3), -400, "Cafe", Some("food.eating_out"));
    let id = f.file(
        SuggestionKind::Categorization,
        categorization(json!([
            {"transaction_id": coffee.get(), "category": "food.eating_out"},
            {"transaction_id": lunch.get(), "category": "food.eating_out"},
            {"transaction_id": already.get(), "category": "food.eating_out"},
        ])),
    );

    let approved = approve(&mut f.db, id, ApproveOptions::default()).unwrap();
    let Applied::Categorized {
        batch: Some(batch),
        transactions: 2,
    } = approved.applied
    else {
        panic!("expected a batch over two rows, got {:?}", approved.applied);
    };
    assert_eq!(approved.kind, SuggestionKind::Categorization);
    assert!(approved.settled.is_empty());
    for row in [coffee, lunch, already] {
        assert_eq!(f.category_of(row).as_deref(), Some("food.eating_out"));
    }
    assert_eq!(source_of(&f, coffee).as_deref(), Some("ai_approved"));
    assert_eq!(source_of(&f, lunch).as_deref(), Some("ai_approved"));
    // Already in the category: left as the user set it.
    assert_eq!(source_of(&f, already).as_deref(), Some("user"));

    let stored = get(&f.db, id).unwrap();
    assert_eq!(stored.status, SuggestionStatus::Approved);
    assert!(stored.decided_at.is_some());
    assert_eq!(stored.change_batch, Some(batch));
    assert_eq!(stored.decision_reason, None);
    assert_eq!(
        stored.applied,
        Some(json!({"change_batch_id": batch.get(),
                    "transactions": [coffee.get(), lunch.get()]}))
    );
    let info = change::get(&f.db, batch).unwrap();
    assert_eq!(info.kind, change::ChangeKind::BulkEdit);
    assert_eq!(info.transactions, 2);

    change::undo(&mut f.db, batch).unwrap();
    assert_eq!(f.category_of(coffee), None);
    assert_eq!(f.category_of(lunch).as_deref(), Some("food.groceries"));
    assert_eq!(source_of(&f, lunch).as_deref(), Some("user"));
    assert_eq!(
        change::get(&f.db, batch).unwrap().status,
        BatchStatus::Undone
    );
}

#[test]
fn approving_split_lines_rewrites_each_transactions_lines_once() {
    let mut f = Fixture::new();
    let shop = f.row(day(1), -1000, "Market", None);
    let lines = f.split(shop, &[-600, -300, -100]);
    let id = f.file(
        SuggestionKind::Categorization,
        categorization(json!([
            {"split_id": lines[0].get(), "category": "food.groceries"},
            {"split_id": lines[2].get(), "category": "home.household"},
        ])),
    );

    let approved = approve(&mut f.db, id, ApproveOptions::default()).unwrap();
    let Applied::Categorized {
        batch: Some(batch),
        transactions: 1,
    } = approved.applied
    else {
        panic!("expected one row, got {:?}", approved.applied);
    };
    let categories: Vec<(i64, Option<String>)> = f
        .conn()
        .prepare(
            "SELECT s.amount_minor, c.key FROM transaction_split s \
             LEFT JOIN category c ON c.id = s.category_id \
             WHERE s.transaction_id = ?1 ORDER BY s.line_no",
        )
        .unwrap()
        .query_map([shop], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        categories,
        [
            (-600, Some("food.groceries".to_owned())),
            (-300, None),
            (-100, Some("home.household".to_owned())),
        ]
    );
    let items = change::items(&f.db, batch).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].field(), change::Field::Split);

    change::undo(&mut f.db, batch).unwrap();
    let uncategorised: i64 = f
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM transaction_split WHERE transaction_id = ?1 \
             AND category_id IS NULL",
            [shop],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(uncategorised, 3);
}

#[test]
fn a_categorization_that_changes_nothing_records_no_batch() {
    let mut f = Fixture::new();
    let coffee = f.row(day(1), -350, "Coffee", Some("food.eating_out"));
    let id = f.file(
        SuggestionKind::Categorization,
        categorization(json!([{"transaction_id": coffee.get(), "category": "food.eating_out"}])),
    );
    let approved = approve(&mut f.db, id, ApproveOptions::default()).unwrap();
    assert_eq!(
        approved.applied,
        Applied::Categorized {
            batch: None,
            transactions: 0
        }
    );
    let stored = get(&f.db, id).unwrap();
    assert_eq!(stored.status, SuggestionStatus::Approved);
    assert_eq!(stored.change_batch, None);
    assert_eq!(f.count("change_batch"), 0);
}

// Rules ---------------------------------------------------------------------------

#[test]
fn approving_a_rule_stores_it_first_without_touching_history() {
    let mut f = Fixture::new();
    let existing = crate::rule::create(
        &mut f.db,
        &match Payload::parse(SuggestionKind::Rule, &grocery_rule()).unwrap() {
            Payload::Rule(rule) => rule,
            _ => unreachable!(),
        },
    )
    .unwrap();
    let tesco = f.row(day(1), -2000, "Tesco", None);
    let id = f.file(
        SuggestionKind::Rule,
        json!({"match_field": "merchant", "match_kind": "is", "pattern": "Tesco",
               "actions": [{"kind": "set_category", "value": "food.eating_out"}]}),
    );

    let approved = approve(&mut f.db, id, ApproveOptions::default()).unwrap();
    let Applied::Rule {
        rule,
        history: None,
    } = approved.applied
    else {
        panic!("expected a rule, got {:?}", approved.applied);
    };
    let order: Vec<_> = crate::rule::list(&f.db)
        .unwrap()
        .iter()
        .map(|stored| stored.id())
        .collect();
    assert_eq!(order, [rule, existing]);
    assert_eq!(f.category_of(tesco), None);
    assert_eq!(f.count("change_batch"), 0);
    let stored = get(&f.db, id).unwrap();
    assert_eq!(stored.status, SuggestionStatus::Approved);
    assert_eq!(stored.change_batch, None);
    assert_eq!(
        stored.applied,
        Some(json!({"rule_id": rule.get(), "history_batch_id": null, "settled": []}))
    );
}

#[test]
fn applying_a_rule_to_history_leaves_categories_the_user_set() {
    let mut f = Fixture::new();
    let blank = f.row(day(1), -2000, "Tesco", None);
    let preset = f.row(day(2), -1500, "Tesco", None);
    set_by_rule(&f, preset, "shopping.clothing");
    let mine = f.row(day(3), -700, "Tesco", Some("home.household"));
    let other = f.row(day(4), -300, "Aldi", None);
    let id = f.file(SuggestionKind::Rule, grocery_rule());

    let approved = approve(&mut f.db, id, HISTORY).unwrap();
    let Applied::Rule {
        rule,
        history: Some(batch),
    } = approved.applied
    else {
        panic!("expected a history batch, got {:?}", approved.applied);
    };
    assert_eq!(f.category_of(blank).as_deref(), Some("food.groceries"));
    assert_eq!(f.category_of(preset).as_deref(), Some("food.groceries"));
    assert_eq!(source_of(&f, preset).as_deref(), Some("rule"));
    assert_eq!(f.category_of(mine).as_deref(), Some("home.household"));
    assert_eq!(source_of(&f, mine).as_deref(), Some("user"));
    assert_eq!(f.category_of(other), None);

    let info = change::get(&f.db, batch).unwrap();
    assert_eq!(info.kind, change::ChangeKind::RuleApply);
    assert_eq!(info.transactions, 2);
    let stored = get(&f.db, id).unwrap();
    assert_eq!(stored.change_batch, Some(batch));
    assert_eq!(
        stored.applied,
        Some(json!({"rule_id": rule.get(), "history_batch_id": batch.get(), "settled": []}))
    );
}

#[test]
fn a_rule_applied_to_history_settles_the_categorizations_it_covers() {
    let mut f = Fixture::new();
    let a = f.row(day(1), -2000, "Tesco", None);
    let b = f.row(day(2), -1500, "Tesco", None);
    let mine = f.row(day(3), -700, "Tesco", Some("home.household"));
    let aldi = f.row(day(4), -300, "Aldi", None);
    let covered = f.file(
        SuggestionKind::Categorization,
        categorization(json!([
            {"transaction_id": a.get(), "category": "food.groceries"},
            {"transaction_id": b.get(), "category": "food.groceries"},
        ])),
    );
    // Another category for a row the rule changes.
    let different = f.file(
        SuggestionKind::Categorization,
        categorization(json!([{"transaction_id": a.get(), "category": "food.eating_out"}])),
    );
    // A row the rule leaves alone (the user's category).
    let left = f.file(
        SuggestionKind::Categorization,
        categorization(json!([{"transaction_id": mine.get(), "category": "food.groceries"}])),
    );
    // Partly covered.
    let partly = f.file(
        SuggestionKind::Categorization,
        categorization(json!([
            {"transaction_id": b.get(), "category": "food.groceries"},
            {"transaction_id": aldi.get(), "category": "food.groceries"},
        ])),
    );
    let rule = f.file(SuggestionKind::Rule, grocery_rule());

    let approved = approve(&mut f.db, rule, HISTORY).unwrap();
    assert_eq!(approved.settled, [covered]);
    let Applied::Rule {
        history: Some(batch),
        ..
    } = approved.applied
    else {
        panic!("expected a history batch");
    };
    let settled = get(&f.db, covered).unwrap();
    assert_eq!(settled.status, SuggestionStatus::Approved);
    assert_eq!(settled.change_batch, Some(batch));
    assert!(settled.decided_at.is_some());
    assert_eq!(settled.applied, Some(json!({"settled_by": rule.get()})));
    for id in [different, left, partly] {
        assert_pending(&f, id);
    }
    assert_eq!(
        get(&f.db, rule).unwrap().applied.unwrap()["settled"],
        json!([covered.get()])
    );
}

#[test]
fn a_rule_stored_without_history_settles_nothing() {
    let mut f = Fixture::new();
    let a = f.row(day(1), -2000, "Tesco", None);
    let pending = f.file(
        SuggestionKind::Categorization,
        categorization(json!([{"transaction_id": a.get(), "category": "food.groceries"}])),
    );
    let rule = f.file(SuggestionKind::Rule, grocery_rule());
    let approved = approve(&mut f.db, rule, ApproveOptions::default()).unwrap();
    assert!(approved.settled.is_empty());
    assert_pending(&f, pending);
}

// Transfers -----------------------------------------------------------------------

#[test]
fn approving_a_free_pair_records_a_confirmed_transfer() {
    let mut f = Fixture::new();
    let out = f.row(day(2), -5000, "To savings", None);
    let inn = f.row_on(f.savings, day(2), 5000, "From current", None);
    let id = f.file(
        SuggestionKind::TransferPair,
        json!({"outflow_id": out.get(), "inflow_id": inn.get()}),
    );
    let approved = approve(&mut f.db, id, ApproveOptions::default()).unwrap();
    let Applied::TransferPair { group } = approved.applied else {
        panic!("expected a transfer");
    };
    let stored = crate::transfer::group(&f.db, group).unwrap();
    assert_eq!(stored.status, crate::transfer::TransferStatus::Confirmed);
    assert_eq!(stored.transactions, [out, inn]);
    let decided = get(&f.db, id).unwrap();
    assert_eq!(decided.status, SuggestionStatus::Approved);
    assert_eq!(decided.change_batch, None);
    assert_eq!(
        decided.applied,
        Some(json!({"transfer_group_id": group.get()}))
    );
}

#[test]
fn approving_detections_suggested_pair_confirms_its_group() {
    let mut f = Fixture::new();
    let out = f.row(day(2), -5000, "To savings", None);
    let inn = f.row_on(f.savings, day(2), 5000, "From current", None);
    f.conn()
        .execute(
            "INSERT INTO transfer_group (status) VALUES ('suggested')",
            [],
        )
        .unwrap();
    let suggested = f.conn().last_insert_rowid();
    f.conn()
        .execute(
            "UPDATE \"transaction\" SET transfer_group_id = ?1 WHERE id IN (?2, ?3)",
            params![suggested, out, inn],
        )
        .unwrap();
    let id = f.file(
        SuggestionKind::TransferPair,
        json!({"outflow_id": out.get(), "inflow_id": inn.get()}),
    );
    let approved = approve(&mut f.db, id, ApproveOptions::default()).unwrap();
    let Applied::TransferPair { group } = approved.applied else {
        panic!("expected a transfer");
    };
    assert_eq!(group.get(), suggested);
    assert_eq!(f.count("transfer_group"), 1);
    assert_eq!(
        crate::transfer::group(&f.db, group).unwrap().status,
        crate::transfer::TransferStatus::Confirmed
    );
}

#[test]
fn a_linked_row_is_not_made_a_transfer() {
    let mut f = Fixture::new();
    let shoes = f.row_on(f.savings, day(1), -5000, "Shoes", None);
    let out = f.row(day(2), -5000, "To savings", None);
    let inn = f.row_on(f.savings, day(2), 5000, "From current", None);
    let id = f.file(
        SuggestionKind::TransferPair,
        json!({"outflow_id": out.get(), "inflow_id": inn.get()}),
    );
    crate::link::create(
        &mut f.db,
        &crate::link::NewLink {
            kind: crate::link::LinkKind::Refund,
            inflow: inn,
            target: crate::link::LinkTarget::Purchase(shoes),
            amount: Money::new(5000, crate::money::Currency::GBP),
        },
    )
    .unwrap();
    let before = snapshot(&f);
    let error = approve(&mut f.db, id, ApproveOptions::default()).unwrap_err();
    assert!(
        matches!(error, ApproveError::Linked(row) if row == inn),
        "{error}"
    );
    assert_eq!(snapshot(&f), before);
    assert_pending(&f, id);
}

// Names and links -------------------------------------------------------------------

#[test]
fn approving_an_alias_renames_matching_rows() {
    let mut f = Fixture::new();
    let a = f.row(day(1), -350, "COFFEE", None);
    let b = f.row(day(2), -350, "coffee", None);
    f.row(day(3), -350, "Tea", None);
    let id = f.file(
        SuggestionKind::MerchantAlias,
        json!({"pattern": "COFFEE", "display_name": "Corner Café"}),
    );
    let approved = approve(&mut f.db, id, ApproveOptions::default()).unwrap();
    let Applied::MerchantAlias { alias, renamed: 2 } = approved.applied else {
        panic!("expected two renames, got {:?}", approved.applied);
    };
    for row in [a, b] {
        let name: String = f
            .conn()
            .query_row(
                "SELECT merchant_display FROM \"transaction\" WHERE id = ?1",
                [row],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(name, "Corner Café");
    }
    assert_eq!(
        get(&f.db, id).unwrap().applied,
        Some(json!({"merchant_alias_id": alias.get(), "renamed": 2}))
    );
}

#[test]
fn a_duplicate_alias_is_refused_and_stays_pending() {
    let mut f = Fixture::new();
    f.row(day(1), -350, "COFFEE", None);
    let id = f.file(
        SuggestionKind::MerchantAlias,
        json!({"pattern": "COFFEE", "display_name": "Corner Café"}),
    );
    let Payload::MerchantAlias(alias) = get(&f.db, id).unwrap().payload else {
        unreachable!();
    };
    crate::merchant::add(&mut f.db, &alias).unwrap();
    let before = snapshot(&f);
    let error = approve(&mut f.db, id, ApproveOptions::default()).unwrap_err();
    assert!(
        matches!(
            error,
            ApproveError::Merchant(crate::merchant::Error::Duplicate(_))
        ),
        "{error}"
    );
    assert!(!error.to_string().contains("COFFEE"));
    assert_eq!(snapshot(&f), before);
    assert_pending(&f, id);
}

#[test]
fn approving_a_refund_link_with_an_amount() {
    let mut f = Fixture::new();
    let shoes = f.row(day(3), -6000, "Shoes", None);
    let back = f.row(day(9), 4000, "Shoes", None);
    let id = f.file(
        SuggestionKind::RefundLink,
        json!({"refund_id": back.get(), "purchase_id": shoes.get(), "link_kind": "refund",
               "amount_minor": 2500}),
    );
    let approved = approve(&mut f.db, id, ApproveOptions::default()).unwrap();
    let Applied::RefundLink { link } = approved.applied else {
        panic!("expected a link");
    };
    let stored = crate::link::get(&f.db, link).unwrap();
    assert_eq!(stored.status, LinkStatus::Confirmed);
    assert_eq!(stored.amount.minor(), 2500);
    assert_eq!(stored.inflow, back);
    assert_eq!(
        get(&f.db, id).unwrap().applied,
        Some(json!({"link_id": link.get(), "amount_minor": 2500}))
    );
}

#[test]
fn a_refund_link_without_an_amount_takes_what_both_sides_have_left() {
    let mut f = Fixture::new();
    let shoes = f.row(day(3), -6000, "Shoes", None);
    let back = f.row(day(9), 9000, "Shoes", None);
    let earlier = f.row(day(5), 1500, "Shoes", None);
    crate::link::create(
        &mut f.db,
        &crate::link::NewLink {
            kind: crate::link::LinkKind::Refund,
            inflow: earlier,
            target: crate::link::LinkTarget::Purchase(shoes),
            amount: Money::new(1500, crate::money::Currency::GBP),
        },
    )
    .unwrap();
    let id = f.file(
        SuggestionKind::RefundLink,
        json!({"refund_id": back.get(), "purchase_id": shoes.get(), "link_kind": "refund"}),
    );
    let Applied::RefundLink { link } = approve(&mut f.db, id, ApproveOptions::default())
        .unwrap()
        .applied
    else {
        panic!("expected a link");
    };
    // The purchase had 4500 left; the refund 9000.
    assert_eq!(crate::link::get(&f.db, link).unwrap().amount.minor(), 4500);
}

#[test]
fn a_refund_link_with_nothing_left_is_refused() {
    let mut f = Fixture::new();
    let shoes = f.row(day(3), -6000, "Shoes", None);
    let back = f.row(day(9), 6000, "Shoes", None);
    let earlier = f.row(day(5), 6000, "Shoes", None);
    let id = f.file(
        SuggestionKind::RefundLink,
        json!({"refund_id": back.get(), "purchase_id": shoes.get(), "link_kind": "refund"}),
    );
    crate::link::create(
        &mut f.db,
        &crate::link::NewLink {
            kind: crate::link::LinkKind::Refund,
            inflow: earlier,
            target: crate::link::LinkTarget::Purchase(shoes),
            amount: Money::new(6000, crate::money::Currency::GBP),
        },
    )
    .unwrap();
    let before = snapshot(&f);
    let error = approve(&mut f.db, id, ApproveOptions::default()).unwrap_err();
    assert!(
        matches!(error, ApproveError::NothingToLink { refund, purchase }
            if refund == back && purchase == shoes),
        "{error}"
    );
    assert_eq!(snapshot(&f), before);
    assert_pending(&f, id);
}

// Refusals --------------------------------------------------------------------------

#[test]
fn approving_twice_is_refused() {
    let mut f = Fixture::new();
    let coffee = f.row(day(1), -350, "Coffee", None);
    let id = f.file(
        SuggestionKind::Categorization,
        categorization(json!([{"transaction_id": coffee.get(), "category": "food.eating_out"}])),
    );
    approve(&mut f.db, id, ApproveOptions::default()).unwrap();
    let before = snapshot(&f);
    let error = approve(&mut f.db, id, ApproveOptions::default()).unwrap_err();
    assert!(
        matches!(
            error,
            ApproveError::NotPending {
                status: SuggestionStatus::Approved,
                ..
            }
        ),
        "{error}"
    );
    assert_eq!(snapshot(&f), before);

    let rejected = f.file(
        SuggestionKind::Categorization,
        categorization(json!([{"transaction_id": coffee.get(), "category": "food.takeaway"}])),
    );
    reject(&mut f.db, rejected, None).unwrap();
    assert!(matches!(
        approve(&mut f.db, rejected, ApproveOptions::default()),
        Err(ApproveError::NotPending {
            status: SuggestionStatus::Rejected,
            ..
        })
    ));
}

#[test]
fn a_target_deleted_since_refuses_the_whole_approval() {
    let mut f = Fixture::new();
    let coffee = f.row(day(1), -350, "Coffee", None);
    let gone = f.row(day(2), -900, "Lunch", None);
    let id = f.file(
        SuggestionKind::Categorization,
        categorization(json!([
            {"transaction_id": coffee.get(), "category": "food.eating_out"},
            {"transaction_id": gone.get(), "category": "food.eating_out"},
        ])),
    );
    manual::delete(&mut f.db, gone).unwrap();
    let before = snapshot(&f);
    let error = approve(&mut f.db, id, ApproveOptions::default()).unwrap_err();
    assert!(
        matches!(error, ApproveError::Invalid(Invalid::TransactionNotFound(row)) if row == gone),
        "{error}"
    );
    assert_eq!(snapshot(&f), before);
    assert_eq!(f.category_of(coffee), None);
    assert_pending(&f, id);
}

#[test]
fn a_split_line_whose_transaction_was_split_again_is_refused() {
    let mut f = Fixture::new();
    let shop = f.row(day(1), -1000, "Market", None);
    let lines = f.split(shop, &[-600, -400]);
    let id = f.file(
        SuggestionKind::Categorization,
        categorization(json!([{"split_id": lines[0].get(), "category": "food.groceries"}])),
    );
    f.split(shop, &[-500, -500]);
    let before = snapshot(&f);
    let error = approve(&mut f.db, id, ApproveOptions::default()).unwrap_err();
    assert!(
        matches!(error, ApproveError::Invalid(Invalid::SplitLineNotFound(line)) if line == lines[0]),
        "{error}"
    );
    assert_eq!(snapshot(&f), before);
    assert_pending(&f, id);
}

#[test]
fn a_transaction_split_since_is_refused() {
    let mut f = Fixture::new();
    let shop = f.row(day(1), -1000, "Market", None);
    let id = f.file(
        SuggestionKind::Categorization,
        categorization(json!([{"transaction_id": shop.get(), "category": "food.groceries"}])),
    );
    f.split(shop, &[-500, -500]);
    let error = approve(&mut f.db, id, ApproveOptions::default()).unwrap_err();
    assert!(
        matches!(error, ApproveError::Invalid(Invalid::Split(row)) if row == shop),
        "{error}"
    );
    assert_pending(&f, id);
}

#[test]
fn a_transfer_side_deleted_since_is_refused() {
    let mut f = Fixture::new();
    let out = f.row(day(2), -5000, "To savings", None);
    let inn = f.row_on(f.savings, day(2), 5000, "From current", None);
    let id = f.file(
        SuggestionKind::TransferPair,
        json!({"outflow_id": out.get(), "inflow_id": inn.get()}),
    );
    manual::delete(&mut f.db, inn).unwrap();
    let before = snapshot(&f);
    let error = approve(&mut f.db, id, ApproveOptions::default()).unwrap_err();
    assert!(
        matches!(error, ApproveError::Invalid(Invalid::TransactionNotFound(row)) if row == inn),
        "{error}"
    );
    assert_eq!(snapshot(&f), before);
    assert_pending(&f, id);
}

#[test]
fn a_failure_after_the_first_write_rolls_everything_back() {
    let mut f = Fixture::new();
    let a = f.row(day(1), -2000, "Tesco", None);
    let covered = f.file(
        SuggestionKind::Categorization,
        categorization(json!([{"transaction_id": a.get(), "category": "food.groceries"}])),
    );
    let id = f.file(SuggestionKind::Rule, grocery_rule());
    // The rule is stored, applied to history and the categorisation
    // settled before marking the suggestion fails.
    f.conn()
        .execute_batch(
            "CREATE TEMP TRIGGER refuse_decision BEFORE UPDATE ON suggestion \
             BEGIN SELECT RAISE(ABORT, 'refused for the test'); END;",
        )
        .unwrap();
    let before = snapshot(&f);
    let error = approve(&mut f.db, id, HISTORY).unwrap_err();
    assert!(matches!(error, ApproveError::Database(_)), "{error}");
    assert_eq!(snapshot(&f), before);
    assert_eq!(f.category_of(a), None);
    f.conn()
        .execute_batch("DROP TRIGGER refuse_decision;")
        .unwrap();
    assert_pending(&f, id);
    assert_pending(&f, covered);
}

#[test]
fn locks_guard_balances_only_so_no_suggestion_kind_meets_one() {
    // `reconciled_through` guards exclusions only (rules decision point 8,
    // M1 wave 4 point 3), and a suggestion never excludes. Categories,
    // splits, rules, transfers, names and links all go through under a
    // lock.
    let mut f = Fixture::new();
    let coffee = f.row(day(1), -350, "Coffee", None);
    let tesco = f.row(day(2), -2000, "Tesco", None);
    let out = f.row(day(3), -5000, "To savings", None);
    let inn = f.row_on(f.savings, day(3), 5000, "From current", None);
    let shoes = f.row(day(4), -6000, "Shoes", None);
    let back = f.row(day(5), 6000, "Shoes", None);
    let ids = [
        f.file(
            SuggestionKind::Categorization,
            categorization(
                json!([{"transaction_id": coffee.get(), "category": "food.eating_out"}]),
            ),
        ),
        f.file(SuggestionKind::Rule, grocery_rule()),
        f.file(
            SuggestionKind::TransferPair,
            json!({"outflow_id": out.get(), "inflow_id": inn.get()}),
        ),
        f.file(
            SuggestionKind::MerchantAlias,
            json!({"pattern": "COFFEE", "display_name": "Corner Café"}),
        ),
        f.file(
            SuggestionKind::RefundLink,
            json!({"refund_id": back.get(), "purchase_id": shoes.get(), "link_kind": "refund"}),
        ),
    ];
    for account in [f.account, f.savings] {
        f.conn()
            .execute(
                "UPDATE account SET reconciled_through = '2026-10-31' WHERE id = ?1",
                [account],
            )
            .unwrap();
    }
    for id in ids {
        approve(&mut f.db, id, HISTORY).unwrap();
    }
    assert_eq!(f.category_of(coffee).as_deref(), Some("food.eating_out"));
    assert_eq!(f.category_of(tesco).as_deref(), Some("food.groceries"));
}

#[test]
fn lock_errors_underneath_are_reported_as_locked() {
    let error = ApproveError::from(ApplyError::Locked(vec![TransactionId::new(4)]));
    assert!(matches!(&error, ApproveError::Locked(rows) if rows == &[TransactionId::new(4)]));
    assert!(error.to_string().contains("unlock the account first"));
    let error = ApproveError::from(change::Error::Locked {
        transaction: TransactionId::new(7),
        date: day(1),
        through: day(31),
    });
    assert!(matches!(&error, ApproveError::Locked(rows) if rows == &[TransactionId::new(7)]));
}

// Several at once ---------------------------------------------------------------------

#[test]
fn approve_all_applies_every_suggestion_in_one_go() {
    let mut f = Fixture::new();
    let coffee = f.row(day(1), -350, "Coffee", None);
    let out = f.row(day(2), -5000, "To savings", None);
    let inn = f.row_on(f.savings, day(2), 5000, "From current", None);
    let categorize = f.file(
        SuggestionKind::Categorization,
        categorization(json!([{"transaction_id": coffee.get(), "category": "food.eating_out"}])),
    );
    let pair = f.file(
        SuggestionKind::TransferPair,
        json!({"outflow_id": out.get(), "inflow_id": inn.get()}),
    );
    let rule = f.file(SuggestionKind::Rule, grocery_rule());

    let approved = approve_all(
        &mut f.db,
        &[pair, categorize, rule],
        ApproveOptions::default(),
    )
    .unwrap();
    assert_eq!(
        approved.iter().map(|a| a.id).collect::<Vec<_>>(),
        [pair, categorize, rule]
    );
    assert!(matches!(approved[0].applied, Applied::TransferPair { .. }));
    assert!(matches!(
        approved[1].applied,
        Applied::Categorized {
            batch: Some(_),
            transactions: 1
        }
    ));
    assert!(matches!(
        approved[2].applied,
        Applied::Rule { history: None, .. }
    ));
    for id in [pair, categorize, rule] {
        assert_eq!(get(&f.db, id).unwrap().status, SuggestionStatus::Approved);
    }
    assert_eq!(f.category_of(coffee).as_deref(), Some("food.eating_out"));
    assert!(
        approve_all(&mut f.db, &[], ApproveOptions::default())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn approve_all_with_one_bad_id_applies_none() {
    let mut f = Fixture::new();
    let coffee = f.row(day(1), -350, "Coffee", None);
    let gone = f.row(day(2), -900, "Lunch", None);
    let good = f.file(
        SuggestionKind::Categorization,
        categorization(json!([{"transaction_id": coffee.get(), "category": "food.eating_out"}])),
    );
    let rule = f.file(SuggestionKind::Rule, grocery_rule());
    let stale = f.file(
        SuggestionKind::Categorization,
        categorization(json!([{"transaction_id": gone.get(), "category": "food.groceries"}])),
    );
    manual::delete(&mut f.db, gone).unwrap();
    let before = snapshot(&f);

    // A suggestion that no longer fits, last: the two before it roll back.
    let error = approve_all(&mut f.db, &[good, rule, stale], HISTORY).unwrap_err();
    assert!(
        matches!(&error, ApproveError::InList { id, error }
            if *id == stale
                && matches!(**error, ApproveError::Invalid(Invalid::TransactionNotFound(row))
                    if row == gone)),
        "{error}"
    );
    assert!(
        error.to_string().starts_with(&format!(
            "couldn't approve suggestion {stale}: the suggestion no longer fits"
        )),
        "{error}"
    );
    assert_eq!(snapshot(&f), before);
    assert_eq!(f.category_of(coffee), None);
    for id in [good, rule, stale] {
        assert_pending(&f, id);
    }

    // An ID that doesn't exist.
    let missing = SuggestionId::new(9_999);
    let error = approve_all(&mut f.db, &[good, missing], ApproveOptions::default()).unwrap_err();
    assert!(
        matches!(&error, ApproveError::InList { id, error }
            if *id == missing && matches!(**error, ApproveError::Inbox(Error::NotFound(_)))),
        "{error}"
    );
    assert_eq!(snapshot(&f), before);
    assert_pending(&f, good);

    // One already decided.
    reject(&mut f.db, stale, None).unwrap();
    let error = approve_all(&mut f.db, &[good, stale], ApproveOptions::default()).unwrap_err();
    assert!(
        matches!(&error, ApproveError::InList { id, error }
            if *id == stale && matches!(**error, ApproveError::NotPending { .. })),
        "{error}"
    );
    assert_pending(&f, good);

    // The same ID twice is refused before anything is tried.
    let error = approve_all(&mut f.db, &[good, rule, good], ApproveOptions::default()).unwrap_err();
    assert!(
        matches!(error, ApproveError::Repeated(id) if id == good),
        "{error}"
    );
    assert_pending(&f, good);
    assert_pending(&f, rule);
}

#[test]
fn approve_all_skips_a_suggestion_an_earlier_one_settled() {
    let mut f = Fixture::new();
    let a = f.row(day(1), -2000, "Tesco", None);
    let b = f.row(day(2), -1500, "Tesco", None);
    let covered = f.file(
        SuggestionKind::Categorization,
        categorization(json!([
            {"transaction_id": a.get(), "category": "food.groceries"},
            {"transaction_id": b.get(), "category": "food.groceries"},
        ])),
    );
    let rule = f.file(SuggestionKind::Rule, grocery_rule());

    // The rule first: it settles the categorisation, which is skipped.
    let approved = approve_all(&mut f.db, &[rule, covered], HISTORY).unwrap();
    assert_eq!(approved.len(), 1);
    assert_eq!(approved[0].id, rule);
    assert_eq!(approved[0].settled, [covered]);
    let settled = get(&f.db, covered).unwrap();
    assert_eq!(settled.status, SuggestionStatus::Approved);
    assert_eq!(settled.applied, Some(json!({"settled_by": rule.get()})));

    // The categorisation first: approved on its own, so the rule settles
    // nothing.
    let mut f = Fixture::new();
    let a = f.row(day(1), -2000, "Tesco", None);
    let first = f.file(
        SuggestionKind::Categorization,
        categorization(json!([{"transaction_id": a.get(), "category": "food.groceries"}])),
    );
    let rule = f.file(SuggestionKind::Rule, grocery_rule());
    let approved = approve_all(&mut f.db, &[first, rule], HISTORY).unwrap();
    assert_eq!(
        approved.iter().map(|a| a.id).collect::<Vec<_>>(),
        [first, rule]
    );
    assert!(approved[1].settled.is_empty());
    assert!(matches!(
        approved[0].applied,
        Applied::Categorized {
            batch: Some(_),
            transactions: 1
        }
    ));
}
