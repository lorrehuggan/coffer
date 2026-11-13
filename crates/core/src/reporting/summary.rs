//! `monthly_summary`: income, spending and net per month.
//!
//! Every month from the first through the last gets an entry, empty months
//! included, each with per-currency totals ([`Totals`]: currencies are never
//! added together). Amounts are the user's share ([`Basis::Personal`]) of
//! the lines in reports ([`Ledger`]), so a split purchase counts per line, a
//! shared cost counts as the user's part, a purchase being claimed back
//! counts as nothing, and excluded rows and confirmed transfers are left
//! out.
//!
//! Each line is classified by its category's kind:
//!
//! - `income`: income;
//! - `spending`: spending, whatever its sign: money in under a spending
//!   category (a refund's part, which carries its purchase's category on
//!   the refund's date, or an unlinked refund) lowers spending and is never
//!   income;
//! - `transfer`: neither (money moving between the user's own accounts that
//!   isn't a confirmed pair);
//! - uncategorised: by the sign of the amount that counts: money out is
//!   spending, money in is income.
//!
//! Spending is shown as a positive amount spent (it goes below zero only
//! when refunds outweigh purchases), income as positive, and net is income
//! minus spending.

use std::collections::BTreeMap;

use crate::category::CategoryKind;
use crate::db::Reader;
use crate::money::Money;

use super::{Basis, Capped, DateRange, Error, Ledger, LedgerLine, Limit, Period, Totals};

/// One month's totals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonthSummary {
    pub period: Period,
    /// Money earned or received, positive.
    pub income: Totals,
    /// Money spent, positive; refunds reduce it.
    pub spending: Totals,
    /// Income minus spending, per currency.
    pub net: Totals,
}

/// Where a line counts in the summary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Income,
    Spending,
    Neither,
}

/// Which side `line` counts on (module docs), for an amount `counted`.
pub(crate) fn side(line: &LedgerLine, counted: Money) -> Side {
    match line.category.as_ref().map(|category| category.kind) {
        Some(CategoryKind::Income) => Side::Income,
        Some(CategoryKind::Spending) => Side::Spending,
        Some(CategoryKind::Transfer) => Side::Neither,
        None if counted.is_negative() => Side::Spending,
        None if counted.is_positive() => Side::Income,
        None => Side::Neither,
    }
}

/// Income, spending and net for every month from `first` through `last`
/// (module docs). A range that ends before it starts is
/// [`Error::EmptyRange`]; one longer than [`Limit::MAX`] months is cut
/// there, with `truncated` set.
pub fn monthly_summary(
    reader: &Reader,
    first: Period,
    last: Period,
) -> Result<Capped<MonthSummary>, Error> {
    let periods = Capped::new(DateRange::months(first, last)?.periods(), Limit::MAX);
    // A valid range has at least one month.
    let cut = periods.items.last().copied().unwrap_or(last);
    let lines = Ledger::new()
        .within(DateRange::months(first, cut)?)
        .lines(reader)?;

    // Income, and spending as the signed sum (below zero), per month.
    let mut months: BTreeMap<Period, (Totals, Totals)> = BTreeMap::new();
    for line in &lines {
        let counted = line.counted(Basis::Personal);
        if counted.is_zero() {
            continue;
        }
        let (income, spending) = months.entry(Period::containing(line.date)).or_default();
        match side(line, counted) {
            Side::Income => income.add(counted)?,
            Side::Spending => spending.add(counted)?,
            Side::Neither => {}
        }
    }

    let mut summaries = Vec::with_capacity(periods.len());
    for &period in &periods.items {
        let (income, signed_spending) = months.remove(&period).unwrap_or_default();
        let mut net = Totals::new();
        net.add_all(&income)?;
        net.add_all(&signed_spending)?;
        summaries.push(MonthSummary {
            period,
            income,
            spending: signed_spending.negated()?,
            net,
        });
    }
    Ok(Capped {
        items: summaries,
        truncated: periods.truncated,
    })
}

#[cfg(test)]
mod tests;
