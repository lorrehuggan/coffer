//! M1 wave 3 through the public API, against a real database: merchant
//! names in the import commit (M1.23), splits (M1.20), tags and notes
//! (M1.21), manual rows and exclusions (M1.22), and undoing an import that
//! carries them.
//!
//! The statement is synthetic bank text with the noise merchant clean-up
//! removes, read by the `uk-example-current` test preset.

mod common;

use chrono::NaiveDate;
use coffer_core::account::AccountKind;
use coffer_core::category::CategoryKey;
use coffer_core::exclusion::{self, ExclusionReason};
use coffer_core::id::{AccountId, TransactionId};
use coffer_core::import;
use coffer_core::manual::{self, ManualTransaction};
use coffer_core::merchant::{self, AliasDefinition, AliasKind, DisplayName};
use coffer_core::money::{Currency, Money};
use coffer_core::note;
use coffer_core::rule::{
    self, AmountBounds, MatchField, MatchKind, Pattern, RuleAction, RuleDefinition,
};
use coffer_core::split::{self, SplitLine};
use coffer_core::statement::Statement;
use coffer_core::tag::{self, TagName};
use coffer_import::{AnyImporter, SourceFile};
use common::{Scratch, mappings};

const CSV: &str = "\
Transaction ID,Date,Time,Type,Name,Category,Amount,Currency,Local amount,Local currency,Notes,Description
tx_w3_1,01/11/2026,08:15:02,Card payment,SQ *BEAN THERE,Eating out,-3.40,GBP,-3.40,GBP,,SQ *BEAN THERE LONDON
tx_w3_2,02/11/2026,17:20:00,Card payment,CONTACTLESS CORNER SHOP CARD 1234,Groceries,-12.00,GBP,-12.00,GBP,,CORNER SHOP
tx_w3_3,03/11/2026,,Direct debit,,Bills,-45.00,GBP,-45.00,GBP,,EXAMPLE ENERGY
";

fn statement() -> Statement {
    let presets = mappings();
    let preset = presets.get("uk-example-current").expect("test preset");
    preset
        .importer()
        .import(&SourceFile::from_bytes("wave3.csv", CSV.as_bytes()))
        .unwrap()
}

fn key(key: &str) -> CategoryKey {
    CategoryKey::new(key).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn date(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, day).unwrap()
}

/// The row with source ID `id`.
fn row(scratch: &Scratch, id: &str) -> TransactionId {
    TransactionId::new(
        scratch
            .reader()
            .query_row(
                "SELECT id FROM \"transaction\" WHERE external_id = ?1",
                [id],
                |row| row.get(0),
            )
            .unwrap(),
    )
}

/// `(merchant, merchant_display, merchant_display_source)` of a row.
fn names(scratch: &Scratch, id: TransactionId) -> (Option<String>, Option<String>, Option<String>) {
    scratch
        .reader()
        .query_row(
            "SELECT merchant, merchant_display, merchant_display_source \
             FROM \"transaction\" WHERE id = ?1",
            [id.get()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap()
}

fn display(scratch: &Scratch, id: TransactionId) -> Option<String> {
    names(scratch, id).1
}

fn category(scratch: &Scratch, id: TransactionId) -> Option<String> {
    scratch
        .reader()
        .query_row(
            "SELECT c.key FROM \"transaction\" t LEFT JOIN category c ON c.id = t.category_id \
             WHERE t.id = ?1",
            [id.get()],
            |row| row.get(0),
        )
        .unwrap()
}

fn ledger_lines(scratch: &Scratch, id: TransactionId) -> Vec<(i64, Option<String>)> {
    let reader = scratch.reader();
    let mut statement = reader
        .prepare(
            "SELECT l.amount_minor, c.key FROM ledger_line l \
             LEFT JOIN category c ON c.id = l.category_id \
             WHERE l.transaction_id = ?1 ORDER BY l.line_no",
        )
        .unwrap();
    statement
        .query_map([id.get()], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn setup() -> (Scratch, AccountId) {
    let mut scratch = Scratch::new();
    let account = scratch.current("Current", Currency::GBP);
    (scratch, account)
}

#[test]
fn import_names_merchants_before_rules_and_rules_match_cleaned_text() {
    let (mut scratch, account) = setup();
    // An alias stored before the import, and a rule written against the
    // cleaned bank text (not the raw "SQ *BEAN THERE", not an alias name).
    merchant::add(
        &mut scratch.db,
        &AliasDefinition::new(
            AliasKind::Prefix,
            "corner shop",
            DisplayName::new("Corner Shop").unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    merchant::add(
        &mut scratch.db,
        &AliasDefinition::new(
            AliasKind::Exact,
            "bean there",
            DisplayName::new("Bean There Café").unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    rule::create(
        &mut scratch.db,
        &RuleDefinition::new(
            MatchField::Merchant,
            Pattern::new(MatchKind::Is, "bean there").unwrap(),
            AmountBounds::new(None, None).unwrap(),
            None,
            vec![RuleAction::SetCategory(key("food.takeaway"))],
        )
        .unwrap(),
    )
    .unwrap();

    let summary = scratch.import_batch(account, "wave3.csv", statement());
    assert_eq!(summary.transactions, 3);
    assert_eq!(summary.categorized_by_rules, 1);

    let bean = row(&scratch, "tx_w3_1");
    let shop = row(&scratch, "tx_w3_2");
    let energy = row(&scratch, "tx_w3_3");
    // The raw merchant is stored as the bank wrote it; the display name is
    // cleaned, then aliased, and derived (no source).
    assert_eq!(
        names(&scratch, bean),
        (
            Some("SQ *BEAN THERE".into()),
            Some("Bean There Café".into()),
            None
        )
    );
    assert_eq!(display(&scratch, shop).as_deref(), Some("Corner Shop"));
    // No merchant column value: the description is the source text.
    assert_eq!(display(&scratch, energy).as_deref(), Some("EXAMPLE ENERGY"));
    // The rule matched the cleaned text, over the preset's category.
    assert_eq!(category(&scratch, bean).as_deref(), Some("food.takeaway"));
    assert_eq!(category(&scratch, shop).as_deref(), Some("food.groceries"));
}

#[test]
fn aliases_rename_derived_rows_at_once_but_never_a_users_name() {
    let (mut scratch, account) = setup();
    scratch.import_batch(account, "wave3.csv", statement());
    let bean = row(&scratch, "tx_w3_1");
    let energy = row(&scratch, "tx_w3_3");
    assert_eq!(display(&scratch, bean).as_deref(), Some("BEAN THERE"));

    let (alias, changed) = merchant::add(
        &mut scratch.db,
        &AliasDefinition::new(
            AliasKind::Exact,
            "BEAN THERE",
            DisplayName::new("Bean There").unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(changed, 1);
    assert_eq!(display(&scratch, bean).as_deref(), Some("Bean There"));

    // A name the user set survives any alias.
    merchant::set_user_name(
        &mut scratch.db,
        energy,
        &DisplayName::new("Energy supplier").unwrap(),
    )
    .unwrap();
    let (_, changed) = merchant::add(
        &mut scratch.db,
        &AliasDefinition::new(
            AliasKind::Regex,
            "energy",
            DisplayName::new("Example Energy").unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(changed, 0);
    assert_eq!(
        names(&scratch, energy),
        (None, Some("Energy supplier".into()), Some("user".into()))
    );

    // Deleting the alias puts the cleaned bank text back.
    assert_eq!(merchant::delete(&mut scratch.db, alias).unwrap(), 1);
    assert_eq!(display(&scratch, bean).as_deref(), Some("BEAN THERE"));
    assert_eq!(
        names(&scratch, bean).0.as_deref(),
        Some("SQ *BEAN THERE"),
        "the raw merchant never changes"
    );
}

#[test]
fn undo_removes_splits_and_tag_links_with_the_batch() {
    let (mut scratch, account) = setup();
    let summary = scratch.import_batch(account, "wave3.csv", statement());
    let bean = row(&scratch, "tx_w3_1");
    let shop = row(&scratch, "tx_w3_2");

    let work = tag::create(&mut scratch.db, &TagName::new("Work").unwrap(), false).unwrap();
    let private = tag::create(&mut scratch.db, &TagName::new("Gift").unwrap(), true).unwrap();
    assert!(tag::add_to_transaction(&mut scratch.db, bean, work).unwrap());
    assert!(tag::add_to_transaction(&mut scratch.db, shop, private).unwrap());
    note::set(&mut scratch.db, bean, "  client coffee  ").unwrap();
    split::set(
        &mut scratch.db,
        shop,
        &[
            SplitLine::new(gbp(-800), Some(key("food.groceries"))),
            SplitLine::new(gbp(-400), Some(key("home.household"))),
        ],
    )
    .unwrap();

    // The split parent has no category; reports read its lines.
    assert_eq!(category(&scratch, shop), None);
    assert_eq!(
        ledger_lines(&scratch, shop),
        vec![
            (-800, Some("food.groceries".into())),
            (-400, Some("home.household".into()))
        ]
    );
    assert_eq!(ledger_lines(&scratch, bean).len(), 1);
    assert_eq!(
        note::get(&scratch.db, bean).unwrap().as_deref(),
        Some("client coffee")
    );
    assert_eq!(tag::public_tags_of(&scratch.db, shop).unwrap(), vec![]);

    let preview = import::undo_preview(&scratch.db, summary.batch).unwrap();
    assert_eq!(
        preview.edited, 2,
        "a tagged, noted row and a split, tagged row"
    );
    import::undo(&mut scratch.db, summary.batch).unwrap();

    assert_eq!(scratch.count("SELECT COUNT(*) FROM \"transaction\""), 0);
    assert_eq!(scratch.count("SELECT COUNT(*) FROM transaction_split"), 0);
    assert_eq!(scratch.count("SELECT COUNT(*) FROM transaction_tag"), 0);
    // The tags themselves stay.
    assert_eq!(tag::list(&scratch.db).unwrap().len(), 2);

    // Importing again brings the rows back, without the edits.
    scratch.import_batch(account, "wave3.csv", statement());
    assert_eq!(scratch.count("SELECT COUNT(*) FROM ledger_line"), 3);
}

#[test]
fn manual_rows_get_names_keep_splits_whole_and_delete_cleanly() {
    let mut scratch = Scratch::new();
    let wallet = scratch.account("Wallet", AccountKind::Cash, Currency::GBP);
    let house = scratch.account("House", AccountKind::Asset, Currency::GBP);

    let mut entry = ManualTransaction::new(wallet, date(7), gbp(-2_000), "Market");
    entry.merchant = Some("SUMUP *MARKET STALL".into());
    let id = manual::create(&mut scratch.db, &entry).unwrap();
    assert_eq!(display(&scratch, id).as_deref(), Some("MARKET STALL"));

    // Valuation accounts hold no transactions.
    let mut elsewhere = entry.clone();
    elsewhere.account = house;
    assert!(matches!(
        manual::create(&mut scratch.db, &elsewhere),
        Err(manual::Error::HoldsValuations(_))
    ));

    split::set(
        &mut scratch.db,
        id,
        &[
            SplitLine::new(gbp(-1_500), Some(key("food.groceries"))),
            SplitLine::new(gbp(-500), Some(key("entertainment.hobbies"))),
        ],
    )
    .unwrap();
    let cash = tag::create(&mut scratch.db, &TagName::new("Cash").unwrap(), false).unwrap();
    tag::add_to_transaction(&mut scratch.db, id, cash).unwrap();

    // While split: no amount change and no category of its own.
    let mut bigger = entry.clone();
    bigger.amount = gbp(-2_500);
    assert!(matches!(
        manual::update(&mut scratch.db, id, &bigger),
        Err(manual::Error::Split(_))
    ));
    let mut categorised = entry.clone();
    categorised.category = Some(key("food.groceries"));
    assert!(matches!(
        manual::update(&mut scratch.db, id, &categorised),
        Err(manual::Error::Split(_))
    ));
    // Text edits are fine, and rename the row.
    let mut renamed = entry.clone();
    renamed.merchant = Some("CONTACTLESS FARM SHOP".into());
    manual::update(&mut scratch.db, id, &renamed).unwrap();
    assert_eq!(display(&scratch, id).as_deref(), Some("FARM SHOP"));
    assert_eq!(ledger_lines(&scratch, id).len(), 2);

    manual::delete(&mut scratch.db, id).unwrap();
    assert_eq!(scratch.count("SELECT COUNT(*) FROM \"transaction\""), 0);
    assert_eq!(scratch.count("SELECT COUNT(*) FROM transaction_split"), 0);
    assert_eq!(scratch.count("SELECT COUNT(*) FROM transaction_tag"), 0);
    assert_eq!(tag::list(&scratch.db).unwrap().len(), 1);
}

#[test]
fn imported_rows_are_excluded_and_included_without_touching_source_fields() {
    let (mut scratch, account) = setup();
    scratch.import_batch(account, "wave3.csv", statement());
    let shop = row(&scratch, "tx_w3_2");
    let before = scratch.stored(account);

    // Imported rows can't be edited or deleted as manual ones.
    assert!(matches!(
        manual::delete(&mut scratch.db, shop),
        Err(manual::Error::NotManual { .. })
    ));
    assert!(matches!(
        exclusion::exclude(&mut scratch.db, shop, ExclusionReason::Other, None),
        Err(exclusion::Error::NoteRequired)
    ));
    exclusion::exclude(
        &mut scratch.db,
        shop,
        ExclusionReason::BankError,
        Some("charged twice"),
    )
    .unwrap();
    assert_eq!(
        scratch.count("SELECT COUNT(*) FROM ledger_line WHERE excluded_reason = 'bank_error'"),
        1
    );
    exclusion::include_again(&mut scratch.db, shop).unwrap();
    assert_eq!(exclusion::get(&scratch.db, shop).unwrap(), None);
    assert_eq!(scratch.stored(account), before);
}
