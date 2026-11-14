//! Category colours (`docs/DESIGN.md` → "Charts" → "Categorical palette";
//! ROADMAP M3.6, user decision 2026-11-14).
//!
//! The design gives a category one of six chart colours "by spending rank
//! over the last 12 months, then stored on the category". Nothing stores it
//! yet, so [`category_palette`] ranks at read time: the six spending
//! categories with the most spent in the 12 months up to `today` get slots
//! 1 to 6, in that order; every other category is "Other" (the outline
//! marker), and uncategorised keeps its dashed marker. The app maps a slot
//! to `chart-1` … `chart-6`.
//!
//! Spending is [`super::spending`]'s: lines that count as spending, as the
//! user's share, money in under a spending category taking off its total.
//! Amounts in different currencies are added as minor units: only the
//! order matters here, and conversion comes with M4.6. Ties go to the
//! smaller key, so the order is stable.

use std::collections::HashMap;

use chrono::{Months, NaiveDate};

use crate::category::CategoryKey;
use crate::db::Reader;

use super::summary::{Side, side};
use super::{Basis, DateRange, Error, Ledger};

/// How many categories get a colour of their own.
pub const SLOTS: usize = 6;

/// Which categories have a colour of their own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Palette {
    slots: HashMap<CategoryKey, u8>,
}

impl Palette {
    /// A palette from categories in rank order: the first gets slot 1. Only
    /// the first [`SLOTS`] count.
    pub fn ranked<'a>(keys: impl IntoIterator<Item = &'a CategoryKey>) -> Palette {
        let slots = keys
            .into_iter()
            .take(SLOTS)
            .zip(1u8..)
            .map(|(key, slot)| (key.clone(), slot))
            .collect();
        Palette { slots }
    }

    /// The category's colour slot, 1 to [`SLOTS`]; `None` for "Other".
    pub fn slot(&self, key: &CategoryKey) -> Option<u8> {
        self.slots.get(key).copied()
    }
}

/// The palette for `today` (module docs).
pub fn category_palette(reader: &Reader, today: NaiveDate) -> Result<Palette, Error> {
    let from = today
        .checked_sub_months(Months::new(12))
        .and_then(|date| date.succ_opt())
        .unwrap_or(NaiveDate::MIN);
    let lines = Ledger::new()
        .within(DateRange::new(from, today)?)
        .lines(reader)?;
    let mut spent: HashMap<CategoryKey, i128> = HashMap::new();
    for line in &lines {
        let counted = line.counted(Basis::Personal);
        if counted.is_zero() || side(line, counted) != Side::Spending {
            continue;
        }
        if let Some(category) = &line.category {
            *spent.entry(category.key.clone()).or_default() -= i128::from(counted.minor());
        }
    }
    let mut ranked: Vec<(CategoryKey, i128)> =
        spent.into_iter().filter(|(_, total)| *total > 0).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(Palette::ranked(ranked.iter().map(|(key, _)| key)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::money::Currency;
    use crate::reporting::test_ledger::TestLedger;

    fn key(key: &str) -> CategoryKey {
        CategoryKey::new(key).unwrap()
    }

    #[test]
    fn ranks_the_last_twelve_months_of_spending() {
        let mut ledger = TestLedger::new();
        let account = ledger.account("Current account", Currency::GBP);
        let today = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
        // Groceries most, then eating out; transport only before the window.
        ledger.row(account, "2026-11-01", -5_000, Some("food.groceries"));
        ledger.row(account, "2026-06-01", -4_000, Some("food.groceries"));
        ledger.row(account, "2026-10-01", -6_000, Some("food.eating_out"));
        ledger.row(account, "2025-11-14", -50_000, Some("transport.public"));
        // Money in under a spending category takes off its total.
        ledger.row(account, "2026-10-02", 1_000, Some("food.eating_out"));
        // Income never ranks.
        ledger.row(account, "2026-10-28", 300_000, Some("income.salary"));
        let palette = category_palette(&ledger.reader(), today).unwrap();
        assert_eq!(palette.slot(&key("food.groceries")), Some(1));
        assert_eq!(palette.slot(&key("food.eating_out")), Some(2));
        assert_eq!(palette.slot(&key("transport.public")), None);
        assert_eq!(palette.slot(&key("income.salary")), None);
    }

    #[test]
    fn six_slots_then_other_and_ties_by_key() {
        let keys: Vec<CategoryKey> = ["a", "b", "c", "d", "e", "f", "g"]
            .iter()
            .map(|k| key(k))
            .collect();
        let palette = Palette::ranked(&keys);
        assert_eq!(palette.slot(&key("a")), Some(1));
        assert_eq!(palette.slot(&key("f")), Some(6));
        assert_eq!(palette.slot(&key("g")), None);
    }
}
