//! The user's edits to one transaction in the Transactions inspector
//! (ROADMAP M3.9; `docs/ARCHITECTURE.md` → "Decision record: splits, tags
//! and notes in the inspector").
//!
//! Each function is one edit: its split, its tags, its note or its display
//! merchant. Each runs in one DB transaction and is recorded as one
//! undoable change batch ([`change::Recorder`], kind
//! [`ChangeKind::BulkEdit`], as the table's category edits are through
//! `inbox::own::recategorize`), so [`change::undo`] reverts it and the
//! change history (M3.13) lists it. An edit that changes nothing records
//! nothing and returns `None`.
//!
//! Only user-facing fields change here, never an imported source field.
//! Splits, tags, notes and names are user text or the user's choices:
//! nothing here logs them, and errors never quote them.
//!
//! - **Splits** go through the same checks as [`split::set`] (not a
//!   transfer, not linked money in, lines in the amount's sign and currency
//!   summing to it exactly, the categories exist). Splitting clears the
//!   transaction's own category and share first, in the same batch, so
//!   undoing it puts them back after the lines go.
//! - **Tags**: [`add_tag`] and [`remove_tag`] link and unlink an existing
//!   tag; [`add_new_tag`] links the tag with that name (ignoring case),
//!   creating it first, public or private, when there is none. Undoing the
//!   batch unlinks it; the tag itself stays.
//! - **Notes** follow [`UserNote`]: trimmed, at most 2,000
//!   characters, blank clears. The bank's imported note is never written.
//! - **The display merchant**: a name the user types is theirs
//!   (`merchant_display_source = user`) and no recompute changes it; blank
//!   goes back to the name derived from the bank's text and the aliases.

#[cfg(test)]
mod tests;

use rusqlite::{Connection, TransactionBehavior};

use crate::change::{
    self, CategoryValue, ChangeKind, Field, MerchantValue, NameSource, Recorder, ShareValue,
    SplitLineValue, Value,
};
use crate::db::Database;
use crate::id::{ChangeBatchId, TagId, TransactionId};
use crate::merchant::DisplayName;
use crate::note::{NoteError, UserNote};
use crate::split::{self, SplitLine};
use crate::tag::{self, TagName, TagNameError};

/// Why an edit wasn't saved. Messages never contain transaction text,
/// notes, tag names or merchant names.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Split(#[from] split::Error),

    #[error(transparent)]
    Note(#[from] NoteError),

    #[error(transparent)]
    TagName(#[from] TagNameError),

    #[error(transparent)]
    Tag(#[from] tag::Error),

    #[error(transparent)]
    Change(change::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

impl From<change::Error> for Error {
    fn from(error: change::Error) -> Error {
        match error {
            change::Error::Split(error) => Error::Split(error),
            change::Error::TransactionNotFound(id) => {
                Error::Split(split::Error::TransactionNotFound(id))
            }
            other => Error::Change(other),
        }
    }
}

/// Runs `edit` with a recorder in one immediate DB transaction and
/// commits the batch it records, if any.
fn record(
    db: &mut Database,
    edit: impl FnOnce(&Connection, &mut Recorder) -> Result<(), Error>,
) -> Result<Option<ChangeBatchId>, Error> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut recorder = Recorder::new(ChangeKind::BulkEdit);
    edit(&tx, &mut recorder)?;
    let batch = recorder.finish(&tx)?;
    tx.commit()?;
    Ok(batch)
}

/// Splits `transaction` into `lines`, replacing any split it has: one
/// undoable change batch (module docs). `None` when the split is already
/// exactly this.
pub fn set_split(
    db: &mut Database,
    transaction: TransactionId,
    lines: &[SplitLine],
) -> Result<Option<ChangeBatchId>, Error> {
    record(db, |conn, recorder| {
        let categories = split::prepare(conn, transaction, lines)?;
        let values: Vec<SplitLineValue> = lines
            .iter()
            .zip(categories)
            .map(|(line, category_id)| SplitLineValue {
                amount_minor: line.amount.minor(),
                category_id,
                my_share_minor: line.my_share.map(|share| share.minor()),
                share_label: line.share_label.clone(),
                user_note: line.user_note.clone(),
            })
            .collect();
        let values = Value::Split(values);
        // Already exactly this: nothing changes, link suggestions included.
        if change::read(conn, transaction, Field::Split)? == values {
            return Ok(());
        }
        split::release_suggestions(conn, transaction)?;
        recorder.set(conn, transaction, Value::Category(CategoryValue::NONE))?;
        recorder.set(conn, transaction, Value::Share(ShareValue::WHOLE))?;
        recorder.set(conn, transaction, values)?;
        Ok(())
    })
}

/// Removes `transaction`'s split, leaving it uncategorised: one undoable
/// change batch. `None` when it isn't split.
pub fn remove_split(
    db: &mut Database,
    transaction: TransactionId,
) -> Result<Option<ChangeBatchId>, Error> {
    record(db, |conn, recorder| {
        recorder.set(conn, transaction, Value::Split(Vec::new()))?;
        Ok(())
    })
}

/// `transaction`'s tags.
fn tags(conn: &Connection, transaction: TransactionId) -> Result<Vec<TagId>, Error> {
    match change::read(conn, transaction, Field::Tags)? {
        Value::Tags(tags) => Ok(tags),
        _ => unreachable!("reading tags gives tags"),
    }
}

fn tag_exists(conn: &Connection, tag: TagId) -> Result<(), Error> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM tag WHERE id = ?1)",
        [tag],
        |row| row.get(0),
    )?;
    if exists {
        Ok(())
    } else {
        Err(tag::Error::NotFound(tag).into())
    }
}

fn link_tag(
    conn: &Connection,
    recorder: &mut Recorder,
    transaction: TransactionId,
    tag: TagId,
) -> Result<(), Error> {
    let mut tags = tags(conn, transaction)?;
    if !tags.contains(&tag) {
        tags.push(tag);
        recorder.set(conn, transaction, Value::Tags(tags))?;
    }
    Ok(())
}

/// Tags `transaction` with `tag`: one undoable change batch, `None` when
/// it already has it.
pub fn add_tag(
    db: &mut Database,
    transaction: TransactionId,
    tag: TagId,
) -> Result<Option<ChangeBatchId>, Error> {
    record(db, |conn, recorder| {
        tag_exists(conn, tag)?;
        link_tag(conn, recorder, transaction, tag)
    })
}

/// Takes `tag` off `transaction` (the tag stays): one undoable change
/// batch, `None` when it didn't have it.
pub fn remove_tag(
    db: &mut Database,
    transaction: TransactionId,
    tag: TagId,
) -> Result<Option<ChangeBatchId>, Error> {
    record(db, |conn, recorder| {
        let mut tags = tags(conn, transaction)?;
        let before = tags.len();
        tags.retain(|have| *have != tag);
        if tags.len() != before {
            recorder.set(conn, transaction, Value::Tags(tags))?;
        }
        Ok(())
    })
}

/// Tags `transaction` with the tag named `name`, ignoring case, creating it
/// (`private` or not) when there is none: one DB transaction and one
/// undoable change batch for the link. An existing tag keeps its own
/// privacy. Returns the tag and the batch (`None` when the transaction
/// already had it).
pub fn add_new_tag(
    db: &mut Database,
    transaction: TransactionId,
    name: &str,
    private: bool,
) -> Result<(TagId, Option<ChangeBatchId>), Error> {
    let name = TagName::new(name)?;
    let mut id = None;
    let batch = record(db, |conn, recorder| {
        // Checked first, so a missing row doesn't leave a new tag behind
        // (the transaction would roll back anyway).
        change::read(conn, transaction, Field::Tags)?;
        let (tag, _) = tag::find_or_create_in(conn, &name, private)?;
        id = Some(tag);
        link_tag(conn, recorder, transaction, tag)
    })?;
    Ok((id.expect("set by the edit"), batch))
}

/// Sets the user's note on `transaction` from what they typed (blank
/// clears it): one undoable change batch, `None` when unchanged.
pub fn set_note(
    db: &mut Database,
    transaction: TransactionId,
    text: &str,
) -> Result<Option<ChangeBatchId>, Error> {
    let note = UserNote::new(text)?;
    record(db, |conn, recorder| {
        recorder.set(
            conn,
            transaction,
            Value::UserNote(note.map(|note| note.as_str().to_owned())),
        )?;
        Ok(())
    })
}

/// Names `transaction`'s merchant as the user typed it, or, when blank,
/// goes back to the name derived from the bank's text and the aliases:
/// one undoable change batch, `None` when unchanged.
pub fn set_merchant(
    db: &mut Database,
    transaction: TransactionId,
    text: &str,
) -> Result<Option<ChangeBatchId>, Error> {
    // A display name is refused only when blank.
    let value = match DisplayName::new(text).ok() {
        Some(name) => MerchantValue {
            name: Some(name.as_str().to_owned()),
            source: Some(NameSource::User),
        },
        None => MerchantValue {
            name: None,
            source: None,
        },
    };
    record(db, |conn, recorder| {
        recorder.set(conn, transaction, Value::Merchant(value))?;
        Ok(())
    })
}
