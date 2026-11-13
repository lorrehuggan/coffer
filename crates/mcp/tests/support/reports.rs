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

/// A pot under `parent`, in its currency.
pub fn add_pot(fixture: &Fixture, parent: AccountId, name: &str) -> AccountId {
    let mut db = writable(fixture);
    let currency = account::get(&db, parent).unwrap().currency;
    let mut pot = NewAccount::new(name, AccountKind::Pot, currency);
    pot.parent = Some(parent);
    account::create(&mut db, &pot).unwrap()
}

/// An entered (manual) balance for `account` at the end of `date`.
pub fn add_balance(fixture: &Fixture, account: AccountId, date: &str, minor: i64) {
    let mut db = writable(fixture);
    let currency = account::get(&db, account).unwrap().currency;
    account::add_balance(
        &mut db,
        account,
        &account::EnteredBalance {
            date: day(date),
            balance: Money::new(minor, currency),
            source: account::EnteredSource::Manual,
            note: None,
        },
    )
    .unwrap();
}

/// Stores `rule` first in the rule list.
pub fn add_rule(
    fixture: &Fixture,
    rule: &coffer_core::rule::RuleDefinition,
) -> coffer_core::id::RuleId {
    coffer_core::rule::create(&mut writable(fixture), rule).unwrap()
}

/// A report saved by `client`; `created_at` (`YYYY-MM-DDTHH:MM:SS.sssZ`)
/// replaces the time it was saved, when given.
pub fn add_report(
    fixture: &Fixture,
    client: &str,
    title: &str,
    body: &str,
    created_at: Option<&str>,
) -> i64 {
    let mut db = writable(fixture);
    let id = coffer_core::inbox::insert_report(
        &mut db,
        &coffer_core::mcp_client::ClientName::new(client).unwrap(),
        &coffer_core::inbox::NewReport {
            period: "2026-11".to_owned(),
            title: title.to_owned(),
            body_markdown: body.to_owned(),
        },
    )
    .unwrap();
    drop(db);
    if let Some(at) = created_at {
        let conn = rusqlite::Connection::open(fixture.path()).unwrap();
        conn.execute(
            "UPDATE report SET created_at = ?1 WHERE id = ?2",
            rusqlite::params![at, id],
        )
        .unwrap();
    }
    id
}
