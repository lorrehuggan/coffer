//! Balance assertions, checked against the transactions between them.
//!
//! Every balance an account has (entered by the user, or read from a file)
//! says what the account held at the end of a day. The earliest one is the
//! starting point; each later one is checked against the last one that
//! passed plus the transactions in between, and a failure names that window
//! (`docs/ARCHITECTURE.md` → "Decision record: review queues", point 1, and
//! "Reconciliation").
//!
//! Pure: callers load the balances and the per-day sums of transactions.
//! The import preview runs it before and after the rows it would add
//! (M1.12); M1.27's standing checks and lock build on the same function.
//! Sums are `i128`, so adding up stored `i64` amounts can't overflow.

use std::collections::BTreeMap;
use std::ops::Bound;

use chrono::NaiveDate;

/// The net movement of an account per day, in minor units. Days without
/// transactions may be missing.
pub type DailyMovements = BTreeMap<NaiveDate, i128>;

/// A balance at the end of `date`, in the account's minor units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Assertion {
    pub date: NaiveDate,
    pub balance_minor: i64,
}

/// What checking one assertion found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckResult {
    /// The account's earliest balance: where counting starts, not a check.
    StartingPoint,
    /// It equals the balance on `since` (the last one that passed) plus the
    /// transactions after that day, up to and including this one's.
    Passed { since: NaiveDate },
    /// It doesn't: the transactions give `computed_minor`. The gap is in the
    /// window after `since`, up to and including this assertion's date.
    Failed {
        since: NaiveDate,
        computed_minor: i128,
    },
}

impl CheckResult {
    pub fn is_failed(&self) -> bool {
        matches!(self, CheckResult::Failed { .. })
    }
}

/// Checks `assertions` in the order given, which must be by date (ties
/// keep their order). Returns one result per assertion, in the same order.
///
/// A failed assertion doesn't become the base for the next one: the window
/// of a later failure starts at the last one that passed.
pub fn check(assertions: &[Assertion], movements: &DailyMovements) -> Vec<CheckResult> {
    debug_assert!(assertions.is_sorted_by_key(|a| a.date));
    let mut base: Option<Assertion> = None;
    assertions
        .iter()
        .map(|assertion| {
            let Some(last) = base else {
                base = Some(*assertion);
                return CheckResult::StartingPoint;
            };
            let moved: i128 = movements
                .range((Bound::Excluded(last.date), Bound::Included(assertion.date)))
                .map(|(_, sum)| sum)
                .sum();
            let computed = i128::from(last.balance_minor) + moved;
            if computed == i128::from(assertion.balance_minor) {
                base = Some(*assertion);
                CheckResult::Passed { since: last.date }
            } else {
                CheckResult::Failed {
                    since: last.date,
                    computed_minor: computed,
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    fn at(d: u32, balance_minor: i64) -> Assertion {
        Assertion {
            date: day(d),
            balance_minor,
        }
    }

    fn movements(days: &[(u32, i128)]) -> DailyMovements {
        days.iter().map(|&(d, sum)| (day(d), sum)).collect()
    }

    #[test]
    fn nothing_to_check() {
        assert_eq!(check(&[], &movements(&[(1, -500)])), []);
        assert_eq!(
            check(&[at(1, 10_000)], &DailyMovements::new()),
            [CheckResult::StartingPoint]
        );
    }

    #[test]
    fn the_first_balance_is_the_starting_point_and_later_ones_are_checked() {
        // Transactions on the starting day are already in its balance.
        let moved = movements(&[(1, -9_999), (2, -500), (3, 1_000), (5, -250)]);
        let results = check(&[at(1, 10_000), at(3, 10_500), at(5, 10_250)], &moved);
        assert_eq!(
            results,
            [
                CheckResult::StartingPoint,
                CheckResult::Passed { since: day(1) },
                CheckResult::Passed { since: day(3) },
            ]
        );
    }

    #[test]
    fn a_failure_names_the_window_since_the_last_pass() {
        let moved = movements(&[(2, -500), (4, -4_250)]);
        let results = check(
            &[at(1, 10_000), at(3, 9_500), at(4, 5_000), at(6, 5_250)],
            &moved,
        );
        assert_eq!(
            results,
            [
                CheckResult::StartingPoint,
                CheckResult::Passed { since: day(1) },
                CheckResult::Failed {
                    since: day(3),
                    computed_minor: 5_250,
                },
                // The failure isn't a base: 5_250 is right from day 3.
                CheckResult::Passed { since: day(3) },
            ]
        );
        assert!(results[2].is_failed());
    }

    #[test]
    fn two_balances_on_one_day_must_agree() {
        let results = check(&[at(1, 100), at(1, 100), at(1, 90)], &movements(&[(1, 5)]));
        assert_eq!(
            results,
            [
                CheckResult::StartingPoint,
                CheckResult::Passed { since: day(1) },
                CheckResult::Failed {
                    since: day(1),
                    computed_minor: 100,
                },
            ]
        );
    }

    #[test]
    fn sums_do_not_overflow() {
        let moved = movements(&[(2, i128::from(i64::MAX)), (3, i128::from(i64::MAX))]);
        let results = check(&[at(1, i64::MAX), at(3, 0)], &moved);
        assert_eq!(
            results[1],
            CheckResult::Failed {
                since: day(1),
                computed_minor: 3 * i128::from(i64::MAX),
            }
        );
    }
}
