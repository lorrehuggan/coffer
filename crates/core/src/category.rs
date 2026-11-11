//! Categories: their stable keys, their kinds, the default set and the tree
//! read back from the database.
//!
//! A category has a translatable label for people and a fixed key for
//! everything else (`category.key` in the schema). Keys are one or two
//! segments of lowercase ASCII letters, digits and `_`, joined by a dot:
//! `transfer`, `food.groceries`. The second segment is a subcategory of the
//! first, matching the two-level category tree.
//!
//! Every new database starts with [`DEFAULT_CATEGORIES`]
//! (`migrations/V2__default_categories.sql`). [`CategoryTree`] is what a
//! database holds now: the defaults, as the user renamed them, plus their own.
//! `docs/ARCHITECTURE.md` → "Categories" has the set and how keys stay
//! stable through renames, additions and deletions.

mod defaults;

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use rusqlite::Connection;
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};

pub use defaults::{DEFAULT_CATEGORIES, DefaultCategory, default_category};

use crate::rule::UnknownKey;

/// A category's stable key, e.g. `food.groceries`.
///
/// Only the spelling is checked here. Whether a category with this key
/// exists is the database's question.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CategoryKey(String);

/// Text that isn't a valid category key. Keys come from presets and rules,
/// not from transactions, so the text is kept for the message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "{0:?} isn't a category key: use one or two parts of lowercase letters, digits and `_`, \
     joined by a dot (`food.groceries`)"
)]
pub struct CategoryKeyError(pub String);

impl CategoryKey {
    pub fn new(key: &str) -> Result<CategoryKey, CategoryKeyError> {
        let segment_ok = |segment: &str| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        };
        let mut segments = key.split('.');
        let valid = segments.next().is_some_and(segment_ok)
            && segments.next().is_none_or(segment_ok)
            && segments.next().is_none();
        if valid {
            Ok(CategoryKey(key.to_owned()))
        } else {
            Err(CategoryKeyError(key.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The top-level key of a subcategory (`food` for `food.groceries`),
    /// `None` for a top-level key.
    pub fn parent(&self) -> Option<&str> {
        self.0.split_once('.').map(|(parent, _)| parent)
    }
}

impl fmt::Display for CategoryKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for CategoryKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CategoryKey({})", self.0)
    }
}

impl FromStr for CategoryKey {
    type Err = CategoryKeyError;

    fn from_str(key: &str) -> Result<CategoryKey, CategoryKeyError> {
        CategoryKey::new(key)
    }
}

impl serde::Serialize for CategoryKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for CategoryKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<CategoryKey, D::Error> {
        let key = String::deserialize(deserializer)?;
        CategoryKey::new(&key).map_err(serde::de::Error::custom)
    }
}

impl ToSql for CategoryKey {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(ValueRef::Text(self.0.as_bytes())))
    }
}

/// A stored key is checked like any other: the schema's CHECK and this agree,
/// so a failure means the file was changed outside Coffer.
impl FromSql for CategoryKey {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<CategoryKey> {
        CategoryKey::new(value.as_str()?).map_err(|error| FromSqlError::Other(Box::new(error)))
    }
}

/// What a category's amounts count as (`category.kind`). A subcategory has
/// its group's kind, and a category's kind never changes (the schema refuses
/// both), so a group's total never mixes income and spending and history
/// never moves between them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CategoryKind {
    /// Money spent. Money in under a spending category (an unlinked refund)
    /// lowers that category's spending; it isn't income.
    Spending,
    /// Money earned or received.
    Income,
    /// Money moving between the user's own accounts, or to one Coffer
    /// doesn't see (a savings account at another bank). Neither income nor
    /// spending, like a confirmed transfer pair.
    Transfer,
}

impl CategoryKind {
    /// Every kind, in the schema's order.
    pub const ALL: &'static [CategoryKind] = &[
        CategoryKind::Spending,
        CategoryKind::Income,
        CategoryKind::Transfer,
    ];

    const KEYS: &'static [&'static str] = &["spending", "income", "transfer"];

    /// The key the database stores.
    pub const fn as_str(self) -> &'static str {
        match self {
            CategoryKind::Spending => "spending",
            CategoryKind::Income => "income",
            CategoryKind::Transfer => "transfer",
        }
    }
}

impl fmt::Display for CategoryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for CategoryKind {
    type Err = UnknownKey;

    fn from_str(key: &str) -> Result<CategoryKind, UnknownKey> {
        CategoryKind::ALL
            .iter()
            .copied()
            .find(|kind| kind.as_str() == key)
            .ok_or(UnknownKey {
                what: "category kind",
                expected: CategoryKind::KEYS,
            })
    }
}

impl ToSql for CategoryKind {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(ValueRef::Text(
            self.as_str().as_bytes(),
        )))
    }
}

impl FromSql for CategoryKind {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<CategoryKind> {
        value
            .as_str()?
            .parse()
            .map_err(|error| FromSqlError::Other(Box::new(error)))
    }
}

/// A category as stored: a group (no parent) or one of its subcategories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    id: i64,
    parent_id: Option<i64>,
    key: CategoryKey,
    label: String,
    kind: CategoryKind,
}

impl Category {
    /// Its row ID (`category.id`), what transactions, splits and budgets
    /// point at. Presets, rules and MCP use the [key](Category::key).
    pub fn id(&self) -> i64 {
        self.id
    }

    /// Its group's row ID, `None` for a group.
    pub fn parent_id(&self) -> Option<i64> {
        self.parent_id
    }

    /// Its stable key.
    pub fn key(&self) -> &CategoryKey {
        &self.key
    }

    /// What the user sees: the default's English label or the user's own.
    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn kind(&self) -> CategoryKind {
        self.kind
    }

    /// Whether this is a group (a top-level category). Transactions can be
    /// filed under a group directly, e.g. `transfer` or `other`.
    pub fn is_group(&self) -> bool {
        self.parent_id.is_none()
    }
}

/// A top-level category and its subcategories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryGroup {
    category: Category,
    children: Vec<Category>,
}

impl CategoryGroup {
    /// The group's own category.
    pub fn category(&self) -> &Category {
        &self.category
    }

    /// Its subcategories, in the order they were created.
    pub fn children(&self) -> &[Category] {
        &self.children
    }
}

/// Every category in a database, as a two-level tree, looked up by key.
///
/// Groups and subcategories are in the order they were created, which puts
/// the defaults in [`DEFAULT_CATEGORIES`] order and the user's own after
/// them. Load it with [`Database::categories`](crate::db::Database::categories).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryTree {
    groups: Vec<CategoryGroup>,
    /// Where each key is: its group's index and, for a subcategory, its index
    /// among the group's children.
    by_key: HashMap<CategoryKey, (usize, Option<usize>)>,
}

/// A stored category that isn't under a top-level group. The schema's
/// triggers refuse that, so it means the file was changed outside Coffer.
#[derive(Debug, thiserror::Error)]
#[error("category {key} isn't under a top-level category")]
struct MisplacedCategory {
    key: CategoryKey,
}

impl CategoryTree {
    /// Reads the tree. Works inside a transaction too (`&Transaction`
    /// dereferences to `&Connection`), for code that maps keys to IDs while
    /// it writes.
    pub(crate) fn load(conn: &Connection) -> rusqlite::Result<CategoryTree> {
        let mut statement = conn.prepare(
            "SELECT id, parent_id, key, label, kind FROM category \
             ORDER BY parent_id IS NOT NULL, id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Category {
                id: row.get(0)?,
                parent_id: row.get(1)?,
                key: row.get(2)?,
                label: row.get(3)?,
                kind: row.get(4)?,
            })
        })?;

        let mut groups: Vec<CategoryGroup> = Vec::new();
        let mut group_index: HashMap<i64, usize> = HashMap::new();
        let mut by_key = HashMap::new();
        for category in rows {
            let category = category?;
            match category.parent_id {
                None => {
                    group_index.insert(category.id, groups.len());
                    by_key.insert(category.key.clone(), (groups.len(), None));
                    groups.push(CategoryGroup {
                        category,
                        children: Vec::new(),
                    });
                }
                Some(parent) => {
                    let Some(&index) = group_index.get(&parent) else {
                        return Err(rusqlite::Error::FromSqlConversionFailure(
                            1,
                            rusqlite::types::Type::Integer,
                            Box::new(MisplacedCategory { key: category.key }),
                        ));
                    };
                    let children = &mut groups[index].children;
                    by_key.insert(category.key.clone(), (index, Some(children.len())));
                    children.push(category);
                }
            }
        }
        Ok(CategoryTree { groups, by_key })
    }

    /// The groups, each with its subcategories.
    pub fn groups(&self) -> &[CategoryGroup] {
        &self.groups
    }

    /// The category with `key`, if the database has one.
    pub fn get(&self, key: &CategoryKey) -> Option<&Category> {
        let &(group, child) = self.by_key.get(key)?;
        let group = &self.groups[group];
        Some(match child {
            None => &group.category,
            Some(child) => &group.children[child],
        })
    }

    /// The group a category is in (itself, for a group).
    pub fn group_of(&self, key: &CategoryKey) -> Option<&CategoryGroup> {
        self.by_key.get(key).map(|&(group, _)| &self.groups[group])
    }

    /// Every category, each group followed by its subcategories.
    pub fn iter(&self) -> impl Iterator<Item = &Category> {
        self.groups
            .iter()
            .flat_map(|group| std::iter::once(&group.category).chain(&group.children))
    }

    /// How many categories there are, groups included.
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_one_or_two_segments() {
        for key in [
            "transfer",
            "food.groceries",
            "food.eating_out",
            "bills2.tv_licence",
        ] {
            assert_eq!(CategoryKey::new(key).unwrap().as_str(), key);
        }
    }

    #[test]
    fn refuses_anything_else() {
        for key in [
            "",
            ".",
            "food.",
            ".groceries",
            "food..groceries",
            "a.b.c",
            "Food",
            "food.Groceries",
            "food groceries",
            "food-groceries",
            " food",
            "café",
        ] {
            assert_eq!(
                CategoryKey::new(key),
                Err(CategoryKeyError(key.to_owned())),
                "{key:?}"
            );
        }
    }

    #[test]
    fn parent_is_the_first_segment_of_a_subcategory() {
        assert_eq!(
            CategoryKey::new("food.groceries").unwrap().parent(),
            Some("food")
        );
        assert_eq!(CategoryKey::new("transfer").unwrap().parent(), None);
    }

    #[test]
    fn kinds_round_trip_and_match_the_schema() {
        const MIGRATION: &str = include_str!("../migrations/V2__default_categories.sql");
        let start = "CHECK (kind IN (";
        let from = MIGRATION.find(start).unwrap() + start.len();
        let list = &MIGRATION[from..from + MIGRATION[from..].find(')').unwrap()];
        let keys: Vec<&str> = list
            .split(',')
            .map(|k| k.trim().trim_matches('\''))
            .collect();
        assert_eq!(
            keys,
            CategoryKind::ALL
                .iter()
                .map(|k| k.as_str())
                .collect::<Vec<_>>()
        );
        for kind in CategoryKind::ALL {
            assert_eq!(kind.to_string().parse::<CategoryKind>(), Ok(*kind));
        }
        assert!("Income".parse::<CategoryKind>().is_err());
    }

    /// The default set is a two-level tree: each group is listed before its
    /// subcategories, which follow it directly and share its kind.
    #[test]
    fn defaults_are_a_two_level_tree_of_unique_valid_keys() {
        let mut seen = std::collections::HashSet::new();
        let mut group: Option<&DefaultCategory> = None;
        for category in DEFAULT_CATEGORIES {
            let key = CategoryKey::new(category.key).unwrap();
            assert!(seen.insert(category.key), "{} twice", category.key);
            assert!(!category.label.trim().is_empty(), "{}", category.key);
            match key.parent() {
                None => group = Some(category),
                Some(parent) => {
                    let group = group.unwrap_or_else(|| panic!("{} has no group", category.key));
                    assert_eq!(parent, group.key, "{} isn't after its group", category.key);
                    assert_eq!(category.kind, group.kind, "{}", category.key);
                }
            }
        }
        let labels: std::collections::HashSet<_> =
            DEFAULT_CATEGORIES.iter().map(|c| c.label).collect();
        assert_eq!(labels.len(), DEFAULT_CATEGORIES.len(), "labels repeat");
    }

    /// The keys other parts of Coffer and its documentation name.
    #[test]
    fn defaults_include_the_keys_the_docs_use() {
        for (key, kind) in [
            ("transfer", CategoryKind::Transfer),
            ("income", CategoryKind::Income),
            ("food.groceries", CategoryKind::Spending),
            ("food.eating_out", CategoryKind::Spending),
            ("home", CategoryKind::Spending),
            ("other", CategoryKind::Spending),
        ] {
            assert_eq!(default_category(key).map(|c| c.kind), Some(kind), "{key}");
        }
        assert_eq!(default_category("food.fruit"), None);
        // Exactly one group per kind that isn't spending, so "income" and
        // "transfers" are each one place in the tree.
        for kind in [CategoryKind::Income, CategoryKind::Transfer] {
            let groups = DEFAULT_CATEGORIES
                .iter()
                .filter(|c| c.kind == kind && !c.key.contains('.'))
                .count();
            assert_eq!(groups, 1, "{kind}");
        }
    }

    #[test]
    fn serde_uses_the_text_and_validates_it() {
        let key: CategoryKey = serde_json::from_str(r#""food.groceries""#).unwrap();
        assert_eq!(serde_json::to_string(&key).unwrap(), r#""food.groceries""#);
        assert!(serde_json::from_str::<CategoryKey>(r#""Food""#).is_err());
    }
}
