//! A scratch database for the inbox's tests: accounts, rows and filing
//! helpers. Shared by every test module under `inbox` (and `change`'s hook
//! tests), so they seed data the same way.
#![allow(dead_code)] // Each test module uses a subset.

use chrono::NaiveDate;
use rusqlite::{Connection, params};
use serde_json::Value;
use tempfile::TempDir;

use super::{Filed, NewSuggestion, SuggestionId, SuggestionKind, file_suggestion};
use crate::account::{self, AccountKind, NewAccount, date_key};
use crate::category::CategoryKey;
use crate::db::Database;
use crate::id::{AccountId, SplitId, TransactionId};
use crate::mcp_client::ClientName;
use crate::money::{Currency, Money};
use crate::split::{self, SplitLine};

pub(crate) fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
}

pub(crate) struct Fixture {
    pub db: Database,
    /// A GBP current account.
    pub account: AccountId,
    /// A second GBP account, for transfers.
    pub savings: AccountId,
    _dir: TempDir,
}

impl Fixture {
    pub fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
        let account = account::create(
            &mut db,
            &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        let savings = account::create(
            &mut db,
            &NewAccount::new("Savings", AccountKind::Savings, Currency::GBP),
        )
        .unwrap();
        Fixture {
            db,
            account,
            savings,
            _dir: dir,
        }
    }

    pub fn conn(&self) -> &Connection {
        self.db.conn()
    }

    pub fn account(&mut self, name: &str, currency: Currency) -> AccountId {
        account::create(
            &mut self.db,
            &NewAccount::new(name, AccountKind::Current, currency),
        )
        .unwrap()
    }

    /// A manual row of `minor` in the account's currency on `date` at
    /// `merchant` (also its display name and description), in `category`
    /// set by the user.
    pub fn row_on(
        &self,
        account: AccountId,
        date: NaiveDate,
        minor: i64,
        merchant: &str,
        category: Option<&str>,
    ) -> TransactionId {
        self.conn()
            .execute(
                "INSERT INTO \"transaction\" (account_id, date, amount_minor, currency, \
                 merchant, merchant_display, description_raw, description_normalized, origin, \
                 category_id, category_source) \
                 VALUES (?1, ?2, ?3, (SELECT currency FROM account WHERE id = ?1), ?4, ?4, ?4, \
                 upper(?4), 'manual', (SELECT id FROM category WHERE key = ?5), \
                 CASE WHEN ?5 IS NULL THEN NULL ELSE 'user' END)",
                params![account, date_key(date), minor, merchant, category],
            )
            .unwrap();
        TransactionId::new(self.conn().last_insert_rowid())
    }

    /// [`Fixture::row_on`] on the current account.
    pub fn row(
        &self,
        date: NaiveDate,
        minor: i64,
        merchant: &str,
        category: Option<&str>,
    ) -> TransactionId {
        self.row_on(self.account, date, minor, merchant, category)
    }

    /// Splits `transaction` into lines of these amounts (GBP), uncategorised.
    pub fn split(&mut self, transaction: TransactionId, amounts: &[i64]) -> Vec<SplitId> {
        let lines: Vec<SplitLine> = amounts
            .iter()
            .map(|minor| SplitLine::new(Money::new(*minor, Currency::GBP), None))
            .collect();
        split::set(&mut self.db, transaction, &lines).unwrap()
    }

    /// The category key `transaction` has, if any.
    pub fn category_of(&self, transaction: TransactionId) -> Option<String> {
        self.conn()
            .query_row(
                "SELECT c.key FROM \"transaction\" t LEFT JOIN category c ON c.id = t.category_id \
                 WHERE t.id = ?1",
                [transaction],
                |row| row.get(0),
            )
            .unwrap()
    }

    /// Files a suggestion from client `client`.
    pub fn file_from(
        &mut self,
        client: &str,
        kind: SuggestionKind,
        payload: Value,
    ) -> Result<Filed, super::Error> {
        file_suggestion(
            &mut self.db,
            &ClientName::new(client).unwrap(),
            &NewSuggestion {
                kind,
                payload,
                rationale: Some("Looks right".to_owned()),
            },
        )
    }

    /// Files a suggestion from `claude-code` and returns its ID.
    pub fn file(&mut self, kind: SuggestionKind, payload: Value) -> SuggestionId {
        self.file_from("claude-code", kind, payload).unwrap().id
    }

    /// How many rows `table` has.
    pub fn count(&self, table: &str) -> i64 {
        self.conn()
            .query_row(&format!("SELECT COUNT(*) FROM \"{table}\""), [], |row| {
                row.get(0)
            })
            .unwrap()
    }
}

pub(crate) fn key(key: &str) -> CategoryKey {
    CategoryKey::new(key).unwrap()
}
