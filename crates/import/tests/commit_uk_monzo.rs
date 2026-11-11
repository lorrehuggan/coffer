//! The whole import path on the Monzo fixture: preset → statement →
//! preview → commit, with the preset's category map, a user rule that
//! overrides it, and transfer detection, against a real database.

use std::path::Path;

use coffer_core::account::{self, AccountKind, NewAccount};
use coffer_core::category::CategoryKey;
use coffer_core::db::Database;
use coffer_core::id::AccountId;
use coffer_core::import::{self, CommitSummary, Committed};
use coffer_core::money::Currency;
use coffer_core::rule::{
    self, AmountBounds, MatchField, MatchKind, Pattern, RuleAction, RuleDefinition,
};
use coffer_core::statement::Statement;
use coffer_import::preset::Presets;
use coffer_import::{AnyImporter, SourceFile};
use rusqlite::{Connection, OpenFlags};
use tempfile::TempDir;

fn statement() -> Statement {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/presets/uk/monzo.csv");
    let presets = Presets::bundled().unwrap();
    let monzo = presets.get("uk-monzo").expect("bundled");
    monzo
        .importer()
        .import(&SourceFile::read(&path).unwrap())
        .unwrap()
}

fn commit(db: &mut Database, account: AccountId) -> Committed {
    let preview = import::preview(db, account, "monzo.csv", statement()).unwrap();
    let confirm = preview.needs_confirmation();
    import::commit(db, &preview, confirm).unwrap()
}

fn batch(committed: Committed) -> CommitSummary {
    match committed {
        Committed::Batch(summary) => summary,
        Committed::NothingNew => panic!("expected a new batch"),
    }
}

/// A read-only connection of its own to `db`'s file, for checking what
/// the commit stored.
fn reader(db: &Database) -> Connection {
    Connection::open_with_flags(db.path(), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

/// `category_source` and category key of the row with Monzo ID `n`.
fn category_of(db: &Database, n: u32) -> (Option<String>, Option<String>) {
    reader(db)
        .query_row(
            "SELECT t.category_source, c.key FROM \"transaction\" t \
             LEFT JOIN category c ON c.id = t.category_id WHERE t.external_id = ?1",
            [format!("tx_0000FAKE{n:015}")],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

fn pair(source: &str, key: &str) -> (Option<String>, Option<String>) {
    (Some(source.to_owned()), Some(key.to_owned()))
}

#[test]
fn monzo_imports_with_preset_categories_rules_and_transfers() {
    let dir = TempDir::new().unwrap();
    let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
    let current = account::create(
        &mut db,
        &NewAccount::new("Monzo", AccountKind::Current, Currency::GBP),
    )
    .unwrap();

    // The user files Bean There under takeaway, over Monzo's "Eating out",
    // but only for spending: the refund (line 7) is left to the preset.
    let rule = rule::create(
        &mut db,
        &RuleDefinition::new(
            MatchField::Merchant,
            Pattern::new(MatchKind::Is, "bean there").unwrap(),
            AmountBounds::new(None, Some(-1)).unwrap(),
            None,
            vec![RuleAction::SetCategory(
                CategoryKey::new("food.takeaway").unwrap(),
            )],
        )
        .unwrap(),
    )
    .unwrap();

    let summary = batch(commit(&mut db, current));
    assert_eq!(summary.categorized_by_rules, 2, "lines 3 and 4");
    // Every line the map covers (12 of 14), less the two the rule took.
    assert_eq!(summary.categorized_by_preset, 10);
    // Nothing pairs within one account.
    assert_eq!(summary.transfer_pairs, 0);

    assert_eq!(category_of(&db, 3), pair("rule", "food.takeaway"));
    assert_eq!(category_of(&db, 4), pair("rule", "food.takeaway"));
    assert_eq!(category_of(&db, 7), pair("preset_map", "food.eating_out"));
    assert_eq!(category_of(&db, 11), pair("preset_map", "food.groceries"));
    assert_eq!(category_of(&db, 2), pair("preset_map", "transfer.savings"));
    assert_eq!(category_of(&db, 8), (None, None), "General isn't mapped");
    assert_eq!(category_of(&db, 12), (None, None), "Transfers isn't mapped");
    let rule_ids: i64 = reader(&db)
        .query_row(
            "SELECT COUNT(*) FROM \"transaction\" WHERE rule_id = ?1",
            [rule.get()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rule_ids, 2);

    // Re-importing stores nothing, and so categorises nothing.
    assert_eq!(commit(&mut db, current), Committed::NothingNew);

    // A second account receiving line 6's 24.00 the next day: the commit
    // suggests the two as a transfer pair.
    let savings = account::create(
        &mut db,
        &NewAccount::new("Savings", AccountKind::Savings, Currency::GBP),
    )
    .unwrap();
    let mut incoming = Statement::new(coffer_core::statement::SourceFormat::Csv, Currency::GBP);
    incoming.push(
        coffer_core::statement::SourceLocation::Line(2),
        Ok(coffer_core::statement::StatementLine::new(
            chrono::NaiveDate::from_ymd_opt(2026, 10, 5).unwrap(),
            coffer_core::money::Money::new(2_400, Currency::GBP),
            "Synthetic transfer in",
        )),
    );
    let preview = import::preview(&db, savings, "savings.csv", incoming).unwrap();
    let summary = batch(import::commit(&mut db, &preview, preview.needs_confirmation()).unwrap());
    assert_eq!(summary.transfer_pairs, 1);
}
