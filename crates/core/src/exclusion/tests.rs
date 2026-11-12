//! Tests for excluding and including transactions against a real database.

use chrono::NaiveDate;
use rusqlite::params;
use tempfile::TempDir;

use super::*;
use crate::account::{AccountKind, EnteredBalance, EnteredSource, NewAccount, date_key};
use crate::balance::CheckResult;
use crate::id::AccountId;
use crate::import::{self, Committed};
use crate::manual::{self, ManualTransaction};
use crate::money::{Currency, Money};
use crate::statement::{DatedBalance, SourceFormat, SourceLocation, Statement, StatementLine};

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

    fn account(&mut self, kind: AccountKind) -> AccountId {
        account::create(
            &mut self.db,
            &NewAccount::new("Account", kind, Currency::GBP),
        )
        .unwrap()
    }

    fn manual(&mut self, account: AccountId, d: u32, minor: i64) -> TransactionId {
        manual::create(
            &mut self.db,
            &ManualTransaction::new(account, day(d), gbp(minor), "Market stall"),
        )
        .unwrap()
    }

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

    fn lock(&self, account: AccountId, d: u32) {
        self.db
            .conn()
            .execute(
                "UPDATE account SET reconciled_through = ?1 WHERE id = ?2",
                params![date_key(day(d)), account],
            )
            .unwrap();
    }

    fn unlock(&self, account: AccountId) {
        self.db
            .conn()
            .execute(
                "UPDATE account SET reconciled_through = NULL WHERE id = ?1",
                [account],
            )
            .unwrap();
    }

    /// What the account's transactions give for the end of `d`, counted
    /// from the entered balance on day 1: the result of checking a file
    /// balance of `stated` there, as the import preview does.
    fn check(&self, account: AccountId, d: u32, stated: i64) -> CheckResult {
        let mut statement = Statement::new(SourceFormat::Ofx, Currency::GBP);
        statement
            .push_balance(DatedBalance {
                date: day(d),
                balance: gbp(stated),
            })
            .unwrap();
        let preview = import::preview(&self.db, account, "statement.ofx", statement).unwrap();
        let checks = &preview.reconciliation().checks;
        assert_eq!(checks.len(), 1);
        checks[0].after
    }
}

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

#[test]
fn reason_keys_match_the_schema() {
    assert_eq!(
        ExclusionReason::ALL.map(ExclusionReason::as_str),
        ["duplicate", "bank_error", "disputed", "other"]
    );
    for reason in ExclusionReason::ALL {
        assert_eq!(ExclusionReason::from_key(reason.as_str()), Some(reason));
    }
    assert_eq!(ExclusionReason::from_key("fraud"), None);

    // The schema takes each key, and nothing else.
    let mut f = Fixture::new();
    let account = f.account(AccountKind::Cash);
    let id = f.manual(account, 3, -100);
    for reason in ExclusionReason::ALL {
        exclude(&mut f.db, id, reason, Some("why")).unwrap();
        assert_eq!(get(&f.db, id).unwrap().unwrap().reason, reason);
    }
    let refused = f.db.conn().execute(
        "UPDATE \"transaction\" SET excluded_reason = 'fraud' WHERE id = ?1",
        [id],
    );
    assert!(refused.is_err());
}

#[test]
fn excludes_with_a_reason_and_includes_again() {
    let mut f = Fixture::new();
    let account = f.account(AccountKind::Current);
    // Any origin: an imported row's source fields stay as they are.
    let id = f.imported(account, 3, -999);
    assert_eq!(get(&f.db, id).unwrap(), None);

    let excluded = exclude(&mut f.db, id, ExclusionReason::BankError, None).unwrap();
    assert_eq!(excluded.reason, ExclusionReason::BankError);
    assert_eq!(excluded.note, None);
    // A UTC timestamp in the schema's form.
    assert_eq!(excluded.excluded_at.len(), "2026-11-12T09:00:00.000Z".len());
    assert!(excluded.excluded_at.ends_with('Z'));
    assert_eq!(get(&f.db, id).unwrap(), Some(excluded));

    // The reason can change; a blank note counts as none.
    let changed = exclude(&mut f.db, id, ExclusionReason::Disputed, Some("   ")).unwrap();
    assert_eq!(
        (changed.reason, changed.note),
        (ExclusionReason::Disputed, None)
    );
    let changed = exclude(
        &mut f.db,
        id,
        ExclusionReason::Duplicate,
        Some("Also in the OFX"),
    )
    .unwrap();
    assert_eq!(changed.note.as_deref(), Some("Also in the OFX"));

    include_again(&mut f.db, id).unwrap();
    assert_eq!(get(&f.db, id).unwrap(), None);
    let cleared: (Option<String>, Option<String>, Option<String>) =
        f.db.conn()
            .query_row(
                "SELECT excluded_reason, excluded_note, excluded_at FROM \"transaction\" \
             WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
    assert_eq!(cleared, (None, None, None));
    assert!(matches!(
        include_again(&mut f.db, id),
        Err(Error::NotExcluded(missing)) if missing == id
    ));

    let missing = TransactionId::new(99);
    assert!(matches!(
        exclude(&mut f.db, missing, ExclusionReason::Duplicate, None),
        Err(Error::TransactionNotFound(_))
    ));
    assert!(matches!(
        include_again(&mut f.db, missing),
        Err(Error::TransactionNotFound(_))
    ));
    assert!(matches!(
        get(&f.db, missing),
        Err(Error::TransactionNotFound(_))
    ));
}

#[test]
fn other_needs_a_note_that_errors_never_quote() {
    let mut f = Fixture::new();
    let account = f.account(AccountKind::Cash);
    let id = f.manual(account, 3, -100);
    for note in [None, Some(""), Some(" \n ")] {
        let error = exclude(&mut f.db, id, ExclusionReason::Other, note).unwrap_err();
        assert!(matches!(error, Error::NoteRequired));
    }
    assert_eq!(get(&f.db, id).unwrap(), None);

    let note = "Paid back by Sam";
    let excluded = exclude(&mut f.db, id, ExclusionReason::Other, Some(note)).unwrap();
    assert_eq!(excluded.note.as_deref(), Some(note));

    f.lock(account, 5);
    let error = exclude(&mut f.db, id, ExclusionReason::Duplicate, Some(note)).unwrap_err();
    assert!(!error.to_string().contains("Sam"));
}

#[test]
fn only_duplicate_changes_meet_the_lock() {
    let mut f = Fixture::new();
    let account = f.account(AccountKind::Current);
    let id = f.imported(account, 3, -999);
    let after = f.imported(account, 7, -999);
    f.lock(account, 3);

    let locked = |result: Result<(), Error>| {
        assert!(
            matches!(result, Err(Error::Locked { date, through, .. }) if date == day(3) && through == day(3)),
            "{result:?}"
        );
    };

    // Disputed money really moved, so excluding it in a locked window is
    // fine, and so is including it again.
    exclude(&mut f.db, id, ExclusionReason::Disputed, None).unwrap();
    include_again(&mut f.db, id).unwrap();
    exclude(&mut f.db, id, ExclusionReason::Disputed, None).unwrap();
    // Turning it into a duplicate would change the locked balance.
    locked(exclude(&mut f.db, id, ExclusionReason::Duplicate, None).map(|_| ()));
    assert_eq!(
        get(&f.db, id).unwrap().unwrap().reason,
        ExclusionReason::Disputed
    );
    include_again(&mut f.db, id).unwrap();
    locked(exclude(&mut f.db, id, ExclusionReason::Duplicate, None).map(|_| ()));
    assert_eq!(get(&f.db, id).unwrap(), None);

    // A duplicate excluded before the lock stays one: its note can change,
    // but including it again or giving it another reason can't.
    f.unlock(account);
    exclude(&mut f.db, id, ExclusionReason::Duplicate, None).unwrap();
    f.lock(account, 3);
    exclude(&mut f.db, id, ExclusionReason::Duplicate, Some("same as 7")).unwrap();
    locked(include_again(&mut f.db, id));
    locked(exclude(&mut f.db, id, ExclusionReason::BankError, None).map(|_| ()));
    assert_eq!(
        get(&f.db, id).unwrap().unwrap().reason,
        ExclusionReason::Duplicate
    );

    // After the lock date nothing is refused.
    exclude(&mut f.db, after, ExclusionReason::Duplicate, None).unwrap();
    include_again(&mut f.db, after).unwrap();
}

#[test]
fn a_manual_row_counts_in_the_balance_until_excluded_as_a_duplicate() {
    let mut f = Fixture::new();
    let account = f.account(AccountKind::Cash);
    account::add_balance(
        &mut f.db,
        account,
        &EnteredBalance {
            date: day(1),
            balance: gbp(10_000),
            source: EnteredSource::Manual,
            note: None,
        },
    )
    .unwrap();
    let id = f.manual(account, 3, -2_500);
    let passed = CheckResult::Passed { since: day(1) };
    assert_eq!(f.check(account, 5, 7_500), passed);

    for reason in [
        ExclusionReason::BankError,
        ExclusionReason::Disputed,
        ExclusionReason::Other,
    ] {
        exclude(&mut f.db, id, reason, Some("why")).unwrap();
        assert_eq!(f.check(account, 5, 7_500), passed, "{reason:?}");
    }

    exclude(&mut f.db, id, ExclusionReason::Duplicate, None).unwrap();
    assert_eq!(f.check(account, 5, 10_000), passed);
    include_again(&mut f.db, id).unwrap();
    assert_eq!(f.check(account, 5, 7_500), passed);

    // Edits and deletes move the balance with them.
    manual::update(
        &mut f.db,
        id,
        &ManualTransaction::new(account, day(3), gbp(-1_000), "Market stall"),
    )
    .unwrap();
    assert_eq!(f.check(account, 5, 9_000), passed);
    manual::delete(&mut f.db, id).unwrap();
    assert_eq!(f.check(account, 5, 10_000), passed);
}
