//! Category colours for the Inbox (`docs/DESIGN.md` → "Charts" →
//! "Categorical palette"; ROADMAP M3.6, user decision 2026-11-14).
//!
//! The design gives a category one of six chart colours "by spending rank
//! over the last 12 months, then stored on the category". Nothing stores it
//! yet (M4.1), so the colours are ranked at read time by the same rule as
//! the Transactions register ([`super::register`] → "Category colours"):
//! the six top-level groups with the most spending in the 12 months up to
//! `today` take slots 1 to 6, their subcategories share the group's slot,
//! and everything else is "Other" (the outline marker); uncategorised keeps
//! its dashed marker. The app maps a slot to `chart-1` … `chart-6`. Sharing
//! the ranking keeps a category's colour the same on both screens.

use std::collections::HashMap;

use chrono::NaiveDate;

use crate::category::{CategoryKey, CategoryTree};
use crate::db::Reader;

use super::Error;
use super::register::{NAMED_COLOURS, ranked_groups};

/// How many categories get a colour of their own.
pub const SLOTS: usize = NAMED_COLOURS;

/// Which categories have a colour of their own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Palette {
    slots: HashMap<CategoryKey, u8>,
}

impl Palette {
    /// The category's colour slot, 1 to [`SLOTS`]; `None` for "Other".
    pub fn slot(&self, key: &CategoryKey) -> Option<u8> {
        self.slots.get(key).copied()
    }
}

/// The palette for `today` (module docs).
pub fn category_palette(reader: &Reader, today: NaiveDate) -> Result<Palette, Error> {
    let tree = CategoryTree::load(reader.conn())?;
    let ranked = ranked_groups(reader, &tree, today)?;
    let mut slots = HashMap::new();
    for group in tree.groups() {
        let parent = group.category();
        let Some(rank) = ranked.iter().position(|&id| id == parent.id()) else {
            continue;
        };
        let slot = rank as u8 + 1;
        slots.insert(parent.key().clone(), slot);
        for child in group.children() {
            slots.insert(child.key().clone(), slot);
        }
    }
    Ok(Palette { slots })
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
    fn ranks_groups_over_the_last_twelve_months_of_spending() {
        let mut ledger = TestLedger::new();
        let account = ledger.account("Current account", Currency::GBP);
        let today = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
        // Food most (its subcategories added up), then bills; transport
        // only before the window.
        ledger.row(account, "2026-11-01", -5_000, Some("food.groceries"));
        ledger.row(account, "2026-10-01", -6_000, Some("food.eating_out"));
        ledger.row(account, "2026-10-05", -8_000, Some("bills"));
        ledger.row(account, "2025-11-14", -50_000, Some("transport.public"));
        // Income never ranks.
        ledger.row(account, "2026-10-28", 300_000, Some("income.salary"));
        let palette = category_palette(&ledger.reader(), today).unwrap();
        assert_eq!(palette.slot(&key("food")), Some(1));
        assert_eq!(palette.slot(&key("food.groceries")), Some(1));
        assert_eq!(palette.slot(&key("food.eating_out")), Some(1));
        assert_eq!(palette.slot(&key("bills")), Some(2));
        assert_eq!(palette.slot(&key("transport.public")), None);
        assert_eq!(palette.slot(&key("income.salary")), None);
    }

    #[test]
    fn agrees_with_the_transactions_register() {
        let mut ledger = TestLedger::new();
        let account = ledger.account("Current account", Currency::GBP);
        let today = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
        for (category, minor) in [
            ("food.groceries", -9000),
            ("bills", -8000),
            ("transport.public", -7000),
            ("shopping", -6000),
            ("entertainment", -5000),
            ("health", -4000),
            ("travel", -3000),
        ] {
            ledger.row(account, "2026-10-01", minor, Some(category));
        }
        ledger.row(account, "2026-10-01", -100_000, Some("other"));
        let reader = ledger.reader();
        let palette = category_palette(&reader, today).unwrap();
        let register = super::super::register::Register::load(&reader, today).unwrap();
        for category in register.categories() {
            let expected = match category.colour {
                super::super::register::Colour::Chart(slot) => Some(slot),
                super::super::register::Colour::Other => None,
            };
            assert_eq!(
                palette.slot(&category.key),
                expected,
                "{}",
                category.key.as_str()
            );
        }
    }
}
