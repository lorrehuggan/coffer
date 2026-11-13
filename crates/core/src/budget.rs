//! Budgets: how much the user means to spend in a category each month
//! (`budget`, `migrations/V1__initial_schema.sql`; `docs/ARCHITECTURE.md` →
//! "Decision record: budgets and the forecast").
//!
//! A budget belongs to one `spending` category (a group or a subcategory):
//! budgets on `income` or `transfer` categories are refused. It is either
//! **standing** (`month` is `None`: every month) or for **one month**,
//! which overrides the standing budget for that month only. There is at
//! most one of each per category (the schema's unique indexes), so
//! [`set`] replaces what is there.
//!
//! # Groups and rollover
//!
//! Each budget is in a [`BudgetGroup`]: `fixed`, `flex` or `non_monthly`.
//! Rollover follows the group, as the schema's CHECKs require: a
//! `non_monthly` budget always rolls over, a `fixed` one never does, and a
//! `flex` one doesn't for now. M4.9 adds the rollover toggle for `flex`
//! budgets, the non-monthly set-aside and the rollover arithmetic; until
//! then rollover is stored and reported, never applied.
//!
//! # Order
//!
//! [`list`] returns standing budgets first, then month budgets by month,
//! each in category tree order (groups in the user's order, each followed
//! by its subcategories).
//!
//! Errors never contain transaction text: budgets hold a category key, an
//! amount and a month, nothing from the bank.

use std::fmt;

use rusqlite::{Connection, OptionalExtension, params};

use crate::category::{CategoryKey, CategoryKind, CategoryTree};
use crate::db::Database;
use crate::money::{Currency, Money};
use crate::reporting::Period;

/// Which kind of budget it is (`budget.group_kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BudgetGroup {
    /// The same every month (rent, subscriptions). Never rolls over.
    Fixed,
    /// Day-to-day spending (groceries, eating out). The default.
    Flex,
    /// Spent a few times a year (insurance, gifts), set aside monthly.
    /// Always rolls over.
    NonMonthly,
}

impl BudgetGroup {
    pub const ALL: [BudgetGroup; 3] = [
        BudgetGroup::Fixed,
        BudgetGroup::Flex,
        BudgetGroup::NonMonthly,
    ];

    /// The key stored in `budget.group_kind`.
    pub const fn as_str(self) -> &'static str {
        match self {
            BudgetGroup::Fixed => "fixed",
            BudgetGroup::Flex => "flex",
            BudgetGroup::NonMonthly => "non_monthly",
        }
    }

    pub fn from_key(key: &str) -> Option<BudgetGroup> {
        Self::ALL.into_iter().find(|group| group.as_str() == key)
    }

    /// Whether a budget in this group rolls over (module docs): only
    /// `non_monthly` ones, until M4.9 adds a toggle for `flex`.
    pub const fn rollover(self) -> bool {
        matches!(self, BudgetGroup::NonMonthly)
    }
}

impl fmt::Display for BudgetGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A stored budget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Budget {
    pub category: CategoryKey,
    /// `None` for a standing budget (every month).
    pub month: Option<Period>,
    /// What may be spent in the month, never negative.
    pub amount: Money,
    pub group: BudgetGroup,
    /// Stored and reported; applied from M4.9.
    pub rollover: bool,
}

/// A budget to set ([`set`]). Rollover follows the group
/// ([`BudgetGroup::rollover`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewBudget {
    pub category: CategoryKey,
    /// `None` for a standing budget; a month overrides it for that month.
    pub month: Option<Period>,
    /// At least zero.
    pub amount: Money,
    pub group: BudgetGroup,
}

/// What [`set`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Saved {
    /// There was no budget for the category and month.
    Added,
    /// The one there was replaced.
    Replaced,
}

/// Why a budget operation failed. Messages never contain transaction text.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("there is no category {0}")]
    UnknownCategory(CategoryKey),

    #[error("{category} is a {kind} category; budgets are for spending categories only")]
    NotSpending {
        category: CategoryKey,
        kind: CategoryKind,
    },

    #[error("a budget can't be negative")]
    NegativeAmount,

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// Sets the budget for `budget.category` and `budget.month`, replacing the
/// one there is. The category must exist and be a spending category; the
/// amount must be at least zero.
pub fn set(db: &mut Database, budget: &NewBudget) -> Result<Saved, Error> {
    if budget.amount.is_negative() {
        return Err(Error::NegativeAmount);
    }
    let tx = db.conn_mut().transaction()?;
    let category_id = spending_category(&tx, &budget.category)?;
    let month = budget.month.map(|month| month.key());
    let existing: Option<i64> = tx
        .query_row(
            "SELECT id FROM budget WHERE category_id = ?1 AND month IS ?2",
            params![category_id, month],
            |row| row.get(0),
        )
        .optional()?;
    let rollover = budget.group.rollover();
    let saved = match existing {
        Some(id) => {
            // A set-aside belongs to a non-monthly budget only (schema
            // CHECK): kept while it stays non-monthly, dropped otherwise.
            tx.execute(
                "UPDATE budget SET amount_minor = ?2, currency = ?3, rollover = ?4, \
                        group_kind = ?5, \
                        opening_set_aside_minor = \
                            CASE WHEN ?5 = 'non_monthly' THEN opening_set_aside_minor END \
                 WHERE id = ?1",
                params![
                    id,
                    budget.amount.minor(),
                    budget.amount.currency(),
                    rollover,
                    budget.group.as_str(),
                ],
            )?;
            Saved::Replaced
        }
        None => {
            tx.execute(
                "INSERT INTO budget (category_id, month, amount_minor, currency, rollover, \
                                     group_kind) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    category_id,
                    month,
                    budget.amount.minor(),
                    budget.amount.currency(),
                    rollover,
                    budget.group.as_str(),
                ],
            )?;
            Saved::Added
        }
    };
    tx.commit()?;
    Ok(saved)
}

/// Removes the budget for `category` and `month` (`None`: the standing
/// one). `false` when there was none. A month's budget and the standing
/// one are separate: removing one leaves the other.
pub fn remove(
    db: &mut Database,
    category: &CategoryKey,
    month: Option<Period>,
) -> Result<bool, Error> {
    let conn = db.conn();
    let tree = CategoryTree::load(conn)?;
    let id = tree
        .get(category)
        .ok_or_else(|| Error::UnknownCategory(category.clone()))?
        .id();
    let removed = conn.execute(
        "DELETE FROM budget WHERE category_id = ?1 AND month IS ?2",
        params![id, month.map(|month| month.key())],
    )?;
    Ok(removed > 0)
}

/// Every budget, standing ones first, then by month, each in category tree
/// order.
pub fn list(db: &Database) -> Result<Vec<Budget>, Error> {
    Ok(stored(db.conn())?
        .into_iter()
        .map(|budget| budget.budget)
        .collect())
}

/// A budget with its category's row ID and its place in the tree.
pub(crate) struct StoredBudget {
    pub category_id: i64,
    pub budget: Budget,
}

/// Every budget, in [`list`]'s order. For [`crate::reporting`] too.
pub(crate) fn stored(conn: &Connection) -> rusqlite::Result<Vec<StoredBudget>> {
    let tree = CategoryTree::load(conn)?;
    let mut statement = conn.prepare_cached(
        "SELECT b.category_id, c.key, b.month, b.amount_minor, b.currency, b.group_kind, \
                b.rollover \
         FROM budget b JOIN category c ON c.id = b.category_id",
    )?;
    let rows = statement.query_map([], |row| {
        let month: Option<String> = row.get(2)?;
        let month = month
            .map(|text| text.parse::<Period>().map_err(|_| invalid(2, "month")))
            .transpose()?;
        let currency: Currency = row.get(4)?;
        let group: String = row.get(5)?;
        let group = BudgetGroup::from_key(&group).ok_or_else(|| invalid(5, "group_kind"))?;
        Ok(StoredBudget {
            category_id: row.get(0)?,
            budget: Budget {
                category: row.get(1)?,
                month,
                amount: Money::new(row.get(3)?, currency),
                group,
                rollover: row.get(6)?,
            },
        })
    })?;
    let mut budgets: Vec<StoredBudget> = rows.collect::<Result<_, _>>()?;
    let position = |id: i64| tree.iter().position(|category| category.id() == id);
    budgets.sort_by_key(|stored| (stored.budget.month, position(stored.category_id)));
    Ok(budgets)
}

/// The ID of the spending category `key`.
fn spending_category(conn: &Connection, key: &CategoryKey) -> Result<i64, Error> {
    let tree = CategoryTree::load(conn)?;
    let category = tree
        .get(key)
        .ok_or_else(|| Error::UnknownCategory(key.clone()))?;
    if category.kind() != CategoryKind::Spending {
        return Err(Error::NotSpending {
            category: key.clone(),
            kind: category.kind(),
        });
    }
    Ok(category.id())
}

/// A stored value the schema should have refused: the file was changed
/// outside Coffer.
fn invalid(column: usize, name: &'static str) -> rusqlite::Error {
    #[derive(Debug, thiserror::Error)]
    #[error("budget.{0} holds a value Coffer doesn't know")]
    struct Invalid(&'static str);
    rusqlite::Error::FromSqlConversionFailure(
        column,
        rusqlite::types::Type::Text,
        Box::new(Invalid(name)),
    )
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn db() -> (TempDir, Database) {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("coffer.db")).unwrap();
        (dir, db)
    }

    fn key(text: &str) -> CategoryKey {
        CategoryKey::new(text).unwrap()
    }

    fn month(text: &str) -> Option<Period> {
        Some(text.parse().unwrap())
    }

    fn gbp(minor: i64) -> Money {
        Money::new(minor, Currency::GBP)
    }

    fn budget(category: &str, month: Option<Period>, minor: i64, group: BudgetGroup) -> NewBudget {
        NewBudget {
            category: key(category),
            month,
            amount: gbp(minor),
            group,
        }
    }

    fn summary(db: &Database) -> Vec<(String, Option<String>, i64, &'static str, bool)> {
        list(db)
            .unwrap()
            .into_iter()
            .map(|b| {
                (
                    b.category.as_str().to_owned(),
                    b.month.map(|m| m.key()),
                    b.amount.minor(),
                    b.group.as_str(),
                    b.rollover,
                )
            })
            .collect()
    }

    #[test]
    fn groups_read_and_write_their_keys() {
        for group in BudgetGroup::ALL {
            assert_eq!(BudgetGroup::from_key(group.as_str()), Some(group));
        }
        assert_eq!(BudgetGroup::from_key("monthly"), None);
        assert!(BudgetGroup::NonMonthly.rollover());
        assert!(!BudgetGroup::Fixed.rollover());
        assert!(!BudgetGroup::Flex.rollover());
    }

    #[test]
    fn set_adds_then_replaces_per_category_and_month() {
        let (_dir, mut db) = db();
        let standing = budget("food.groceries", None, 25_000, BudgetGroup::Flex);
        assert_eq!(set(&mut db, &standing).unwrap(), Saved::Added);
        let december = budget(
            "food.groceries",
            month("2026-12"),
            40_000,
            BudgetGroup::Flex,
        );
        assert_eq!(set(&mut db, &december).unwrap(), Saved::Added);
        let changed = budget("food.groceries", None, 30_000, BudgetGroup::Fixed);
        assert_eq!(set(&mut db, &changed).unwrap(), Saved::Replaced);
        assert_eq!(
            summary(&db),
            [
                ("food.groceries".into(), None, 30_000, "fixed", false),
                (
                    "food.groceries".into(),
                    Some("2026-12".into()),
                    40_000,
                    "flex",
                    false
                ),
            ]
        );
    }

    #[test]
    fn rollover_follows_the_group() {
        let (_dir, mut db) = db();
        set(
            &mut db,
            &budget("shopping", None, 12_000, BudgetGroup::NonMonthly),
        )
        .unwrap();
        db.conn()
            .execute("UPDATE budget SET opening_set_aside_minor = 5000", [])
            .unwrap();
        // Still non-monthly: the set-aside stays.
        set(
            &mut db,
            &budget("shopping", None, 15_000, BudgetGroup::NonMonthly),
        )
        .unwrap();
        let set_aside = |db: &Database| -> Option<i64> {
            db.conn()
                .query_row("SELECT opening_set_aside_minor FROM budget", [], |row| {
                    row.get(0)
                })
                .unwrap()
        };
        assert_eq!(set_aside(&db), Some(5000));
        assert!(summary(&db)[0].4);
        // Flex: no rollover yet, and the set-aside goes (schema CHECK).
        set(
            &mut db,
            &budget("shopping", None, 15_000, BudgetGroup::Flex),
        )
        .unwrap();
        assert_eq!(set_aside(&db), None);
        assert!(!summary(&db)[0].4);
    }

    #[test]
    fn refuses_unknown_and_non_spending_categories_and_negative_amounts() {
        let (_dir, mut db) = db();
        assert!(matches!(
            set(&mut db, &budget("nope", None, 100, BudgetGroup::Flex)),
            Err(Error::UnknownCategory(_))
        ));
        for (category, kind) in [
            ("income", CategoryKind::Income),
            ("income.salary", CategoryKind::Income),
            ("transfer", CategoryKind::Transfer),
        ] {
            let error = set(&mut db, &budget(category, None, 100, BudgetGroup::Flex)).unwrap_err();
            assert!(
                matches!(&error, Error::NotSpending { kind: found, .. } if *found == kind),
                "{category}: {error:?}"
            );
            assert!(error.to_string().contains("spending categories only"));
        }
        assert!(matches!(
            set(&mut db, &budget("food", None, -1, BudgetGroup::Flex)),
            Err(Error::NegativeAmount)
        ));
        // Zero is a budget: nothing to spend.
        set(&mut db, &budget("food", None, 0, BudgetGroup::Flex)).unwrap();
        assert!(list(&db).unwrap()[0].amount.is_zero());
    }

    #[test]
    fn remove_takes_only_the_named_budget() {
        let (_dir, mut db) = db();
        set(&mut db, &budget("food", None, 100, BudgetGroup::Flex)).unwrap();
        set(
            &mut db,
            &budget("food", month("2026-11"), 200, BudgetGroup::Flex),
        )
        .unwrap();
        assert!(remove(&mut db, &key("food"), month("2026-11")).unwrap());
        assert!(!remove(&mut db, &key("food"), month("2026-11")).unwrap());
        assert!(!remove(&mut db, &key("home"), None).unwrap());
        assert!(matches!(
            remove(&mut db, &key("nope"), None),
            Err(Error::UnknownCategory(_))
        ));
        assert_eq!(summary(&db), [("food".into(), None, 100, "flex", false)]);
    }

    #[test]
    fn list_is_standing_first_then_by_month_in_tree_order() {
        let (_dir, mut db) = db();
        for (category, month_key) in [
            ("shopping", Some("2026-12")),
            ("food.groceries", Some("2026-11")),
            ("shopping", None),
            ("food", Some("2026-12")),
            ("food.groceries", None),
        ] {
            let month = month_key.and_then(month);
            set(&mut db, &budget(category, month, 100, BudgetGroup::Flex)).unwrap();
        }
        let order: Vec<(String, Option<String>)> = summary(&db)
            .into_iter()
            .map(|(category, month, ..)| (category, month))
            .collect();
        let tree = db.categories().unwrap();
        let position = |k: &str| tree.iter().position(|c| c.key().as_str() == k).unwrap();
        assert!(position("food") < position("food.groceries"));
        assert!(position("food.groceries") < position("shopping"));
        assert_eq!(
            order,
            [
                ("food.groceries".into(), None),
                ("shopping".into(), None),
                ("food.groceries".into(), Some("2026-11".into())),
                ("food".into(), Some("2026-12".into())),
                ("shopping".into(), Some("2026-12".into())),
            ]
        );
    }
}
