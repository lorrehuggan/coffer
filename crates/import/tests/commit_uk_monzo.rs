//! The whole import path on the Monzo fixture: preset → statement →
//! preview → commit, with the preset's category map, a user rule that
//! overrides it, pot transfers mirrored onto pots (M1.17), and transfer
//! detection, against a real database.

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
use coffer_core::statement::SubAccountKind;
use coffer_core::sub_account::{self, NewPot};
use coffer_core::transfer::{self, TransferStatus};
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
             LEFT JOIN category c ON c.id = t.category_id \
             WHERE t.external_id = ?1 AND t.origin = 'imported'",
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

const RAINY_DAY: &str = "pot_0000FAKEPOT00000000001";
const HOLIDAY: &str = "pot_0000FAKEPOT00000000002";

/// `(account, amount, origin, transfer group)` of every row with Monzo ID
/// `n`, parent row first.
fn sides(db: &Database, n: u32) -> Vec<(AccountId, i64, String, Option<i64>)> {
    let reader = reader(db);
    let mut statement = reader
        .prepare(
            "SELECT account_id, amount_minor, origin, transfer_group_id FROM \"transaction\" \
             WHERE external_id = ?1 ORDER BY id",
        )
        .unwrap();
    statement
        .query_map([format!("tx_0000FAKE{n:015}")], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn balance(db: &Database, account: AccountId) -> i64 {
    reader(db)
        .query_row(
            "SELECT COALESCE(SUM(amount_minor), 0) FROM \"transaction\" WHERE account_id = ?1",
            [account],
            |row| row.get(0),
        )
        .unwrap()
}

fn count(db: &Database, sql: &str) -> i64 {
    reader(db).query_row(sql, [], |row| row.get(0)).unwrap()
}

#[test]
fn monzo_pot_transfers_are_mirrored_onto_their_pots() {
    let dir = TempDir::new().unwrap();
    let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
    let mut monzo = NewAccount::new("Monzo", AccountKind::Current, Currency::GBP);
    monzo.institution = Some("Monzo".into());
    let current = account::create(&mut db, &monzo).unwrap();

    // The preview says which pots the commit creates. Rainy Day was renamed
    // Emergency Fund at the bank (line 14, the latest), and a new pot takes
    // the latest name.
    let preview = import::preview(&db, current, "monzo.csv", statement()).unwrap();
    let plan = preview.sub_accounts();
    assert_eq!(plan.mirrored_rows, 4, "lines 2, 10, 13 and 14");
    assert_eq!(
        plan.new_pots,
        [
            NewPot {
                kind: SubAccountKind::Pot,
                key: RAINY_DAY.into(),
                name: "Emergency Fund".into(),
            },
            NewPot {
                kind: SubAccountKind::Pot,
                key: HOLIDAY.into(),
                name: "Holiday Fund".into(),
            },
        ]
    );
    assert!(plan.existing_pots.is_empty() && plan.closed_pots.is_empty());

    let summary = batch(import::commit(&mut db, &preview, preview.needs_confirmation()).unwrap());
    assert_eq!(
        summary.transactions, 14,
        "the file's rows; mirrored rows apart"
    );
    assert_eq!((summary.mirrored, summary.pots_created), (4, 2));
    // The pot pairs are confirmed already, so detection suggests none.
    assert_eq!(summary.transfer_pairs, 0);

    let pots = sub_account::pots(&db, current).unwrap();
    assert_eq!(pots.len(), 2);
    let (rainy, holiday) = (&pots[0].account, &pots[1].account);
    assert_eq!(pots[0].key.as_deref(), Some(RAINY_DAY));
    assert_eq!(pots[1].key.as_deref(), Some(HOLIDAY));
    for pot in [rainy, holiday] {
        assert_eq!(pot.kind, AccountKind::Pot);
        assert_eq!(pot.parent, Some(current));
        assert_eq!(pot.currency, Currency::GBP);
        assert_eq!(pot.institution.as_deref(), Some("Monzo"));
    }
    assert_eq!(rainy.name, "Emergency Fund");

    // Each pot transfer is on both sides, opposite, in one confirmed pair.
    for (n, pot, minor) in [
        (2, rainy.id, -20_000),
        (10, rainy.id, 5_000),
        (13, holiday.id, -7_500),
        (14, rainy.id, -2_500),
    ] {
        let found = sides(&db, n);
        assert_eq!(found.len(), 2, "line {n}");
        let (parent, mirrored) = (&found[0], &found[1]);
        assert_eq!(
            (parent.0, parent.1, parent.2.as_str()),
            (current, minor, "imported")
        );
        assert_eq!(
            (mirrored.0, mirrored.1, mirrored.2.as_str()),
            (pot, -minor, "mirrored")
        );
        let group = parent.3.expect("paired");
        assert_eq!(mirrored.3, Some(group));
        let group = transfer::group(&db, coffer_core::id::TransferGroupId::new(group)).unwrap();
        assert_eq!(group.status, TransferStatus::Confirmed);
    }
    // Into Rainy Day 200.00 and 25.00, back out 50.00; into Holiday 75.00.
    assert_eq!(balance(&db, rainy.id), 17_500);
    assert_eq!(balance(&db, holiday.id), 7_500);
    // Mirrored rows share the pot transfer's category; the preset counts
    // only the file's rows.
    assert_eq!(category_of(&db, 2), pair("preset_map", "transfer.savings"));
    assert_eq!(summary.categorized_by_preset, 12);

    // Importing the file again mirrors nothing and creates no pot.
    assert_eq!(commit(&mut db, current), Committed::NothingNew);
    assert_eq!(sub_account::pots(&db, current).unwrap().len(), 2);

    // Undo takes the mirrored rows and their pairs; the pots stay, and the
    // file imported again finds them by key.
    let undo = import::undo_preview(&db, summary.batch).unwrap();
    assert_eq!(
        (undo.transactions, undo.mirrored, undo.transfers),
        (14, 4, 0)
    );
    import::undo(&mut db, summary.batch).unwrap();
    assert_eq!(count(&db, "SELECT COUNT(*) FROM \"transaction\""), 0);
    assert_eq!(count(&db, "SELECT COUNT(*) FROM transfer_group"), 0);
    assert_eq!(sub_account::pots(&db, current).unwrap().len(), 2);
    let again = batch(commit(&mut db, current));
    assert_eq!((again.mirrored, again.pots_created), (4, 0));
    assert_eq!(balance(&db, rainy.id), 17_500);
    assert_eq!(sub_account::pots(&db, current).unwrap().len(), 2);
}

#[test]
fn monzo_pot_transfers_need_an_account_that_can_have_pots() {
    let dir = TempDir::new().unwrap();
    let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
    let card = account::create(
        &mut db,
        &NewAccount::new("Card", AccountKind::CreditCard, Currency::GBP),
    )
    .unwrap();
    let error = import::preview(&db, card, "monzo.csv", statement()).unwrap_err();
    assert!(
        matches!(
            error,
            import::Error::SubAccounts(sub_account::Error::CannotHavePots { .. })
        ),
        "{error}"
    );
    assert_eq!(count(&db, "SELECT COUNT(*) FROM account"), 1);
}
