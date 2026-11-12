//! Change batches: previewed, undoable changes to user-facing fields on
//! existing transactions (ROADMAP M1.24, M1.26; `docs/ARCHITECTURE.md` →
//! Decisions, "Bulk changes").
//!
//! A rule applied to history, a duplicate merge or a bulk edit changes
//! fields on many stored transactions at once. Each such change is one
//! `change_batch` (`migrations/V8__change_batch.sql`) with a `change_item`
//! per transaction and [`Field`], holding the [`Value`] before and after.
//!
//! - **Recording.** A [`Recorder`] makes the changes and remembers them:
//!   [`Recorder::set`] reads the field's current value, writes the new one
//!   and keeps both, and [`Recorder::finish`] writes the batch and its items.
//!   It runs inside the caller's DB transaction, so a preview is the same
//!   code run in a transaction that is rolled back ([`dry_run`]), and what
//!   the user saw is what a commit does.
//! - **Undo restores only what still holds.** [`undo`] restores a field only
//!   where it still holds the batch's new value (decision record "rules",
//!   point 6); a field changed since keeps the later value and is counted in
//!   [`UndoSummary::changed_since`]. Items are restored newest first, so a
//!   split made after clearing a category is removed before the category
//!   comes back.
//! - **Locks.** `reconciled_through` guards balances only (rules decision
//!   point 8), so of the fields here only the exclusion meets it. Any change
//!   to an exclusion in a batch, on a row dated on or before its account's
//!   `reconciled_through`, is refused ([`Error::Locked`]), whichever reason
//!   and whichever way: a rule's exclusion (decided by the user,
//!   2026-11-12), a duplicate merge, and the undo of either. The user
//!   unlocks first.
//! - **Deleting a transaction deletes its items.** Undoing an import or
//!   deleting a manual row removes the row's items first ([`clear`],
//!   [`clear_batch`]): the batch stays in the history with what is left.
//!
//! Values can hold user text (a note, a merchant name): nothing here logs
//! them, and errors never quote them.

#[cfg(test)]
mod tests;

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::str::FromStr;

use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use crate::account;
use crate::db::Database;
use crate::exclusion::ExclusionReason;
use crate::id::{BatchId, ChangeBatchId, RuleId, TagId, TransactionId};
use crate::merchant;
use crate::money::{Currency, Money};
use crate::split;

/// What made a change batch (`change_batch.kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    /// A rule applied to past transactions (M1.24).
    RuleApply,
    /// A near-duplicate merged into the row it duplicates (M1.26).
    DuplicateMerge,
    /// The user's edit to several rows at once.
    BulkEdit,
}

impl ChangeKind {
    pub const ALL: [ChangeKind; 3] = [
        ChangeKind::RuleApply,
        ChangeKind::DuplicateMerge,
        ChangeKind::BulkEdit,
    ];

    /// The key `change_batch.kind` stores.
    pub const fn as_str(self) -> &'static str {
        match self {
            ChangeKind::RuleApply => "rule_apply",
            ChangeKind::DuplicateMerge => "duplicate_merge",
            ChangeKind::BulkEdit => "bulk_edit",
        }
    }

    pub fn from_key(key: &str) -> Option<ChangeKind> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == key)
    }
}

/// A user-facing field a change batch can change (`change_item.field`).
/// Imported source fields are never among them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Field {
    /// `category_id`, `category_source` and `rule_id`.
    Category,
    /// `merchant_display` and `merchant_display_source`.
    Merchant,
    /// `user_note`.
    UserNote,
    /// The transaction's tags (`transaction_tag`).
    Tags,
    /// `excluded_reason`, `excluded_note` and `excluded_at`.
    Exclusion,
    /// The split lines (`transaction_split`).
    Split,
    /// `my_share_minor` and `share_label`.
    Share,
    /// `reimbursable`.
    Reimbursable,
}

impl Field {
    pub const ALL: [Field; 8] = [
        Field::Category,
        Field::Merchant,
        Field::UserNote,
        Field::Tags,
        Field::Exclusion,
        Field::Split,
        Field::Share,
        Field::Reimbursable,
    ];

    /// The key `change_item.field` stores.
    pub const fn as_str(self) -> &'static str {
        match self {
            Field::Category => "category",
            Field::Merchant => "merchant",
            Field::UserNote => "user_note",
            Field::Tags => "tags",
            Field::Exclusion => "exclusion",
            Field::Split => "split",
            Field::Share => "share",
            Field::Reimbursable => "reimbursable",
        }
    }

    pub fn from_key(key: &str) -> Option<Field> {
        Self::ALL.into_iter().find(|field| field.as_str() == key)
    }

    /// Whether a change to this field on a row in a locked period needs an
    /// unlock first (module docs, "Locks").
    pub const fn guarded_by_lock(self) -> bool {
        matches!(self, Field::Exclusion)
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Who set a category (`transaction.category_source`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CategorySource {
    Rule,
    PresetMap,
    User,
    AiApproved,
}

impl CategorySource {
    pub const fn as_str(self) -> &'static str {
        match self {
            CategorySource::Rule => "rule",
            CategorySource::PresetMap => "preset_map",
            CategorySource::User => "user",
            CategorySource::AiApproved => "ai_approved",
        }
    }
}

impl FromStr for CategorySource {
    type Err = ();

    fn from_str(key: &str) -> Result<CategorySource, ()> {
        [
            CategorySource::Rule,
            CategorySource::PresetMap,
            CategorySource::User,
            CategorySource::AiApproved,
        ]
        .into_iter()
        .find(|source| source.as_str() == key)
        .ok_or(())
    }
}

/// Who set a display merchant (`transaction.merchant_display_source`).
/// `None` in a [`MerchantValue`] means the name is derived.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NameSource {
    Rule,
    User,
}

impl NameSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            NameSource::Rule => "rule",
            NameSource::User => "user",
        }
    }
}

/// A category with who set it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CategoryValue {
    /// `None`: uncategorised (or split).
    pub category_id: Option<i64>,
    pub source: Option<CategorySource>,
    /// The rule that set it, when a rule did.
    pub rule_id: Option<RuleId>,
}

impl CategoryValue {
    /// No category.
    pub const NONE: CategoryValue = CategoryValue {
        category_id: None,
        source: None,
        rule_id: None,
    };
}

/// A display merchant with who set it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MerchantValue {
    pub name: Option<String>,
    /// `None`: derived from the bank text and the aliases. Writing a derived
    /// value recomputes the name rather than storing `name`.
    pub source: Option<NameSource>,
}

/// An exclusion.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExclusionValue {
    #[serde(with = "reason_key")]
    pub reason: ExclusionReason,
    pub note: Option<String>,
    /// When it was excluded (UTC, `YYYY-MM-DDTHH:MM:SS.SSSZ`). `None` in a
    /// value passed to [`Recorder::set`] means now.
    pub at: Option<String>,
}

impl ExclusionValue {
    /// Excluded now for `reason`, with no note.
    pub fn now(reason: ExclusionReason) -> ExclusionValue {
        ExclusionValue {
            reason,
            note: None,
            at: None,
        }
    }
}

mod reason_key {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    use crate::exclusion::ExclusionReason;

    pub fn serialize<S: Serializer>(reason: &ExclusionReason, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(reason.as_str())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<ExclusionReason, D::Error> {
        let key = String::deserialize(d)?;
        ExclusionReason::from_key(&key).ok_or_else(|| D::Error::custom("unknown reason"))
    }
}

/// One split line as stored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SplitLineValue {
    pub amount_minor: i64,
    pub category_id: Option<i64>,
    pub my_share_minor: Option<i64>,
    pub share_label: Option<String>,
    pub user_note: Option<String>,
}

/// The user's share of a transaction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShareValue {
    /// `None`: all of it.
    pub my_share_minor: Option<i64>,
    pub share_label: Option<String>,
}

impl ShareValue {
    /// All of it, unlabelled.
    pub const WHOLE: ShareValue = ShareValue {
        my_share_minor: None,
        share_label: None,
    };
}

/// A field's value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Category(CategoryValue),
    Merchant(MerchantValue),
    UserNote(Option<String>),
    /// Tag IDs; read back sorted and distinct.
    Tags(Vec<TagId>),
    Exclusion(Option<ExclusionValue>),
    /// The lines in order; empty when the row isn't split. A row takes
    /// lines only with no category or share of its own: set
    /// [`CategoryValue::NONE`] and [`ShareValue::WHOLE`] first, in the same
    /// recorder, so undo restores them after removing the lines.
    Split(Vec<SplitLineValue>),
    Share(ShareValue),
    Reimbursable(bool),
}

impl Value {
    /// The field the value is for.
    pub const fn field(&self) -> Field {
        match self {
            Value::Category(_) => Field::Category,
            Value::Merchant(_) => Field::Merchant,
            Value::UserNote(_) => Field::UserNote,
            Value::Tags(_) => Field::Tags,
            Value::Exclusion(_) => Field::Exclusion,
            Value::Split(_) => Field::Split,
            Value::Share(_) => Field::Share,
            Value::Reimbursable(_) => Field::Reimbursable,
        }
    }

    /// The canonical JSON `change_item` stores.
    pub fn to_json(&self) -> String {
        let json = match self {
            Value::Category(value) => serde_json::to_string(value),
            Value::Merchant(value) => serde_json::to_string(value),
            Value::UserNote(value) => serde_json::to_string(value),
            Value::Tags(value) => serde_json::to_string(value),
            Value::Exclusion(value) => serde_json::to_string(value),
            Value::Split(value) => serde_json::to_string(value),
            Value::Share(value) => serde_json::to_string(value),
            Value::Reimbursable(value) => serde_json::to_string(value),
        };
        json.expect("change values always encode")
    }

    /// Decodes `json` as a value of `field`.
    pub fn from_json(field: Field, json: &str) -> Option<Value> {
        let value = match field {
            Field::Category => Value::Category(serde_json::from_str(json).ok()?),
            Field::Merchant => Value::Merchant(serde_json::from_str(json).ok()?),
            Field::UserNote => Value::UserNote(serde_json::from_str(json).ok()?),
            Field::Tags => Value::Tags(serde_json::from_str(json).ok()?),
            Field::Exclusion => Value::Exclusion(serde_json::from_str(json).ok()?),
            Field::Split => Value::Split(serde_json::from_str(json).ok()?),
            Field::Share => Value::Share(serde_json::from_str(json).ok()?),
            Field::Reimbursable => Value::Reimbursable(serde_json::from_str(json).ok()?),
        };
        Some(value)
    }

    /// Whether the field, now `self`, still holds what a batch set it to
    /// (`recorded`), so undo may restore it. A category or a derived
    /// merchant compares only what the batch decided: the category and its
    /// source (deleting a rule clears `rule_id` but leaves the category, rules
    /// decision point 7), and that the name is derived (an alias edit since
    /// renames it without anyone choosing a name).
    fn still_holds(&self, recorded: &Value) -> bool {
        match (self, recorded) {
            (Value::Category(now), Value::Category(then)) => {
                now.category_id == then.category_id && now.source == then.source
            }
            (Value::Merchant(now), Value::Merchant(then)) if then.source.is_none() => {
                now.source.is_none()
            }
            _ => self == recorded,
        }
    }
}

/// Why a change, or undoing one, failed. Messages never contain
/// transaction text or values.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("there is no transaction {0}")]
    TransactionNotFound(TransactionId),

    #[error("there is no change batch {0}")]
    BatchNotFound(ChangeBatchId),

    #[error("change batch {0} is already undone")]
    AlreadyUndone(ChangeBatchId),

    #[error(
        "transaction {transaction} is dated {date}, on or before {through}, which is locked, \
         and this changes whether it counts; unlock the account first"
    )]
    Locked {
        transaction: TransactionId,
        date: NaiveDate,
        through: NaiveDate,
    },

    #[error("excluding a transaction as \"other\" needs a note saying why")]
    NoteRequired,

    #[error("a value for {field} on transaction {transaction} can't be written: {reason}")]
    Unwritable {
        transaction: TransactionId,
        field: Field,
        reason: &'static str,
    },

    #[error("change batch {batch} has an item that can't be read")]
    Corrupt { batch: ChangeBatchId },

    #[error("split: {0}")]
    Split(#[from] split::Error),

    #[error("merchant names: {0}")]
    Merchant(#[from] merchant::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

// Reading and writing fields --------------------------------------------------

/// What a change needs to know of a transaction.
#[derive(Clone, Copy, Debug)]
struct Row {
    date: NaiveDate,
    locked_through: Option<NaiveDate>,
    amount: i64,
    currency: Currency,
}

impl Row {
    fn locked(&self) -> Option<NaiveDate> {
        self.locked_through.filter(|through| self.date <= *through)
    }
}

fn load_row(conn: &Connection, transaction: TransactionId) -> Result<Row, Error> {
    conn.query_row(
        "SELECT t.date, a.reconciled_through, t.amount_minor, t.currency \
         FROM \"transaction\" t JOIN account a ON a.id = t.account_id WHERE t.id = ?1",
        [transaction],
        |row| {
            Ok(Row {
                date: account::date(row, "date")?,
                locked_through: account::optional_date(row, "reconciled_through")?,
                amount: row.get(2)?,
                currency: row.get(3)?,
            })
        },
    )
    .optional()?
    .ok_or(Error::TransactionNotFound(transaction))
}

/// The lock date that a change to `field` on `transaction` would meet:
/// `Some(reconciled_through)` when the field is guarded and the row is
/// dated on or before it. For previews; [`Recorder::set`] and [`undo`]
/// refuse such changes.
pub fn locked(
    conn: &Connection,
    transaction: TransactionId,
    field: Field,
) -> Result<Option<NaiveDate>, Error> {
    if !field.guarded_by_lock() {
        return Ok(None);
    }
    Ok(load_row(conn, transaction)?.locked())
}

/// The current value of `field` on `transaction`.
pub fn read(conn: &Connection, transaction: TransactionId, field: Field) -> Result<Value, Error> {
    load_row(conn, transaction)?;
    read_field(conn, transaction, field)
}

fn read_field(conn: &Connection, id: TransactionId, field: Field) -> Result<Value, Error> {
    let one = |sql: &str| -> rusqlite::Result<rusqlite::types::Value> {
        conn.query_row(sql, [id], |row| row.get(0))
    };
    let value = match field {
        Field::Category => conn.query_row(
            "SELECT category_id, category_source, rule_id FROM \"transaction\" WHERE id = ?1",
            [id],
            |row| {
                let source: Option<String> = row.get(1)?;
                Ok(Value::Category(CategoryValue {
                    category_id: row.get(0)?,
                    source: source
                        .map(|key| {
                            key.parse()
                                .map_err(|()| account::invalid(row, "category_source"))
                        })
                        .transpose()?,
                    rule_id: row.get(2)?,
                }))
            },
        )?,
        Field::Merchant => conn.query_row(
            "SELECT merchant_display, merchant_display_source FROM \"transaction\" WHERE id = ?1",
            [id],
            |row| {
                let source: Option<String> = row.get(1)?;
                let source = match source.as_deref() {
                    None => None,
                    Some("rule") => Some(NameSource::Rule),
                    Some("user") => Some(NameSource::User),
                    Some(_) => return Err(account::invalid(row, "merchant_display_source")),
                };
                Ok(Value::Merchant(MerchantValue {
                    name: row.get(0)?,
                    source,
                }))
            },
        )?,
        Field::UserNote => match one("SELECT user_note FROM \"transaction\" WHERE id = ?1")? {
            rusqlite::types::Value::Text(text) => Value::UserNote(Some(text)),
            _ => Value::UserNote(None),
        },
        Field::Tags => {
            let mut statement = conn.prepare_cached(
                "SELECT tag_id FROM transaction_tag WHERE transaction_id = ?1 ORDER BY tag_id",
            )?;
            let tags = statement
                .query_map([id], |row| row.get(0))?
                .collect::<Result<_, _>>()?;
            Value::Tags(tags)
        }
        Field::Exclusion => conn.query_row(
            "SELECT excluded_reason, excluded_note, excluded_at FROM \"transaction\" \
             WHERE id = ?1",
            [id],
            |row| {
                let reason: Option<String> = row.get(0)?;
                let Some(reason) = reason else {
                    return Ok(Value::Exclusion(None));
                };
                Ok(Value::Exclusion(Some(ExclusionValue {
                    reason: ExclusionReason::from_key(&reason)
                        .ok_or_else(|| account::invalid(row, "excluded_reason"))?,
                    note: row.get(1)?,
                    at: row.get(2)?,
                })))
            },
        )?,
        Field::Split => {
            let mut statement = conn.prepare_cached(
                "SELECT amount_minor, category_id, my_share_minor, share_label, user_note \
                 FROM transaction_split WHERE transaction_id = ?1 ORDER BY line_no",
            )?;
            let lines = statement
                .query_map([id], |row| {
                    Ok(SplitLineValue {
                        amount_minor: row.get(0)?,
                        category_id: row.get(1)?,
                        my_share_minor: row.get(2)?,
                        share_label: row.get(3)?,
                        user_note: row.get(4)?,
                    })
                })?
                .collect::<Result<_, _>>()?;
            Value::Split(lines)
        }
        Field::Share => conn.query_row(
            "SELECT my_share_minor, share_label FROM \"transaction\" WHERE id = ?1",
            [id],
            |row| {
                Ok(Value::Share(ShareValue {
                    my_share_minor: row.get(0)?,
                    share_label: row.get(1)?,
                }))
            },
        )?,
        Field::Reimbursable => conn.query_row(
            "SELECT reimbursable FROM \"transaction\" WHERE id = ?1",
            [id],
            |row| Ok(Value::Reimbursable(row.get(0)?)),
        )?,
    };
    Ok(value)
}

/// Why `value` can't be written to the row as it is now, if it can't: the
/// schema would refuse it, or what it names is gone. Undo counts such an
/// item as changed since; [`Recorder::set`] reports it as an error.
fn unwritable(
    conn: &Connection,
    id: TransactionId,
    row: &Row,
    value: &Value,
) -> Result<Option<&'static str>, Error> {
    let exists = |sql: &str, key: i64| -> rusqlite::Result<bool> {
        conn.query_row(sql, [key], |row| row.get(0))
    };
    let category_exists = |category: i64| {
        exists(
            "SELECT EXISTS (SELECT 1 FROM category WHERE id = ?1)",
            category,
        )
    };
    let reason = match value {
        Value::Category(value) => match value.category_id {
            Some(category) if !category_exists(category)? => Some("its category is gone"),
            Some(_) if split::is_split(conn, id)? => Some("the transaction is split"),
            _ => None,
        },
        Value::Share(value)
            if (value.my_share_minor.is_some() || value.share_label.is_some())
                && split::is_split(conn, id)? =>
        {
            Some("the transaction is split")
        }
        Value::Share(ShareValue {
            my_share_minor: Some(share),
            ..
        }) if !share_within(*share, row.amount) => Some("the share isn't part of the amount"),
        Value::Merchant(MerchantValue { name, source }) => match (source, name) {
            (Some(_), None) => Some("a set name can't be empty"),
            (Some(_), Some(name)) if name.trim().is_empty() => Some("a set name can't be blank"),
            _ => None,
        },
        Value::Exclusion(Some(exclusion))
            if exclusion.reason.needs_note()
                && exclusion
                    .note
                    .as_deref()
                    .is_none_or(|note| note.trim().is_empty()) =>
        {
            return Err(Error::NoteRequired);
        }
        Value::Split(lines) if !lines.is_empty() => {
            let parent: (Option<i64>, Option<i64>, Option<String>) = conn.query_row(
                "SELECT category_id, my_share_minor, share_label FROM \"transaction\" WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
            let in_transfer: bool = conn.query_row(
                "SELECT EXISTS (SELECT 1 FROM \"transaction\" t \
                 JOIN transfer_group g ON g.id = t.transfer_group_id \
                 WHERE t.id = ?1 AND g.status <> 'rejected')",
                [id],
                |row| row.get(0),
            )?;
            let candidate: Vec<split::SplitLine> = lines
                .iter()
                .map(|line| split::SplitLine {
                    amount: Money::new(line.amount_minor, row.currency),
                    category: None,
                    my_share: line
                        .my_share_minor
                        .map(|share| Money::new(share, row.currency)),
                    share_label: line.share_label.clone(),
                    user_note: None,
                })
                .collect();
            if parent.0.is_some() || parent.1.is_some() || parent.2.is_some() {
                Some("the transaction has a category or share of its own")
            } else if in_transfer {
                Some("the transaction is in a transfer")
            } else if split::validate(Money::new(row.amount, row.currency), &candidate).is_err() {
                Some("the lines don't split the amount")
            } else {
                let mut gone = false;
                for category in lines.iter().filter_map(|line| line.category_id) {
                    gone |= !category_exists(category)?;
                }
                gone.then_some("a line's category is gone")
            }
        }
        _ => None,
    };
    Ok(reason)
}

fn share_within(share: i64, amount: i64) -> bool {
    if amount >= 0 {
        (0..=amount).contains(&share)
    } else {
        (amount..=0).contains(&share)
    }
}

/// Writes `value` to its field, without checks beyond the schema's.
fn write_field(conn: &Connection, id: TransactionId, value: &Value) -> Result<(), Error> {
    match value {
        Value::Category(value) => {
            // A rule deleted since is cleared, as deleting it does.
            let rule = match value.rule_id {
                Some(rule) => conn
                    .query_row("SELECT id FROM rule WHERE id = ?1", [rule], |row| {
                        row.get::<_, RuleId>(0)
                    })
                    .optional()?,
                None => None,
            };
            conn.execute(
                "UPDATE \"transaction\" SET category_id = ?1, category_source = ?2, rule_id = ?3 \
                 WHERE id = ?4",
                params![
                    value.category_id,
                    value.source.map(CategorySource::as_str),
                    rule,
                    id
                ],
            )?;
        }
        Value::Merchant(value) => match value.source {
            Some(source) => {
                conn.execute(
                    "UPDATE \"transaction\" SET merchant_display = ?1, \
                     merchant_display_source = ?2 WHERE id = ?3",
                    params![value.name, source.as_str(), id],
                )?;
            }
            None => {
                conn.execute(
                    "UPDATE \"transaction\" SET merchant_display_source = NULL WHERE id = ?1",
                    [id],
                )?;
                merchant::apply_to_transaction(conn, id)?;
            }
        },
        Value::UserNote(note) => {
            conn.execute(
                "UPDATE \"transaction\" SET user_note = ?1 WHERE id = ?2",
                params![note, id],
            )?;
        }
        Value::Tags(tags) => {
            let want: BTreeSet<TagId> = tags.iter().copied().collect();
            let Value::Tags(have) = read_field(conn, id, Field::Tags)? else {
                unreachable!("reading tags gives tags");
            };
            for tag in &have {
                if !want.contains(tag) {
                    conn.execute(
                        "DELETE FROM transaction_tag WHERE transaction_id = ?1 AND tag_id = ?2",
                        params![id, tag],
                    )?;
                }
            }
            for tag in want {
                if !have.contains(&tag) {
                    // A tag deleted since stays gone.
                    conn.execute(
                        "INSERT INTO transaction_tag (transaction_id, tag_id) \
                         SELECT ?1, id FROM tag WHERE id = ?2",
                        params![id, tag],
                    )?;
                }
            }
        }
        Value::Exclusion(None) => {
            conn.execute(
                "UPDATE \"transaction\" SET excluded_reason = NULL, excluded_note = NULL, \
                 excluded_at = NULL WHERE id = ?1",
                [id],
            )?;
        }
        Value::Exclusion(Some(exclusion)) => {
            let note = exclusion
                .note
                .as_deref()
                .filter(|note| !note.trim().is_empty());
            conn.execute(
                "UPDATE \"transaction\" SET excluded_reason = ?1, excluded_note = ?2, \
                 excluded_at = coalesce(?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) WHERE id = ?4",
                params![exclusion.reason.as_str(), note, exclusion.at, id],
            )?;
        }
        Value::Split(lines) => {
            split::clear(conn, id)?;
            let mut insert = conn.prepare_cached(
                "INSERT INTO transaction_split (transaction_id, line_no, amount_minor, \
                 category_id, my_share_minor, share_label, user_note) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for (index, line) in lines.iter().enumerate() {
                insert.execute(params![
                    id,
                    index as i64 + 1,
                    line.amount_minor,
                    line.category_id,
                    line.my_share_minor,
                    line.share_label,
                    line.user_note,
                ])?;
            }
        }
        Value::Share(share) => {
            conn.execute(
                "UPDATE \"transaction\" SET my_share_minor = ?1, share_label = ?2 WHERE id = ?3",
                params![share.my_share_minor, share.share_label, id],
            )?;
        }
        Value::Reimbursable(flag) => {
            conn.execute(
                "UPDATE \"transaction\" SET reimbursable = ?1 WHERE id = ?2",
                params![flag, id],
            )?;
        }
    }
    Ok(())
}

// Recording -------------------------------------------------------------------

/// A change to one field of one transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub transaction: TransactionId,
    pub old: Value,
    pub new: Value,
}

impl Change {
    pub fn field(&self) -> Field {
        self.new.field()
    }
}

/// Makes changes to existing transactions and records them as one change
/// batch. Runs inside the caller's DB transaction; nothing is recorded until
/// [`Recorder::finish`].
#[derive(Debug)]
pub struct Recorder {
    kind: ChangeKind,
    changes: Vec<Change>,
    index: HashMap<(TransactionId, Field), usize>,
}

impl Recorder {
    pub fn new(kind: ChangeKind) -> Recorder {
        Recorder {
            kind,
            changes: Vec::new(),
            index: HashMap::new(),
        }
    }

    pub fn kind(&self) -> ChangeKind {
        self.kind
    }

    /// Sets a field of `transaction` to `value` and records the change.
    /// Returns whether the field changed: a value it already holds changes
    /// nothing and isn't recorded. Setting the same field twice records one
    /// change, from the first old value to the last new one.
    ///
    /// Refuses a change to an exclusion in a locked period
    /// ([`Error::Locked`]) and a value the row can't take now
    /// ([`Error::Unwritable`], [`Error::NoteRequired`]). On an error nothing
    /// was written for this call; the caller rolls its transaction back.
    pub fn set(
        &mut self,
        conn: &Connection,
        transaction: TransactionId,
        value: Value,
    ) -> Result<bool, Error> {
        let row = load_row(conn, transaction)?;
        let field = value.field();
        let old = read_field(conn, transaction, field)?;
        if old == value {
            return Ok(false);
        }
        // Excluding again for the same reason and note keeps the date.
        if let (Value::Exclusion(Some(now)), Value::Exclusion(Some(new))) = (&old, &value)
            && new.at.is_none()
            && now.reason == new.reason
            && now.note == new.note
        {
            return Ok(false);
        }
        if field.guarded_by_lock()
            && let Some(through) = row.locked()
        {
            return Err(Error::Locked {
                transaction,
                date: row.date,
                through,
            });
        }
        if let Some(reason) = unwritable(conn, transaction, &row, &value)? {
            return Err(Error::Unwritable {
                transaction,
                field,
                reason,
            });
        }
        write_field(conn, transaction, &value)?;
        // What the database now holds: a timestamp filled in, tags sorted, a
        // derived name recomputed.
        let new = read_field(conn, transaction, field)?;
        if new == old {
            return Ok(false);
        }
        match self.index.get(&(transaction, field)) {
            Some(&at) => self.changes[at].new = new,
            None => {
                self.index.insert((transaction, field), self.changes.len());
                self.changes.push(Change {
                    transaction,
                    old,
                    new,
                });
            }
        }
        Ok(true)
    }

    /// The changes so far, in the order made, without fields set back to
    /// where they started.
    pub fn changes(&self) -> impl Iterator<Item = &Change> {
        self.changes
            .iter()
            .filter(|change| change.old != change.new)
    }

    /// How many transactions have a change.
    pub fn transactions(&self) -> usize {
        self.changes()
            .map(|change| change.transaction)
            .collect::<BTreeSet<_>>()
            .len()
    }

    /// Whether nothing changed.
    pub fn is_empty(&self) -> bool {
        self.changes().next().is_none()
    }

    /// Writes the batch and its items. `None`, writing nothing, when nothing
    /// changed.
    pub fn finish(self, conn: &Connection) -> Result<Option<ChangeBatchId>, Error> {
        if self.is_empty() {
            return Ok(None);
        }
        conn.execute(
            "INSERT INTO change_batch (kind) VALUES (?1)",
            [self.kind.as_str()],
        )?;
        let batch = ChangeBatchId::new(conn.last_insert_rowid());
        let mut insert = conn.prepare_cached(
            "INSERT INTO change_item (batch_id, transaction_id, field, old_value, new_value) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for change in self.changes() {
            insert.execute(params![
                batch,
                change.transaction,
                change.field().as_str(),
                change.old.to_json(),
                change.new.to_json(),
            ])?;
        }
        tracing::info!(
            batch = %batch,
            kind = self.kind.as_str(),
            transactions = self.transactions(),
            "recorded a change batch"
        );
        Ok(Some(batch))
    }
}

/// Runs `plan` in a DB transaction that is then rolled back, and returns
/// what it gave: a preview by the same code a commit runs. Takes a write
/// lock while it runs (`BEGIN IMMEDIATE`), so nothing changes underneath.
pub fn dry_run<T, E>(db: &Database, plan: impl FnOnce(&Connection) -> Result<T, E>) -> Result<T, E>
where
    E: From<rusqlite::Error>,
{
    let tx = Transaction::new_unchecked(db.conn(), TransactionBehavior::Immediate)?;
    let result = plan(&tx);
    tx.rollback()?;
    result
}

// Batches and undo ------------------------------------------------------------

/// Whether a batch is in effect or undone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchStatus {
    Applied,
    Undone,
}

/// A stored change batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchInfo {
    pub id: ChangeBatchId,
    pub kind: ChangeKind,
    pub created_at: String,
    pub status: BatchStatus,
    pub undone_at: Option<String>,
    /// Its items: one per transaction and field.
    pub items: usize,
    /// The transactions it changed.
    pub transactions: usize,
}

/// A stored item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub transaction: TransactionId,
    pub old: Value,
    pub new: Value,
}

impl Item {
    pub fn field(&self) -> Field {
        self.new.field()
    }
}

const BATCH_COLUMNS: &str = "b.id, b.kind, b.created_at, b.status, b.undone_at, \
     (SELECT COUNT(*) FROM change_item i WHERE i.batch_id = b.id) AS items, \
     (SELECT COUNT(DISTINCT transaction_id) FROM change_item i WHERE i.batch_id = b.id) \
     AS transactions";

fn batch_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<BatchInfo> {
    let kind: String = row.get("kind")?;
    let status: String = row.get("status")?;
    Ok(BatchInfo {
        id: row.get("id")?,
        kind: ChangeKind::from_key(&kind).ok_or_else(|| account::invalid(row, "kind"))?,
        created_at: row.get("created_at")?,
        status: match status.as_str() {
            "applied" => BatchStatus::Applied,
            "undone" => BatchStatus::Undone,
            _ => return Err(account::invalid(row, "status")),
        },
        undone_at: row.get("undone_at")?,
        items: row.get::<_, i64>("items")?.try_into().unwrap_or(0),
        transactions: row.get::<_, i64>("transactions")?.try_into().unwrap_or(0),
    })
}

fn load_batch(conn: &Connection, batch: ChangeBatchId) -> Result<BatchInfo, Error> {
    conn.query_row(
        &format!("SELECT {BATCH_COLUMNS} FROM change_batch b WHERE b.id = ?1"),
        [batch],
        batch_from_row,
    )
    .optional()?
    .ok_or(Error::BatchNotFound(batch))
}

/// Change batch `batch`.
pub fn get(db: &Database, batch: ChangeBatchId) -> Result<BatchInfo, Error> {
    load_batch(db.conn(), batch)
}

/// Every change batch, newest first.
pub fn list(db: &Database) -> Result<Vec<BatchInfo>, Error> {
    let mut statement = db.conn().prepare(&format!(
        "SELECT {BATCH_COLUMNS} FROM change_batch b ORDER BY b.id DESC"
    ))?;
    let batches = statement
        .query_map([], batch_from_row)?
        .collect::<Result<_, _>>()?;
    Ok(batches)
}

/// The items of `batch`, in the order they were made.
pub fn items(db: &Database, batch: ChangeBatchId) -> Result<Vec<Item>, Error> {
    load_batch(db.conn(), batch)?;
    load_items(db.conn(), batch)
}

fn load_items(conn: &Connection, batch: ChangeBatchId) -> Result<Vec<Item>, Error> {
    let mut statement = conn.prepare(
        "SELECT transaction_id, field, old_value, new_value FROM change_item \
         WHERE batch_id = ?1 ORDER BY id",
    )?;
    let rows = statement
        .query_map([batch], |row| {
            Ok((
                row.get::<_, TransactionId>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(transaction, field, old, new)| {
            let field = Field::from_key(&field).ok_or(Error::Corrupt { batch })?;
            Ok(Item {
                transaction,
                old: Value::from_json(field, &old).ok_or(Error::Corrupt { batch })?,
                new: Value::from_json(field, &new).ok_or(Error::Corrupt { batch })?,
            })
        })
        .collect()
}

/// What undoing a batch does, or did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoSummary {
    pub batch: BatchInfo,
    /// Items restored to their old value.
    pub restored: usize,
    /// Items left as they are because the field changed since (or what the
    /// old value names is gone), for the undo dialog to count.
    pub changed_since: usize,
    /// Transactions with at least one restored item.
    pub transactions: usize,
}

/// What [`undo`] would do. Runs the undo in a DB transaction that is rolled
/// back ([`dry_run`]), so it fails as `undo` would.
pub fn undo_preview(db: &Database, batch: ChangeBatchId) -> Result<UndoSummary, Error> {
    dry_run(db, |conn| undo_in(conn, batch))
}

/// Undoes `batch` in one DB transaction: restores each item whose field
/// still holds the batch's new value, newest item first, and marks the
/// batch `undone`. Refuses an exclusion change in a locked period
/// ([`Error::Locked`]).
pub fn undo(db: &mut Database, batch: ChangeBatchId) -> Result<UndoSummary, Error> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let summary = undo_in(&tx, batch)?;
    tx.commit()?;
    tracing::info!(
        batch = %batch,
        restored = summary.restored,
        changed_since = summary.changed_since,
        "undid a change batch"
    );
    Ok(summary)
}

/// [`undo`] inside the caller's DB transaction.
pub fn undo_in(conn: &Connection, batch: ChangeBatchId) -> Result<UndoSummary, Error> {
    let info = load_batch(conn, batch)?;
    if info.status == BatchStatus::Undone {
        return Err(Error::AlreadyUndone(batch));
    }
    let items = load_items(conn, batch)?;
    let mut restored = 0;
    let mut changed_since = 0;
    let mut transactions = BTreeSet::new();
    for item in items.iter().rev() {
        let row = load_row(conn, item.transaction)?;
        let current = read_field(conn, item.transaction, item.field())?;
        if !current.still_holds(&item.new) {
            changed_since += 1;
            continue;
        }
        if current == item.old {
            continue;
        }
        if item.field().guarded_by_lock()
            && let Some(through) = row.locked()
        {
            return Err(Error::Locked {
                transaction: item.transaction,
                date: row.date,
                through,
            });
        }
        if unwritable(conn, item.transaction, &row, &item.old)?.is_some() {
            changed_since += 1;
            continue;
        }
        write_field(conn, item.transaction, &item.old)?;
        restored += 1;
        transactions.insert(item.transaction);
    }
    conn.execute(
        "UPDATE change_batch SET status = 'undone', \
         undone_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
        [batch],
    )?;
    after_undo(conn, &info)?;
    Ok(UndoSummary {
        batch: load_batch(conn, batch)?,
        restored,
        changed_since,
        transactions: transactions.len(),
    })
}

/// What a batch's kind undoes beyond its items, in the same DB transaction.
fn after_undo(conn: &Connection, info: &BatchInfo) -> Result<(), Error> {
    match info.kind {
        // M1.26: a merged duplicate candidate goes back to `pending` here.
        ChangeKind::DuplicateMerge => {
            let _ = conn;
            Ok(())
        }
        ChangeKind::RuleApply | ChangeKind::BulkEdit => Ok(()),
    }
}

// Deleting transactions -------------------------------------------------------

/// Deletes the change items of `transaction`, for whoever deletes the row (a
/// manual row): the foreign key has no `ON DELETE`. The batches stay.
/// Opens no transaction of its own.
pub fn clear(conn: &Connection, transaction: TransactionId) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM change_item WHERE transaction_id = ?1",
        [transaction],
    )
}

/// Deletes the change items of every transaction of import `batch`
/// (mirrored rows included), for undoing the import. Opens no transaction
/// of its own.
pub fn clear_batch(conn: &Connection, batch: BatchId) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM change_item WHERE transaction_id IN \
         (SELECT id FROM \"transaction\" WHERE batch_id = ?1)",
        [batch],
    )
}
