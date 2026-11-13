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

// Row tools (`search_transactions`, `uncategorized_transactions`,
// `merchant_history`).

/// A manual row with a merchant (its display name is derived from it).
pub fn add_merchant_row(
    fixture: &Fixture,
    account: AccountId,
    date: &str,
    minor: i64,
    category: Option<&str>,
    merchant: &str,
) -> TransactionId {
    let mut db = writable(fixture);
    let currency = account::get(&db, account).unwrap().currency;
    let mut entry = ManualTransaction::new(
        account,
        day(date),
        Money::new(minor, currency),
        format!("CARD PAYMENT {}", merchant.to_uppercase()),
    );
    entry.merchant = Some(merchant.to_owned());
    entry.category = category.map(|key| CategoryKey::new(key).unwrap());
    manual::create(&mut db, &entry).unwrap()
}

/// Splits `transaction` into lines of `(minor, category)`, in its currency.
pub fn split_row(fixture: &Fixture, transaction: TransactionId, lines: &[(i64, Option<&str>)]) {
    let currency = transaction_currency(fixture, transaction);
    let mut db = writable(fixture);
    let lines: Vec<coffer_core::split::SplitLine> = lines
        .iter()
        .map(|&(minor, category)| {
            coffer_core::split::SplitLine::new(
                Money::new(minor, currency),
                category.map(|key| CategoryKey::new(key).unwrap()),
            )
        })
        .collect();
    coffer_core::split::set(&mut db, transaction, &lines).unwrap();
}

/// A confirmed refund of `minor` from `inflow` to `purchase`.
pub fn refund_row(fixture: &Fixture, inflow: TransactionId, purchase: TransactionId, minor: i64) {
    use coffer_core::link::{self, LinkKind, LinkTarget, NewLink};
    let currency = transaction_currency(fixture, inflow);
    let mut db = writable(fixture);
    link::create(
        &mut db,
        &NewLink {
            kind: LinkKind::Refund,
            inflow,
            target: LinkTarget::Purchase(purchase),
            amount: Money::new(minor, currency),
        },
    )
    .unwrap();
}

/// Tags `transaction` with `name` (created on first use).
pub fn tag_row(fixture: &Fixture, transaction: TransactionId, name: &str, private: bool) {
    use coffer_core::tag::{self, TagName};
    let mut db = writable(fixture);
    let name = TagName::new(name).unwrap();
    let id = match tag::find(&db, &name).unwrap() {
        Some(tag) => tag.id,
        None => tag::create(&mut db, &name, private).unwrap(),
    };
    tag::add_to_transaction(&mut db, transaction, id).unwrap();
}

fn transaction_currency(fixture: &Fixture, transaction: TransactionId) -> Currency {
    rusqlite::Connection::open(fixture.path())
        .unwrap()
        .query_row(
            "SELECT currency FROM \"transaction\" WHERE id = ?1",
            [transaction],
            |row| row.get(0),
        )
        .unwrap()
}

/// The rows of a row tool's result that belong to `transaction`.
pub fn rows_of(result: &serde_json::Value, transaction: TransactionId) -> Vec<serde_json::Value> {
    result["rows"]
        .as_array()
        .expect("a row tool returns `rows`")
        .iter()
        .filter(|row| row["transaction_id"] == transaction.get())
        .cloned()
        .collect()
}

/// What replaces a description holding account details.
pub const ACCOUNT_DETAILS_WITHHELD: &str = "[withheld: account details]";

/// Checks the fixture's manual row (bank text with a name, sort code,
/// account number and IBAN, shared) in a row tool's result at `standard`
/// with descriptions: there, with its whole amount and the user's share,
/// no merchant, and its description withheld.
pub fn assert_manual_row_guarded(result: &serde_json::Value, fixture: &Fixture) {
    let rows = rows_of(result, fixture.seeded.manual);
    assert_eq!(rows.len(), 1, "{result:#}");
    let row = &rows[0];
    assert_eq!(row["date"], "2026-11-20");
    assert_eq!(row["amount"]["minor"], -4500);
    assert_eq!(row["my_amount"]["minor"], -2250);
    assert_eq!(row["merchant"], serde_json::Value::Null);
    assert_eq!(row["description_untrusted"], ACCOUNT_DETAILS_WITHHELD);
}
