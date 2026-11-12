//! Tests for manual transactions against a real database.

use chrono::{NaiveDate, NaiveTime};
use rusqlite::params;
use tempfile::TempDir;

use super::*;
use crate::account::{AccountKind, NewAccount};
use crate::import::{self, Committed};
use crate::statement::{SourceFormat, SourceLocation, Statement, StatementLine};
use crate::transfer;

/// `(origin, batch_id, external_id, fingerprint, description_normalized,
/// category_source, merchant_display)`.
type StoredRow = (
    String,
    Option<i64>,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
);

struct Fixture {
    db: Database,
    _dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("coffer.db")).unwrap();
        Fixture { db, _dir: dir }
    }

    fn account(&mut self, name: &str, kind: AccountKind) -> AccountId {
        account::create(&mut self.db, &NewAccount::new(name, kind, Currency::GBP)).unwrap()
    }

    fn sql(&self, sql: &str) {
        self.db.conn().execute_batch(sql).unwrap();
    }

    fn lock(&self, account: AccountId, d: u32) {
        self.db
            .conn()
            .execute(
                "UPDATE account SET reconciled_through = ?1 WHERE id = ?2",
                params![date_key(day(d)), account],
            )
            .unwrap();
    }

    /// Imports one row into `account` and returns its ID.
    fn imported(&mut self, account: AccountId, d: u32, minor: i64) -> TransactionId {
        let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
        statement.push(
            SourceLocation::Line(2),
            Ok(StatementLine::new(day(d), gbp(minor), "Card payment")),
        );
        let preview = import::preview(&self.db, account, "statement.csv", statement).unwrap();
        let confirm = preview.needs_confirmation();
        let Committed::Batch(summary) = import::commit(&mut self.db, &preview, confirm).unwrap()
        else {
            panic!("expected a batch");
        };
        self.db
            .conn()
            .query_row(
                "SELECT id FROM \"transaction\" WHERE batch_id = ?1",
                [summary.batch],
                |row| row.get(0),
            )
            .unwrap()
    }

    /// The row's [`StoredRow`] fields.
    fn stored(&self, id: TransactionId) -> StoredRow {
        self.db
            .conn()
            .query_row(
                "SELECT origin, batch_id, external_id, fingerprint, description_normalized, \
                 category_source, merchant_display FROM \"transaction\" WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .unwrap()
    }

    fn exists(&self, id: TransactionId) -> bool {
        self.db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM \"transaction\" WHERE id = ?1",
                [id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap()
            == 1
    }
}

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn key(key: &str) -> CategoryKey {
    CategoryKey::new(key).unwrap()
}

fn entry(account: AccountId, d: u32, minor: i64) -> ManualTransaction {
    ManualTransaction::new(account, day(d), gbp(minor), "  Market   stall ")
}

#[test]
fn origin_keys_match_the_schema() {
    assert_eq!(
        Origin::ALL.map(Origin::as_str),
        ["imported", "mirrored", "manual"]
    );
    for origin in Origin::ALL {
        assert_eq!(Origin::from_key(origin.as_str()), Some(origin));
    }
    assert_eq!(Origin::from_key("typed"), None);
}

#[test]
fn creates_reads_edits_and_deletes_on_a_cash_and_a_current_account() {
    let mut f = Fixture::new();
    for kind in [AccountKind::Cash, AccountKind::Current] {
        let account = f.account("Wallet", kind);
        let mut new = entry(account, 3, -450);
        new.time = Some(NaiveTime::from_hms_opt(14, 5, 0).unwrap());
        new.merchant = Some("Corner shop".into());
        new.category = Some(key("food.groceries"));
        let id = create(&mut f.db, &new).unwrap();

        assert_eq!(get(&f.db, id).unwrap(), new);
        let (origin, batch, external_id, fingerprint, normalized, source, display) = f.stored(id);
        assert_eq!(origin, "manual");
        assert_eq!((batch, external_id, fingerprint), (None, None, None));
        assert_eq!(normalized, "MARKET STALL");
        assert_eq!(source.as_deref(), Some("user"));
        assert_eq!(display, None);

        // Every field changes.
        let other = f.account("Other", kind);
        let edited = ManualTransaction {
            account: other,
            date: day(4),
            time: None,
            amount: gbp(1_200),
            merchant: None,
            description: "Sold a bike".into(),
            category: None,
        };
        let updated = update(&mut f.db, id, &edited).unwrap();
        assert_eq!(updated.dissolved_transfer, None);
        assert_eq!(get(&f.db, id).unwrap(), edited);
        let (_, _, _, _, normalized, source, _) = f.stored(id);
        assert_eq!(normalized, "SOLD A BIKE");
        assert_eq!(source, None);

        assert_eq!(delete(&mut f.db, id).unwrap().dissolved_transfer, None);
        assert!(!f.exists(id));
        assert!(matches!(
            get(&f.db, id),
            Err(Error::TransactionNotFound(missing)) if missing == id
        ));
        assert!(matches!(
            delete(&mut f.db, id),
            Err(Error::TransactionNotFound(_))
        ));
    }
}

#[test]
fn every_account_kind_that_holds_transactions_takes_manual_rows() {
    let mut f = Fixture::new();
    let current = f.account("Current", AccountKind::Current);
    for kind in [
        AccountKind::Savings,
        AccountKind::CreditCard,
        AccountKind::Cash,
    ] {
        let account = f.account("Account", kind);
        create(&mut f.db, &entry(account, 3, -100)).unwrap();
    }
    let mut pot = NewAccount::new("Holiday", AccountKind::Pot, Currency::GBP);
    pot.parent = Some(current);
    let pot = account::create(&mut f.db, &pot).unwrap();
    create(&mut f.db, &entry(pot, 3, 100)).unwrap();

    // Valuation accounts are refused in core, before the schema's trigger.
    for kind in [AccountKind::Asset, AccountKind::Liability] {
        let valued = f.account("Valued", kind);
        assert!(matches!(
            create(&mut f.db, &entry(valued, 3, -100)),
            Err(Error::HoldsValuations(name)) if name == kind.as_str()
        ));
        let id = create(&mut f.db, &entry(current, 3, -100)).unwrap();
        assert!(matches!(
            update(&mut f.db, id, &entry(valued, 3, -100)),
            Err(Error::HoldsValuations(_))
        ));
    }
    assert!(matches!(
        create(&mut f.db, &entry(AccountId::new(99), 3, -100)),
        Err(Error::AccountNotFound(_))
    ));
}

#[test]
fn refuses_a_bad_entry() {
    let mut f = Fixture::new();
    let account = f.account("Cash", AccountKind::Cash);

    let mut euros = entry(account, 3, -100);
    euros.amount = Money::new(-100, Currency::EUR);
    assert!(matches!(
        create(&mut f.db, &euros),
        Err(Error::CurrencyMismatch {
            account: Currency::GBP,
            amount: Currency::EUR
        })
    ));

    let mut blank = entry(account, 3, -100);
    blank.description = " \t ".into();
    assert!(matches!(
        create(&mut f.db, &blank),
        Err(Error::BlankDescription)
    ));

    let mut unknown = entry(account, 3, -100);
    unknown.category = Some(key("food.caviar"));
    assert!(matches!(
        create(&mut f.db, &unknown),
        Err(Error::UnknownCategory(k)) if k.as_str() == "food.caviar"
    ));

    let id = create(&mut f.db, &entry(account, 3, -100)).unwrap();
    assert!(matches!(
        update(&mut f.db, id, &euros),
        Err(Error::CurrencyMismatch { .. })
    ));
    assert!(matches!(
        update(&mut f.db, id, &blank),
        Err(Error::BlankDescription)
    ));
    assert_eq!(get(&f.db, id).unwrap(), entry(account, 3, -100));
    assert!(matches!(
        update(&mut f.db, TransactionId::new(99), &entry(account, 3, 1)),
        Err(Error::TransactionNotFound(_))
    ));
}

#[test]
fn imported_and_mirrored_rows_are_refused() {
    let mut f = Fixture::new();
    let account = f.account("Current", AccountKind::Current);
    let imported = f.imported(account, 3, -999);

    let refused = |result: Result<_, Error>| match result {
        Err(error @ Error::NotManual { .. }) => error,
        other => panic!("expected NotManual, got {other:?}"),
    };
    let error = refused(update(&mut f.db, imported, &entry(account, 3, -1)).map(|_| ()));
    assert!(matches!(
        error,
        Error::NotManual { id, origin: Origin::Imported } if id == imported
    ));
    assert!(error.to_string().contains("undo its import or exclude it"));
    refused(delete(&mut f.db, imported).map(|_| ()));
    refused(get(&f.db, imported).map(|_| ()));
    assert!(f.exists(imported));

    // The pot side of a pot transfer, as an import writes it.
    let mut pot = NewAccount::new("Holiday", AccountKind::Pot, Currency::GBP);
    pot.parent = Some(account);
    let pot = account::create(&mut f.db, &pot).unwrap();
    f.db.conn()
        .execute(
            "INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, currency, \
             description_raw, description_normalized, origin) \
             SELECT ?1, batch_id, date, 999, currency, description_raw, \
             description_normalized, 'mirrored' FROM \"transaction\" WHERE id = ?2",
            params![pot, imported],
        )
        .unwrap();
    let mirrored = TransactionId::new(f.db.conn().last_insert_rowid());
    assert!(matches!(
        refused(update(&mut f.db, mirrored, &entry(pot, 3, 1)).map(|_| ())),
        Error::NotManual {
            origin: Origin::Mirrored,
            ..
        }
    ));
    refused(delete(&mut f.db, mirrored).map(|_| ()));
    assert!(f.exists(mirrored));
}

#[test]
fn locked_dates_are_refused() {
    let mut f = Fixture::new();
    let account = f.account("Cash", AccountKind::Cash);
    let other = f.account("Other cash", AccountKind::Cash);
    let early = create(&mut f.db, &entry(account, 3, -100)).unwrap();
    let late = create(&mut f.db, &entry(account, 10, -100)).unwrap();
    f.lock(account, 5);

    let locked = |result: Result<(), Error>, d: u32| {
        assert!(
            matches!(result, Err(Error::Locked { date, through }) if date == day(d) && through == day(5)),
            "{result:?}"
        );
    };
    // On the lock date, as well as before it.
    locked(create(&mut f.db, &entry(account, 5, -1)).map(|_| ()), 5);
    // The old date is locked.
    locked(
        update(&mut f.db, early, &entry(account, 8, -100)).map(|_| ()),
        3,
    );
    locked(delete(&mut f.db, early).map(|_| ()), 3);
    // The new date is.
    locked(
        update(&mut f.db, late, &entry(account, 4, -100)).map(|_| ()),
        4,
    );
    // Moving a locked row to another account changes the locked one.
    locked(
        update(&mut f.db, early, &entry(other, 3, -100)).map(|_| ()),
        3,
    );
    assert_eq!(get(&f.db, early).unwrap(), entry(account, 3, -100));

    // The category is a user-facing field, editable on a locked row.
    let mut categorised = entry(account, 3, -100);
    categorised.category = Some(key("food.groceries"));
    update(&mut f.db, early, &categorised).unwrap();
    assert_eq!(get(&f.db, early).unwrap(), categorised);
    let mut amount = categorised.clone();
    amount.amount = gbp(-200);
    locked(update(&mut f.db, early, &amount).map(|_| ()), 3);

    // After the lock everything works.
    create(&mut f.db, &entry(account, 6, -1)).unwrap();
    update(&mut f.db, late, &entry(account, 6, -300)).unwrap();
    delete(&mut f.db, late).unwrap();
}

#[test]
fn closed_accounts_take_no_rows_after_closing() {
    let mut f = Fixture::new();
    let account = f.account("Old wallet", AccountKind::Cash);
    let id = create(&mut f.db, &entry(account, 9, -100)).unwrap();
    f.sql("UPDATE account SET closed_on = '2026-11-08'");

    assert!(matches!(
        create(&mut f.db, &entry(account, 9, -1)),
        Err(Error::AfterClosing { closed_on }) if closed_on == day(8)
    ));
    // On the closing day is fine.
    create(&mut f.db, &entry(account, 8, -1)).unwrap();
    // A row already after it can still be fixed without moving it later,
    // and moved back inside.
    let mut categorised = entry(account, 9, -100);
    categorised.category = Some(key("food"));
    update(&mut f.db, id, &categorised).unwrap();
    assert!(matches!(
        update(&mut f.db, id, &entry(account, 10, -100)),
        Err(Error::AfterClosing { .. })
    ));
    update(&mut f.db, id, &entry(account, 7, -100)).unwrap();
    delete(&mut f.db, id).unwrap();
}

#[test]
fn an_unchanged_category_keeps_its_source_and_a_new_one_is_the_users() {
    let mut f = Fixture::new();
    let account = f.account("Cash", AccountKind::Cash);
    let mut new = entry(account, 3, -100);
    new.category = Some(key("food.groceries"));
    let id = create(&mut f.db, &new).unwrap();
    // As if a rule had set it.
    f.sql("UPDATE \"transaction\" SET category_source = 'rule'");

    new.description = "Farm shop".into();
    update(&mut f.db, id, &new).unwrap();
    assert_eq!(f.stored(id).5.as_deref(), Some("rule"));

    new.category = Some(key("food"));
    update(&mut f.db, id, &new).unwrap();
    assert_eq!(f.stored(id).5.as_deref(), Some("user"));
    assert_eq!(get(&f.db, id).unwrap().category, Some(key("food")));
}

#[test]
fn edits_keep_the_other_user_fields() {
    let mut f = Fixture::new();
    let account = f.account("Cash", AccountKind::Cash);
    let id = create(&mut f.db, &entry(account, 3, -1_000)).unwrap();
    f.sql(
        "UPDATE \"transaction\" SET user_note = 'mine', my_share_minor = -500, \
         reimbursable = 1",
    );
    update(&mut f.db, id, &entry(account, 4, -800)).unwrap();
    let kept: (String, i64, i64) =
        f.db.conn()
            .query_row(
                "SELECT user_note, my_share_minor, reimbursable FROM \"transaction\"",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
    assert_eq!(kept, ("mine".into(), -500, 1));

    // The share must stay part of the amount.
    for minor in [-400, 500] {
        assert!(matches!(
            update(&mut f.db, id, &entry(account, 4, minor)),
            Err(Error::ShareOutsideAmount)
        ));
    }
}

#[test]
fn deleting_or_moving_money_dissolves_a_transfer_pair() {
    let mut f = Fixture::new();
    let cash = f.account("Cash", AccountKind::Cash);
    let current = f.account("Current", AccountKind::Current);
    let out = f.imported(current, 3, -2_000);
    let into = create(&mut f.db, &entry(cash, 3, 2_000)).unwrap();
    let group = transfer::record_confirmed_pair(f.db.conn(), out, into).unwrap();

    // A new date or description keeps the pair.
    let mut moved = entry(cash, 4, 2_000);
    moved.description = "Cash withdrawal".into();
    assert_eq!(
        update(&mut f.db, into, &moved).unwrap().dissolved_transfer,
        None
    );
    assert!(transfer::group_of(&f.db, into).unwrap().is_some());

    // A new amount doesn't.
    moved.amount = gbp(1_500);
    assert_eq!(
        update(&mut f.db, into, &moved).unwrap().dissolved_transfer,
        Some(group)
    );
    assert!(transfer::group_of(&f.db, into).unwrap().is_none());
    assert!(transfer::group_of(&f.db, out).unwrap().is_none());
    assert!(transfer::group(&f.db, group).is_err());

    // Deleting a paired row leaves the other side as an ordinary one.
    moved.amount = gbp(2_000);
    update(&mut f.db, into, &moved).unwrap();
    let group = transfer::record_confirmed_pair(f.db.conn(), out, into).unwrap();
    assert_eq!(
        delete(&mut f.db, into).unwrap().dissolved_transfer,
        Some(group)
    );
    assert!(f.exists(out));
    assert!(transfer::group_of(&f.db, out).unwrap().is_none());
}
