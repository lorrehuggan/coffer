//! Reconciling an import (M1.12): the file's running balances against its
//! own rows, and every balance against the account's history.

use std::collections::HashSet;

use chrono::NaiveDate;

use super::Error;
use crate::balance::{self, Assertion, CheckResult, DailyMovements};
use crate::id::BalanceId;
use crate::money::{Currency, Money};
use crate::statement::{SourceLocation, Statement, StatementLine, StatementRow};

/// Which way a file with running balances lists its rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileOrder {
    OldestFirst,
    /// Newest first, as many banks export: a row's running balance then
    /// follows from the row below it.
    NewestFirst,
}

/// The file's running balances, checked row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunningBalances {
    /// The order the rows were read in: the one under which fewer balances
    /// fail, then the one the dates suggest.
    pub order: FileOrder,
    /// How many running balances were checked against the one before. A
    /// flagged row breaks the chain: the next balance starts it again
    /// unchecked.
    pub checked: usize,
    pub mismatches: Vec<RunningMismatch>,
}

/// A row whose running balance isn't the previous balance plus its amount.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunningMismatch {
    pub location: SourceLocation,
    /// The previous row's balance plus the amounts since.
    pub computed: Money,
    /// The balance the row states.
    pub stated: Money,
}

/// One of the account's balances, as checked by the preview.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckedBalance {
    /// A balance this import would record: the index into
    /// [`ImportPreview::balances`](super::ImportPreview::balances).
    FromFile(usize),
    /// A stored balance (entered, or from an earlier import).
    Stored(BalanceId),
    /// The account's opening balance (`account.opening_balance_minor`).
    Opening,
}

/// One balance check the import adds or changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BalanceCheck {
    pub balance: CheckedBalance,
    pub date: NaiveDate,
    pub stated: Money,
    /// The result before this import; `None` for a balance it adds.
    pub before: Option<CheckResult>,
    /// The result with this import's rows and balances.
    pub after: CheckResult,
}

impl BalanceCheck {
    /// What the transactions give for this date, when the check failed.
    pub fn computed(&self) -> Option<Money> {
        match self.after {
            CheckResult::Failed { computed_minor, .. } => i64::try_from(computed_minor)
                .ok()
                .map(|minor| Money::new(minor, self.stated.currency())),
            _ => None,
        }
    }
}

/// The overall answer for the import screen's reconciliation block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconciliationStatus {
    /// Nothing could be checked: the file has no balances and its rows
    /// fall outside every window between the account's balances. Expected
    /// for exports without balances (Monzo); the account's entered balance
    /// anchors it.
    NoBalances,
    /// The file's balance is the account's first: counting starts there,
    /// with nothing to check it against yet.
    StartingPoint,
    /// Every check passed.
    Reconciled,
    /// A running balance or a balance check failed. Committing needs the
    /// user's confirmation and leaves the failure visible.
    Mismatch,
}

/// What reconciling the import found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reconciliation {
    /// `None` when no row has a running balance.
    pub running: Option<RunningBalances>,
    /// The checks this import adds, the ones whose result it changes, and
    /// those of stored balances the file states again.
    pub checks: Vec<BalanceCheck>,
}

impl Reconciliation {
    pub fn status(&self) -> ReconciliationStatus {
        let running_failed = self
            .running
            .as_ref()
            .is_some_and(|running| !running.mismatches.is_empty());
        if running_failed || self.checks.iter().any(|check| check.after.is_failed()) {
            ReconciliationStatus::Mismatch
        } else if self
            .running
            .as_ref()
            .is_some_and(|running| running.checked > 0)
            || self
                .checks
                .iter()
                .any(|check| matches!(check.after, CheckResult::Passed { .. }))
        {
            ReconciliationStatus::Reconciled
        } else if self.checks.is_empty() {
            ReconciliationStatus::NoBalances
        } else {
            ReconciliationStatus::StartingPoint
        }
    }
}

/// The running-balance check, and the balance the file opens with: the
/// end of the day before its first row. `None` when no row has a running
/// balance.
///
/// Only the opening is recorded from running balances, not the closing:
/// an export taken during the day ends part-way through its last day, and
/// that day's later rows would make an end-of-day balance from it fail.
pub(super) fn running(
    statement: &Statement,
) -> Result<Option<(RunningBalances, Option<Opening>)>, Error> {
    let rows = statement.rows();
    let has_balances = rows
        .iter()
        .any(|row| matches!(&row.outcome, Ok(line) if line.balance_after.is_some()));
    if !has_balances {
        return Ok(None);
    }

    let forward = walk(rows.iter());
    let backward = walk(rows.iter().rev());
    let order = match forward.1.len().cmp(&backward.1.len()) {
        std::cmp::Ordering::Less => FileOrder::OldestFirst,
        std::cmp::Ordering::Greater => FileOrder::NewestFirst,
        std::cmp::Ordering::Equal => {
            let mut lines = statement.lines().map(|(_, line)| line.date);
            let first = lines.next();
            let last = lines.last().or(first);
            if first > last {
                FileOrder::NewestFirst
            } else {
                FileOrder::OldestFirst
            }
        }
    };
    let (checked, mismatches) = match order {
        FileOrder::OldestFirst => forward,
        FileOrder::NewestFirst => backward,
    };
    let currency = statement.currency();
    let mismatches = mismatches
        .into_iter()
        .map(|(location, computed, stated)| {
            Ok(RunningMismatch {
                location,
                computed: money(computed, currency)?,
                stated: Money::new(stated, currency),
            })
        })
        .collect::<Result<_, Error>>()?;

    let opening = match order {
        FileOrder::OldestFirst => opening(rows.iter(), currency)?,
        FileOrder::NewestFirst => opening(rows.iter().rev(), currency)?,
    };
    Ok(Some((
        RunningBalances {
            order,
            checked,
            mismatches,
        },
        opening,
    )))
}

/// The balance a file opens with, at the end of that day.
pub(super) type Opening = (NaiveDate, Money);

type Mismatches = Vec<(SourceLocation, i128, i64)>;

/// Walks rows oldest first: each stated balance must be the previous one
/// plus the amounts since. A flagged row's amount is unknown, so the chain
/// restarts after it.
fn walk<'a>(rows: impl Iterator<Item = &'a StatementRow>) -> (usize, Mismatches) {
    let mut running: Option<i128> = None;
    let mut checked = 0;
    let mut mismatches = Vec::new();
    for row in rows {
        let Ok(line) = &row.outcome else {
            running = None;
            continue;
        };
        running = running.map(|sum| sum + i128::from(line.amount.minor()));
        if let Some(stated) = line.balance_after {
            if let Some(computed) = running {
                checked += 1;
                if computed != i128::from(stated.minor()) {
                    mismatches.push((row.location, computed, stated.minor()));
                }
            }
            running = Some(i128::from(stated.minor()));
        }
    }
    (checked, mismatches)
}

/// The balance before the first row, oldest first: the first stated
/// balance less the amounts up to and including its row. Needs no flagged
/// row before it, and a first row on the file's earliest date.
fn opening<'a>(
    mut rows: impl Iterator<Item = &'a StatementRow> + Clone,
    currency: Currency,
) -> Result<Option<Opening>, Error> {
    let earliest = rows
        .clone()
        .filter_map(|row| row.outcome.as_ref().ok())
        .map(|line| line.date)
        .min();
    let mut first: Option<&StatementLine> = None;
    let mut moved: i128 = 0;
    for row in rows.by_ref() {
        let Ok(line) = &row.outcome else {
            return Ok(None);
        };
        let first_line = *first.get_or_insert(line);
        moved += i128::from(line.amount.minor());
        if let Some(stated) = line.balance_after {
            if Some(first_line.date) != earliest {
                return Ok(None);
            }
            let Some(date) = first_line.date.pred_opt() else {
                return Ok(None);
            };
            let balance = money(i128::from(stated.minor()) - moved, currency)?;
            return Ok(Some((date, balance)));
        }
    }
    Ok(None)
}

/// What the account holds before the import.
#[derive(Debug, Default)]
pub(super) struct History {
    /// By date, then the opening balance, then stored balances by ID.
    pub assertions: Vec<(CheckedBalance, Assertion)>,
    pub movements: DailyMovements,
}

/// Checks the account's balances with and without the import, and keeps
/// the checks that matter for it: every balance it adds, every stored one
/// whose result it changes, and the stored ones in `restated`.
pub(super) fn checks(
    history: &History,
    added: &[(usize, Assertion)],
    new_movements: &DailyMovements,
    restated: &HashSet<BalanceId>,
    currency: Currency,
) -> Vec<BalanceCheck> {
    let stored: Vec<Assertion> = history.assertions.iter().map(|(_, a)| *a).collect();
    let before = balance::check(&stored, &history.movements);

    let mut combined: Vec<(CheckedBalance, Assertion)> = history.assertions.clone();
    combined.extend(
        added
            .iter()
            .map(|&(index, assertion)| (CheckedBalance::FromFile(index), assertion)),
    );
    // Stable: on one day, stored balances stay ahead of the file's.
    combined.sort_by_key(|(_, assertion)| assertion.date);
    let mut movements = history.movements.clone();
    for (date, sum) in new_movements {
        *movements.entry(*date).or_default() += sum;
    }
    let after_assertions: Vec<Assertion> = combined.iter().map(|(_, a)| *a).collect();
    let after = balance::check(&after_assertions, &movements);

    let before_of = |balance: CheckedBalance| {
        history
            .assertions
            .iter()
            .position(|(stored, _)| *stored == balance)
            .map(|index| before[index])
    };
    combined
        .iter()
        .zip(after)
        .filter_map(|(&(balance, assertion), after)| {
            let before = match balance {
                CheckedBalance::FromFile(_) => None,
                stored => before_of(stored),
            };
            let keep = match balance {
                CheckedBalance::FromFile(_) => true,
                CheckedBalance::Stored(id) if restated.contains(&id) => true,
                _ => before != Some(after),
            };
            keep.then_some(BalanceCheck {
                balance,
                date: assertion.date,
                stated: Money::new(assertion.balance_minor, currency),
                before,
                after,
            })
        })
        .collect()
}

fn money(minor: i128, currency: Currency) -> Result<Money, Error> {
    i64::try_from(minor)
        .map(|minor| Money::new(minor, currency))
        .map_err(|_| Error::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::statement::{Field, RowProblem, SourceFormat};

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    fn gbp(minor: i64) -> Money {
        Money::new(minor, Currency::GBP)
    }

    /// `(day, amount, balance after)`; `None` for a flagged row.
    fn statement(rows: &[Option<(u32, i64, Option<i64>)>]) -> Statement {
        let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
        for (index, row) in rows.iter().enumerate() {
            let location = SourceLocation::Line(index as u64 + 2);
            let outcome = match *row {
                Some((d, amount, balance)) => {
                    let mut line = StatementLine::new(day(d), gbp(amount), "x");
                    line.balance_after = balance.map(gbp);
                    Ok(line)
                }
                None => Err(RowProblem::Missing(Field::Amount)),
            };
            statement.push(location, outcome);
        }
        statement
    }

    #[test]
    fn no_running_balances() {
        let file = statement(&[Some((1, -500, None)), Some((2, 100, None))]);
        assert_eq!(running(&file).unwrap(), None);
    }

    #[test]
    fn oldest_first_balances_check_and_give_the_opening() {
        let file = statement(&[
            Some((1, -500, Some(9_500))),
            Some((1, -250, None)),
            Some((2, 1_000, Some(10_250))),
        ]);
        let (running, opening) = running(&file).unwrap().unwrap();
        assert_eq!(running.order, FileOrder::OldestFirst);
        assert_eq!(running.checked, 1);
        assert_eq!(running.mismatches, []);
        assert_eq!(opening, Some((day(1).pred_opt().unwrap(), gbp(10_000))));
    }

    #[test]
    fn newest_first_files_are_read_backwards() {
        let file = statement(&[
            Some((3, 1_000, Some(10_250))),
            Some((2, -250, Some(9_250))),
            Some((1, -500, Some(9_500))),
        ]);
        let (running, opening) = running(&file).unwrap().unwrap();
        assert_eq!(running.order, FileOrder::NewestFirst);
        assert_eq!(running.checked, 2);
        assert_eq!(running.mismatches, []);
        assert_eq!(opening, Some((day(1).pred_opt().unwrap(), gbp(10_000))));
    }

    #[test]
    fn a_wrong_balance_is_a_mismatch_and_the_chain_resyncs() {
        let file = statement(&[
            Some((1, -500, Some(9_500))),
            Some((2, -250, Some(9_000))),
            Some((3, -100, Some(8_900))),
        ]);
        let (running, _) = running(&file).unwrap().unwrap();
        assert_eq!(running.order, FileOrder::OldestFirst);
        assert_eq!(running.checked, 2);
        assert_eq!(
            running.mismatches,
            [RunningMismatch {
                location: SourceLocation::Line(3),
                computed: gbp(9_250),
                stated: gbp(9_000),
            }]
        );
    }

    #[test]
    fn a_flagged_row_breaks_the_chain_and_hides_the_opening() {
        let file = statement(&[Some((1, -500, Some(9_500))), None, Some((3, -100, Some(1)))]);
        let (running, opening) = running(&file).unwrap().unwrap();
        assert_eq!(running.checked, 0);
        assert_eq!(running.mismatches, []);
        assert_eq!(opening, Some((day(1).pred_opt().unwrap(), gbp(10_000))));

        let flagged_first = statement(&[None, Some((1, -500, Some(9_500)))]);
        let (_, opening) = super::running(&flagged_first).unwrap().unwrap();
        assert_eq!(opening, None, "the flagged row's amount is unknown");
    }

    #[test]
    fn no_opening_when_the_first_row_is_not_the_earliest() {
        // Read oldest first (the 2nd, then the 3rd), but the 1st is in between.
        let file = statement(&[
            Some((2, -500, None)),
            Some((1, -250, None)),
            Some((3, -100, Some(9_500))),
        ]);
        let (running, opening) = running(&file).unwrap().unwrap();
        assert_eq!(running.order, FileOrder::OldestFirst);
        assert_eq!(opening, None);
    }

    fn at(d: u32, balance_minor: i64) -> Assertion {
        Assertion {
            date: day(d),
            balance_minor,
        }
    }

    #[test]
    fn checks_keep_what_the_import_adds_or_changes() {
        let history = History {
            assertions: vec![
                (CheckedBalance::Stored(BalanceId::new(1)), at(1, 10_000)),
                (CheckedBalance::Stored(BalanceId::new(2)), at(5, 9_000)),
                (CheckedBalance::Stored(BalanceId::new(3)), at(9, 9_000)),
            ],
            movements: [(day(3), -1_000)].into_iter().collect(),
        };
        // A new row inside the 5–9 window, and a new balance on the 10th.
        let new_movements = [(day(7), -200), (day(10), 50)].into_iter().collect();
        let checks = checks(
            &history,
            &[(0, at(10, 8_850))],
            &new_movements,
            &HashSet::new(),
            Currency::GBP,
        );
        assert_eq!(
            checks,
            [
                BalanceCheck {
                    balance: CheckedBalance::Stored(BalanceId::new(3)),
                    date: day(9),
                    stated: gbp(9_000),
                    before: Some(CheckResult::Passed { since: day(5) }),
                    after: CheckResult::Failed {
                        since: day(5),
                        computed_minor: 8_800,
                    },
                },
                BalanceCheck {
                    balance: CheckedBalance::FromFile(0),
                    date: day(10),
                    stated: gbp(8_850),
                    before: None,
                    after: CheckResult::Passed { since: day(5) },
                },
            ]
        );
        assert_eq!(checks[0].computed(), Some(gbp(8_800)));
        assert_eq!(checks[1].computed(), None);
    }

    #[test]
    fn status_summarizes_the_checks() {
        let running_ok = RunningBalances {
            order: FileOrder::OldestFirst,
            checked: 3,
            mismatches: vec![],
        };
        let check = |after| BalanceCheck {
            balance: CheckedBalance::FromFile(0),
            date: day(1),
            stated: gbp(1),
            before: None,
            after,
        };
        let status = |running, checks| Reconciliation { running, checks }.status();
        assert_eq!(status(None, vec![]), ReconciliationStatus::NoBalances);
        assert_eq!(
            status(None, vec![check(CheckResult::StartingPoint)]),
            ReconciliationStatus::StartingPoint
        );
        assert_eq!(
            status(
                Some(running_ok.clone()),
                vec![check(CheckResult::StartingPoint)]
            ),
            ReconciliationStatus::Reconciled
        );
        assert_eq!(
            status(
                Some(running_ok),
                vec![check(CheckResult::Failed {
                    since: day(1),
                    computed_minor: 0
                })]
            ),
            ReconciliationStatus::Mismatch
        );
    }
}
