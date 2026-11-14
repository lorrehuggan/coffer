//! Tests for the inspector's edits: each is one undoable change batch.

use chrono::NaiveDate;
use tempfile::TempDir;

use super::*;
use crate::account::{self, AccountKind, NewAccount};
use crate::category::CategoryKey;
use crate::id::AccountId;
use crate::manual::{self, ManualTransaction};
use crate::money::{Currency, Money};
use crate::transfer::{self, TransferWindow};

struct Fixture {
    db: Database,
    account: AccountId,
    _dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
        let account = account::create(
            &mut db,
            &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        Fixture {
            db,
            account,
            _dir: dir,
        }
    }

    fn manual_on(
        &mut self,
        account: AccountId,
        minor: i64,
        category: Option<&str>,
    ) -> TransactionId {
        let mut entry = ManualTransaction::new(
            account,
            NaiveDate::from_ymd_opt(2026, 11, 7).unwrap(),
            gbp(minor),
            "WAITROSE 0716 CLAPHAM JCT GB",
        );
        entry.merchant = Some("Waitrose & Partners".into());
        entry.category = category.map(key);
        manual::create(&mut self.db, &entry).unwrap()
    }

    fn manual(&mut self, minor: i64, category: Option<&str>) -> TransactionId {
        self.manual_on(self.account, minor, category)
    }

    fn category_of(&self, id: TransactionId) -> Option<i64> {
        self.db
            .conn()
            .query_row(
                "SELECT category_id FROM \"transaction\" WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn display(&self, id: TransactionId) -> (Option<String>, Option<String>) {
        self.db
            .conn()
            .query_row(
                "SELECT merchant_display, merchant_display_source FROM \"transaction\" \
                 WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
    }

    fn tag_names(&self, id: TransactionId) -> Vec<String> {
        tag::tags_of(&self.db, id)
            .unwrap()
            .into_iter()
            .map(|tag| tag.name.as_str().to_owned())
            .collect()
    }
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn key(key: &str) -> CategoryKey {
    CategoryKey::new(key).unwrap()
}

fn line(minor: i64, category: Option<&str>) -> SplitLine {
    SplitLine::new(gbp(minor), category.map(key))
}

#[test]
fn a_split_is_one_batch_and_undo_brings_the_category_back() {
    let mut fixture = Fixture::new();
    let id = fixture.manual(-5216, Some("food.groceries"));
    let groceries = fixture.category_of(id).unwrap();

    let lines = [
        line(-3866, Some("food.groceries")),
        line(-950, Some("shopping")),
        line(-400, None),
    ];
    let batch = set_split(&mut fixture.db, id, &lines).unwrap().unwrap();
    let stored = split::lines(&fixture.db, id).unwrap();
    assert_eq!(
        stored.iter().map(|line| line.amount).collect::<Vec<_>>(),
        [gbp(-3866), gbp(-950), gbp(-400)]
    );
    assert_eq!(fixture.category_of(id), None);
    // The same split again records nothing.
    assert_eq!(set_split(&mut fixture.db, id, &lines).unwrap(), None);

    change::undo(&mut fixture.db, batch).unwrap();
    assert!(split::lines(&fixture.db, id).unwrap().is_empty());
    assert_eq!(fixture.category_of(id), Some(groceries));
}

#[test]
fn editing_a_split_replaces_its_lines_and_undo_restores_them() {
    let mut fixture = Fixture::new();
    let id = fixture.manual(-5216, None);
    set_split(
        &mut fixture.db,
        id,
        &[line(-3866, Some("food.groceries")), line(-1350, None)],
    )
    .unwrap();
    let edit = set_split(
        &mut fixture.db,
        id,
        &[
            line(-3866, Some("food.groceries")),
            line(-950, Some("shopping")),
            line(-400, Some("other")),
        ],
    )
    .unwrap()
    .unwrap();
    assert_eq!(split::lines(&fixture.db, id).unwrap().len(), 3);
    change::undo(&mut fixture.db, edit).unwrap();
    let back = split::lines(&fixture.db, id).unwrap();
    assert_eq!(
        back.iter().map(|line| line.amount).collect::<Vec<_>>(),
        [gbp(-3866), gbp(-1350)]
    );
}

#[test]
fn removing_a_split_leaves_the_row_uncategorised_and_is_undoable() {
    let mut fixture = Fixture::new();
    let id = fixture.manual(-1000, Some("food"));
    assert_eq!(remove_split(&mut fixture.db, id).unwrap(), None);
    set_split(
        &mut fixture.db,
        id,
        &[line(-600, Some("food")), line(-400, None)],
    )
    .unwrap();

    let batch = remove_split(&mut fixture.db, id).unwrap().unwrap();
    assert!(split::lines(&fixture.db, id).unwrap().is_empty());
    assert_eq!(fixture.category_of(id), None);
    change::undo(&mut fixture.db, batch).unwrap();
    assert_eq!(split::lines(&fixture.db, id).unwrap().len(), 2);
}

#[test]
fn a_split_that_doesnt_add_up_or_a_transfer_is_refused_and_nothing_changes() {
    let mut fixture = Fixture::new();
    let id = fixture.manual(-1000, Some("food"));
    let food = fixture.category_of(id);
    assert!(matches!(
        set_split(&mut fixture.db, id, &[line(-600, None), line(-300, None)]),
        Err(Error::Split(split::Error::SumMismatch { .. }))
    ));
    assert!(matches!(
        set_split(
            &mut fixture.db,
            id,
            &[line(-600, Some("no.such")), line(-400, None)]
        ),
        Err(Error::Split(split::Error::UnknownCategory(1)))
    ));
    assert_eq!(fixture.category_of(id), food);
    assert!(change::list(&fixture.db).unwrap().is_empty());

    let savings = account::create(
        &mut fixture.db,
        &NewAccount::new("Savings", AccountKind::Savings, Currency::GBP),
    )
    .unwrap();
    let out = fixture.manual(-500, None);
    fixture.manual_on(savings, 500, None);
    let pairs = transfer::detect(fixture.db.conn(), &[out], TransferWindow::DEFAULT).unwrap();
    assert_eq!(pairs.len(), 1);
    assert!(matches!(
        set_split(&mut fixture.db, out, &[line(-300, None), line(-200, None)]),
        Err(Error::Split(split::Error::InTransfer(_)))
    ));
    assert!(matches!(
        set_split(
            &mut fixture.db,
            TransactionId::new(9999),
            &[line(-1, None), line(-1, None)]
        ),
        Err(Error::Split(split::Error::TransactionNotFound(_)))
    ));
}

#[test]
fn tags_are_added_created_and_removed_one_batch_each() {
    let mut fixture = Fixture::new();
    let id = fixture.manual(-5216, None);

    let (birthday, created) = add_new_tag(&mut fixture.db, id, "  Mum's birthday ", true).unwrap();
    assert!(created.is_some());
    assert!(tag::get(&fixture.db, birthday).unwrap().private);
    assert_eq!(fixture.tag_names(id), ["Mum's birthday"]);

    // The same name in another case is the same tag, keeping its privacy.
    let (same, again) = add_new_tag(&mut fixture.db, id, "MUM'S BIRTHDAY", false).unwrap();
    assert_eq!(same, birthday);
    assert_eq!(again, None);
    assert!(tag::get(&fixture.db, birthday).unwrap().private);

    let weekly = tag::create(
        &mut fixture.db,
        &TagName::new("Weekly shop").unwrap(),
        false,
    )
    .unwrap();
    let added = add_tag(&mut fixture.db, id, weekly).unwrap().unwrap();
    assert_eq!(add_tag(&mut fixture.db, id, weekly).unwrap(), None);
    assert_eq!(fixture.tag_names(id), ["Mum's birthday", "Weekly shop"]);

    let removed = remove_tag(&mut fixture.db, id, birthday).unwrap().unwrap();
    assert_eq!(remove_tag(&mut fixture.db, id, birthday).unwrap(), None);
    assert_eq!(fixture.tag_names(id), ["Weekly shop"]);

    change::undo(&mut fixture.db, removed).unwrap();
    change::undo(&mut fixture.db, added).unwrap();
    assert_eq!(fixture.tag_names(id), ["Mum's birthday"]);
    // The tag outlives the undo of the batch that linked it.
    let first = created.unwrap();
    change::undo(&mut fixture.db, first).unwrap();
    assert!(fixture.tag_names(id).is_empty());
    assert!(tag::get(&fixture.db, birthday).is_ok());

    assert!(matches!(
        add_new_tag(&mut fixture.db, id, "   ", false),
        Err(Error::TagName(_))
    ));
    assert!(matches!(
        add_tag(&mut fixture.db, id, TagId::new(9999)),
        Err(Error::Tag(tag::Error::NotFound(_)))
    ));
    // No tag is left behind by an edit of a row that isn't there.
    assert!(add_new_tag(&mut fixture.db, TransactionId::new(9999), "Orphan", false).is_err());
    assert!(
        tag::find(&fixture.db, &TagName::new("Orphan").unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn the_note_is_trimmed_cleared_when_blank_and_undoable() {
    let mut fixture = Fixture::new();
    let id = fixture.manual(-5216, None);
    let batch = set_note(&mut fixture.db, id, "  Card for Mum is in the bag too ")
        .unwrap()
        .unwrap();
    assert_eq!(
        crate::note::get(&fixture.db, id).unwrap().as_deref(),
        Some("Card for Mum is in the bag too")
    );
    assert_eq!(
        set_note(&mut fixture.db, id, "Card for Mum is in the bag too").unwrap(),
        None
    );
    assert!(matches!(
        set_note(&mut fixture.db, id, &"x".repeat(2001)),
        Err(Error::Note(_))
    ));
    let cleared = set_note(&mut fixture.db, id, " ").unwrap().unwrap();
    assert_eq!(crate::note::get(&fixture.db, id).unwrap(), None);
    change::undo(&mut fixture.db, cleared).unwrap();
    change::undo(&mut fixture.db, batch).unwrap();
    assert_eq!(crate::note::get(&fixture.db, id).unwrap(), None);
}

#[test]
fn a_typed_merchant_is_the_users_and_blank_goes_back_to_the_derived_name() {
    let mut fixture = Fixture::new();
    let id = fixture.manual(-5216, None);
    let derived = fixture.display(id);
    assert_eq!(derived.1, None);
    // Blank on a derived name changes nothing.
    assert_eq!(set_merchant(&mut fixture.db, id, "").unwrap(), None);

    let batch = set_merchant(&mut fixture.db, id, " Waitrose ")
        .unwrap()
        .unwrap();
    assert_eq!(
        fixture.display(id),
        (Some("Waitrose".into()), Some("user".into()))
    );
    let reset = set_merchant(&mut fixture.db, id, "").unwrap().unwrap();
    assert_eq!(fixture.display(id), derived);
    change::undo(&mut fixture.db, reset).unwrap();
    assert_eq!(fixture.display(id).0.as_deref(), Some("Waitrose"));
    change::undo(&mut fixture.db, batch).unwrap();
    assert_eq!(fixture.display(id), derived);
}
