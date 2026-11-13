//! Seeding and checks for the inbox tools' tests (`tests/inbox.rs`).
//!
//! - [`seed`] adds what the proposals need to the fixture database and
//!   returns the IDs: money in and out on the Monzo account, a matching
//!   transfer pair across two accounts, and a split transaction.
//! - [`outside_inbox`] snapshots every table an MCP tool must not write;
//!   [`suggestions`], [`reports`] and [`inbox_counts`] read what it may.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use coffer_core::category::CategoryKey;
use coffer_core::id::{SplitId, TransactionId};
use coffer_core::manual::{self, ManualTransaction};
use coffer_core::money::{Currency, Money};
use coffer_core::split::{self, SplitLine};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OpenFlags};

use super::Fixture;

/// The rows the inbox tests propose things about.
#[derive(Debug, Clone, Copy)]
pub struct InboxIds {
    /// An unsplit GBP purchase (money out) on the Monzo account.
    pub money_out: TransactionId,
    /// An unsplit GBP payment in on the Monzo account.
    pub money_in: TransactionId,
    /// Money out on the Monzo account, with [`InboxIds::transfer_in`] the
    /// same amount in on the bank account: a transfer no one has paired.
    pub transfer_out: TransactionId,
    pub transfer_in: TransactionId,
    /// A transaction split into two lines.
    pub split_parent: TransactionId,
    /// The split's first line.
    pub split_line: SplitId,
}

/// The amount of the seeded transfer, in minor units.
pub const TRANSFER_MINOR: i64 = 31_337;

/// Adds the transfer pair and the split to `fixture` and finds the rest.
pub fn seed(fixture: &Fixture) -> InboxIds {
    let mut db = fixture.open();
    let date = NaiveDate::from_ymd_opt(2026, 11, 20).unwrap();
    let mut add = |account, minor| {
        manual::create(
            &mut db,
            &ManualTransaction::new(
                account,
                date,
                Money::new(minor, Currency::GBP),
                "Inbox fixture row",
            ),
        )
        .unwrap()
    };
    let transfer_out = add(fixture.seeded.monzo, -TRANSFER_MINOR);
    let transfer_in = add(fixture.seeded.bank, TRANSFER_MINOR);
    let split_parent = add(fixture.seeded.monzo, -1_000);
    let lines = split::set(
        &mut db,
        split_parent,
        &[
            SplitLine::new(
                Money::new(-600, Currency::GBP),
                Some(CategoryKey::new("food.groceries").unwrap()),
            ),
            SplitLine::new(
                Money::new(-400, Currency::GBP),
                Some(CategoryKey::new("food.eating_out").unwrap()),
            ),
        ],
    )
    .unwrap();

    let conn = read_only(fixture);
    let imported = |sign: &str| -> TransactionId {
        conn.query_row(
            &format!(
                "SELECT id FROM \"transaction\" t WHERE account_id = ?1 AND origin = 'imported' \
                 AND currency = 'GBP' AND amount_minor {sign} 0 AND excluded_reason IS NULL \
                 AND transfer_group_id IS NULL \
                 AND NOT EXISTS (SELECT 1 FROM transaction_split s WHERE s.transaction_id = t.id) \
                 ORDER BY id LIMIT 1"
            ),
            [fixture.seeded.monzo.get()],
            |row| row.get(0),
        )
        .map(TransactionId::new)
        .unwrap()
    };
    InboxIds {
        money_out: imported("<"),
        money_in: imported(">"),
        transfer_out,
        transfer_in,
        split_parent,
        split_line: lines[0],
    }
}

/// A read-only connection to the fixture database.
pub fn read_only(fixture: &Fixture) -> Connection {
    Connection::open_with_flags(fixture.path(), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

/// The tables an MCP tool may write, or that aren't data: the inbox, the
/// server's own log, migrations, SQLite's internals and `mcp_client` (the
/// server's own record of its clients: `store.rs`, Block B).
const NOT_COMPARED: &[&str] = &[
    "suggestion",
    "report",
    "mcp_access_log",
    "refinery_schema_history",
    "mcp_client",
];

/// Every row of every table an MCP tool must not write, by table, as
/// sorted text.
pub fn outside_inbox(fixture: &Fixture) -> BTreeMap<String, Vec<String>> {
    let conn = read_only(fixture);
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    tables
        .into_iter()
        .filter(|table| !NOT_COMPARED.contains(&table.as_str()))
        .map(|table| {
            let mut statement = conn.prepare(&format!("SELECT * FROM \"{table}\"")).unwrap();
            let columns = statement.column_count();
            let mut rows: Vec<String> = statement
                .query_map([], |row| {
                    let values = (0..columns)
                        .map(|i| row.get::<_, SqlValue>(i))
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(format!("{values:?}"))
                })
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            rows.sort();
            (table, rows)
        })
        .collect()
}

/// A stored suggestion, as the tests check it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSuggestion {
    pub id: i64,
    pub kind: String,
    pub payload_json: String,
    pub rationale: Option<String>,
    pub source_client: String,
    pub status: String,
}

/// Every suggestion, oldest first.
pub fn suggestions(fixture: &Fixture) -> Vec<StoredSuggestion> {
    read_only(fixture)
        .prepare(
            "SELECT id, kind, payload_json, rationale, source_client, status FROM suggestion \
             ORDER BY id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok(StoredSuggestion {
                id: row.get(0)?,
                kind: row.get(1)?,
                payload_json: row.get(2)?,
                rationale: row.get(3)?,
                source_client: row.get(4)?,
                status: row.get(5)?,
            })
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Every report as `(id, source_client)`, oldest first.
pub fn reports(fixture: &Fixture) -> Vec<(i64, String)> {
    read_only(fixture)
        .prepare("SELECT id, source_client FROM report ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// How many suggestions and reports there are.
pub fn inbox_counts(fixture: &Fixture) -> (usize, usize) {
    (suggestions(fixture).len(), reports(fixture).len())
}
