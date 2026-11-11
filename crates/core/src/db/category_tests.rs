//! Tests for the default categories (`V2__default_categories.sql`) and
//! [`Database::categories`]: the seed applies once, matches
//! [`DEFAULT_CATEGORIES`], and the tree looks categories up by key.

use tempfile::TempDir;

use super::Database;
use crate::category::{
    CategoryKey, CategoryKind, CategoryTree, DEFAULT_CATEGORIES, DefaultCategory, default_category,
};

fn open(dir: &TempDir) -> Database {
    Database::open(&dir.path().join("coffer.db")).unwrap()
}

/// What a tree holds, comparable with [`DEFAULT_CATEGORIES`].
fn rows(tree: &CategoryTree) -> Vec<DefaultCategory> {
    tree.iter()
        .map(|category| {
            let default = default_category(category.key().as_str())
                .unwrap_or_else(|| panic!("{} isn't a default", category.key()));
            assert_eq!(category.label(), default.label);
            assert_eq!(category.kind(), default.kind);
            *default
        })
        .collect()
}

#[test]
fn a_new_database_has_exactly_the_defaults() {
    let dir = TempDir::new().unwrap();
    let tree = open(&dir).categories().unwrap();

    assert_eq!(rows(&tree), DEFAULT_CATEGORIES);
    assert_eq!(tree.len(), DEFAULT_CATEGORIES.len());
    for group in tree.groups() {
        assert!(group.category().is_group());
        for child in group.children() {
            assert_eq!(child.parent_id(), Some(group.category().id()));
            assert_eq!(child.key().parent(), Some(group.category().key().as_str()));
        }
    }
}

#[test]
fn reopening_seeds_nothing_twice_and_keeps_changes() {
    let dir = TempDir::new().unwrap();
    let db = open(&dir);
    db.conn
        .execute(
            "UPDATE category SET label = 'Food shop' WHERE key = 'food.groceries'",
            [],
        )
        .unwrap();
    db.conn
        .execute("DELETE FROM category WHERE key = 'food.takeaway'", [])
        .unwrap();
    let before = db.categories().unwrap();
    drop(db);

    let after = open(&dir).categories().unwrap();
    assert_eq!(after, before);
    assert_eq!(after.len(), DEFAULT_CATEGORIES.len() - 1);
    let groceries = after.get(&key("food.groceries")).unwrap();
    assert_eq!(groceries.label(), "Food shop");
    assert_eq!(after.get(&key("food.takeaway")), None);

    let existing = Database::open_existing(&dir.path().join("coffer.db")).unwrap();
    assert_eq!(existing.categories().unwrap(), before);
}

#[test]
fn looks_categories_up_by_key() {
    let dir = TempDir::new().unwrap();
    let tree = open(&dir).categories().unwrap();

    let food = tree.get(&key("food")).unwrap();
    let groceries = tree.get(&key("food.groceries")).unwrap();
    assert!(food.is_group());
    assert_eq!(food.label(), "Food");
    assert_eq!(groceries.label(), "Groceries");
    assert_eq!(groceries.parent_id(), Some(food.id()));
    assert_eq!(groceries.kind(), CategoryKind::Spending);
    assert_eq!(
        tree.get(&key("transfer.savings")).unwrap().kind(),
        CategoryKind::Transfer
    );
    assert_eq!(
        tree.group_of(&key("food.groceries")).unwrap().category(),
        food
    );
    assert_eq!(tree.group_of(&key("food")).unwrap().category(), food);
    assert_eq!(tree.get(&key("food.fruit")), None);
    assert!(tree.group_of(&key("pets")).is_none());
}

#[test]
fn user_categories_come_after_the_defaults_in_their_group() {
    let dir = TempDir::new().unwrap();
    let db = open(&dir);
    db.conn
        .execute_batch(
            "INSERT INTO category (key, label) VALUES ('garden', 'Garden'); \
             INSERT INTO category (parent_id, key, label) \
             SELECT id, 'food.coffee', 'Coffee' FROM category WHERE key = 'food'; \
             INSERT INTO category (parent_id, key, label, kind) \
             SELECT id, 'income.side_job', 'Side job', 'income' \
             FROM category WHERE key = 'income';",
        )
        .unwrap();
    let tree = db.categories().unwrap();

    assert_eq!(
        tree.groups().last().unwrap().category().key().as_str(),
        "garden"
    );
    let food = tree.group_of(&key("food.coffee")).unwrap();
    assert_eq!(food.children().last().unwrap().label(), "Coffee");
    assert_eq!(
        tree.get(&key("income.side_job")).unwrap().kind(),
        CategoryKind::Income
    );
    assert_eq!(tree.len(), DEFAULT_CATEGORIES.len() + 3);
}

#[test]
fn keys_and_kinds_survive_the_database() {
    let dir = TempDir::new().unwrap();
    let db = open(&dir);
    let stored: (CategoryKey, CategoryKind) = db
        .conn
        .query_row(
            "SELECT key, kind FROM category WHERE key = ?1 AND kind = ?2",
            rusqlite::params![key("income.salary"), CategoryKind::Income],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored, (key("income.salary"), CategoryKind::Income));
}

fn key(key: &str) -> CategoryKey {
    CategoryKey::new(key).unwrap()
}

/// A database made before the defaults existed (schema version 1) gets them
/// when it's next opened.
#[test]
fn a_version_1_database_gets_the_defaults() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("coffer.db");
    let all = super::migrations::embedded();
    drop(Database::open_with(&path, &all[..1]).unwrap());

    let db = Database::open(&path).unwrap();

    assert_eq!(
        db.schema_version().unwrap(),
        Database::supported_schema_version()
    );
    assert_eq!(rows(&db.categories().unwrap()), DEFAULT_CATEGORIES);
}

/// A subcategory has its group's kind, and no kind changes, so a group's
/// total never mixes income and spending.
#[test]
fn kinds_follow_the_group_and_never_change() {
    let dir = TempDir::new().unwrap();
    let db = open(&dir);
    let insert_under = |parent: &str, key: &str, kind: &str| {
        db.conn.execute(
            "INSERT INTO category (parent_id, key, label, kind) \
             SELECT id, ?2, 'x', ?3 FROM category WHERE key = ?1",
            rusqlite::params![parent, key, kind],
        )
    };
    let refused = |result: rusqlite::Result<usize>| {
        assert_eq!(
            result.unwrap_err().sqlite_error_code(),
            Some(rusqlite::ErrorCode::ConstraintViolation)
        );
    };

    refused(insert_under("income", "income.bonus", "spending"));
    refused(insert_under("food", "food.wages", "income"));
    refused(insert_under("transfer", "transfer.isa", "spending"));
    refused(db.conn.execute(
        "INSERT INTO category (key, label, kind) VALUES ('wages', 'Wages', 'earnings')",
        [],
    ));
    insert_under("income", "income.bonus", "income").unwrap();
    // Without a kind, a new category is spending.
    db.conn
        .execute(
            "INSERT INTO category (key, label) VALUES ('garden', 'Garden')",
            [],
        )
        .unwrap();
    assert_eq!(
        db.categories().unwrap().get(&key("garden")).unwrap().kind(),
        CategoryKind::Spending
    );

    for key in ["income", "income.salary", "garden"] {
        refused(db.conn.execute(
            "UPDATE category SET kind = 'transfer' WHERE key = ?1",
            [key],
        ));
    }
    // Labels still change.
    db.conn
        .execute(
            "UPDATE category SET label = 'Wages' WHERE key = 'income.salary'",
            [],
        )
        .unwrap();
}
