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
