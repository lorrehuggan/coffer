//! `budget_status`: budget against spending per category for a month.
//!
//! Every category with a budget for the month ([`crate::budget`]): its
//! budget for that month if it has one, otherwise its standing budget.
//! Spent is the user's share ([`Basis::Personal`]) of the month's lines in
//! the category, as a positive amount spent (refunds on their own date
//! reduce it); a group's budget counts its subcategories too. Only spending
//! in the budget's currency counts against it: spending in the category in
//! other currencies is reported separately, never converted (M4.6).
//! Remaining is budget minus spent, below zero when over.
//!
//! Rollover and the non-monthly set-aside are reported, not applied: they
//! come with M4.9. Budgets are in category tree order.

use std::collections::HashMap;

use crate::budget::{self, BudgetGroup};
use crate::category::{CategoryKey, CategoryTree};
use crate::db::Reader;
use crate::money::Money;

use super::summary::{Side, side};
use super::{Basis, Error, Ledger, Period, Totals};

/// Every budget's status in one month.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetStatus {
    pub period: Period,
    /// In category tree order.
    pub budgets: Vec<BudgetLine>,
}

/// One category's budget against its spending.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetLine {
    pub category: CategoryKey,
    /// The user's label.
    pub label: String,
    pub group: BudgetGroup,
    /// Stored, not applied yet (M4.9).
    pub rollover: bool,
    /// The budget is the month's own, not the standing one.
    pub for_month: bool,
    pub budget: Money,
    /// Spent in the budget's currency, positive (refunds reduce it).
    pub spent: Money,
    /// `budget - spent`; below zero when over.
    pub remaining: Money,
    /// Spent in the category in other currencies, positive, per currency.
    pub spent_other_currencies: Totals,
}

/// How each budget stands in `month` (module docs).
pub fn budget_status(reader: &Reader, month: Period) -> Result<BudgetStatus, Error> {
    let conn = reader.conn();
    let tree = CategoryTree::load(conn)?;

    // The month's budget per category: its own over the standing one.
    // `stored` is standing first, so a month row comes later and wins.
    let mut chosen: Vec<budget::StoredBudget> = Vec::new();
    for stored in budget::stored(conn)? {
        if stored.budget.month.is_some_and(|m| m != month) {
            continue;
        }
        match chosen
            .iter_mut()
            .find(|c| c.category_id == stored.category_id)
        {
            Some(existing) => *existing = stored,
            None => chosen.push(stored),
        }
    }
    let position = |id: i64| tree.iter().position(|category| category.id() == id);
    chosen.sort_by_key(|stored| position(stored.category_id));

    // Spending per category ID, signed (money out below zero).
    let mut by_category: HashMap<i64, Totals> = HashMap::new();
    for line in Ledger::new().within(month.range()).lines(reader)? {
        let counted = line.counted(Basis::Personal);
        if counted.is_zero() || side(&line, counted) != Side::Spending {
            continue;
        }
        if let Some(category) = &line.category {
            by_category.entry(category.id).or_default().add(counted)?;
        }
    }

    let mut budgets = Vec::with_capacity(chosen.len());
    for stored in chosen {
        let budget = stored.budget;
        let mut signed = Totals::new();
        if let Some(own) = by_category.get(&stored.category_id) {
            signed.add_all(own)?;
        }
        // A group's budget covers its subcategories.
        if let Some(group) = tree
            .groups()
            .iter()
            .find(|group| group.category().id() == stored.category_id)
        {
            for child in group.children() {
                if let Some(spent) = by_category.get(&child.id()) {
                    signed.add_all(spent)?;
                }
            }
        }
        let currency = budget.amount.currency();
        let mut other = Totals::new();
        let mut spent = Money::zero(currency);
        for money in signed.negated()?.iter() {
            if money.currency() == currency {
                spent = money;
            } else {
                other.add(money)?;
            }
        }
        let label = tree
            .get(&budget.category)
            .map(|category| category.label().to_owned())
            .unwrap_or_default();
        budgets.push(BudgetLine {
            label,
            group: budget.group,
            rollover: budget.rollover,
            for_month: budget.month.is_some(),
            remaining: budget.amount.checked_sub(spent)?,
            budget: budget.amount,
            spent,
            spent_other_currencies: other,
            category: budget.category,
        });
    }
    Ok(BudgetStatus {
        period: month,
        budgets,
    })
}

#[cfg(test)]
mod tests;
