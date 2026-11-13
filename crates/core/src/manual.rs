//! Manual transactions (M1.22): rows the user enters themselves, such as
//! cash spending, or a payment the bank's export doesn't have yet.
//!
//! A manual row has `origin = 'manual'`, no import batch, no source ID and
//! no fingerprint, so it is never deduplicated against an import. It is the
//! user's own data: every field can be edited and the row can be deleted
//! (`docs/ARCHITECTURE.md` → Decisions, "Manual entries and corrections").
//! Imported and mirrored rows are refused here: their source fields stay
//! read-only, and a wrong one is fixed by undoing its batch or excluding it
//! ([`crate::exclusion`]).
//!
//! Decisions:
//!
//! - **Accounts**: any account that holds transactions (current, savings,
//!   credit card, pot, cash). `asset` and `liability` accounts hold
//!   valuations only ([`Error::HoldsValuations`]). The amount is in the
//!   account's currency.
//! - **Locked periods**: the row's account, date, time, amount, merchant and
//!   description are what a bank row's source fields are, so creating,
//!   deleting, or changing any of those on a row dated on or before its
//!   account's `reconciled_through` is refused ([`Error::Locked`]); for an
//!   edit both the old and the new date (on the old and the new account)
//!   count. The category is a user-facing field, editable on a locked row
//!   like on an imported one.
//! - **Closed accounts**: a row can't be dated after its account's
//!   `closed_on` ([`Error::AfterClosing`]); one on or before the closing
//!   date (the last cash spend before closing a wallet) is fine. Deleting is
//!   always allowed outside a locked period.
//! - **Edits** replace every field [`ManualTransaction`] holds. The other
//!   user-facing fields (note, share, reimbursable, exclusion) are left as
//!   they are. A category that doesn't change keeps its source; a new one is
//!   the user's (`category_source = 'user'`). A row in a transfer pair whose
//!   account or amount changes is no longer that pair: the group is
//!   dissolved, like [`crate::import::undo`] does, and [`Updated`] says so.
//! - **Deleting** a row dissolves its transfer group the same way, and
//!   removes its split lines and tag links.
//! - **Split rows** ([`crate::split`]): an edit that changes the amount or
//!   currency of a split row, or gives it a category, is refused
//!   ([`Error::Split`]); the split is removed or replaced first. Moving one
//!   to another account in the same currency, or changing its date or text,
//!   is fine.
//!
//! The display merchant (`merchant_display`) is derived like an imported
//! row's ([`crate::merchant`]): the merchant (or the description) cleaned,
//! then the merchant aliases. It is set on create and on every edit, unless
//! the user named the merchant themselves. Nothing here logs transaction
//! text, and error messages never quote it.

use chrono::{NaiveDate, NaiveTime};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::account::{self, Account, date_key};
use crate::category::CategoryKey;
use crate::change;
use crate::db::Database;
use crate::duplicate;
use crate::id::{AccountId, TransactionId, TransferGroupId};
use crate::merchant;
use crate::money::{Currency, Money};
use crate::split;
use crate::statement::normalize_description;
use crate::tag;

/// A manual transaction to create, or what to store in place of one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManualTransaction {
    pub account: AccountId,
    pub date: NaiveDate,
    /// Whole seconds are stored; anything finer is dropped.
    pub time: Option<NaiveTime>,
    /// Negative for money out. In the account's currency.
    pub amount: Money,
    /// Who was paid or paid in, as the user writes it.
    pub merchant: Option<String>,
    /// Required: some text that isn't only whitespace.
    pub description: String,
    pub category: Option<CategoryKey>,
}

impl ManualTransaction {
    /// A transaction with no time, merchant or category.
    pub fn new(
        account: AccountId,
        date: NaiveDate,
        amount: Money,
        description: impl Into<String>,
    ) -> Self {
        ManualTransaction {
            account,
            date,
            time: None,
            amount,
            merchant: None,
            description: description.into(),
            category: None,
        }
    }
}

/// Where a transaction came from (`transaction.origin`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Origin {
    Imported,
    /// The pot side of a sub-account transfer, created by an import.
    Mirrored,
    Manual,
}

impl Origin {
    pub const ALL: [Origin; 3] = [Origin::Imported, Origin::Mirrored, Origin::Manual];

    pub const fn as_str(self) -> &'static str {
        match self {
            Origin::Imported => "imported",
            Origin::Mirrored => "mirrored",
            Origin::Manual => "manual",
        }
    }

    pub fn from_key(key: &str) -> Option<Origin> {
        Self::ALL.into_iter().find(|origin| origin.as_str() == key)
    }
}

/// What [`update`] changed besides the row itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Updated {
    /// The transfer pair the row was in, dissolved because its account or
    /// amount changed. Its other rows are ordinary transactions again.
    pub dissolved_transfer: Option<TransferGroupId>,
}

/// What [`delete`] removed besides the row itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deleted {
    /// The transfer pair the row was in, dissolved. Its other rows are
    /// ordinary transactions again.
    pub dissolved_transfer: Option<TransferGroupId>,
}

/// Why a manual transaction couldn't be stored, changed or deleted.
/// Messages never contain transaction text.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("there is no account {0}")]
    AccountNotFound(AccountId),

    #[error("{0} accounts hold valuations, not transactions; record a valuation instead")]
    HoldsValuations(&'static str),

    #[error("the amount is in {amount}, but the account is in {account}")]
    CurrencyMismatch { account: Currency, amount: Currency },

    #[error("a transaction needs a description")]
    BlankDescription,

    #[error("there is no category {0}")]
    UnknownCategory(CategoryKey),

    #[error("there is no transaction {0}")]
    TransactionNotFound(TransactionId),

    #[error(
        "transaction {id} is {origin}, not manual: its source fields are read-only; \
         undo its import or exclude it instead",
        origin = .origin.as_str()
    )]
    NotManual { id: TransactionId, origin: Origin },

    #[error("{date} is on or before {through}, which is locked; unlock the account first")]
    Locked { date: NaiveDate, through: NaiveDate },

    #[error("the account was closed on {closed_on}, so it takes no transactions dated after it")]
    AfterClosing { closed_on: NaiveDate },

    #[error(
        "the transaction has a share of its amount set, and the new amount doesn't contain it; \
         change the share first"
    )]
    ShareOutsideAmount,

    /// The row is split: its amount and currency stay as they are and it
    /// has no category of its own until the split is removed.
    #[error(
        "transaction {0} is split: remove or replace the split before changing its amount \
         or giving it a category"
    )]
    Split(TransactionId),

    #[error("merchant names: {0}")]
    Merchant(#[from] merchant::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// Stores a manual transaction and returns its ID.
pub fn create(db: &mut Database, entry: &ManualTransaction) -> Result<TransactionId, Error> {
    let tx = db.conn_mut().transaction()?;
    let account = check_entry(&tx, entry)?;
    check_open(&account, entry.date)?;
    check_lock(&account, entry.date)?;
    let category = category_id(&tx, entry.category.as_ref())?;
    tx.execute(
        "INSERT INTO \"transaction\" (account_id, date, time, amount_minor, currency, merchant, \
         description_raw, description_normalized, origin, category_id, category_source) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'manual', ?9, ?10)",
        params![
            entry.account,
            date_key(entry.date),
            entry.time.map(time_key),
            entry.amount.minor(),
            entry.amount.currency(),
            entry.merchant,
            entry.description,
            normalize_description(&entry.description),
            category,
            category.map(|_| "user"),
        ],
    )?;
    let id = TransactionId::new(tx.last_insert_rowid());
    merchant::apply_to_transaction(&tx, id)?;
    tx.commit()?;
    tracing::info!(transaction = %id, account = %entry.account, "added a manual transaction");
    Ok(id)
}

/// The manual transaction with this ID, as [`update`] takes it.
pub fn get(db: &Database, id: TransactionId) -> Result<ManualTransaction, Error> {
    let stored = load(db.conn(), id)?;
    stored.manual(id)?;
    let category = stored
        .category_id
        .map(|category| {
            db.conn().query_row(
                "SELECT key FROM category WHERE id = ?1",
                [category],
                |row| {
                    let key: String = row.get("key")?;
                    CategoryKey::new(&key).map_err(|_| account::invalid(row, "key"))
                },
            )
        })
        .transpose()?;
    Ok(ManualTransaction {
        account: stored.account,
        date: stored.date,
        time: stored.time,
        amount: Money::new(stored.amount_minor, stored.currency),
        merchant: stored.merchant,
        description: stored.description,
        category,
    })
}

/// Replaces manual transaction `id`'s fields with `entry`'s. See the module
/// docs for what else changes.
pub fn update(
    db: &mut Database,
    id: TransactionId,
    entry: &ManualTransaction,
) -> Result<Updated, Error> {
    let tx = db.conn_mut().transaction()?;
    let stored = load(&tx, id)?;
    stored.manual(id)?;
    let account = check_entry(&tx, entry)?;

    let moves_money = entry.account != stored.account
        || entry.amount.minor() != stored.amount_minor
        || entry.amount.currency() != stored.currency;
    let changes_source = moves_money
        || entry.date != stored.date
        || entry.time.map(time_key) != stored.time.map(time_key)
        || entry.merchant != stored.merchant
        || entry.description != stored.description;
    if changes_source {
        let old_account = if stored.account == account.id {
            account.clone()
        } else {
            account::find(&tx, stored.account)?.ok_or(Error::AccountNotFound(stored.account))?
        };
        check_lock(&old_account, stored.date)?;
        check_lock(&account, entry.date)?;
    }
    if entry.date != stored.date || entry.account != stored.account {
        check_open(&account, entry.date)?;
    }

    if let Some(share) = stored.my_share_minor {
        let amount = entry.amount.minor();
        let inside = if amount >= 0 {
            (0..=amount).contains(&share)
        } else {
            (amount..=0).contains(&share)
        };
        if !inside {
            return Err(Error::ShareOutsideAmount);
        }
    }

    let new_category = category_id(&tx, entry.category.as_ref())?;
    let category_changed = new_category != stored.category_id;
    // The schema refuses these too (`transaction_split_parent`); this says
    // why in words.
    let changes_amount =
        entry.amount.minor() != stored.amount_minor || entry.amount.currency() != stored.currency;
    if (changes_amount || new_category.is_some()) && split::is_split(&tx, id)? {
        return Err(Error::Split(id));
    }
    let dissolved_transfer = match stored.transfer_group {
        Some(group) if moves_money => {
            dissolve_transfer_group(&tx, group)?;
            Some(group)
        }
        _ => None,
    };

    tx.execute(
        "UPDATE \"transaction\" SET account_id = ?1, date = ?2, time = ?3, amount_minor = ?4, \
         currency = ?5, merchant = ?6, description_raw = ?7, description_normalized = ?8 \
         WHERE id = ?9",
        params![
            entry.account,
            date_key(entry.date),
            entry.time.map(time_key),
            entry.amount.minor(),
            entry.amount.currency(),
            entry.merchant,
            entry.description,
            normalize_description(&entry.description),
            id,
        ],
    )?;
    if category_changed {
        tx.execute(
            "UPDATE \"transaction\" SET category_id = ?1, category_source = ?2, rule_id = NULL \
             WHERE id = ?3",
            params![new_category, new_category.map(|_| "user"), id],
        )?;
    }
    merchant::apply_to_transaction(&tx, id)?;
    tx.commit()?;
    tracing::info!(transaction = %id, account = %entry.account, "edited a manual transaction");
    Ok(Updated { dissolved_transfer })
}

/// Deletes manual transaction `id`, with what refers to it: its transfer
/// pair is dissolved.
pub fn delete(db: &mut Database, id: TransactionId) -> Result<Deleted, Error> {
    let tx = db.conn_mut().transaction()?;
    let stored = load(&tx, id)?;
    stored.manual(id)?;
    let account =
        account::find(&tx, stored.account)?.ok_or(Error::AccountNotFound(stored.account))?;
    check_lock(&account, stored.date)?;

    let dissolved_transfer = release_references(&tx, id)?;
    tx.execute("DELETE FROM \"transaction\" WHERE id = ?1", [id])?;
    tx.commit()?;
    tracing::info!(transaction = %id, account = %stored.account, "deleted a manual transaction");
    Ok(Deleted { dissolved_transfer })
}

/// Clears everything that refers to transaction `id` before it is deleted,
/// and returns the transfer group it dissolved.
///
/// The one place a manual row's references go: split lines, tag links,
/// change items and duplicate candidates today; tables that point at a
/// transaction later (refund links, schedule matches) are cleared here as
/// they arrive. Until one is, a row it references can't be deleted: the
/// foreign key fails the delete, and nothing changes.
fn release_references(
    tx: &Transaction<'_>,
    id: TransactionId,
) -> Result<Option<TransferGroupId>, Error> {
    let group: Option<TransferGroupId> = tx.query_row(
        "SELECT transfer_group_id FROM \"transaction\" WHERE id = ?1",
        [id],
        |row| row.get(0),
    )?;
    if let Some(group) = group {
        dissolve_transfer_group(tx, group)?;
    }
    split::clear(tx, id)?;
    tag::clear(tx, id)?;
    change::clear(tx, id)?;
    duplicate::clear(tx, id)?;
    Ok(group)
}

/// Deletes a transfer group, leaving its rows as ordinary transactions.
fn dissolve_transfer_group(conn: &Connection, group: TransferGroupId) -> Result<(), Error> {
    conn.execute(
        "UPDATE \"transaction\" SET transfer_group_id = NULL WHERE transfer_group_id = ?1",
        [group],
    )?;
    conn.execute("DELETE FROM transfer_group WHERE id = ?1", [group])?;
    Ok(())
}

/// Checks `entry` against its account and returns the account.
fn check_entry(conn: &Connection, entry: &ManualTransaction) -> Result<Account, Error> {
    let account =
        account::find(conn, entry.account)?.ok_or(Error::AccountNotFound(entry.account))?;
    if !account.kind.holds_transactions() {
        return Err(Error::HoldsValuations(account.kind.as_str()));
    }
    if entry.amount.currency() != account.currency {
        return Err(Error::CurrencyMismatch {
            account: account.currency,
            amount: entry.amount.currency(),
        });
    }
    if entry.description.trim().is_empty() {
        return Err(Error::BlankDescription);
    }
    Ok(account)
}

/// Refuses `date` when it's after `account` was closed.
fn check_open(account: &Account, date: NaiveDate) -> Result<(), Error> {
    match account.closed_on {
        Some(closed_on) if date > closed_on => Err(Error::AfterClosing { closed_on }),
        _ => Ok(()),
    }
}

/// Refuses `date` when it's in `account`'s locked period.
fn check_lock(account: &Account, date: NaiveDate) -> Result<(), Error> {
    match account.reconciled_through {
        Some(through) if date <= through => Err(Error::Locked { date, through }),
        _ => Ok(()),
    }
}

fn category_id(conn: &Connection, key: Option<&CategoryKey>) -> Result<Option<i64>, Error> {
    let Some(key) = key else {
        return Ok(None);
    };
    conn.query_row(
        "SELECT id FROM category WHERE key = ?1",
        [key.as_str()],
        |row| row.get(0),
    )
    .optional()?
    .map(Some)
    .ok_or_else(|| Error::UnknownCategory(key.clone()))
}

/// The `HH:MM:SS` form `transaction.time` holds.
fn time_key(time: NaiveTime) -> String {
    time.format("%H:%M:%S").to_string()
}

/// What this module reads of a stored transaction.
struct Stored {
    origin: Origin,
    account: AccountId,
    date: NaiveDate,
    time: Option<NaiveTime>,
    amount_minor: i64,
    currency: Currency,
    merchant: Option<String>,
    description: String,
    category_id: Option<i64>,
    transfer_group: Option<TransferGroupId>,
    my_share_minor: Option<i64>,
}

impl Stored {
    /// Refuses a row that isn't manual.
    fn manual(&self, id: TransactionId) -> Result<(), Error> {
        match self.origin {
            Origin::Manual => Ok(()),
            origin => Err(Error::NotManual { id, origin }),
        }
    }
}

fn load(conn: &Connection, id: TransactionId) -> Result<Stored, Error> {
    conn.query_row(
        "SELECT origin, account_id, date, time, amount_minor, currency, merchant, \
         description_raw, category_id, transfer_group_id, my_share_minor \
         FROM \"transaction\" WHERE id = ?1",
        [id],
        |row| {
            let origin: String = row.get("origin")?;
            let time: Option<String> = row.get("time")?;
            Ok(Stored {
                origin: Origin::from_key(&origin).ok_or_else(|| account::invalid(row, "origin"))?,
                account: row.get("account_id")?,
                date: account::date(row, "date")?,
                time: time
                    .map(|time| {
                        NaiveTime::parse_from_str(&time, "%H:%M:%S")
                            .map_err(|_| account::invalid(row, "time"))
                    })
                    .transpose()?,
                amount_minor: row.get("amount_minor")?,
                currency: row.get("currency")?,
                merchant: row.get("merchant")?,
                description: row.get("description_raw")?,
                category_id: row.get("category_id")?,
                transfer_group: row.get("transfer_group_id")?,
                my_share_minor: row.get("my_share_minor")?,
            })
        },
    )
    .optional()?
    .ok_or(Error::TransactionNotFound(id))
}

#[cfg(test)]
mod tests;
