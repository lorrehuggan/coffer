//! The user's note on a transaction (ROADMAP M1.21; `docs/ARCHITECTURE.md`
//! → Decisions, "Tags and notes").
//!
//! A transaction has two notes. `"transaction".note` is the note from the
//! bank's export: a source field, read-only on imported and mirrored rows
//! (the `transaction_source_fields_read_only` trigger), and never written
//! here. `"transaction".user_note` is the user's own, set and cleared by
//! [`set`] and [`clear`] on a transaction of any origin (imported, mirrored
//! or manual).
//!
//! A [`UserNote`] is trimmed of surrounding whitespace; line breaks and
//! other text inside it are kept as typed. Blank text (empty or only
//! whitespace) is no note: [`set`] stores `NULL`. A note is at most
//! [`MAX_USER_NOTE_LEN`] characters once trimmed. The schema can't check
//! that limit without rebuilding `"transaction"`, so it holds for notes
//! written through this module.
//!
//! The user's note isn't sent over MCP by default (`AGENTS.md` →
//! Invariants), and a note read back is untrusted text, matched at most,
//! never acted on. Errors never repeat a note, and notes must not be logged
//! at `info` or above.

use std::fmt;

use rusqlite::{Connection, OptionalExtension, params};

use crate::db::Database;
use crate::id::TransactionId;

/// The longest user note, in characters (Unicode scalar values), after
/// trimming.
pub const MAX_USER_NOTE_LEN: usize = 2_000;

/// A user note to store: trimmed, not blank, at most
/// [`MAX_USER_NOTE_LEN`] characters.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct UserNote(String);

impl UserNote {
    /// Trims `text` and checks it. Blank text is `Ok(None)`: no note.
    pub fn new(text: &str) -> Result<Option<UserNote>, NoteError> {
        let note = text.trim();
        if note.is_empty() {
            return Ok(None);
        }
        if note.chars().count() > MAX_USER_NOTE_LEN {
            return Err(NoteError::TooLong {
                max: MAX_USER_NOTE_LEN,
            });
        }
        Ok(Some(UserNote(note.to_owned())))
    }

    /// The note, trimmed.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for UserNote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // User text: length only.
        write!(f, "UserNote(len {})", self.0.len())
    }
}

/// Why a note was refused. Messages never repeat the note.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NoteError {
    /// Longer than [`MAX_USER_NOTE_LEN`] characters once trimmed.
    #[error("a note is at most {max} characters")]
    TooLong {
        /// The limit.
        max: usize,
    },
}

/// Why setting or reading a note failed. Messages never contain a note.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Note(#[from] NoteError),

    #[error("there is no transaction {0}")]
    TransactionNotFound(TransactionId),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// Sets the user's note on a transaction from what they typed. Blank text
/// clears it. Returns the note as stored (`None` when cleared).
pub fn set(
    db: &mut Database,
    transaction: TransactionId,
    text: &str,
) -> Result<Option<UserNote>, Error> {
    let note = UserNote::new(text)?;
    write(db.conn(), transaction, note.as_ref())?;
    Ok(note)
}

/// Clears the user's note on a transaction.
pub fn clear(db: &mut Database, transaction: TransactionId) -> Result<(), Error> {
    write(db.conn(), transaction, None)
}

/// The user's note on a transaction, `None` when there is none. Never the
/// imported note.
pub fn get(db: &Database, transaction: TransactionId) -> Result<Option<String>, Error> {
    db.conn()
        .query_row(
            "SELECT user_note FROM \"transaction\" WHERE id = ?1",
            [transaction],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(Error::TransactionNotFound(transaction))
}

fn write(
    conn: &Connection,
    transaction: TransactionId,
    note: Option<&UserNote>,
) -> Result<(), Error> {
    let changed = conn.execute(
        "UPDATE \"transaction\" SET user_note = ?1 WHERE id = ?2",
        params![note.map(UserNote::as_str), transaction],
    )?;
    if changed == 0 {
        return Err(Error::TransactionNotFound(transaction));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
