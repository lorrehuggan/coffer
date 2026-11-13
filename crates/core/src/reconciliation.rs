//! Standing balance checks and lock dates (M1.27).
//!
//! Every balance an account has is an assertion about the end of a day
//! (`docs/ARCHITECTURE.md` → "Reconciliation" and "Decision record: review
//! queues", points 1–2). This module checks all of them against the
//! account's transactions, for one account ([`account_checks`]) or every
//! account ([`all_checks`]), and sets and clears the account's lock date
//! (`account.reconciled_through`; [`lock`], [`unlock`]).
//!
//! - **The assertions** are the account's opening balance
//!   (`opening_balance_minor` on `opening_date`, when set) and every
//!   `balance_snapshot` (entered or from a file), ordered by date; on one
//!   day the opening balance comes first, then the snapshots by ID. The
//!   first is the starting point; each later one must equal the last one
//!   that passed plus the transactions after its day, up to and including
//!   its own ([`crate::balance::check`]).
//! - **Valuations set the balance.** A `valuation` snapshot (`asset` and
//!   `liability` accounts) is never a check: it is a new starting point,
//!   and the assertions after it are counted from it ([`Outcome::Valuation`]).
//! - **Which transactions count.** Every transaction of the account, except
//!   rows excluded as `duplicate`: a duplicate never happened at the bank.
//!   Rows excluded for another reason record money that really moved
//!   ("Decision record: Transactions additions", point 1). A split row
//!   counts once, with its whole amount.
//! - **A failure names its window**: the last passing assertion's date and
//!   the failing one's ([`AssertionCheck::window`]). A failed assertion
//!   isn't a base for the next one, so later windows start at the same pass.
//! - **Locks.** [`lock`] sets `reconciled_through` only to the date of a
//!   passing check, and only when every check up to and including that date
//!   passes. Moving an existing lock later is a lock like any other; moving
//!   it earlier would unlock part of the history and is refused: there's no
//!   partial unlock, so the user [`unlock`]s, which clears it entirely, and
//!   locks again. Valuation accounts have no checks, so they can't be
//!   locked.
//!
//! What a lock guards is enforced where history changes: imports and undos
//! (`import`), manual rows (`manual`), exclusions (`exclusion`), change
//! batches (`change`), the pot mirror (`sub_account`) and entered balances
//! ([`crate::account::add_balance`]).
//!
//! The pure part ([`check`]) needs no database. Nothing here reads
//! transaction text, so nothing can log it.

#[cfg(test)]
mod tests;

use chrono::NaiveDate;
use rusqlite::{Connection, TransactionBehavior, params};

use crate::account::{self, Account, date_key};
use crate::balance::{self, Assertion, CheckResult, DailyMovements};
use crate::db::Database;
use crate::id::{AccountId, BalanceId};
use crate::money::{Currency, Money};

/// One balance to check, as [`check`] takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatedBalance {
    /// The balance is at the end of this day.
    pub date: NaiveDate,
    pub balance_minor: i64,
    /// A valuation sets the balance; it is never checked.
    pub valuation: bool,
}

/// What checking one balance found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The account's first balance: where counting starts, not a check.
    StartingPoint,
    /// A valuation: it sets the balance, and counting starts again from it.
    /// Not a check.
    Valuation,
    /// It equals the balance on `since` (the last one that passed, or the
    /// starting point) plus the transactions after that day, up to and
    /// including this one's.
    Passed { since: NaiveDate },
    /// It doesn't: the transactions give `computed_minor`. The gap is in
    /// the window after `since`, up to and including this balance's date.
    Failed {
        since: NaiveDate,
        computed_minor: i128,
    },
}

impl Outcome {
    /// Whether this was a check (it passed or failed), rather than a
    /// starting point or a valuation.
    pub const fn is_check(&self) -> bool {
        matches!(self, Outcome::Passed { .. } | Outcome::Failed { .. })
    }

    pub const fn is_failed(&self) -> bool {
        matches!(self, Outcome::Failed { .. })
    }

    pub const fn is_passed(&self) -> bool {
        matches!(self, Outcome::Passed { .. })
    }
}

impl From<CheckResult> for Outcome {
    fn from(result: CheckResult) -> Self {
        match result {
            CheckResult::StartingPoint => Outcome::StartingPoint,
            CheckResult::Passed { since } => Outcome::Passed { since },
            CheckResult::Failed {
                since,
                computed_minor,
            } => Outcome::Failed {
                since,
                computed_minor,
            },
        }
    }
}

/// Checks `balances` in the order given, which must be by date (ties keep
/// their order), against the account's per-day `movements`. Returns one
/// outcome per balance, in the same order.
///
/// Like [`balance::check`], with valuations: each one starts the count
/// again from its own amount.
pub fn check(balances: &[StatedBalance], movements: &DailyMovements) -> Vec<Outcome> {
    debug_assert!(balances.is_sorted_by_key(|b| b.date));
    let mut outcomes = Vec::with_capacity(balances.len());
    let mut start = 0;
    while start < balances.len() {
        // A run from one valuation (or the first balance) up to the next.
        let end = balances[start + 1..]
            .iter()
            .position(|b| b.valuation)
            .map_or(balances.len(), |offset| start + 1 + offset);
        let run: Vec<Assertion> = balances[start..end]
            .iter()
            .map(|b| Assertion {
                date: b.date,
                balance_minor: b.balance_minor,
            })
            .collect();
        for (index, result) in balance::check(&run, movements).into_iter().enumerate() {
            outcomes.push(if index == 0 && balances[start].valuation {
                Outcome::Valuation
            } else {
                result.into()
            });
        }
        start = end;
    }
    outcomes
}

/// Which of an account's balances a check is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BalanceRef {
    /// `account.opening_balance_minor` on `opening_date`.
    Opening,
    /// A `balance_snapshot` row.
    Snapshot(BalanceId),
}

/// Where a balance comes from: the account's opening balance, or a
/// snapshot's `balance_snapshot.source`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BalanceSource {
    /// The account's opening balance.
    Opening,
    /// An OFX file's stated balance (`LEDGERBAL`), recorded by its import.
    Ofx,
    /// A CSV file's opening balance, from its running balances.
    Csv,
    /// Entered from a paper or PDF statement.
    Statement,
    /// Entered by the user (a balance off the bank's app, a cash count).
    Manual,
    /// A valuation of an `asset` or `liability` account.
    Valuation,
}

impl BalanceSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            BalanceSource::Opening => "opening",
            BalanceSource::Ofx => "ofx",
            BalanceSource::Csv => "csv",
            BalanceSource::Statement => "statement",
            BalanceSource::Manual => "manual",
            BalanceSource::Valuation => "valuation",
        }
    }

    /// The source of a snapshot, from `balance_snapshot.source`.
    fn from_snapshot_key(key: &str) -> Option<BalanceSource> {
        Some(match key {
            "ofx" => BalanceSource::Ofx,
            "csv" => BalanceSource::Csv,
            "statement" => BalanceSource::Statement,
            "manual" => BalanceSource::Manual,
            "valuation" => BalanceSource::Valuation,
            _ => return None,
        })
    }
}

/// The days between two balances: the transactions dated after `from`, up
/// to and including `to`. Both balances are at the end of their day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    /// The last passing balance's date (or the starting point's).
    pub from: NaiveDate,
    /// The failing balance's date.
    pub to: NaiveDate,
}

/// One of an account's balances, checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssertionCheck {
    pub balance: BalanceRef,
    pub source: BalanceSource,
    /// The balance is at the end of this day.
    pub date: NaiveDate,
    /// What the balance says.
    pub stated: Money,
    pub outcome: Outcome,
}

impl AssertionCheck {
    /// What the transactions give for this date, when the check failed.
    /// `None` when it didn't, or when the sum is outside what [`Money`]
    /// holds.
    pub fn computed(&self) -> Option<Money> {
        match self.outcome {
            Outcome::Failed { computed_minor, .. } => i64::try_from(computed_minor)
                .ok()
                .map(|minor| Money::new(minor, self.stated.currency())),
            _ => None,
        }
    }

    /// The stated balance less the computed one, in minor units, when the
    /// check failed: what's missing from the transactions in the window.
    pub fn difference_minor(&self) -> Option<i128> {
        match self.outcome {
            Outcome::Failed { computed_minor, .. } => {
                Some(i128::from(self.stated.minor()) - computed_minor)
            }
            _ => None,
        }
    }

    /// The window a failed check names: from the last passing balance to
    /// this one. `None` when the check didn't fail.
    pub fn window(&self) -> Option<Window> {
        match self.outcome {
            Outcome::Failed { since, .. } => Some(Window {
                from: since,
                to: self.date,
            }),
            _ => None,
        }
    }
}

/// The overall answer for one account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Nothing to check: no balances, only a starting point, or only
    /// valuations.
    NothingToCheck,
    /// Every check passed.
    Reconciled,
    /// At least one check failed.
    Mismatch,
}

/// Every balance of one account, checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountChecks {
    pub account: AccountId,
    pub currency: Currency,
    /// The account's lock date (`reconciled_through`).
    pub locked_through: Option<NaiveDate>,
    /// By date; on one day the opening balance first, then snapshots by ID.
    pub checks: Vec<AssertionCheck>,
}

impl AccountChecks {
    /// The checks that failed, by date.
    pub fn failures(&self) -> impl Iterator<Item = &AssertionCheck> {
        self.checks.iter().filter(|check| check.outcome.is_failed())
    }

    pub fn status(&self) -> Status {
        if self.checks.iter().any(|check| check.outcome.is_failed()) {
            Status::Mismatch
        } else if self.checks.iter().any(|check| check.outcome.is_passed()) {
            Status::Reconciled
        } else {
            Status::NothingToCheck
        }
    }

    /// The latest date [`lock`] accepts: the latest passing check's date
    /// with no failed check on or before it. `None` when there is none.
    pub fn lockable_through(&self) -> Option<NaiveDate> {
        let first_failure = self.failures().map(|check| check.date).min();
        self.checks
            .iter()
            .filter(|check| check.outcome.is_passed())
            .map(|check| check.date)
            .filter(|date| first_failure.is_none_or(|failed| *date < failed))
            .max()
    }

    /// Whether [`lock`] accepts `through`: a passing check is on that date
    /// and none on or before it fails.
    fn lock_problem(&self, through: NaiveDate) -> Option<Error> {
        if let Some(failed) = self.failures().map(|check| check.date).min()
            && failed <= through
        {
            return Some(Error::FailedCheck {
                through,
                failed_on: failed,
            });
        }
        let passes_there = self
            .checks
            .iter()
            .any(|check| check.date == through && check.outcome.is_passed());
        (!passes_there).then_some(Error::NoPassingCheck { date: through })
    }
}

/// Why checking or locking failed. Messages never contain transaction text.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("there is no account {0}")]
    AccountNotFound(AccountId),

    #[error(
        "there's no passing balance check on {date}; an account is locked only through the \
         date of a balance that passes its check"
    )]
    NoPassingCheck { date: NaiveDate },

    #[error(
        "the balance check on {failed_on} fails, so the account can't be locked through \
         {through}; every check up to the lock date must pass"
    )]
    FailedCheck {
        through: NaiveDate,
        failed_on: NaiveDate,
    },

    #[error(
        "the account is locked through {locked_through}; locking through an earlier date would \
         unlock part of it. Unlock the account first, then lock it again"
    )]
    WouldUnlockPart { locked_through: NaiveDate },

    #[error(transparent)]
    Account(#[from] account::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// Every balance of `account`, checked.
pub fn account_checks(db: &Database, account: AccountId) -> Result<AccountChecks, Error> {
    let stored = account::find(db.conn(), account)?.ok_or(Error::AccountNotFound(account))?;
    Ok(load(db.conn(), &stored)?)
}

/// Every account's balances, checked, in the order the accounts were
/// created.
pub fn all_checks(db: &Database) -> Result<Vec<AccountChecks>, Error> {
    account::list(db)?
        .iter()
        .map(|account| Ok(load(db.conn(), account)?))
        .collect()
}

/// The account's lock date: history up to and including it is locked.
pub fn locked_through(db: &Database, account: AccountId) -> Result<Option<NaiveDate>, Error> {
    let stored = account::find(db.conn(), account)?.ok_or(Error::AccountNotFound(account))?;
    Ok(stored.reconciled_through)
}

/// Locks `account`'s history through `through` (module docs): it must be
/// the date of a passing check, with every check up to it passing, and not
/// before the current lock date. Locking again through the same date
/// changes nothing.
pub fn lock(db: &mut Database, account: AccountId, through: NaiveDate) -> Result<(), Error> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let stored = account::find(&tx, account)?.ok_or(Error::AccountNotFound(account))?;
    match stored.reconciled_through {
        Some(locked) if through == locked => return Ok(()),
        Some(locked) if through < locked => {
            return Err(Error::WouldUnlockPart {
                locked_through: locked,
            });
        }
        _ => {}
    }
    if let Some(problem) = load(&tx, &stored)?.lock_problem(through) {
        return Err(problem);
    }
    tx.execute(
        "UPDATE account SET reconciled_through = ?1 WHERE id = ?2",
        params![date_key(through), account],
    )?;
    tx.commit()?;
    tracing::info!(%account, %through, "locked an account's history");
    Ok(())
}

/// Clears `account`'s lock date entirely and returns what it was (`None`
/// when it wasn't locked). There's no partial unlock.
pub fn unlock(db: &mut Database, account: AccountId) -> Result<Option<NaiveDate>, Error> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let stored = account::find(&tx, account)?.ok_or(Error::AccountNotFound(account))?;
    if stored.reconciled_through.is_some() {
        tx.execute(
            "UPDATE account SET reconciled_through = NULL WHERE id = ?1",
            [account],
        )?;
    }
    tx.commit()?;
    if let Some(through) = stored.reconciled_through {
        tracing::info!(%account, %through, "unlocked an account's history");
    }
    Ok(stored.reconciled_through)
}

/// Loads `account`'s balances and per-day movements and checks them.
fn load(conn: &Connection, account: &Account) -> rusqlite::Result<AccountChecks> {
    let mut balances: Vec<(BalanceRef, BalanceSource, StatedBalance)> = Vec::new();
    if let Some((date, balance)) = account.opening_balance {
        balances.push((
            BalanceRef::Opening,
            BalanceSource::Opening,
            StatedBalance {
                date,
                balance_minor: balance.minor(),
                valuation: false,
            },
        ));
    }
    let mut snapshots = conn.prepare_cached(
        "SELECT id, date, balance_minor, source FROM balance_snapshot \
         WHERE account_id = ?1 ORDER BY date, id",
    )?;
    let rows = snapshots.query_map([account.id], |row| {
        let key: String = row.get("source")?;
        let source = BalanceSource::from_snapshot_key(&key)
            .ok_or_else(|| account::invalid(row, "source"))?;
        Ok((
            BalanceRef::Snapshot(row.get("id")?),
            source,
            StatedBalance {
                date: account::date(row, "date")?,
                balance_minor: row.get("balance_minor")?,
                valuation: source == BalanceSource::Valuation,
            },
        ))
    })?;
    for row in rows {
        balances.push(row?);
    }
    // Stable: the opening balance stays ahead of a snapshot on its day.
    balances.sort_by_key(|(_, _, balance)| balance.date);

    let movements = movements(conn, account.id)?;
    let stated: Vec<StatedBalance> = balances.iter().map(|(_, _, b)| *b).collect();
    let outcomes = check(&stated, &movements);
    let checks = balances
        .into_iter()
        .zip(outcomes)
        .map(|((balance, source, stated), outcome)| AssertionCheck {
            balance,
            source,
            date: stated.date,
            stated: Money::new(stated.balance_minor, account.currency),
            outcome,
        })
        .collect();
    Ok(AccountChecks {
        account: account.id,
        currency: account.currency,
        locked_through: account.reconciled_through,
        checks,
    })
}

/// The account's net movement per day: every transaction except those
/// excluded as `duplicate`. Summed in `i128`, so no day's total overflows.
fn movements(conn: &Connection, account: AccountId) -> rusqlite::Result<DailyMovements> {
    let mut statement = conn.prepare_cached(
        "SELECT date, amount_minor FROM \"transaction\" \
         WHERE account_id = ?1 AND excluded_reason IS NOT 'duplicate'",
    )?;
    let rows = statement.query_map([account], |row| {
        Ok((
            account::date(row, "date")?,
            row.get::<_, i64>("amount_minor")?,
        ))
    })?;
    let mut movements = DailyMovements::new();
    for row in rows {
        let (date, minor) = row?;
        *movements.entry(date).or_default() += i128::from(minor);
    }
    Ok(movements)
}
