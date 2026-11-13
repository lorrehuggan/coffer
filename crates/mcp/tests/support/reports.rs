//! Seeding helpers for Block C's tool tests (the read tools): rows and
//! values the fixture database doesn't have, written through core's API
//! where one exists, and through a plain connection where core has none
//! yet. Every helper takes the [`Fixture`](super::Fixture) and writes to its
//! database before a server connects.
//!
//! Add helpers at the end of this file.

use chrono::NaiveDate;
use coffer_core::account::{self, AccountKind, NewAccount};
use coffer_core::category::CategoryKey;
use coffer_core::db::Database;
use coffer_core::id::{AccountId, TransactionId};
use coffer_core::manual::{self, ManualTransaction};
use coffer_core::money::{Currency, Money};

use super::Fixture;

/// A `YYYY-MM-DD` date.
pub fn day(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}

/// Opens the fixture's database read-write, for seeding.
pub fn writable(fixture: &Fixture) -> Database {
    Database::open(fixture.path()).unwrap()
}

/// A current account in `currency`.
pub fn add_account(fixture: &Fixture, name: &str, currency: Currency) -> AccountId {
    account::create(
        &mut writable(fixture),
        &NewAccount::new(name, AccountKind::Current, currency),
    )
    .unwrap()
}

/// A manual row on `account` (in its currency), optionally categorised.
pub fn add_row(
    fixture: &Fixture,
    account: AccountId,
    date: &str,
    minor: i64,
    category: Option<&str>,
    description: &str,
) -> TransactionId {
    let mut db = writable(fixture);
    let currency = account::get(&db, account).unwrap().currency;
    let mut entry =
        ManualTransaction::new(account, day(date), Money::new(minor, currency), description);
    entry.category = category.map(|key| CategoryKey::new(key).unwrap());
    manual::create(&mut db, &entry).unwrap()
}

/// Sets a budget through core: `month` is `None` for a standing one.
pub fn add_budget(
    fixture: &Fixture,
    category: &str,
    month: Option<&str>,
    amount: Money,
    group: coffer_core::budget::BudgetGroup,
) {
    coffer_core::budget::set(
        &mut writable(fixture),
        &coffer_core::budget::NewBudget {
            category: CategoryKey::new(category).unwrap(),
            month: month.map(|month| month.parse().unwrap()),
            amount,
            group,
        },
    )
    .unwrap();
}

/// An `asset` or `liability` account in `currency`.
pub fn add_valuation_account(
    fixture: &Fixture,
    name: &str,
    kind: AccountKind,
    currency: Currency,
) -> AccountId {
    account::create(
        &mut writable(fixture),
        &NewAccount::new(name, kind, currency),
    )
    .unwrap()
}

/// A valuation of an `asset` or `liability` account at the end of `date`.
/// Core has no API for valuations yet, so it is written directly.
pub fn add_valuation(fixture: &Fixture, account: AccountId, date: &str, minor: i64) {
    rusqlite::Connection::open(fixture.path())
        .unwrap()
        .execute(
            "INSERT INTO balance_snapshot (account_id, date, balance_minor, source) \
             VALUES (?1, ?2, ?3, 'valuation')",
            rusqlite::params![account.get(), date, minor],
        )
        .unwrap();
}

/// `[(currency, minor)]` of a tool's list of amounts. Every amount's
/// `home` is null until home-currency conversion exists.
pub fn amounts(value: &serde_json::Value) -> Vec<(String, i64)> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("not a list of amounts: {value}"))
        .iter()
        .map(|amount| {
            assert!(amount["home"].is_null(), "{amount}");
            (
                amount["currency"].as_str().unwrap().to_owned(),
                amount["minor"].as_i64().unwrap(),
            )
        })
        .collect()
}

/// One amount in GBP, as [`amounts`] reads it.
pub fn gbp(minor: i64) -> Vec<(String, i64)> {
    vec![("GBP".to_owned(), minor)]
}

/// An access-log row's `args_redacted_json` has exactly `keys` (sorted)
/// and none of `free_text`.
pub fn assert_redacted(json: &str, keys: &[&str], free_text: &[&str]) {
    let value: serde_json::Value = serde_json::from_str(json).unwrap();
    let found: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(found, keys, "{json}");
    for text in free_text {
        assert!(!json.contains(text), "{text:?} in {json}");
    }
}
