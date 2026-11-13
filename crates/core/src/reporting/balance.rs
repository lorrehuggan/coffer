//! What an account held at the end of a day: gross, like the bank's
//! balance ("Decision record: refunds, reimbursements and shared costs",
//! point 3: balances and net worth are always gross).
//!
//! Built from the same pieces as the balance checks
//! ([`crate::reconciliation`]):
//!
//! - **Stated balances**: the opening balance, then every
//!   `balance_snapshot` (entered, from a file, or a valuation), by date; on
//!   one day the opening balance comes first, then snapshots by ID, and the
//!   last one on a day wins.
//! - **Movements**: every transaction of the account except those excluded
//!   as `duplicate` (a duplicate never happened at the bank). A split row
//!   counts once, with its whole amount. Confirmed transfers count: they
//!   moved real money between accounts.
//!
//! The balance on day D is the last stated balance on or before D plus the
//! movements after its day up to and including D. With none on or before
//! D, it is the first stated balance after D minus the movements after D
//! up to its day. An account with no stated balance at all is summed from
//! zero and marked [`Anchor::Unanchored`]: the sum is right only if the
//! account started empty.
//!
//! `asset` and `liability` accounts hold valuations only: their balance is
//! the last valuation (or other stated balance) on or before D, and there
//! is none before the first.
//!
//! A stated balance that fails its check still counts as stated: it is
//! what the bank or the user said. Nothing here reads transaction text.

use std::collections::BTreeMap;
use std::ops::Bound;

use chrono::NaiveDate;
use rusqlite::Connection;

use crate::account::{self, Account};
use crate::balance::DailyMovements;
use crate::db::Reader;
use crate::id::AccountId;
use crate::money::{Money, MoneyError};

use super::Error;

/// Where a balance was counted from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Anchor {
    /// From the stated balance (opening, snapshot or valuation) on `date`.
    Stated { date: NaiveDate },
    /// The account has no stated balance: its transactions summed from
    /// zero.
    Unanchored,
}

/// An account's balance at the end of `date`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccountBalance {
    pub account: AccountId,
    pub date: NaiveDate,
    /// In the account's currency, gross.
    pub balance: Money,
    pub anchor: Anchor,
}

/// Every account, in the order they were created.
pub fn accounts(reader: &Reader) -> Result<Vec<Account>, Error> {
    let conn = reader.conn();
    let mut statement = conn.prepare_cached("SELECT id FROM account ORDER BY id")?;
    let ids: Vec<AccountId> = statement
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    ids.into_iter()
        .map(|id| account::find(conn, id)?.ok_or(Error::UnknownAccount(id)))
        .collect()
}

/// The account with this ID.
pub fn account(reader: &Reader, id: AccountId) -> Result<Account, Error> {
    account::find(reader.conn(), id)?.ok_or(Error::UnknownAccount(id))
}

/// `account`'s balance at the end of each of `dates` (module docs), in the
/// same order. `None` for a valuation account before its first valuation.
pub fn balances_at(
    reader: &Reader,
    account: &Account,
    dates: &[NaiveDate],
) -> Result<Vec<Option<AccountBalance>>, Error> {
    let history = History::load(reader.conn(), account)?;
    dates.iter().map(|&date| history.at(date)).collect()
}

/// `account`'s balance at the end of its last active day: the later of its
/// last transaction and its last stated balance. `None` when it has
/// neither.
pub fn latest_balance(reader: &Reader, account: &Account) -> Result<Option<AccountBalance>, Error> {
    let history = History::load(reader.conn(), account)?;
    match history.last_day() {
        Some(date) => history.at(date),
        None => Ok(None),
    }
}

/// An account's stated balances and daily movements.
struct History {
    account: AccountId,
    currency: crate::money::Currency,
    valuations_only: bool,
    /// The last stated balance of each day.
    stated: BTreeMap<NaiveDate, i64>,
    movements: DailyMovements,
}

impl History {
    fn load(conn: &Connection, account: &Account) -> Result<History, Error> {
        let mut stated = BTreeMap::new();
        if let Some((date, balance)) = account.opening_balance {
            stated.insert(date, balance.minor());
        }
        let mut snapshots = conn.prepare_cached(
            "SELECT date, balance_minor FROM balance_snapshot \
             WHERE account_id = ?1 ORDER BY date, id",
        )?;
        let rows = snapshots.query_map([account.id], |row| {
            Ok((account::date(row, "date")?, row.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (date, minor) = row?;
            stated.insert(date, minor);
        }

        let mut movements = DailyMovements::new();
        let mut statement = conn.prepare_cached(
            "SELECT date, amount_minor FROM \"transaction\" \
             WHERE account_id = ?1 AND excluded_reason IS NOT 'duplicate'",
        )?;
        let rows = statement.query_map([account.id], |row| {
            Ok((account::date(row, "date")?, row.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (date, minor) = row?;
            *movements.entry(date).or_default() += i128::from(minor);
        }

        Ok(History {
            account: account.id,
            currency: account.currency,
            valuations_only: !account.kind.holds_transactions(),
            stated,
            movements,
        })
    }

    fn last_day(&self) -> Option<NaiveDate> {
        let stated = self.stated.keys().next_back().copied();
        let moved = self.movements.keys().next_back().copied();
        stated.max(moved)
    }

    fn moved(&self, after: NaiveDate, through: NaiveDate) -> i128 {
        self.movements
            .range((Bound::Excluded(after), Bound::Included(through)))
            .map(|(_, sum)| sum)
            .sum()
    }

    fn at(&self, date: NaiveDate) -> Result<Option<AccountBalance>, Error> {
        let before = self.stated.range(..=date).next_back();
        let (minor, anchor): (i128, Anchor) = if self.valuations_only {
            match before {
                Some((&on, &minor)) => (i128::from(minor), Anchor::Stated { date: on }),
                None => return Ok(None),
            }
        } else if let Some((&on, &minor)) = before {
            (
                i128::from(minor) + self.moved(on, date),
                Anchor::Stated { date: on },
            )
        } else if let Some((&on, &minor)) = self.stated.range(date..).next() {
            (
                i128::from(minor) - self.moved(date, on),
                Anchor::Stated { date: on },
            )
        } else {
            let moved = self.movements.range(..=date).map(|(_, sum)| sum).sum();
            (moved, Anchor::Unanchored)
        };
        let minor = i64::try_from(minor).map_err(|_| MoneyError::Overflow)?;
        Ok(Some(AccountBalance {
            account: self.account,
            date,
            balance: Money::new(minor, self.currency),
            anchor,
        }))
    }
}

#[cfg(test)]
mod tests {
    use crate::account::{AccountKind, EnteredBalance, EnteredSource};
    use crate::exclusion::ExclusionReason;
    use crate::money::Currency;

    use super::super::test_ledger::{TestLedger, date};
    use super::*;

    fn gbp(minor: i64) -> Money {
        Money::new(minor, Currency::GBP)
    }

    fn at(ledger: &TestLedger, account: AccountId, days: &[&str]) -> Vec<Option<(i64, Anchor)>> {
        let reader = ledger.reader();
        let account = super::account(&reader, account).unwrap();
        let dates: Vec<NaiveDate> = days.iter().map(|d| date(d)).collect();
        balances_at(&reader, &account, &dates)
            .unwrap()
            .into_iter()
            .map(|b| b.map(|b| (b.balance.minor(), b.anchor)))
            .collect()
    }

    #[test]
    fn counts_forward_and_back_from_stated_balances() {
        let mut ledger = TestLedger::new();
        let current = ledger.account("Current", Currency::GBP);
        ledger.row(current, "2026-10-05", -1_000, None);
        ledger.row(current, "2026-10-20", 5_000, None);
        ledger.row(current, "2026-11-02", -300, None);
        ledger
            .db
            .conn()
            .execute(
                "UPDATE account SET opening_balance_minor = 10000, opening_date = '2026-10-10' \
                 WHERE id = ?1",
                [current],
            )
            .unwrap();
        account::add_balance(
            &mut ledger.db,
            current,
            &EnteredBalance {
                date: date("2026-10-31"),
                balance: gbp(20_000),
                source: EnteredSource::Manual,
                note: None,
            },
        )
        .unwrap();

        let opening = Anchor::Stated {
            date: date("2026-10-10"),
        };
        let entered = Anchor::Stated {
            date: date("2026-10-31"),
        };
        assert_eq!(
            at(
                &ledger,
                current,
                &[
                    "2026-10-01",
                    "2026-10-10",
                    "2026-10-25",
                    "2026-10-31",
                    "2026-11-30"
                ]
            ),
            [
                // Back from the opening balance: before the 5 Oct payment.
                Some((11_000, opening)),
                Some((10_000, opening)),
                Some((15_000, opening)),
                // The entered balance wins on its day, even though the sum
                // says 15,000.
                Some((20_000, entered)),
                Some((19_700, entered)),
            ]
        );
    }

    #[test]
    fn without_a_stated_balance_sums_from_zero_and_skips_duplicates() {
        let mut ledger = TestLedger::new();
        let cash = ledger.account("Cash", Currency::GBP);
        ledger.row(cash, "2026-11-01", 2_000, None);
        let dup = ledger.row(cash, "2026-11-02", -500, None);
        let other = ledger.row(cash, "2026-11-03", -700, None);
        ledger.exclude(dup, ExclusionReason::Duplicate);
        // Excluded for another reason: the money still moved.
        ledger.exclude(other, ExclusionReason::Disputed);
        assert_eq!(
            at(&ledger, cash, &["2026-10-31", "2026-11-30"]),
            [
                Some((0, Anchor::Unanchored)),
                Some((1_300, Anchor::Unanchored))
            ]
        );
        let reader = ledger.reader();
        let account = super::account(&reader, cash).unwrap();
        let latest = latest_balance(&reader, &account).unwrap().unwrap();
        assert_eq!(
            (latest.date, latest.balance.minor()),
            (date("2026-11-03"), 1_300)
        );
    }

    #[test]
    fn valuation_accounts_hold_their_last_valuation() {
        let mut ledger = TestLedger::new();
        let house = ledger.valuation_account("House", AccountKind::Asset, Currency::GBP);
        ledger.valuation(house, "2026-06-30", 30_000_000);
        ledger.valuation(house, "2026-09-30", 31_000_000);
        let stated = |d| Anchor::Stated { date: date(d) };
        assert_eq!(
            at(&ledger, house, &["2026-06-01", "2026-07-15", "2026-11-30"]),
            [
                None,
                Some((30_000_000, stated("2026-06-30"))),
                Some((31_000_000, stated("2026-09-30")))
            ]
        );
        let reader = ledger.reader();
        let empty = ledger.account("Empty", Currency::EUR);
        let empty = super::account(&reader, empty).unwrap();
        assert_eq!(latest_balance(&reader, &empty).unwrap(), None);
    }
}
