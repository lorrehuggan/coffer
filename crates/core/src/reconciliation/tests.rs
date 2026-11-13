//! Tests for standing balance checks and lock dates.

use rusqlite::params;
use tempfile::TempDir;

use super::*;
use crate::account::{AccountKind, EnteredBalance, EnteredSource, NewAccount};
use crate::exclusion::{self, ExclusionReason};
use crate::id::TransactionId;
use crate::manual::{self, ManualTransaction};
use crate::split::{self, SplitLine};

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

// The pure check ----------------------------------------------------------------

fn stated(d: u32, balance_minor: i64) -> StatedBalance {
    StatedBalance {
        date: day(d),
        balance_minor,
        valuation: false,
    }
}

fn valued(d: u32, balance_minor: i64) -> StatedBalance {
    StatedBalance {
        valuation: true,
        ..stated(d, balance_minor)
    }
}

fn movements(days: &[(u32, i128)]) -> DailyMovements {
    days.iter().map(|&(d, sum)| (day(d), sum)).collect()
}

#[test]
fn nothing_to_check() {
    assert_eq!(check(&[], &movements(&[(1, 5)])), []);
    assert_eq!(
        check(&[stated(1, 100)], &DailyMovements::new()),
        [Outcome::StartingPoint]
    );
}

#[test]
fn checks_count_from_the_last_pass_and_name_the_window() {
    let moved = movements(&[(2, -500), (4, -100), (6, 0)]);
    let outcomes = check(
        &[
            stated(1, 10_000),
            stated(3, 9_500),
            stated(5, 9_000),
            stated(6, 9_400),
        ],
        &moved,
    );
    assert_eq!(
        outcomes,
        [
            Outcome::StartingPoint,
            Outcome::Passed { since: day(1) },
            Outcome::Failed {
                since: day(3),
                computed_minor: 9_400,
            },
            Outcome::Passed { since: day(3) },
        ]
    );
}

#[test]
fn a_valuation_sets_the_balance_and_is_never_a_check() {
    let outcomes = check(
        &[valued(1, 300_000), valued(5, 310_000), valued(9, 290_000)],
        &DailyMovements::new(),
    );
    assert_eq!(outcomes, [Outcome::Valuation; 3]);
    assert!(outcomes.iter().all(|outcome| !outcome.is_check()));

    // A balance after a valuation is counted from the valuation, not from
    // what came before it.
    let outcomes = check(
        &[
            stated(1, 100),
            valued(2, 1_000),
            stated(3, 1_000),
            stated(4, 7),
        ],
        &DailyMovements::new(),
    );
    assert_eq!(
        outcomes,
        [
            Outcome::StartingPoint,
            Outcome::Valuation,
            Outcome::Passed { since: day(2) },
            Outcome::Failed {
                since: day(3),
                computed_minor: 1_000,
            },
        ]
    );
}

// Against the database ---------------------------------------------------------

struct Fixture {
    db: Database,
    _dir: TempDir,
    account: AccountId,
}

impl Fixture {
    /// A GBP current account.
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
            _dir: dir,
            account,
        }
    }

    fn opening(&self, d: u32, minor: i64) {
        self.db
            .conn()
            .execute(
                "UPDATE account SET opening_balance_minor = ?1, opening_date = ?2 WHERE id = ?3",
                params![minor, date_key(day(d)), self.account],
            )
            .unwrap();
    }

    fn spend(&mut self, d: u32, minor: i64) -> TransactionId {
        manual::create(
            &mut self.db,
            &ManualTransaction::new(self.account, day(d), gbp(minor), "Cash"),
        )
        .unwrap()
    }

    fn entered(&mut self, d: u32, minor: i64) -> BalanceId {
        account::add_balance(
            &mut self.db,
            self.account,
            &EnteredBalance {
                date: day(d),
                balance: gbp(minor),
                source: EnteredSource::Manual,
                note: None,
            },
        )
        .unwrap()
    }

    fn checks(&self) -> AccountChecks {
        account_checks(&self.db, self.account).unwrap()
    }

    fn outcomes(&self) -> Vec<(NaiveDate, Outcome)> {
        self.checks()
            .checks
            .iter()
            .map(|check| (check.date, check.outcome))
            .collect()
    }

    /// Opening 100.00 on the 1st; −5.00 on the 2nd; 95.00 entered on the
    /// 3rd (passes); −1.00 on the 4th; 90.00 on the 5th (fails: 94.00);
    /// 94.00 on the 6th (passes from the 3rd).
    fn with_history() -> Fixture {
        let mut fixture = Fixture::new();
        fixture.opening(1, 10_000);
        fixture.spend(2, -500);
        fixture.entered(3, 9_500);
        fixture.spend(4, -100);
        fixture.entered(5, 9_000);
        fixture.entered(6, 9_400);
        fixture
    }
}

#[test]
fn every_balance_is_checked_and_a_failure_names_its_window() {
    let fixture = Fixture::with_history();
    let checks = fixture.checks();
    assert_eq!(checks.account, fixture.account);
    assert_eq!(checks.currency, Currency::GBP);
    assert_eq!(checks.locked_through, None);
    assert_eq!(
        checks
            .checks
            .iter()
            .map(|check| (check.balance, check.source))
            .collect::<Vec<_>>(),
        [
            (BalanceRef::Opening, BalanceSource::Opening),
            (
                BalanceRef::Snapshot(BalanceId::new(1)),
                BalanceSource::Manual
            ),
            (
                BalanceRef::Snapshot(BalanceId::new(2)),
                BalanceSource::Manual
            ),
            (
                BalanceRef::Snapshot(BalanceId::new(3)),
                BalanceSource::Manual
            ),
        ]
    );
    assert_eq!(
        fixture.outcomes(),
        [
            (day(1), Outcome::StartingPoint),
            (day(3), Outcome::Passed { since: day(1) }),
            (
                day(5),
                Outcome::Failed {
                    since: day(3),
                    computed_minor: 9_400,
                }
            ),
            (day(6), Outcome::Passed { since: day(3) }),
        ]
    );
    let failures: Vec<_> = checks.failures().collect();
    assert_eq!(failures.len(), 1);
    let failed = failures[0];
    assert_eq!(failed.stated, gbp(9_000));
    assert_eq!(failed.computed(), Some(gbp(9_400)));
    assert_eq!(failed.difference_minor(), Some(-400));
    assert_eq!(
        failed.window(),
        Some(Window {
            from: day(3),
            to: day(5),
        })
    );
    assert_eq!(checks.checks[1].window(), None);
    assert_eq!(checks.status(), Status::Mismatch);
    assert_eq!(checks.lockable_through(), Some(day(3)));
}

#[test]
fn only_duplicates_leave_the_balance() {
    let mut fixture = Fixture::new();
    fixture.entered(1, 10_000);
    let charge = fixture.spend(2, -500);
    let twice = fixture.spend(2, -500);
    fixture.entered(3, 9_500);
    assert!(fixture.checks().checks[1].outcome.is_failed());

    // A double charge excluded as the bank's error really moved: still in.
    exclusion::exclude(&mut fixture.db, twice, ExclusionReason::BankError, None).unwrap();
    assert!(fixture.checks().checks[1].outcome.is_failed());
    // As a duplicate it never happened at the bank: out.
    exclusion::exclude(&mut fixture.db, twice, ExclusionReason::Duplicate, None).unwrap();
    assert_eq!(fixture.checks().status(), Status::Reconciled);
    // Other exclusions stay in, whatever the reason.
    exclusion::exclude(&mut fixture.db, charge, ExclusionReason::Disputed, None).unwrap();
    assert_eq!(fixture.checks().status(), Status::Reconciled);
}

#[test]
fn a_split_row_counts_once() {
    let mut fixture = Fixture::new();
    fixture.entered(1, 10_000);
    let row = fixture.spend(2, -500);
    fixture.entered(2, 9_500);
    split::set(
        &mut fixture.db,
        row,
        &[
            SplitLine::new(gbp(-200), None),
            SplitLine::new(gbp(-300), None),
        ],
    )
    .unwrap();
    assert_eq!(fixture.checks().status(), Status::Reconciled);
}

#[test]
fn valuation_accounts_have_nothing_to_check_and_cant_be_locked() {
    let mut fixture = Fixture::new();
    let house = account::create(
        &mut fixture.db,
        &NewAccount::new("House", AccountKind::Asset, Currency::GBP),
    )
    .unwrap();
    for (d, minor) in [(1, 30_000_000), (5, 31_000_000)] {
        fixture
            .db
            .conn()
            .execute(
                "INSERT INTO balance_snapshot (account_id, date, balance_minor, source) \
                 VALUES (?1, ?2, ?3, 'valuation')",
                params![house, date_key(day(d)), minor],
            )
            .unwrap();
    }
    let checks = account_checks(&fixture.db, house).unwrap();
    assert_eq!(
        checks
            .checks
            .iter()
            .map(|check| (check.source, check.outcome))
            .collect::<Vec<_>>(),
        [
            (BalanceSource::Valuation, Outcome::Valuation),
            (BalanceSource::Valuation, Outcome::Valuation),
        ]
    );
    assert_eq!(checks.status(), Status::NothingToCheck);
    assert_eq!(checks.lockable_through(), None);
    assert!(matches!(
        lock(&mut fixture.db, house, day(5)),
        Err(Error::NoPassingCheck { date }) if date == day(5)
    ));
}

#[test]
fn all_checks_every_account() {
    let mut fixture = Fixture::with_history();
    let other = account::create(
        &mut fixture.db,
        &NewAccount::new("Savings", AccountKind::Savings, Currency::EUR),
    )
    .unwrap();
    let all = all_checks(&fixture.db).unwrap();
    assert_eq!(
        all.iter()
            .map(|checks| (checks.account, checks.currency, checks.status()))
            .collect::<Vec<_>>(),
        [
            (fixture.account, Currency::GBP, Status::Mismatch),
            (other, Currency::EUR, Status::NothingToCheck),
        ]
    );
    assert!(matches!(
        account_checks(&fixture.db, AccountId::new(99)),
        Err(Error::AccountNotFound(_))
    ));
}

#[test]
fn locks_only_through_a_passing_check_with_none_failing_before() {
    let mut fixture = Fixture::with_history();
    let account = fixture.account;

    // The starting point isn't a check; a day without a balance isn't one.
    for d in [1, 2] {
        assert!(matches!(
            lock(&mut fixture.db, account, day(d)),
            Err(Error::NoPassingCheck { date }) if date == day(d)
        ));
    }
    // The 6th passes, but the 5th, before it, fails; the 5th itself fails.
    for d in [5, 6] {
        assert!(matches!(
            lock(&mut fixture.db, account, day(d)),
            Err(Error::FailedCheck { through, failed_on })
                if through == day(d) && failed_on == day(5)
        ));
    }
    assert_eq!(locked_through(&fixture.db, account).unwrap(), None);

    lock(&mut fixture.db, account, day(3)).unwrap();
    assert_eq!(locked_through(&fixture.db, account).unwrap(), Some(day(3)));
    assert_eq!(fixture.checks().locked_through, Some(day(3)));
    // Again through the same date: nothing changes.
    lock(&mut fixture.db, account, day(3)).unwrap();
    assert!(matches!(
        lock(&mut fixture.db, AccountId::new(99), day(3)),
        Err(Error::AccountNotFound(_))
    ));
}

#[test]
fn a_lock_moves_later_but_never_earlier_and_unlock_clears_it() {
    let mut fixture = Fixture::new();
    fixture.entered(1, 10_000);
    fixture.spend(2, -500);
    fixture.entered(3, 9_500);
    fixture.entered(4, 9_500);
    let account = fixture.account;

    lock(&mut fixture.db, account, day(3)).unwrap();
    lock(&mut fixture.db, account, day(4)).unwrap();
    assert!(matches!(
        lock(&mut fixture.db, account, day(3)),
        Err(Error::WouldUnlockPart { locked_through }) if locked_through == day(4)
    ));
    assert_eq!(locked_through(&fixture.db, account).unwrap(), Some(day(4)));

    assert_eq!(unlock(&mut fixture.db, account).unwrap(), Some(day(4)));
    assert_eq!(locked_through(&fixture.db, account).unwrap(), None);
    assert_eq!(unlock(&mut fixture.db, account).unwrap(), None);
    lock(&mut fixture.db, account, day(3)).unwrap();
    assert!(matches!(
        unlock(&mut fixture.db, AccountId::new(99)),
        Err(Error::AccountNotFound(_))
    ));
}

#[test]
fn a_locked_period_refuses_what_would_change_its_checks() {
    let mut fixture = Fixture::with_history();
    let account = fixture.account;
    lock(&mut fixture.db, account, day(3)).unwrap();

    // A new row, an entered balance or a duplicate exclusion in the lock.
    assert!(matches!(
        manual::create(
            &mut fixture.db,
            &ManualTransaction::new(account, day(2), gbp(-1), "Cash"),
        ),
        Err(manual::Error::Locked { .. })
    ));
    assert!(matches!(
        account::add_balance(
            &mut fixture.db,
            account,
            &EnteredBalance {
                date: day(2),
                balance: gbp(1),
                source: EnteredSource::Statement,
                note: None,
            },
        ),
        Err(account::Error::Locked { .. })
    ));
    let inside: TransactionId = fixture
        .db
        .conn()
        .query_row(
            "SELECT id FROM \"transaction\" WHERE date = ?1",
            [date_key(day(2))],
            |row| row.get(0),
        )
        .unwrap();
    assert!(matches!(
        exclusion::exclude(&mut fixture.db, inside, ExclusionReason::Duplicate, None),
        Err(exclusion::Error::Locked { .. })
    ));
    // The checks it rests on are as they were.
    assert_eq!(fixture.checks().lockable_through(), Some(day(3)));

    // After the lock, the failure on the 5th can be fixed and the lock
    // moved past it.
    fixture.spend(5, -400);
    let checks = fixture.checks();
    assert!(checks.checks[2].outcome.is_passed());
    assert!(checks.checks[3].outcome.is_failed(), "the 6th now fails");
    assert_eq!(checks.lockable_through(), Some(day(5)));
    lock(&mut fixture.db, account, day(5)).unwrap();
    assert!(matches!(
        lock(&mut fixture.db, account, day(6)),
        Err(Error::FailedCheck { .. })
    ));
}
