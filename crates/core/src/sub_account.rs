//! Sub-accounts (M1.17): Monzo Pots first, later Starling Spaces and
//! Revolut Vaults. A pot is an `account` of kind `pot` under the current or
//! savings account it belongs to, and counts toward net worth.
//!
//! A bank's export lists money moving into and out of a pot as rows of the
//! parent account. A preset's `[sub_account_transfer]` recognises those rows
//! and puts the pot's ID on the line ([`StatementLine::sub_account`]). When
//! the import commits such a row, [`mirror`] records the pot's side of it:
//!
//! - **The pot is found by its key**, the bank's stable ID for it
//!   (`account.sub_account_key`, unique under its parent), never by name,
//!   since the user renames pots. A key the parent has no pot for creates
//!   one: kind `pot`, the parent's currency and institution, named after
//!   the pot's name on the latest-dated row with that key (a later row in
//!   the input on a tie), or after the key when no row names it. An
//!   existing pot keeps its name, whatever the rows call it.
//! - **The mirrored row** is the parent row seen from the pot: on the pot,
//!   in the same import batch, with `origin = 'mirrored'`, the same date and
//!   time, the opposite amount (and original amount), and the parent row's
//!   other source fields, keys, note and category as stored. It keeps the
//!   parent row's `external_id` and `fingerprint`: the pot's unique
//!   indexes then hold at most one mirror of each parent row, so mirroring
//!   the same row twice fails rather than doubling the transfer.
//! - **The pair is a confirmed transfer** from the start
//!   ([`transfer::record_confirmed_pair`]), so transfer detection leaves both
//!   rows alone. The bank says it's a transfer; nothing is left to confirm.
//! - **All or nothing.** A parent that can't have pots, or a pot locked on
//!   or before a row's date, fails the whole import: half a transfer is
//!   never imported. A closed pot still takes rows; [`MirrorPlan`] lists
//!   it, so the preview can say so.
//!
//! Undoing the import deletes the mirrored rows with the batch's other rows
//! and dissolves their transfer groups. Pots it created stay (the user may
//! have entered an opening balance or renamed them), and importing the file
//! again finds them by key. Re-importing a file finds its parent rows
//! already imported, so nothing new is mirrored.
//!
//! Nothing here logs transaction text; logs carry counts and IDs only.

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, params};

use crate::account::{self, Account, AccountKind};
use crate::db::Database;
use crate::id::{AccountId, BatchId, TransactionId, TransferGroupId};
use crate::statement::{StatementLine, SubAccountKind, SubAccountRef};
use crate::transfer;

/// Why planning or mirroring sub-account transfers failed. Messages never
/// contain transaction text.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "the file has transfers to and from pots, but {kind} accounts can't have pots; \
         import it into a current or savings account"
    )]
    CannotHavePots { kind: &'static str },

    #[error(
        "the file has a transfer to or from pot {pot} on or before {through}, which is \
         locked; unlock the pot first"
    )]
    PotLocked { pot: AccountId, through: NaiveDate },

    #[error("transaction {transaction} isn't one of import batch {batch}'s rows on the account")]
    NotInBatch {
        transaction: TransactionId,
        batch: BatchId,
    },

    #[error("an amount is too large to mirror")]
    Overflow,

    #[error("transfer: {0}")]
    Transfer(#[from] transfer::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// A pot an import would create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewPot {
    pub kind: SubAccountKind,
    /// The bank's ID for it (`account.sub_account_key`).
    pub key: String,
    /// What it will be called.
    pub name: String,
}

/// What [`mirror`] would do for an import: for the preview ("creates pot
/// Rainy Day"). Made by [`plan`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MirrorPlan {
    /// Pots to create, in the order their keys first appear.
    pub new_pots: Vec<NewPot>,
    /// Pots the parent already has that get rows, in the order their keys
    /// first appear.
    pub existing_pots: Vec<AccountId>,
    /// Those of `existing_pots` that are closed. They still get their rows:
    /// the bank says the money moved.
    pub closed_pots: Vec<AccountId>,
    /// Rows to mirror, one per sub-account transfer row.
    pub mirrored_rows: usize,
}

impl MirrorPlan {
    /// Whether there is nothing to mirror.
    pub fn is_empty(&self) -> bool {
        self.mirrored_rows == 0
    }
}

/// One parent row and its mirror.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MirroredRow {
    /// The imported row on the parent account.
    pub parent_row: TransactionId,
    /// Its pot-side row (`origin = 'mirrored'`).
    pub mirrored_row: TransactionId,
    pub pot: AccountId,
    /// The `confirmed` transfer group holding the two.
    pub group: TransferGroupId,
}

/// What [`mirror`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MirrorSummary {
    /// Pots created, in the order their keys first appear.
    pub created_pots: Vec<AccountId>,
    /// Every mirrored row, in the order given; each is one confirmed pair.
    pub rows: Vec<MirroredRow>,
}

impl MirrorSummary {
    /// Mirrored rows, which is also the number of confirmed pairs.
    pub fn mirrored_rows(&self) -> usize {
        self.rows.len()
    }
}

/// A pot and the bank's ID for it, if it has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pot {
    pub account: Account,
    /// `None` for a pot the user created by hand.
    pub key: Option<String>,
}

/// Plans [`mirror`] for the lines an import would commit as new rows into
/// `parent`; lines without [`StatementLine::sub_account`] are skipped.
/// Reads only.
///
/// Fails like [`mirror`] would: with [`Error::CannotHavePots`] when there
/// is a sub-account transfer and `parent` isn't a current or savings
/// account, and with [`Error::PotLocked`] when an existing pot is locked on
/// or before one of its rows.
pub fn plan<'a>(
    conn: &Connection,
    parent: &Account,
    lines: impl IntoIterator<Item = &'a StatementLine>,
) -> Result<MirrorPlan, Error> {
    let keys = by_key(
        lines
            .into_iter()
            .filter_map(|line| Some((line.date, line.sub_account.as_ref()?))),
    );
    let mut plan = MirrorPlan::default();
    if keys.is_empty() {
        return Ok(plan);
    }
    check_parent(parent)?;
    for key in keys {
        plan.mirrored_rows += key.rows;
        match existing_pot(conn, parent.id, &key)? {
            Some(pot) => {
                plan.existing_pots.push(pot.id);
                if pot.closed_on.is_some() {
                    plan.closed_pots.push(pot.id);
                }
            }
            None => plan.new_pots.push(NewPot {
                kind: key.kind,
                key: key.key.to_owned(),
                name: key.name.to_owned(),
            }),
        }
    }
    Ok(plan)
}

/// Mirrors sub-account transfer rows of import `batch` onto their pots, and
/// pairs each with its mirror as a `confirmed` transfer group (module docs).
///
/// `rows` are the batch's new rows on `parent` that the preset recognised
/// as sub-account transfers: each transaction ID with its line's
/// [`StatementLine::sub_account`]. Each must be an imported row of `batch`
/// on `parent` ([`Error::NotInBatch`]). Pots missing for a key are created.
/// An empty `rows` does nothing, whatever the parent.
///
/// Run it inside the import's DB transaction, after the batch's rows are
/// stored and before rules and transfer detection, so they see the
/// mirrored rows. It writes through `conn` without a transaction of its
/// own; on an error the caller rolls back, since some rows may be written.
pub fn mirror(
    conn: &Connection,
    parent: &Account,
    batch: BatchId,
    rows: &[(TransactionId, SubAccountRef)],
) -> Result<MirrorSummary, Error> {
    let mut summary = MirrorSummary::default();
    if rows.is_empty() {
        return Ok(summary);
    }
    check_parent(parent)?;

    let mut load = conn.prepare_cached(
        "SELECT date, amount_minor, original_amount_minor FROM \"transaction\" \
         WHERE id = ?1 AND batch_id = ?2 AND account_id = ?3 AND origin = 'imported'",
    )?;
    let mut parents: Vec<ParentRow> = Vec::with_capacity(rows.len());
    for (id, _) in rows {
        let row = load
            .query_row(params![id, batch, parent.id], |row| {
                Ok(ParentRow {
                    date: account::date(row, "date")?,
                    amount: row.get("amount_minor")?,
                    original_amount: row.get("original_amount_minor")?,
                })
            })
            .optional()?
            .ok_or(Error::NotInBatch {
                transaction: *id,
                batch,
            })?;
        parents.push(row);
    }

    let keys = by_key(
        parents
            .iter()
            .zip(rows)
            .map(|(row, (_, sub_account))| (row.date, sub_account)),
    );
    let mut pots: HashMap<&str, AccountId> = HashMap::with_capacity(keys.len());
    for key in &keys {
        let pot = match existing_pot(conn, parent.id, key)? {
            Some(pot) => pot.id,
            None => {
                let pot = create_pot(conn, parent, key)?;
                summary.created_pots.push(pot);
                pot
            }
        };
        pots.insert(key.key, pot);
    }

    let mut insert = conn.prepare_cached(
        "INSERT INTO \"transaction\" (account_id, batch_id, date, time, amount_minor, \
         currency, merchant, description_raw, description_normalized, external_id, \
         fingerprint, source_category, source_type, original_amount_minor, \
         original_currency, note, origin, created_at, merchant_display, category_id, \
         category_source, rule_id) \
         SELECT ?1, batch_id, date, time, ?2, currency, merchant, description_raw, \
         description_normalized, external_id, fingerprint, source_category, source_type, \
         ?3, original_currency, note, 'mirrored', created_at, merchant_display, category_id, \
         category_source, rule_id \
         FROM \"transaction\" WHERE id = ?4",
    )?;
    for ((id, sub_account), row) in rows.iter().zip(&parents) {
        let pot = pots[sub_account.key.as_str()];
        let amount = row.amount.checked_neg().ok_or(Error::Overflow)?;
        let original_amount = row
            .original_amount
            .map(|minor| minor.checked_neg().ok_or(Error::Overflow))
            .transpose()?;
        insert.execute(params![pot, amount, original_amount, id])?;
        let mirrored_row = TransactionId::new(conn.last_insert_rowid());
        let group = transfer::record_confirmed_pair(conn, *id, mirrored_row)?;
        summary.rows.push(MirroredRow {
            parent_row: *id,
            mirrored_row,
            pot,
            group,
        });
    }

    tracing::debug!(
        batch = %batch,
        parent = %parent.id,
        created_pots = summary.created_pots.len(),
        mirrored = summary.rows.len(),
        "mirrored sub-account transfers"
    );
    Ok(summary)
}

/// `parent`'s pots, in the order they were created, with their keys.
pub fn pots(db: &Database, parent: AccountId) -> Result<Vec<Pot>, Error> {
    let conn = db.conn();
    let mut statement =
        conn.prepare("SELECT id, sub_account_key FROM account WHERE parent_id = ?1 ORDER BY id")?;
    let found: Vec<(AccountId, Option<String>)> = statement
        .query_map([parent], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut pots = Vec::with_capacity(found.len());
    for (id, key) in found {
        if let Some(account) = account::find(conn, id)? {
            pots.push(Pot { account, key });
        }
    }
    Ok(pots)
}

/// The account kind a sub-account of `kind` is stored as.
pub const fn account_kind(kind: SubAccountKind) -> AccountKind {
    match kind {
        SubAccountKind::Pot => AccountKind::Pot,
    }
}

/// Only current and savings accounts have pots (the schema says so too).
fn check_parent(parent: &Account) -> Result<(), Error> {
    match parent.kind {
        AccountKind::Current | AccountKind::Savings => Ok(()),
        kind => Err(Error::CannotHavePots {
            kind: kind.as_str(),
        }),
    }
}

/// What mirroring reads of a parent row.
struct ParentRow {
    date: NaiveDate,
    amount: i64,
    original_amount: Option<i64>,
}

/// The rows for one pot key.
struct KeyRows<'a> {
    kind: SubAccountKind,
    key: &'a str,
    /// The name a new pot gets.
    name: &'a str,
    /// The earliest row's date, for the lock check.
    earliest: NaiveDate,
    rows: usize,
}

/// Groups `(date, ref)` items by key, in the order keys first appear. A
/// key's name is the one on its latest-dated item that has one (the later
/// item on a tie), else the key itself.
fn by_key<'a>(items: impl Iterator<Item = (NaiveDate, &'a SubAccountRef)>) -> Vec<KeyRows<'a>> {
    let mut keys: Vec<KeyRows<'a>> = Vec::new();
    let mut named: Vec<Option<NaiveDate>> = Vec::new();
    for (date, sub_account) in items {
        let index = match keys.iter().position(|key| key.key == sub_account.key) {
            Some(index) => index,
            None => {
                keys.push(KeyRows {
                    kind: sub_account.kind,
                    key: &sub_account.key,
                    name: &sub_account.key,
                    earliest: date,
                    rows: 0,
                });
                named.push(None);
                keys.len() - 1
            }
        };
        let key = &mut keys[index];
        key.rows += 1;
        key.earliest = key.earliest.min(date);
        let name = sub_account
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty());
        if let Some(name) = name
            && named[index].is_none_or(|latest| date >= latest)
        {
            key.name = name;
            named[index] = Some(date);
        }
    }
    keys
}

/// The parent's pot with `key`'s key. Fails when it is locked on or
/// before the key's earliest row.
fn existing_pot(
    conn: &Connection,
    parent: AccountId,
    key: &KeyRows<'_>,
) -> Result<Option<Account>, Error> {
    let id: Option<AccountId> = conn
        .prepare_cached("SELECT id FROM account WHERE parent_id = ?1 AND sub_account_key = ?2")?
        .query_row(params![parent, key.key], |row| row.get(0))
        .optional()?;
    let Some(pot) = id.map(|id| account::find(conn, id)).transpose()?.flatten() else {
        return Ok(None);
    };
    if let Some(through) = pot.reconciled_through
        && key.earliest <= through
    {
        return Err(Error::PotLocked {
            pot: pot.id,
            through,
        });
    }
    Ok(Some(pot))
}

fn create_pot(conn: &Connection, parent: &Account, key: &KeyRows<'_>) -> Result<AccountId, Error> {
    conn.execute(
        "INSERT INTO account (parent_id, name, institution, kind, currency, sub_account_key) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            parent.id,
            key.name,
            parent.institution,
            account_kind(key.kind).as_str(),
            parent.currency,
            key.key,
        ],
    )?;
    Ok(AccountId::new(conn.last_insert_rowid()))
}
