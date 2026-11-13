//! Tests for the schema in `crates/core/migrations/`: the conventions listed
//! at the top of `V1__initial_schema.sql`, and the constraints that carry an
//! invariant from `AGENTS.md` (integer money, read-only source fields,
//! dedupe keys, the exclusion fields, currencies and the account
//! structure).
//!
//! Each test opens a real database through [`Database::open`], so the
//! constraints are checked under the connection's own settings (foreign keys
//! on, `trusted_schema` off).

use std::path::Path;

use rusqlite::{Connection, ErrorCode, params};
use tempfile::TempDir;

use super::Database;

/// The tables the migrations create. A new table is added here with its
/// migration.
const TABLES: &[&str] = &[
    "account",
    "balance_snapshot",
    "budget",
    "category",
    "change_batch",
    "change_item",
    "duplicate_candidate",
    "fx_rate",
    "import_batch",
    "import_flag",
    "merchant_alias",
    "report",
    "rule",
    "rule_action",
    "suggestion",
    "tag",
    "transaction",
    "transaction_link",
    "transaction_split",
    "transaction_tag",
    "transfer_group",
];

/// The views the migrations create.
const VIEWS: &[&str] = &["ledger_line"];

struct Fixture {
    db: Database,
    _dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("coffer.db")).unwrap();
        Self { db, _dir: dir }
    }

    /// A database without the default categories, for tests that build
    /// their own tree.
    fn without_categories() -> Self {
        let fixture = Self::new();
        fixture.conn().execute("DELETE FROM category", []).unwrap();
        fixture
    }

    fn conn(&self) -> &Connection {
        &self.db.conn
    }

    fn account(&self, name: &str) -> i64 {
        self.conn()
            .execute(
                "INSERT INTO account (name, kind, currency) VALUES (?1, 'current', 'GBP')",
                [name],
            )
            .unwrap();
        self.conn().last_insert_rowid()
    }

    fn add_account(
        &self,
        parent: Option<i64>,
        name: &str,
        kind: &str,
        currency: &str,
    ) -> rusqlite::Result<i64> {
        self.conn().execute(
            "INSERT INTO account (parent_id, name, kind, currency) VALUES (?1, ?2, ?3, ?4)",
            params![parent, name, kind, currency],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    /// Inserts a transaction of -1.00 on 2026-11-02 with no source ID.
    fn row(
        &self,
        account: i64,
        batch: Option<i64>,
        origin: &str,
        currency: &str,
        fingerprint: Option<&str>,
    ) -> rusqlite::Result<i64> {
        self.conn().execute(
            "INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, currency, \
             description_raw, description_normalized, fingerprint, origin) \
             VALUES (?1, ?2, '2026-11-02', -100, ?3, 'x', 'X', ?4, ?5)",
            params![account, batch, currency, fingerprint, origin],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    fn snapshot(&self, account: i64, batch: Option<i64>, source: &str) -> rusqlite::Result<i64> {
        self.conn().execute(
            "INSERT INTO balance_snapshot (account_id, batch_id, date, balance_minor, source) \
             VALUES (?1, ?2, '2026-11-01', 100000, ?3)",
            params![account, batch, source],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    fn batch(&self, account: i64) -> i64 {
        self.conn()
            .execute(
                "INSERT INTO import_batch (account_id, source_file, format) \
                 VALUES (?1, 'statement.csv', 'csv')",
                [account],
            )
            .unwrap();
        self.conn().last_insert_rowid()
    }

    /// A default category's ID.
    fn default_category(&self, key: &str) -> i64 {
        self.conn()
            .query_row("SELECT id FROM category WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .unwrap()
    }

    fn category(&self, key: &str, parent: Option<i64>) -> rusqlite::Result<i64> {
        self.conn().execute(
            "INSERT INTO category (parent_id, key, label) VALUES (?1, ?2, ?2)",
            params![parent, key],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    /// Inserts an imported transaction of -12.34 GBP on 2026-11-02.
    fn imported(
        &self,
        account: i64,
        batch: i64,
        external_id: Option<&str>,
        fingerprint: &str,
    ) -> i64 {
        self.conn()
            .execute(
                "INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, currency, \
                 description_raw, description_normalized, external_id, fingerprint, origin) \
                 VALUES (?1, ?2, '2026-11-02', -1234, 'GBP', 'Coffee  shop', 'COFFEE SHOP', \
                 ?3, ?4, 'imported')",
                params![account, batch, external_id, fingerprint],
            )
            .unwrap();
        self.conn().last_insert_rowid()
    }

    /// Inserts split line `line_no` of `minor` into transaction `parent`.
    fn split_line(
        &self,
        parent: i64,
        line_no: i64,
        minor: i64,
        category: Option<i64>,
    ) -> rusqlite::Result<i64> {
        self.conn().execute(
            "INSERT INTO transaction_split (transaction_id, line_no, amount_minor, category_id) \
             VALUES (?1, ?2, ?3, ?4)",
            params![parent, line_no, minor, category],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    /// Puts transactions `rows` in a new transfer group with `status`.
    fn transfer_group(&self, status: &str, rows: &[i64]) -> i64 {
        self.conn()
            .execute("INSERT INTO transfer_group (status) VALUES (?1)", [status])
            .unwrap();
        let group = self.conn().last_insert_rowid();
        for row in rows {
            self.conn()
                .execute(
                    "UPDATE \"transaction\" SET transfer_group_id = ?1 WHERE id = ?2",
                    [group, *row],
                )
                .unwrap();
        }
        group
    }

    /// Inserts a manual transaction of -5.00 GBP on 2026-11-03.
    fn manual(&self, account: i64) -> i64 {
        self.conn()
            .execute(
                "INSERT INTO \"transaction\" (account_id, date, amount_minor, currency, \
                 description_raw, description_normalized, origin) \
                 VALUES (?1, '2026-11-03', -500, 'GBP', 'Cash', 'CASH', 'manual')",
                [account],
            )
            .unwrap();
        self.conn().last_insert_rowid()
    }
}

/// Asserts that `result` failed with a constraint violation (CHECK, NOT
/// NULL, UNIQUE, FOREIGN KEY or a trigger's RAISE).
#[track_caller]
fn assert_refused<T: std::fmt::Debug>(result: rusqlite::Result<T>) {
    match result {
        Err(err) if err.sqlite_error_code() == Some(ErrorCode::ConstraintViolation) => {}
        other => panic!("expected a constraint violation, got {other:?}"),
    }
}

#[test]
fn creates_every_table() {
    let fixture = Fixture::new();
    let mut statement = fixture
        .conn()
        .prepare(
            "SELECT name FROM sqlite_schema WHERE type = 'table' \
             AND name NOT LIKE 'sqlite_%' AND name <> 'refinery_schema_history' ORDER BY name",
        )
        .unwrap();
    let tables: Vec<String> = statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    assert_eq!(tables, TABLES);
}

#[test]
fn creates_every_view() {
    let fixture = Fixture::new();
    let mut statement = fixture
        .conn()
        .prepare("SELECT name FROM sqlite_schema WHERE type = 'view' ORDER BY name")
        .unwrap();
    let views: Vec<String> = statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    assert_eq!(views, VIEWS);
}

#[test]
fn every_table_is_strict() {
    let fixture = Fixture::new();
    for table in TABLES {
        let strict: bool = fixture
            .conn()
            .query_row(
                "SELECT strict FROM pragma_table_list WHERE schema = 'main' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert!(strict, "{table} must be STRICT");
    }
}

/// An unindexed foreign key makes every delete of the parent scan the
/// child table.
#[test]
fn every_foreign_key_is_indexed() {
    let fixture = Fixture::new();
    for table in TABLES {
        let mut keys = fixture
            .conn()
            .prepare("SELECT \"from\" FROM pragma_foreign_key_list(?1)")
            .unwrap();
        let columns: Vec<String> = keys
            .query_map([table], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for column in columns {
            // An index (or a UNIQUE constraint) whose first column is the key.
            let indexed: bool = fixture
                .conn()
                .query_row(
                    "SELECT EXISTS (SELECT 1 FROM pragma_index_list(?1) AS list \
                     JOIN pragma_index_info(list.name) AS info \
                     WHERE info.seqno = 0 AND info.name = ?2)",
                    [*table, column.as_str()],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(
                indexed,
                "{table}.{column} is a foreign key without an index"
            );
        }
    }
}

#[test]
fn foreign_keys_are_enforced() {
    let fixture = Fixture::new();
    assert_refused(fixture.conn().execute(
        "INSERT INTO import_batch (account_id, source_file, format) VALUES (99, 'a.csv', 'csv')",
        [],
    ));

    // A parent with children can't be deleted: there are no ON DELETE
    // actions, so nothing disappears as a side effect.
    let account = fixture.account("Current");
    fixture.batch(account);
    assert_refused(
        fixture
            .conn()
            .execute("DELETE FROM account WHERE id = ?1", [account]),
    );
}

#[test]
fn money_is_integer() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let batch = fixture.batch(account);
    let id = fixture.imported(account, batch, None, "fp-1");

    assert_refused(fixture.conn().execute(
        "UPDATE account SET opening_balance_minor = 10.5, opening_date = '2026-01-01' \
         WHERE id = ?1",
        [account],
    ));
    assert_refused(fixture.conn().execute(
        "UPDATE \"transaction\" SET my_share_minor = -6.17 WHERE id = ?1",
        [id],
    ));
    // Text that is a whole number is stored as one; anything else is refused.
    let category = fixture.default_category("food");
    assert_refused(fixture.conn().execute(
        "INSERT INTO budget (category_id, amount_minor, currency, rollover, group_kind) \
         VALUES (?1, '100.50', 'GBP', 0, 'flex')",
        [category],
    ));
}

#[test]
fn dates_times_and_currencies_are_checked() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let batch = fixture.batch(account);
    let insert = |date: &str, time: Option<&str>, currency: &str| {
        fixture.conn().execute(
            "INSERT INTO \"transaction\" (account_id, batch_id, date, time, amount_minor, \
             currency, description_raw, description_normalized, fingerprint, origin) \
             VALUES (?1, ?2, ?3, ?4, -100, ?5, 'x', 'X', ?3 || ?5, 'imported')",
            params![account, batch, date, time, currency],
        )
    };

    insert("2026-02-28", Some("23:59:59"), "GBP").unwrap();
    assert_refused(insert("2026-02-30", None, "GBP"));
    assert_refused(insert("10/11/2026", None, "GBP"));
    assert_refused(insert("2026-11-10", Some("9:30"), "GBP"));
    assert_refused(insert("2026-11-10", Some("24:00:00"), "GBP"));
    assert_refused(insert("2026-11-10", None, "gbp"));
    assert_refused(insert("2026-11-10", None, "GBPX"));

    assert_refused(fixture.conn().execute(
        "INSERT INTO account (name, kind, currency, created_at) \
         VALUES ('Savings', 'savings', 'GBP', '2026-11-10 12:00:00')",
        [],
    ));
}

#[test]
fn created_at_defaults_to_a_utc_timestamp() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let created_at: String = fixture
        .conn()
        .query_row(
            "SELECT created_at FROM account WHERE id = ?1",
            [account],
            |row| row.get(0),
        )
        .unwrap();

    // `YYYY-MM-DDTHH:MM:SS.SSSZ`
    assert_eq!(created_at.len(), 24, "{created_at}");
    assert!(created_at.ends_with('Z'), "{created_at}");
    assert_eq!(&created_at[10..11], "T", "{created_at}");
}

#[test]
fn pots_belong_to_a_current_or_savings_account() {
    let fixture = Fixture::new();
    let current = fixture.account("Current");
    let savings = fixture
        .add_account(None, "Saver", "savings", "GBP")
        .unwrap();
    let pot = fixture
        .add_account(Some(current), "Holiday", "pot", "GBP")
        .unwrap();
    fixture
        .add_account(Some(savings), "Rainy day", "pot", "GBP")
        .unwrap();

    assert_refused(fixture.add_account(Some(pot), "Nested", "pot", "GBP"));
    for kind in ["credit_card", "cash", "asset", "liability"] {
        let parent = fixture.add_account(None, kind, kind, "GBP").unwrap();
        assert_refused(fixture.add_account(Some(parent), "Pot", "pot", "GBP"));
    }
    // Moving a pot follows the same rule.
    let card = fixture
        .add_account(None, "Card", "credit_card", "GBP")
        .unwrap();
    assert_refused(fixture.conn().execute(
        "UPDATE account SET parent_id = ?1 WHERE id = ?2",
        [card, pot],
    ));
    fixture
        .conn()
        .execute(
            "UPDATE account SET parent_id = ?1 WHERE id = ?2",
            [savings, pot],
        )
        .unwrap();
    // An account with pots stays a current or savings account.
    assert_refused(fixture.conn().execute(
        "UPDATE account SET kind = 'credit_card' WHERE id = ?1",
        [savings],
    ));
    fixture
        .conn()
        .execute(
            "UPDATE account SET kind = 'current' WHERE id = ?1",
            [savings],
        )
        .unwrap();
}

#[test]
fn only_pots_have_a_parent() {
    let fixture = Fixture::new();
    let parent = fixture.account("Current");

    fixture
        .conn()
        .execute(
            "INSERT INTO account (parent_id, name, kind, currency) \
             VALUES (?1, 'Holiday', 'pot', 'GBP')",
            [parent],
        )
        .unwrap();
    assert_refused(fixture.conn().execute(
        "INSERT INTO account (parent_id, name, kind, currency) \
         VALUES (?1, 'Savings', 'savings', 'GBP')",
        [parent],
    ));
    assert_refused(fixture.conn().execute(
        "INSERT INTO account (name, kind, currency) VALUES ('Orphan', 'pot', 'GBP')",
        [],
    ));
}

#[test]
fn categories_have_two_levels() {
    let fixture = Fixture::without_categories();
    let food = fixture.category("food", None).unwrap();
    let groceries = fixture.category("food.groceries", Some(food)).unwrap();
    let transport = fixture.category("transport", None).unwrap();

    assert_refused(fixture.category("food.groceries.fruit", Some(groceries)));
    // A parent with children can't become a child.
    assert_refused(fixture.conn().execute(
        "UPDATE category SET parent_id = ?1 WHERE id = ?2",
        [transport, food],
    ));
    // Nor can a category go under a subcategory.
    assert_refused(fixture.conn().execute(
        "UPDATE category SET parent_id = ?1 WHERE id = ?2",
        [groceries, transport],
    ));
    // Keys are stable identifiers, spelled as `CategoryKey` accepts them.
    assert_refused(fixture.category("food", None));
    for key in [
        "",
        ".",
        "food.",
        ".groceries",
        "food..groceries",
        "a.b.c",
        "Eating Out",
        "food-groceries",
    ] {
        assert_refused(fixture.category(key, None));
    }
    let bills = fixture.category("bills2", None).unwrap();
    fixture.category("bills2.tv_licence", Some(bills)).unwrap();
}

/// A key names the category's place in the tree: one part at the top level,
/// `<parent key>.<part>` below it.
#[test]
fn category_keys_match_their_parent() {
    let fixture = Fixture::without_categories();
    let food = fixture.category("food", None).unwrap();
    fixture.category("transport", None).unwrap();

    assert_refused(fixture.category("transport.bus", Some(food)));
    assert_refused(fixture.category("foo.bus", Some(food)));
    assert_refused(fixture.category("foodie.bus", Some(food)));
    assert_refused(fixture.category("food.bus", None));
    assert_refused(fixture.category("bus", Some(food)));
    assert_refused(fixture.category("food", Some(food)));
    fixture.category("food.eating_out", Some(food)).unwrap();
    // A missing parent is a foreign key error, not a key mismatch.
    assert_refused(fixture.category("ghost.x", Some(99)));
}

/// Keys are stable (presets, rules and MCP name categories by key), and a
/// subcategory's key spells its parent, so neither changes; labels do.
#[test]
fn category_keys_and_parents_are_read_only() {
    let fixture = Fixture::without_categories();
    let food = fixture.category("food", None).unwrap();
    let groceries = fixture.category("food.groceries", Some(food)).unwrap();
    let transport = fixture.category("transport", None).unwrap();
    let bus = fixture.category("transport.bus", Some(transport)).unwrap();

    for (sql, id) in [
        // Renaming a parent would orphan its children's keys.
        ("UPDATE category SET key = 'meals' WHERE id = ?1", food),
        (
            "UPDATE category SET key = 'food.shopping' WHERE id = ?1",
            groceries,
        ),
        // A move, with or without the matching key.
        ("UPDATE category SET parent_id = NULL WHERE id = ?1", bus),
        (
            "UPDATE category SET parent_id = (SELECT id FROM category WHERE key = 'food'), \
             key = 'food.bus' WHERE id = ?1",
            bus,
        ),
        ("UPDATE category SET id = 100 WHERE id = ?1", transport),
    ] {
        assert_refused(fixture.conn().execute(sql, [id]));
    }
    fixture
        .conn()
        .execute(
            "UPDATE category SET label = 'Food shopping' WHERE id = ?1",
            [groceries],
        )
        .unwrap();
}

#[test]
fn imported_source_fields_are_read_only() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let batch = fixture.batch(account);
    let category = fixture.default_category("food");
    let id = fixture.imported(account, batch, Some("tx_1"), "fp-1");

    for (column, value) in [
        ("date", "'2026-11-03'"),
        ("time", "'12:00:00'"),
        ("amount_minor", "-1000"),
        ("currency", "'EUR'"),
        ("merchant", "'Other'"),
        ("description_raw", "'Other'"),
        ("description_normalized", "'OTHER'"),
        ("external_id", "'tx_2'"),
        ("fingerprint", "'fp-2'"),
        ("source_category", "'groceries'"),
        ("source_type", "'Card payment'"),
        ("note", "'edited'"),
        ("batch_id", "NULL"),
        ("account_id", "account_id + 1"),
        ("id", "100"),
        ("origin", "'manual'"),
        ("created_at", "'2020-01-01T00:00:00.000Z'"),
    ] {
        let result = fixture.conn().execute(
            &format!("UPDATE \"transaction\" SET {column} = {value} WHERE id = ?1"),
            [id],
        );
        assert_refused(result);
    }

    // User-facing fields stay editable.
    fixture
        .conn()
        .execute(
            "UPDATE \"transaction\" SET category_id = ?1, category_source = 'user', \
             merchant_display = 'Coffee', user_note = 'with Sam', my_share_minor = -617, \
             share_label = 'Sam', reimbursable = 1 WHERE id = ?2",
            params![category, id],
        )
        .unwrap();
}

#[test]
fn manual_transactions_are_editable() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let id = fixture.manual(account);

    fixture
        .conn()
        .execute(
            "UPDATE \"transaction\" SET date = '2026-11-04', amount_minor = -700, \
             description_raw = 'Cash at market', description_normalized = 'CASH AT MARKET' \
             WHERE id = ?1",
            [id],
        )
        .unwrap();
    assert_refused(fixture.conn().execute(
        "UPDATE \"transaction\" SET origin = 'imported' WHERE id = ?1",
        [id],
    ));
    // IDs never change, so nothing that names one comes to mean another row.
    assert_refused(
        fixture
            .conn()
            .execute("UPDATE \"transaction\" SET id = 100 WHERE id = ?1", [id]),
    );
    // Manual rows aren't deduplicated, so they carry no dedupe key.
    assert_refused(fixture.conn().execute(
        "UPDATE \"transaction\" SET external_id = 'tx_1' WHERE id = ?1",
        [id],
    ));
    assert_refused(fixture.conn().execute(
        "UPDATE \"transaction\" SET fingerprint = 'fp-1' WHERE id = ?1",
        [id],
    ));
    fixture
        .conn()
        .execute("DELETE FROM \"transaction\" WHERE id = ?1", [id])
        .unwrap();
}

#[test]
fn origin_matches_the_batch() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let batch = fixture.batch(account);

    // An imported row needs a batch and a fingerprint; a manual row has no
    // batch.
    assert_refused(fixture.conn().execute(
        "INSERT INTO \"transaction\" (account_id, date, amount_minor, currency, \
         description_raw, description_normalized, fingerprint, origin) \
         VALUES (?1, '2026-11-02', -100, 'GBP', 'x', 'X', 'fp', 'imported')",
        [account],
    ));
    assert_refused(fixture.conn().execute(
        "INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, currency, \
         description_raw, description_normalized, origin) \
         VALUES (?1, ?2, '2026-11-02', -100, 'GBP', 'x', 'X', 'imported')",
        [account, batch],
    ));
    assert_refused(fixture.conn().execute(
        "INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, currency, \
         description_raw, description_normalized, origin) \
         VALUES (?1, ?2, '2026-11-02', -100, 'GBP', 'x', 'X', 'manual')",
        [account, batch],
    ));
}

#[test]
fn source_ids_and_fingerprints_are_unique_per_account() {
    let fixture = Fixture::new();
    let current = fixture.account("Current");
    let joint = fixture.account("Joint");
    let current_batch = fixture.batch(current);
    let joint_batch = fixture.batch(joint);
    fixture.imported(current, current_batch, Some("tx_1"), "fp-1");

    let again = |account, batch, external_id, fingerprint: &str| {
        fixture.conn().execute(
            "INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, currency, \
             description_raw, description_normalized, external_id, fingerprint, origin) \
             VALUES (?1, ?2, '2026-11-02', -1234, 'GBP', 'x', 'X', ?3, ?4, 'imported')",
            params![account, batch, external_id, fingerprint],
        )
    };
    assert_refused(again(current, current_batch, Some("tx_1"), "fp-2"));
    // Without a source ID, the fingerprint is the key.
    again(current, current_batch, None, "fp-3").unwrap();
    assert_refused(again(current, current_batch, None, "fp-3"));
    // The same IDs on another account are another transaction.
    again(joint, joint_batch, Some("tx_1"), "fp-1").unwrap();
    again(joint, joint_batch, None, "fp-3").unwrap();
    // Manual rows have no dedupe key, so two identical ones both stay.
    fixture.manual(current);
    fixture.manual(current);
}

/// Source ID first: a row with a source ID is never matched by fingerprint.
/// Two identical payments on one day take their occurrence index from their
/// order in the file, so an overlapping export that orders them differently
/// gives a new transaction a stored row's fingerprint.
#[test]
fn a_new_source_id_is_a_new_transaction_whatever_its_fingerprint() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let first = fixture.batch(account);
    let second = fixture.batch(account);
    fixture.imported(
        account,
        first,
        Some("tx_1"),
        "2026-11-02|-1234|COFFEE SHOP|1",
    );

    fixture.imported(
        account,
        second,
        Some("tx_2"),
        "2026-11-02|-1234|COFFEE SHOP|1",
    );
    // A row without a source ID isn't matched against rows that have one.
    fixture.imported(account, second, None, "2026-11-02|-1234|COFFEE SHOP|1");
}

/// REPLACE deletes the row in the way and inserts a new one, so the
/// read-only trigger never sees it. The insert trigger refuses it first.
#[test]
fn an_insert_never_replaces_a_stored_transaction() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let batch = fixture.batch(account);
    let with_id = fixture.imported(account, batch, Some("tx_1"), "fp-1");
    let without_id = fixture.imported(account, batch, None, "fp-2");

    let replace = |verb: &str, id: Option<i64>, external_id: Option<&str>, fingerprint: &str| {
        fixture.conn().execute(
            &format!(
                "{verb} INTO \"transaction\" (id, account_id, batch_id, date, amount_minor, \
                 currency, description_raw, description_normalized, external_id, fingerprint, \
                 origin) VALUES (?1, ?2, ?3, '2026-11-02', -999999, 'GBP', 'Edited', 'EDITED', \
                 ?4, ?5, 'imported')"
            ),
            params![id, account, batch, external_id, fingerprint],
        )
    };
    assert_refused(replace("INSERT OR REPLACE", None, Some("tx_1"), "fp-9"));
    assert_refused(replace("REPLACE", None, None, "fp-2"));
    assert_refused(replace("INSERT OR REPLACE", Some(with_id), None, "fp-9"));
    // A duplicate is an error, not a silent skip: core dedupes first.
    assert_refused(replace("INSERT OR IGNORE", None, Some("tx_1"), "fp-9"));
    // So is an upsert: the insert trigger runs before the conflict is found
    // (and its `DO UPDATE` would meet the read-only trigger).
    assert_refused(fixture.conn().execute(
        "INSERT INTO \"transaction\" (id, account_id, batch_id, date, amount_minor, currency, \
         description_raw, description_normalized, fingerprint, origin) \
         VALUES (?1, ?2, ?3, '2026-11-02', 0, 'GBP', 'x', 'X', 'fp-9', 'imported') \
         ON CONFLICT (id) DO UPDATE SET amount_minor = excluded.amount_minor",
        params![with_id, account, batch],
    ));

    let stored: Vec<(i64, i64, String)> = fixture
        .conn()
        .prepare("SELECT id, amount_minor, description_raw FROM \"transaction\" ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let original = "Coffee  shop".to_owned();
    assert_eq!(
        stored,
        [
            (with_id, -1234, original.clone()),
            (without_id, -1234, original)
        ]
    );

    // Undoing a batch still deletes its rows.
    fixture
        .conn()
        .execute("DELETE FROM \"transaction\" WHERE batch_id = ?1", [batch])
        .unwrap();
}

/// Whether `source` writes `"transaction"` with REPLACE or an upsert. Works
/// on words, so SQL split over Rust string continuations reads as one
/// statement. A heuristic: `transaction_insert_new` is the real guard.
fn replaces_transactions(source: &str) -> bool {
    let source = source.to_ascii_uppercase().replace(';', " ; ");
    let words: Vec<&str> = source
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ';'))
        .filter(|word| !word.is_empty())
        .collect();
    let at = |i: usize, phrase: &[&str]| words[i..].starts_with(phrase);
    (0..words.len()).any(|i| {
        at(i, &["REPLACE", "INTO", "TRANSACTION"])
            || at(i, &["UPDATE", "OR", "REPLACE", "TRANSACTION"])
            || (at(i, &["INTO", "TRANSACTION"])
                && words[i + 2..]
                    .iter()
                    .take(80)
                    .take_while(|word| !matches!(**word, ";" | "INSERT" | "DELETE"))
                    .collect::<Vec<_>>()
                    .windows(2)
                    .any(|pair| *pair[0] == "ON" && *pair[1] == "CONFLICT"))
    })
}

#[test]
fn replaces_transactions_finds_replace_and_upserts() {
    for sql in [
        "INSERT OR REPLACE INTO \"transaction\" (id) VALUES (1)",
        "conn.execute(\"REPLACE INTO \\\"transaction\\\" (id) \\\n    VALUES (?1)\", [])",
        "update or replace \"transaction\" set amount_minor = 0",
        "INSERT INTO \"transaction\" (id) VALUES (1) \\\n ON CONFLICT (id) DO NOTHING",
    ] {
        assert!(replaces_transactions(sql), "{sql}");
    }
    for sql in [
        "INSERT INTO \"transaction\" (id) VALUES (1)",
        "INSERT OR REPLACE INTO fx_rate (date) VALUES ('2026-11-10')",
        "INSERT INTO \"transaction\" (id) VALUES (1); \
         INSERT INTO fx_rate (date) VALUES ('x') ON CONFLICT DO NOTHING",
        "SELECT replace(description_raw, '  ', ' ') FROM \"transaction\"",
    ] {
        assert!(!replaces_transactions(sql), "{sql}");
    }
}

/// Core never uses REPLACE or an upsert on `"transaction"` (see the
/// conventions in `V1__initial_schema.sql`).
#[test]
fn no_replace_on_transactions() {
    fn visit(dir: &Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "rs" || ext == "sql")
                // This file holds the examples above.
                && path.file_name().is_some_and(|name| name != "schema_tests.rs")
                && replaces_transactions(&std::fs::read_to_string(&path).unwrap())
            {
                found.push(path.display().to_string());
            }
        }
    }
    let mut found = Vec::new();
    visit(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut found,
    );
    assert!(
        found.is_empty(),
        "REPLACE or upsert on \"transaction\" in {found:?}"
    );
}

#[test]
fn exclusion_fields_go_together() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let id = fixture.manual(account);
    let exclude = |reason: Option<&str>, note: Option<&str>, at: Option<&str>| {
        fixture.conn().execute(
            "UPDATE \"transaction\" SET excluded_reason = ?1, excluded_note = ?2, \
             excluded_at = ?3 WHERE id = ?4",
            params![reason, note, at, id],
        )
    };
    let at = Some("2026-11-10T09:00:00.000Z");

    exclude(Some("bank_error"), None, at).unwrap();
    exclude(Some("other"), Some("Paid twice by mistake"), at).unwrap();
    exclude(None, None, None).unwrap();

    assert_refused(exclude(Some("duplicate"), None, None));
    assert_refused(exclude(None, None, at));
    assert_refused(exclude(None, Some("note"), None));
    assert_refused(exclude(Some("other"), None, at));
    assert_refused(exclude(Some("other"), Some("  "), at));
    assert_refused(exclude(Some("mistake"), None, at));
}

#[test]
fn my_share_is_part_of_the_amount() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let id = fixture.manual(account); // -500
    let share = |minor: i64| {
        fixture.conn().execute(
            "UPDATE \"transaction\" SET my_share_minor = ?1 WHERE id = ?2",
            params![minor, id],
        )
    };

    share(-250).unwrap();
    share(0).unwrap();
    share(-500).unwrap();
    assert_refused(share(250));
    assert_refused(share(-501));
}

#[test]
fn a_category_has_a_source() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let category = fixture.default_category("food");
    let id = fixture.manual(account);

    assert_refused(fixture.conn().execute(
        "UPDATE \"transaction\" SET category_id = ?1 WHERE id = ?2",
        [category, id],
    ));
    assert_refused(fixture.conn().execute(
        "UPDATE \"transaction\" SET category_source = 'user' WHERE id = ?1",
        [id],
    ));
}

#[test]
fn balance_snapshots_from_files_belong_to_a_batch() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let batch = fixture.batch(account);

    fixture.snapshot(account, Some(batch), "ofx").unwrap();
    fixture.snapshot(account, None, "manual").unwrap();
    let other = fixture.account("Joint");
    fixture.snapshot(other, None, "statement").unwrap();
    assert_refused(fixture.snapshot(account, None, "csv"));
    assert_refused(fixture.snapshot(account, Some(batch), "manual"));
}

/// Each entered balance asserts the end-of-day balance, so an account has
/// at most one per day. Balances from files can repeat a date: overlapping
/// exports both carry it.
#[test]
fn one_entered_balance_per_account_and_day() {
    let fixture = Fixture::new();
    let current = fixture.account("Current");
    let joint = fixture.account("Joint");
    let first = fixture.batch(current);
    let second = fixture.batch(current);

    // `snapshot` writes 2026-11-01.
    let entered = fixture.snapshot(current, None, "manual").unwrap();
    assert_refused(fixture.snapshot(current, None, "manual"));
    assert_refused(fixture.snapshot(current, None, "statement"));
    fixture.snapshot(joint, None, "statement").unwrap();
    fixture.snapshot(current, Some(first), "csv").unwrap();
    fixture.snapshot(current, Some(second), "csv").unwrap();
    fixture.snapshot(current, Some(second), "csv").unwrap();
    // Another day is another assertion; moving one onto a taken day isn't.
    let next = fixture.conn().execute(
        "INSERT INTO balance_snapshot (account_id, date, balance_minor, source) \
         VALUES (?1, '2026-11-02', 100500, 'statement')",
        [current],
    );
    next.unwrap();
    let next = fixture.conn().last_insert_rowid();
    assert_refused(fixture.conn().execute(
        "UPDATE balance_snapshot SET date = '2026-11-01' WHERE id = ?1",
        [next],
    ));
    // A correction edits the one there.
    fixture
        .conn()
        .execute(
            "UPDATE balance_snapshot SET balance_minor = 99000 WHERE id = ?1",
            [entered],
        )
        .unwrap();

    // Valuations are entered balances too.
    let house = fixture.add_account(None, "House", "asset", "GBP").unwrap();
    fixture.snapshot(house, None, "valuation").unwrap();
    assert_refused(fixture.snapshot(house, None, "valuation"));
}

/// A batch belongs to one account. Its rows and balances sit on that
/// account, or (mirrored pot-side rows) on one of its pots.
#[test]
fn batch_rows_sit_on_the_batch_account_or_its_pots() {
    let fixture = Fixture::new();
    let current = fixture.account("Current");
    let pot = fixture
        .add_account(Some(current), "Holiday", "pot", "GBP")
        .unwrap();
    let joint = fixture.account("Joint");
    let batch = fixture.batch(current);

    fixture
        .row(current, Some(batch), "imported", "GBP", Some("fp-1"))
        .unwrap();
    fixture
        .row(pot, Some(batch), "mirrored", "GBP", None)
        .unwrap();
    assert_refused(fixture.row(joint, Some(batch), "imported", "GBP", Some("fp-2")));
    assert_refused(fixture.row(pot, Some(batch), "imported", "GBP", Some("fp-3")));
    assert_refused(fixture.row(current, Some(batch), "mirrored", "GBP", None));
    assert_refused(fixture.row(joint, Some(batch), "mirrored", "GBP", None));

    let snapshot = fixture.snapshot(current, Some(batch), "csv").unwrap();
    fixture.snapshot(pot, Some(batch), "csv").unwrap();
    assert_refused(fixture.snapshot(joint, Some(batch), "csv"));
    assert_refused(fixture.conn().execute(
        "UPDATE balance_snapshot SET account_id = ?1 WHERE id = ?2",
        [joint, snapshot],
    ));
}

#[test]
fn import_batches_are_read_only_but_for_their_status() {
    let fixture = Fixture::new();
    let current = fixture.account("Current");
    let joint = fixture.account("Joint");
    let batch = fixture.batch(current);

    for (column, value) in [
        ("id", "100".to_owned()),
        ("account_id", joint.to_string()),
        ("source_file", "'other.csv'".to_owned()),
        ("format", "'ofx'".to_owned()),
        ("preset_id", "'uk/monzo'".to_owned()),
        ("imported_at", "'2020-01-01T00:00:00.000Z'".to_owned()),
    ] {
        assert_refused(fixture.conn().execute(
            &format!("UPDATE import_batch SET {column} = {value} WHERE id = ?1"),
            [batch],
        ));
    }
    fixture
        .conn()
        .execute(
            "UPDATE import_batch SET status = 'undone' WHERE id = ?1",
            [batch],
        )
        .unwrap();
}

/// Undo deletes a batch's transactions (pot-side ones included) and
/// balances, then marks it `undone`, in one DB transaction. An undone batch
/// stays empty and stays undone.
#[test]
fn undone_batches_hold_nothing() {
    let mut fixture = Fixture::new();
    let current = fixture.account("Current");
    let pot = fixture
        .add_account(Some(current), "Holiday", "pot", "GBP")
        .unwrap();
    let batch = fixture.batch(current);
    fixture.imported(current, batch, None, "fp-1");
    fixture
        .row(pot, Some(batch), "mirrored", "GBP", None)
        .unwrap();
    let snapshot = fixture.snapshot(current, Some(batch), "csv").unwrap();
    let undo = "UPDATE import_batch SET status = 'undone' WHERE id = ?1";

    // Not while it still has rows or balances.
    assert_refused(fixture.conn().execute(undo, [batch]));
    fixture
        .conn()
        .execute("DELETE FROM \"transaction\" WHERE batch_id = ?1", [batch])
        .unwrap();
    assert_refused(fixture.conn().execute(undo, [batch]));

    // The order core uses, in one DB transaction: delete, then mark.
    let tx = fixture.db.conn.transaction().unwrap();
    tx.execute("DELETE FROM balance_snapshot WHERE batch_id = ?1", [batch])
        .unwrap();
    tx.execute(undo, [batch]).unwrap();
    tx.commit().unwrap();

    // Undoing twice is harmless; it never comes back.
    fixture.conn().execute(undo, [batch]).unwrap();
    assert_refused(fixture.conn().execute(
        "UPDATE import_batch SET status = 'committed' WHERE id = ?1",
        [batch],
    ));

    // Nothing goes into it afterwards.
    assert_refused(fixture.row(current, Some(batch), "imported", "GBP", Some("fp-2")));
    assert_refused(fixture.row(pot, Some(batch), "mirrored", "GBP", None));
    assert_refused(fixture.snapshot(current, Some(batch), "csv"));
    let live = fixture.batch(current);
    let kept = fixture.snapshot(current, Some(live), "csv").unwrap();
    assert_ne!(kept, snapshot);
    assert_refused(fixture.conn().execute(
        "UPDATE balance_snapshot SET batch_id = ?1 WHERE id = ?2",
        [batch, kept],
    ));
}

#[test]
fn import_flags_are_read_only_and_go_before_undo() {
    let fixture = Fixture::new();
    let current = fixture.account("Current");
    let batch = fixture.batch(current);
    let flag = |batch: i64, kind: &str, location_kind: &str, location: i64| {
        fixture.conn().execute(
            "INSERT INTO import_flag (batch_id, location_kind, location, kind, detail) \
             VALUES (?1, ?2, ?3, ?4, 'the date is missing')",
            params![batch, location_kind, location, kind],
        )
    };
    flag(batch, "unreadable", "line", 3).unwrap();
    assert_refused(flag(batch, "skipped", "line", 3));
    assert_refused(flag(batch, "unreadable", "row", 3));
    assert_refused(flag(batch, "unreadable", "line", 0));
    assert_refused(fixture.conn().execute(
        "UPDATE import_flag SET detail = 'changed' WHERE batch_id = ?1",
        [batch],
    ));

    let undo = "UPDATE import_batch SET status = 'undone' WHERE id = ?1";
    assert_refused(fixture.conn().execute(undo, [batch]));
    fixture
        .conn()
        .execute("DELETE FROM import_flag WHERE batch_id = ?1", [batch])
        .unwrap();
    fixture.conn().execute(undo, [batch]).unwrap();
    assert_refused(flag(batch, "locked", "entry", 1));
}

#[test]
fn money_stays_in_the_account_currency() {
    let fixture = Fixture::new();
    let current = fixture.account("Current");
    let euro = fixture.add_account(None, "Euro", "current", "EUR").unwrap();
    let batch = fixture.batch(current);

    assert_refused(fixture.row(current, Some(batch), "imported", "EUR", Some("fp-1")));
    assert_refused(fixture.row(current, None, "manual", "EUR", None));
    let manual = fixture.manual(current);
    // A manual row moves account or changes currency only consistently.
    assert_refused(fixture.conn().execute(
        "UPDATE \"transaction\" SET account_id = ?1 WHERE id = ?2",
        [euro, manual],
    ));
    assert_refused(fixture.conn().execute(
        "UPDATE \"transaction\" SET currency = 'EUR' WHERE id = ?1",
        [manual],
    ));
    fixture
        .conn()
        .execute(
            "UPDATE \"transaction\" SET account_id = ?1, currency = 'EUR' WHERE id = ?2",
            [euro, manual],
        )
        .unwrap();

    // A pot is in its parent's currency.
    assert_refused(fixture.add_account(Some(current), "Holiday", "pot", "EUR"));
    let pot = fixture
        .add_account(Some(current), "Holiday", "pot", "GBP")
        .unwrap();
    assert_refused(
        fixture
            .conn()
            .execute("UPDATE account SET currency = 'EUR' WHERE id = ?1", [pot]),
    );

    // An account's currency is fixed once it has transactions, balances or
    // pots; an empty account's isn't.
    let fresh = fixture
        .add_account(None, "Fresh", "current", "GBP")
        .unwrap();
    fixture
        .conn()
        .execute("UPDATE account SET currency = 'USD' WHERE id = ?1", [fresh])
        .unwrap();
    fixture.snapshot(fresh, None, "manual").unwrap();
    assert_refused(
        fixture
            .conn()
            .execute("UPDATE account SET currency = 'GBP' WHERE id = ?1", [fresh]),
    );
    // `current` has a pot; `euro` has a transaction.
    assert_refused(fixture.conn().execute(
        "UPDATE account SET currency = 'EUR' WHERE id = ?1",
        [current],
    ));
    assert_refused(
        fixture
            .conn()
            .execute("UPDATE account SET currency = 'GBP' WHERE id = ?1", [euro]),
    );
}

/// A rule's amount bounds are in its account's minor units, so a bounded
/// rule fixes the account's currency; a rule without bounds doesn't.
#[test]
fn rule_amounts_fix_the_account_currency() {
    let fixture = Fixture::new();
    let bounded = fixture.account("Bounded");
    let unbounded = fixture.account("Unbounded");
    let add_rule = |account: i64, min: Option<i64>, max: Option<i64>| {
        fixture
            .conn()
            .execute(
                "INSERT INTO rule (priority, match_field, match_kind, pattern, amount_min, \
                 amount_max, account_id) VALUES (1, 'merchant', 'is', 'X', ?1, ?2, ?3)",
                params![min, max, account],
            )
            .unwrap();
    };
    add_rule(unbounded, None, None);
    add_rule(bounded, None, Some(-1000));
    let change = "UPDATE account SET currency = 'EUR' WHERE id = ?1";

    fixture.conn().execute(change, [unbounded]).unwrap();
    assert_refused(fixture.conn().execute(change, [bounded]));

    let lower = fixture.account("Lower");
    add_rule(lower, Some(-5000), None);
    assert_refused(fixture.conn().execute(change, [lower]));
}

/// The other side of `rule_amounts_fix_the_account_currency`: a rule with
/// bounds can't move to an account in another currency either.
#[test]
fn a_bounded_rule_stays_in_its_currency() {
    let fixture = Fixture::new();
    let gbp = fixture.account("Current");
    let other_gbp = fixture.account("Savings");
    let eur = fixture.add_account(None, "Euro", "current", "EUR").unwrap();
    let add_rule = |min: Option<i64>| -> i64 {
        fixture
            .conn()
            .execute(
                "INSERT INTO rule (priority, match_field, match_kind, pattern, amount_min, \
                 account_id) VALUES (1, 'merchant', 'is', 'X', ?1, ?2)",
                params![min, gbp],
            )
            .unwrap();
        fixture.conn().last_insert_rowid()
    };
    let move_to = "UPDATE rule SET account_id = ?1 WHERE id = ?2";
    let bounded = add_rule(Some(-1000));
    let unbounded = add_rule(None);

    assert_refused(fixture.conn().execute(move_to, params![eur, bounded]));
    // Same currency, no bounds, or no particular account: the numbers keep
    // their meaning.
    fixture
        .conn()
        .execute(move_to, params![other_gbp, bounded])
        .unwrap();
    fixture
        .conn()
        .execute(move_to, params![eur, unbounded])
        .unwrap();
    fixture
        .conn()
        .execute("UPDATE rule SET account_id = NULL WHERE id = ?1", [bounded])
        .unwrap();
    // Adding bounds to a rule that moves currency in the same statement is
    // refused too.
    assert_refused(fixture.conn().execute(
        "UPDATE rule SET account_id = ?1, amount_min = -1 WHERE id = ?2",
        params![gbp, unbounded],
    ));
}

/// A pot's mirrored rows and file balances came with a batch on its parent,
/// so a pot that holds anything keeps its parent.
#[test]
fn a_pot_with_history_keeps_its_parent() {
    let fixture = Fixture::new();
    let current = fixture.account("Current");
    let joint = fixture.account("Joint");
    let pot = fixture
        .add_account(Some(current), "Holiday", "pot", "GBP")
        .unwrap();
    let move_to_joint = "UPDATE account SET parent_id = ?1 WHERE id = ?2";

    // An empty pot can move.
    fixture.conn().execute(move_to_joint, [joint, pot]).unwrap();
    fixture
        .conn()
        .execute(move_to_joint, [current, pot])
        .unwrap();

    let batch = fixture.batch(current);
    fixture
        .row(pot, Some(batch), "mirrored", "GBP", None)
        .unwrap();
    assert_refused(fixture.conn().execute(move_to_joint, [joint, pot]));
    assert_refused(fixture.conn().execute(
        "UPDATE account SET parent_id = NULL, kind = 'savings' WHERE id = ?1",
        [pot],
    ));

    let balance_only = fixture
        .add_account(Some(current), "Rainy day", "pot", "GBP")
        .unwrap();
    fixture.snapshot(balance_only, Some(batch), "csv").unwrap();
    assert_refused(fixture.conn().execute(move_to_joint, [joint, balance_only]));
    // Other edits to the pot still work.
    fixture
        .conn()
        .execute(
            "UPDATE account SET name = 'Rainy days' WHERE id = ?1",
            [balance_only],
        )
        .unwrap();
}

/// `asset` and `liability` accounts hold `valuation` snapshots and nothing
/// else; no other account has valuations (ARCHITECTURE.md → "accounts you
/// keep yourself").
#[test]
fn valuation_accounts_hold_valuations_only() {
    let fixture = Fixture::new();
    let current = fixture.account("Current");
    let house = fixture.add_account(None, "House", "asset", "GBP").unwrap();
    let mortgage = fixture
        .add_account(None, "Mortgage", "liability", "GBP")
        .unwrap();

    fixture.snapshot(house, None, "valuation").unwrap();
    fixture.snapshot(mortgage, None, "valuation").unwrap();
    assert_refused(fixture.snapshot(house, None, "manual"));
    assert_refused(fixture.snapshot(current, None, "valuation"));
    assert_refused(fixture.row(house, None, "manual", "GBP", None));
    let manual = fixture.manual(current);
    assert_refused(fixture.conn().execute(
        "UPDATE \"transaction\" SET account_id = ?1 WHERE id = ?2",
        [house, manual],
    ));

    // Kind changes that would leave the wrong history behind.
    assert_refused(
        fixture
            .conn()
            .execute("UPDATE account SET kind = 'asset' WHERE id = ?1", [current]),
    );
    assert_refused(
        fixture
            .conn()
            .execute("UPDATE account SET kind = 'savings' WHERE id = ?1", [house]),
    );
    fixture
        .conn()
        .execute(
            "UPDATE account SET kind = 'liability' WHERE id = ?1",
            [house],
        )
        .unwrap();
    let empty = fixture
        .add_account(None, "Empty", "savings", "GBP")
        .unwrap();
    fixture
        .conn()
        .execute("UPDATE account SET kind = 'asset' WHERE id = ?1", [empty])
        .unwrap();
}

#[test]
fn budget_groups_fix_rollover() {
    let fixture = Fixture::new();
    let category = fixture.default_category("food");
    let budget = |month: Option<&str>, rollover: bool, group: &str, set_aside: Option<i64>| {
        fixture.conn().execute(
            "INSERT INTO budget (category_id, month, amount_minor, currency, rollover, \
             group_kind, opening_set_aside_minor) VALUES (?1, ?2, 30000, 'GBP', ?3, ?4, ?5)",
            params![category, month, rollover, group, set_aside],
        )
    };

    assert_refused(budget(None, true, "fixed", None));
    assert_refused(budget(None, false, "non_monthly", None));
    assert_refused(budget(None, true, "flex", Some(100)));
    assert_refused(budget(Some("2026-13"), true, "flex", None));
    budget(None, true, "flex", None).unwrap();
    budget(Some("2026-12"), true, "flex", None).unwrap();
    // One standing budget and one override per month.
    assert_refused(budget(None, false, "flex", None));
    assert_refused(budget(Some("2026-12"), false, "flex", None));
}

#[test]
fn rule_actions_are_json_and_unique_per_kind() {
    let fixture = Fixture::new();
    fixture
        .conn()
        .execute(
            "INSERT INTO rule (priority, match_field, match_kind, pattern) \
             VALUES (1, 'merchant', 'contains', 'UBER')",
            [],
        )
        .unwrap();
    let rule = fixture.conn().last_insert_rowid();
    let action = |kind: &str, value: &str| {
        fixture.conn().execute(
            "INSERT INTO rule_action (rule_id, kind, value_json) VALUES (?1, ?2, ?3)",
            params![rule, kind, value],
        )
    };

    action("set_category", r#""transport""#).unwrap();
    assert_refused(action("set_category", r#""food""#));
    assert_refused(action("set_merchant", "not json"));
    assert_refused(fixture.conn().execute(
        "INSERT INTO rule (priority, match_field, match_kind, pattern, amount_min, amount_max) \
         VALUES (2, 'merchant', 'is', 'X', 500, 100)",
        [],
    ));
}

#[test]
fn fx_rates_are_fixed_point() {
    let fixture = Fixture::new();
    let rate = |base: &str, quote: &str, rate: rusqlite::types::Value| {
        fixture.conn().execute(
            "INSERT INTO fx_rate (date, base, quote, rate_e9, source) \
             VALUES ('2026-11-10', ?1, ?2, ?3, 'ecb')",
            params![base, quote, rate],
        )
    };

    // 1 EUR = 0.86213 GBP
    rate("EUR", "GBP", 862_130_000.into()).unwrap();
    assert_refused(rate("EUR", "USD", 1.0712.into()));
    assert_refused(rate("EUR", "EUR", 1_000_000_000.into()));
    assert_refused(rate("EUR", "JPY", 0.into()));
}

#[test]
fn suggestions_and_reports() {
    let fixture = Fixture::new();
    fixture
        .conn()
        .execute(
            "INSERT INTO suggestion (kind, payload_json, rationale, source_client) \
             VALUES ('categorization', '{\"transaction_id\": 1}', 'Looks like a café', 'claude')",
            [],
        )
        .unwrap();
    let status: String = fixture
        .conn()
        .query_row("SELECT status FROM suggestion", [], |row| row.get(0))
        .unwrap();
    assert_eq!(status, "pending");
    assert_refused(fixture.conn().execute(
        "INSERT INTO suggestion (kind, payload_json, source_client) \
         VALUES ('delete_everything', '{}', 'claude')",
        [],
    ));

    fixture
        .conn()
        .execute(
            "INSERT INTO report (period, title, body_markdown, source_client) \
             VALUES ('2026-10', 'October review', '*Spending* was down.', 'claude')",
            [],
        )
        .unwrap();
}

/// IDs aren't reused after a delete, so an old suggestion or log entry
/// can't come to name a different transaction.
#[test]
fn tag_names_are_checked_and_unique_ignoring_case() {
    let fixture = Fixture::new();
    let insert = |name: &str, private: i64| {
        fixture.conn().execute(
            "INSERT INTO tag (name, private) VALUES (?1, ?2)",
            params![name, private],
        )
    };

    insert("Work", 0).unwrap();
    insert("Clinic", 1).unwrap();
    // NOCASE folds ASCII: core folds the rest (`coffer_core::tag`).
    assert_refused(insert("WORK", 0));
    assert_refused(insert("work", 1));
    for blank in ["", " ", "\t"] {
        assert_refused(insert(blank, 0));
    }
    for padded in [" Home", "Home ", "\nHome", "Home\t"] {
        assert_refused(insert(padded, 0));
    }
    insert(&"x".repeat(64), 0).unwrap();
    assert_refused(insert(&"y".repeat(65), 0));
    assert_refused(insert("Home", 2));
    assert_refused(
        fixture
            .conn()
            .execute("UPDATE tag SET name = 'clinic' WHERE name = 'Work'", []),
    );
    // Renaming, including to its own name in another case, is fine.
    fixture
        .conn()
        .execute(
            "UPDATE tag SET name = 'WORK', private = 1 WHERE name = 'Work'",
            [],
        )
        .unwrap();
}

#[test]
fn tag_links_are_unique_fixed_and_explicitly_deleted() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let batch = fixture.batch(account);
    let imported = fixture.imported(account, batch, None, "fp-1");
    let manual = fixture.manual(account);
    let tag = |name: &str| {
        fixture
            .conn()
            .execute("INSERT INTO tag (name) VALUES (?1)", [name])
            .unwrap();
        fixture.conn().last_insert_rowid()
    };
    let (work, home) = (tag("Work"), tag("Home"));
    let link = |transaction: i64, tag: i64| {
        fixture.conn().execute(
            "INSERT INTO transaction_tag (transaction_id, tag_id) VALUES (?1, ?2)",
            [transaction, tag],
        )
    };

    // Several tags on a row, and a tag on several rows, of any origin.
    link(imported, work).unwrap();
    link(imported, home).unwrap();
    link(manual, work).unwrap();
    assert_refused(link(imported, work));
    assert_refused(link(imported, 99));
    assert_refused(link(99, work));

    // A link is added or removed, never moved.
    for sql in [
        "UPDATE transaction_tag SET transaction_id = ?1 WHERE tag_id = ?2",
        "UPDATE transaction_tag SET tag_id = ?2 WHERE transaction_id = ?1",
    ] {
        assert_refused(fixture.conn().execute(sql, [manual, home]));
    }

    // No ON DELETE: a tag or a transaction with links can't be deleted
    // until core deletes the links.
    assert_refused(
        fixture
            .conn()
            .execute("DELETE FROM tag WHERE id = ?1", [work]),
    );
    assert_refused(
        fixture
            .conn()
            .execute("DELETE FROM \"transaction\" WHERE id = ?1", [manual]),
    );
    fixture
        .conn()
        .execute(
            "DELETE FROM transaction_tag WHERE transaction_id = ?1",
            [manual],
        )
        .unwrap();
    fixture
        .conn()
        .execute("DELETE FROM \"transaction\" WHERE id = ?1", [manual])
        .unwrap();
}

#[test]
fn ids_are_not_reused() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let first = fixture.manual(account);
    fixture
        .conn()
        .execute("DELETE FROM \"transaction\" WHERE id = ?1", [first])
        .unwrap();

    assert!(fixture.manual(account) > first);
}

#[test]
fn split_lines_have_the_parents_sign() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let id = fixture.manual(account); // -500

    fixture.split_line(id, 1, -300, None).unwrap();
    assert_refused(fixture.split_line(id, 2, 200, None));
    assert_refused(fixture.split_line(id, 2, 0, None));
    // Line numbers are unique per transaction and start at 1.
    assert_refused(fixture.split_line(id, 1, -200, None));
    assert_refused(fixture.split_line(id, 0, -200, None));
    let second = fixture.split_line(id, 2, -200, None).unwrap();
    // Not a fraction, like every other amount.
    assert_refused(fixture.conn().execute(
        "UPDATE transaction_split SET amount_minor = -199.5 WHERE id = ?1",
        [second],
    ));
    assert_refused(fixture.conn().execute(
        "UPDATE transaction_split SET amount_minor = 200 WHERE id = ?1",
        [second],
    ));
    fixture
        .conn()
        .execute(
            "UPDATE transaction_split SET amount_minor = -150 WHERE id = ?1",
            [second],
        )
        .unwrap();

    // A zero transaction has no sign to split into.
    fixture
        .conn()
        .execute(
            "INSERT INTO \"transaction\" (account_id, date, amount_minor, currency, \
             description_raw, description_normalized, origin) \
             VALUES (?1, '2026-11-03', 0, 'GBP', 'Zero', 'ZERO', 'manual')",
            [account],
        )
        .unwrap();
    let zero = fixture.conn().last_insert_rowid();
    assert_refused(fixture.split_line(zero, 1, -1, None));
    assert_refused(fixture.split_line(zero, 1, 1, None));

    // A refund splits into amounts back.
    let refund = fixture.manual(account);
    fixture
        .conn()
        .execute(
            "UPDATE \"transaction\" SET amount_minor = 500 WHERE id = ?1",
            [refund],
        )
        .unwrap();
    fixture.split_line(refund, 1, 100, None).unwrap();
    assert_refused(fixture.split_line(refund, 2, -400, None));
}

#[test]
fn split_lines_stay_with_their_transaction() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let first = fixture.manual(account);
    let second = fixture.manual(account);
    let line = fixture.split_line(first, 1, -300, None).unwrap();

    assert_refused(fixture.conn().execute(
        "UPDATE transaction_split SET transaction_id = ?1 WHERE id = ?2",
        [second, line],
    ));
    assert_refused(fixture.conn().execute(
        "UPDATE transaction_split SET id = 100 WHERE id = ?1",
        [line],
    ));
    // Deleting the transaction is refused while it has lines; they go
    // first.
    assert_refused(
        fixture
            .conn()
            .execute("DELETE FROM \"transaction\" WHERE id = ?1", [first]),
    );
    fixture
        .conn()
        .execute(
            "DELETE FROM transaction_split WHERE transaction_id = ?1",
            [first],
        )
        .unwrap();
    fixture
        .conn()
        .execute("DELETE FROM \"transaction\" WHERE id = ?1", [first])
        .unwrap();
}

#[test]
fn a_split_lines_share_is_part_of_the_line() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let id = fixture.manual(account);
    let line = fixture.split_line(id, 1, -300, None).unwrap();
    let share = |minor: i64| {
        fixture.conn().execute(
            "UPDATE transaction_split SET my_share_minor = ?1 WHERE id = ?2",
            params![minor, line],
        )
    };

    share(-150).unwrap();
    share(0).unwrap();
    share(-300).unwrap();
    assert_refused(share(150));
    assert_refused(share(-301));
    assert_refused(fixture.conn().execute(
        "UPDATE transaction_split SET share_label = '' WHERE id = ?1",
        [line],
    ));
}

#[test]
fn a_split_transaction_has_no_category_or_share_of_its_own() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let food = fixture.default_category("food");
    let id = fixture.manual(account);

    // Not while it has its own category or share.
    fixture
        .conn()
        .execute(
            "UPDATE \"transaction\" SET category_id = ?1, category_source = 'user' \
             WHERE id = ?2",
            [food, id],
        )
        .unwrap();
    assert_refused(fixture.split_line(id, 1, -300, Some(food)));
    fixture
        .conn()
        .execute(
            "UPDATE \"transaction\" SET category_id = NULL, category_source = NULL, \
             my_share_minor = -250 WHERE id = ?1",
            [id],
        )
        .unwrap();
    assert_refused(fixture.split_line(id, 1, -300, Some(food)));
    fixture
        .conn()
        .execute(
            "UPDATE \"transaction\" SET my_share_minor = NULL, share_label = 'Sam' WHERE id = ?1",
            [id],
        )
        .unwrap();
    assert_refused(fixture.split_line(id, 1, -300, Some(food)));
    fixture
        .conn()
        .execute(
            "UPDATE \"transaction\" SET share_label = NULL WHERE id = ?1",
            [id],
        )
        .unwrap();
    fixture.split_line(id, 1, -300, Some(food)).unwrap();

    // Nor once it is split.
    assert_refused(fixture.conn().execute(
        "UPDATE \"transaction\" SET category_id = ?1, category_source = 'user' WHERE id = ?2",
        [food, id],
    ));
    assert_refused(fixture.conn().execute(
        "UPDATE \"transaction\" SET my_share_minor = -100 WHERE id = ?1",
        [id],
    ));
    assert_refused(fixture.conn().execute(
        "UPDATE \"transaction\" SET share_label = 'Sam' WHERE id = ?1",
        [id],
    ));
    // Clearing them, and editing the other fields, is fine.
    fixture
        .conn()
        .execute(
            "UPDATE \"transaction\" SET category_id = NULL, category_source = NULL, \
             user_note = 'Weekly shop', reimbursable = 1, excluded_reason = 'disputed', \
             excluded_at = '2026-11-12T10:00:00.000Z' WHERE id = ?1",
            [id],
        )
        .unwrap();
}

/// The lines must keep summing to the amount, so a manual row's amount and
/// currency are fixed while it is split.
#[test]
fn a_split_transactions_amount_is_fixed() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let euros = fixture.add_account(None, "Euro", "current", "EUR").unwrap();
    let id = fixture.manual(account); // -500
    fixture.split_line(id, 1, -300, None).unwrap();
    fixture.split_line(id, 2, -200, None).unwrap();

    assert_refused(fixture.conn().execute(
        "UPDATE \"transaction\" SET amount_minor = -700 WHERE id = ?1",
        [id],
    ));
    assert_refused(fixture.conn().execute(
        "UPDATE \"transaction\" SET account_id = ?1, currency = 'EUR' WHERE id = ?2",
        [euros, id],
    ));
    // The same amount, and the other source fields of a manual row, can be
    // written.
    fixture
        .conn()
        .execute(
            "UPDATE \"transaction\" SET amount_minor = -500, date = '2026-11-04', \
             description_raw = 'Market', description_normalized = 'MARKET' WHERE id = ?1",
            [id],
        )
        .unwrap();

    // Without the split, the amount is the user's to change again.
    fixture
        .conn()
        .execute(
            "DELETE FROM transaction_split WHERE transaction_id = ?1",
            [id],
        )
        .unwrap();
    fixture
        .conn()
        .execute(
            "UPDATE \"transaction\" SET amount_minor = -700 WHERE id = ?1",
            [id],
        )
        .unwrap();
}

#[test]
fn a_transfer_is_not_split() {
    let fixture = Fixture::new();
    let current = fixture.account("Current");
    let savings = fixture.account("Savings");
    let out = fixture.manual(current);
    let back = fixture.manual(savings);

    for status in ["suggested", "confirmed"] {
        let group = fixture.transfer_group(status, &[out, back]);
        assert_refused(fixture.split_line(out, 1, -300, None));
        fixture
            .conn()
            .execute(
                "UPDATE \"transaction\" SET transfer_group_id = NULL WHERE transfer_group_id = ?1",
                [group],
            )
            .unwrap();
    }

    // A rejected pair is two ordinary transactions.
    let rejected = fixture.transfer_group("rejected", &[out, back]);
    fixture.split_line(out, 1, -300, None).unwrap();
    assert_refused(fixture.conn().execute(
        "UPDATE transfer_group SET status = 'confirmed' WHERE id = ?1",
        [rejected],
    ));
    assert_refused(fixture.conn().execute(
        "UPDATE transfer_group SET status = 'suggested' WHERE id = ?1",
        [rejected],
    ));

    // A split row can leave its group, but not join a live one.
    fixture
        .conn()
        .execute(
            "UPDATE \"transaction\" SET transfer_group_id = NULL WHERE id = ?1",
            [out],
        )
        .unwrap();
    fixture
        .conn()
        .execute(
            "INSERT INTO transfer_group (status) VALUES ('confirmed')",
            [],
        )
        .unwrap();
    let confirmed = fixture.conn().last_insert_rowid();
    assert_refused(fixture.conn().execute(
        "UPDATE \"transaction\" SET transfer_group_id = ?1 WHERE id = ?2",
        [confirmed, out],
    ));
    fixture
        .conn()
        .execute(
            "UPDATE \"transaction\" SET transfer_group_id = ?1 WHERE id = ?2",
            [rejected, out],
        )
        .unwrap();
}

#[test]
fn ledger_line_has_each_split_line_or_the_transaction() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let food = fixture.default_category("food");
    let home = fixture.default_category("home");
    let whole = fixture.manual(account); // -500
    fixture
        .conn()
        .execute(
            "UPDATE \"transaction\" SET category_id = ?1, category_source = 'user', \
             my_share_minor = -250, share_label = 'Sam' WHERE id = ?2",
            [food, whole],
        )
        .unwrap();
    let split = fixture.manual(account); // -500
    let first = fixture.split_line(split, 1, -300, Some(food)).unwrap();
    let second = fixture.split_line(split, 2, -200, Some(home)).unwrap();
    fixture
        .conn()
        .execute(
            "UPDATE transaction_split SET my_share_minor = -100, share_label = 'Alex' \
             WHERE id = ?1",
            [second],
        )
        .unwrap();
    // Excluded rows and transfers stay in the view, to be filtered by
    // whoever reads it.
    fixture
        .conn()
        .execute(
            "UPDATE \"transaction\" SET excluded_reason = 'disputed', \
             excluded_at = '2026-11-12T10:00:00.000Z' WHERE id = ?1",
            [split],
        )
        .unwrap();
    let other = fixture.account("Savings");
    let moved = fixture.manual(other);
    let group = fixture.transfer_group("confirmed", &[moved]);

    type LedgerRow = (
        i64,
        Option<i64>,
        Option<i64>,
        i64,
        String,
        Option<i64>,
        Option<i64>,
        i64,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
    );
    let mut statement = fixture
        .conn()
        .prepare(
            "SELECT transaction_id, split_id, line_no, amount_minor, currency, category_id, \
             my_share_minor, my_amount_minor, share_label, excluded_reason, transfer_group_id, \
             transfer_status FROM ledger_line ORDER BY transaction_id, line_no",
        )
        .unwrap();
    let rows: Vec<LedgerRow> = statement
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
                row.get(11)?,
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let gbp = || "GBP".to_owned();
    let disputed = || Some("disputed".to_owned());

    assert_eq!(
        rows,
        [
            (
                whole,
                None,
                None,
                -500,
                gbp(),
                Some(food),
                Some(-250),
                -250,
                Some("Sam".to_owned()),
                None,
                None,
                None
            ),
            (
                split,
                Some(first),
                Some(1),
                -300,
                gbp(),
                Some(food),
                None,
                -300,
                None,
                disputed(),
                None,
                None
            ),
            (
                split,
                Some(second),
                Some(2),
                -200,
                gbp(),
                Some(home),
                Some(-100),
                -100,
                Some("Alex".to_owned()),
                disputed(),
                None,
                None
            ),
            (
                moved,
                None,
                None,
                -500,
                gbp(),
                None,
                None,
                -500,
                None,
                None,
                Some(group),
                Some("confirmed".to_owned())
            ),
        ]
    );

    // The view's amounts add up to the transactions'.
    let totals: (i64, i64) = fixture
        .conn()
        .query_row(
            "SELECT (SELECT sum(amount_minor) FROM ledger_line), \
             (SELECT sum(amount_minor) FROM \"transaction\")",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(totals, (-1500, -1500));
}

#[test]
fn merchant_aliases_are_checked() {
    let fixture = Fixture::new();
    let alias = |kind: &str, pattern: &str, name: rusqlite::types::Value| {
        fixture.conn().execute(
            "INSERT INTO merchant_alias (match_kind, pattern, display_name) VALUES (?1, ?2, ?3)",
            params![kind, pattern, name],
        )
    };
    let text = |text: &str| rusqlite::types::Value::Text(text.to_owned());

    alias("exact", "ACME", text("Acme")).unwrap();
    alias("prefix", "ACME ", text("Acme")).unwrap();
    alias("regex", "^acme", text("Acme")).unwrap();
    assert_refused(alias("contains", "ACME", text("Acme")));
    assert_refused(alias("exact", "", text("Acme")));
    assert_refused(alias("exact", "  ", text("Acme")));
    assert_refused(alias("exact", "ACME", text("")));
    assert_refused(alias("exact", "ACME", text(" Acme")));
    assert_refused(alias("exact", "ACME", text("Acme ")));
    // STRICT: a name is text.
    assert!(
        alias(
            "exact",
            "ACME",
            rusqlite::types::Value::Blob(b"Acme".to_vec())
        )
        .is_err()
    );
}

/// `merchant_display_source` says who set the name: NULL (derived), a
/// rule or the user. A set name is never blank. Both stay editable on
/// imported rows: they are user-facing fields.
#[test]
fn merchant_display_source_is_checked() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let batch = fixture.batch(account);
    let id = fixture.imported(account, batch, None, "fp-1");
    let set = |name: &str, source: &str| {
        fixture.conn().execute(
            &format!(
                "UPDATE \"transaction\" SET merchant_display = {name}, \
                 merchant_display_source = {source} WHERE id = ?1"
            ),
            [id],
        )
    };

    set("'Coffee'", "NULL").unwrap();
    set("NULL", "NULL").unwrap();
    set("'Coffee'", "'user'").unwrap();
    set("'Coffee shop'", "'rule'").unwrap();
    assert_refused(set("'Coffee'", "'agent'"));
    assert_refused(set("NULL", "'user'"));
    assert_refused(set("''", "'user'"));
    assert_refused(set("'  '", "'rule'"));
}

impl Fixture {
    /// Inserts a manual transaction of `minor` in `currency` on `account`.
    fn manual_amount(&self, account: i64, minor: i64, currency: &str) -> i64 {
        self.conn()
            .execute(
                "INSERT INTO \"transaction\" (account_id, date, amount_minor, currency, \
                 description_raw, description_normalized, origin) \
                 VALUES (?1, '2026-11-03', ?2, ?3, 'Shop', 'SHOP', 'manual')",
                params![account, minor, currency],
            )
            .unwrap();
        self.conn().last_insert_rowid()
    }

    fn link(
        &self,
        kind: &str,
        inflow: i64,
        target: Option<i64>,
        label: Option<&str>,
        minor: i64,
        status: &str,
    ) -> rusqlite::Result<i64> {
        self.conn().execute(
            "INSERT INTO transaction_link (kind, inflow_id, target_id, share_label, \
             amount_minor, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![kind, inflow, target, label, minor, status],
        )?;
        Ok(self.conn().last_insert_rowid())
    }
}

#[test]
fn links_join_money_in_to_money_out_in_one_currency() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let euros = fixture.add_account(None, "Euro", "current", "EUR").unwrap();
    let purchase = fixture.manual_amount(account, -1000, "GBP");
    let refund = fixture.manual_amount(account, 1000, "GBP");
    let euro_refund = fixture.manual_amount(euros, 1000, "EUR");
    let link = |kind, inflow, target, label, minor| {
        fixture.link(kind, inflow, target, label, minor, "confirmed")
    };

    // Shape: a settlement names a person, the others a purchase.
    assert_refused(link("refund", refund, None, None, 100));
    assert_refused(link("refund", refund, Some(purchase), Some("Sam"), 100));
    assert_refused(link("settlement", refund, Some(purchase), Some("Sam"), 100));
    assert_refused(link("settlement", refund, None, Some(""), 100));
    assert_refused(link("loan", refund, Some(purchase), None, 100));
    // Sides, sign, currency, no self-link, a positive amount.
    assert_refused(link("refund", purchase, Some(refund), None, 100));
    assert_refused(link("refund", refund, Some(refund), None, 100));
    assert_refused(link("refund", euro_refund, Some(purchase), None, 100));
    assert_refused(link("refund", refund, Some(purchase), None, 0));
    assert_refused(link("refund", refund, Some(purchase), None, -100));
    assert_refused(fixture.link("refund", refund, Some(purchase), None, 100, "maybe"));
    // A reimbursement repays a claim.
    assert_refused(link("reimbursement", refund, Some(purchase), None, 100));

    // Caps: each link, and confirmed links together, on both sides.
    assert_refused(link("refund", refund, Some(purchase), None, 1001));
    let first = link("refund", refund, Some(purchase), None, 600).unwrap();
    let second_purchase = fixture.manual_amount(account, -500, "GBP");
    assert_refused(link("refund", refund, Some(second_purchase), None, 401));
    link("refund", refund, Some(second_purchase), None, 400).unwrap();
    let second_refund = fixture.manual_amount(account, 1000, "GBP");
    assert_refused(link("refund", second_refund, Some(purchase), None, 401));
    // A suggestion only has to fit on its own.
    fixture
        .link(
            "refund",
            second_refund,
            Some(purchase),
            None,
            1000,
            "suggested",
        )
        .unwrap();
    // One link per pair.
    assert_refused(link("refund", refund, Some(purchase), None, 100));

    // Confirming over the cap is refused; rejecting frees it.
    let suggestion: i64 = fixture
        .conn()
        .query_row(
            "SELECT id FROM transaction_link WHERE status = 'suggested'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let set_status = |id: i64, status: &str| {
        fixture.conn().execute(
            "UPDATE transaction_link SET status = ?1 WHERE id = ?2",
            params![status, id],
        )
    };
    assert_refused(set_status(suggestion, "confirmed"));
    set_status(first, "rejected").unwrap();
    // A rejected pair isn't suggested again, but can be confirmed.
    assert_refused(set_status(first, "suggested"));
    // Only the amount and status change.
    for column in [
        "kind = 'reimbursement'",
        "inflow_id = inflow_id + 1",
        "created_at = '2026-01-01T00:00:00.000Z'",
    ] {
        assert_refused(fixture.conn().execute(
            &format!("UPDATE transaction_link SET {column} WHERE id = ?1"),
            [first],
        ));
    }
    // With the first link rejected, the suggestion fits.
    set_status(suggestion, "confirmed").unwrap();
    assert_refused(set_status(first, "confirmed"));

    // A purchase too large for exact pro-rata arithmetic.
    let huge = fixture.manual_amount(account, -3_037_000_500, "GBP");
    let big_refund = fixture.manual_amount(account, 3_037_000_500, "GBP");
    assert_refused(link("refund", big_refund, Some(huge), None, 100));
}

#[test]
fn a_linked_transaction_keeps_its_amount_and_its_inflow_stays_whole() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let purchase = fixture.manual_amount(account, -1000, "GBP");
    let refund = fixture.manual_amount(account, 1000, "GBP");
    let link = fixture
        .link("refund", refund, Some(purchase), None, 500, "suggested")
        .unwrap();
    for row in [purchase, refund] {
        assert_refused(fixture.conn().execute(
            "UPDATE \"transaction\" SET amount_minor = amount_minor * 2 WHERE id = ?1",
            [row],
        ));
    }
    assert_refused(fixture.conn().execute(
        "UPDATE \"transaction\" SET my_share_minor = 500, share_label = 'Sam' WHERE id = ?1",
        [refund],
    ));
    assert_refused(fixture.split_line(refund, 1, 500, None));
    // The purchase can be split; deleting a linked row is refused until its
    // links go.
    fixture.split_line(purchase, 1, -400, None).unwrap();
    fixture.split_line(purchase, 2, -600, None).unwrap();
    assert_refused(
        fixture
            .conn()
            .execute(
                "DELETE FROM transaction_split WHERE transaction_id = ?1",
                [purchase],
            )
            .and_then(|_| {
                fixture
                    .conn()
                    .execute("DELETE FROM \"transaction\" WHERE id = ?1", [purchase])
            }),
    );

    // Once rejected, the link holds nothing back.
    fixture
        .conn()
        .execute(
            "UPDATE transaction_link SET status = 'rejected' WHERE id = ?1",
            [link],
        )
        .unwrap();
    fixture
        .conn()
        .execute(
            "UPDATE \"transaction\" SET amount_minor = 2000 WHERE id = ?1",
            [refund],
        )
        .unwrap();
}

#[test]
fn ledger_line_nets_confirmed_links_only() {
    let fixture = Fixture::new();
    let account = fixture.account("Current");
    let shopping = fixture.default_category("shopping");
    let purchase = fixture.manual_amount(account, -1000, "GBP");
    fixture
        .conn()
        .execute(
            "UPDATE \"transaction\" SET category_id = ?1, category_source = 'user' WHERE id = ?2",
            [shopping, purchase],
        )
        .unwrap();
    let refund = fixture.manual_amount(account, 1500, "GBP");
    let link = fixture
        .link("refund", refund, Some(purchase), None, 1000, "suggested")
        .unwrap();
    let rows = |transaction: i64| -> Vec<(i64, Option<i64>, Option<i64>, Option<i64>)> {
        let mut statement = fixture
            .conn()
            .prepare(
                "SELECT amount_minor, category_id, link_id, target_id FROM ledger_line \
                 WHERE transaction_id = ?1 ORDER BY link_id IS NOT NULL",
            )
            .unwrap();
        statement
            .query_map([transaction], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(rows(refund), [(1500, None, None, None)]);
    fixture
        .conn()
        .execute(
            "UPDATE transaction_link SET status = 'confirmed' WHERE id = ?1",
            [link],
        )
        .unwrap();
    assert_eq!(
        rows(refund),
        [
            (500, None, None, None),
            (1000, Some(shopping), Some(link), Some(purchase))
        ]
    );
    assert_eq!(rows(purchase), [(-1000, Some(shopping), None, None)]);
}
