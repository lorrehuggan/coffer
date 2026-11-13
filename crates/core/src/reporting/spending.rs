//! `spending_by_category`: totals per category, nested under their groups.
//!
//! The spending lines of one month ([`super::summary`]'s classification:
//! lines in `spending` categories, and uncategorised money out), as the
//! user's share ([`Basis::Personal`]), shown as positive amounts spent.
//! Money in under a spending category, a refund's part on the refund's date
//! above all, reduces its category's total, which can go below zero in a
//! month with more refunds than purchases.
//!
//! The result follows the category tree's order. A group has its own total
//! (lines filed under the group itself), its subcategories' totals and a
//! group total (its own plus its subcategories'). Only categories with
//! activity in the month appear. Every total is per currency ([`Totals`]).

use std::collections::HashMap;

use crate::category::{Category, CategoryKey, CategoryKind, CategoryTree};
use crate::db::Reader;

use super::summary::{Side, side};
use super::{Basis, Error, Ledger, Period, Totals};

/// One month's spending by category.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategorySpending {
    pub period: Period,
    /// Groups with spending, in tree order.
    pub groups: Vec<GroupSpending>,
    /// Uncategorised money out.
    pub uncategorized: Totals,
    /// Everything: every group total plus uncategorised.
    pub total: Totals,
}

/// A group's spending.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupSpending {
    pub key: CategoryKey,
    /// The user's label.
    pub label: String,
    /// Lines filed under the group itself; empty when there are none.
    pub own: Totals,
    /// Subcategories with spending, in tree order.
    pub children: Vec<SubcategorySpending>,
    /// Its own plus its subcategories'.
    pub total: Totals,
}

/// A subcategory's spending.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubcategorySpending {
    pub key: CategoryKey,
    pub label: String,
    pub spent: Totals,
}

/// What was spent in `period`, by category (module docs).
pub fn spending_by_category(reader: &Reader, period: Period) -> Result<CategorySpending, Error> {
    let tree = CategoryTree::load(reader.conn())?;
    let lines = Ledger::new().within(period.range()).lines(reader)?;

    // Signed sums (money out below zero) per category ID.
    let mut by_category: HashMap<i64, Totals> = HashMap::new();
    let mut uncategorized = Totals::new();
    for line in &lines {
        let counted = line.counted(Basis::Personal);
        if counted.is_zero() || side(line, counted) != Side::Spending {
            continue;
        }
        match &line.category {
            Some(category) => by_category.entry(category.id).or_default().add(counted)?,
            None => uncategorized.add(counted)?,
        }
    }

    let spent = |category: &Category| -> Result<Option<Totals>, Error> {
        by_category
            .get(&category.id())
            .map(Totals::negated)
            .transpose()
    };
    let mut groups = Vec::new();
    let mut total = Totals::new();
    for group in tree.groups() {
        let category = group.category();
        if category.kind() != CategoryKind::Spending {
            continue;
        }
        let own = spent(category)?.unwrap_or_default();
        let mut group_total = own.clone();
        let mut children = Vec::new();
        for child in group.children() {
            if let Some(spent) = spent(child)? {
                group_total.add_all(&spent)?;
                children.push(SubcategorySpending {
                    key: child.key().clone(),
                    label: child.label().to_owned(),
                    spent,
                });
            }
        }
        if own.is_empty() && children.is_empty() {
            continue;
        }
        total.add_all(&group_total)?;
        groups.push(GroupSpending {
            key: category.key().clone(),
            label: category.label().to_owned(),
            own,
            children,
            total: group_total,
        });
    }
    let uncategorized = uncategorized.negated()?;
    total.add_all(&uncategorized)?;
    Ok(CategorySpending {
        period,
        groups,
        uncategorized,
        total,
    })
}

#[cfg(test)]
mod tests;
