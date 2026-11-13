//! Tags: user-named labels on transactions, alongside the category
//! (ROADMAP M1.21; `docs/ARCHITECTURE.md` → Decisions, "Tags and notes").
//!
//! A transaction can have any number of tags, and a tag any number of
//! transactions (`tag` and `transaction_tag`, `migrations/V6__tags.sql`).
//!
//! # Names
//!
//! A [`TagName`] is trimmed of surrounding whitespace, isn't blank, is at
//! most [`MAX_TAG_NAME_LEN`] characters and has no control characters (no
//! line breaks or tabs). Its case is kept as typed. Names are unique
//! ignoring case, with the same Unicode case folding rule matching uses
//! (`Café` and `CAFÉ`, `Straße` and `STRASSE` are one name): creating or
//! renaming to a name another tag has is [`Error::NameTaken`]. Rules name
//! tags rather than referring to them by ID, and a rule's `add_tag` list is
//! made of [`TagName`]s too (`rule::TagList`), so both check names the same
//! way.
//!
//! # Private tags
//!
//! A tag marked private ([`Tag::private`]) never leaves the app: it isn't
//! sent over MCP at any access level and isn't written to exports. Code that
//! sends tags anywhere outside the app must pass them through
//! [`without_private`] or read them with [`public_tags_of`]. Tag names that
//! aren't private are still user text: MCP shows them only at `standard`
//! access, which is MCP's own check.
//!
//! # Links
//!
//! [`add_to_transaction`] adding a tag the transaction already has is a
//! no-op that returns `false`, so applying a tag twice (by hand, or a rule
//! after the user) is harmless. [`remove_from_transaction`] removes the link
//! and never the tag. [`delete`] removes a tag's links and then the tag, in
//! one DB transaction. Tags work the same on imported, mirrored and manual
//! transactions: they're user-facing fields, never source fields.
//!
//! Tag names are user text: errors never repeat them, and they must not be
//! logged at `info` or above.

use std::collections::HashSet;
use std::fmt;

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::db::Database;
use crate::id::{BatchId, TagId, TransactionId};
use crate::rule::fold;

/// The longest tag name, in characters (Unicode scalar values). The schema
/// checks the same limit.
pub const MAX_TAG_NAME_LEN: usize = 64;

/// A valid tag name: trimmed, not blank, at most [`MAX_TAG_NAME_LEN`]
/// characters, no control characters.
///
/// Equality is exact (`Work` isn't `work`); whether two names are the same
/// tag is [`TagName::same_name`], which ignores case.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct TagName(String);

impl TagName {
    /// Trims `text` and checks it.
    pub fn new(text: &str) -> Result<TagName, TagNameError> {
        let name = text.trim();
        if name.is_empty() {
            return Err(TagNameError::Blank);
        }
        if name.chars().count() > MAX_TAG_NAME_LEN {
            return Err(TagNameError::TooLong {
                max: MAX_TAG_NAME_LEN,
            });
        }
        if name.chars().any(char::is_control) {
            return Err(TagNameError::ControlCharacter);
        }
        Ok(TagName(name.to_owned()))
    }

    /// The name, as typed (trimmed).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether `self` and `other` name the same tag: equal ignoring case.
    pub fn same_name(&self, other: &TagName) -> bool {
        self.folded() == other.folded()
    }

    /// The name folded for caseless comparison. Two names are the same tag
    /// when their folded forms are equal.
    pub fn folded(&self) -> String {
        fold(&self.0)
    }
}

impl fmt::Debug for TagName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // User text: length only.
        write!(f, "TagName(len {})", self.0.len())
    }
}

/// Why a tag name was refused. Messages never repeat the name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TagNameError {
    /// Empty or only whitespace.
    #[error("the tag name is blank")]
    Blank,

    /// Longer than [`MAX_TAG_NAME_LEN`] characters once trimmed.
    #[error("a tag name is at most {max} characters")]
    TooLong {
        /// The limit.
        max: usize,
    },

    /// A line break, tab or other control character.
    #[error("a tag name can't contain line breaks, tabs or other control characters")]
    ControlCharacter,
}

/// A stored tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub id: TagId,
    pub name: TagName,
    /// A private tag never leaves the app: not over MCP, not in exports.
    pub private: bool,
}

impl Tag {
    /// Whether the tag may leave the app (it isn't private). Even then its
    /// name is user text, shown over MCP only at `standard` access.
    pub const fn is_public(&self) -> bool {
        !self.private
    }
}

/// Why a tag operation failed. Messages never contain tag names.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("there is no tag {0}")]
    NotFound(TagId),

    #[error("there is no transaction {0}")]
    TransactionNotFound(TransactionId),

    /// Another tag has the name, ignoring case.
    #[error("tag {0} already has this name (tag names ignore case)")]
    NameTaken(TagId),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// Creates a tag. Refuses a name another tag has, ignoring case
/// ([`Error::NameTaken`]).
pub fn create(db: &mut Database, name: &TagName, private: bool) -> Result<TagId, Error> {
    let tx = db.conn_mut().transaction()?;
    if let Some(existing) = find_in(&tx, name)? {
        return Err(Error::NameTaken(existing.id));
    }
    tx.execute(
        "INSERT INTO tag (name, private) VALUES (?1, ?2)",
        params![name.as_str(), private],
    )?;
    let id = TagId::new(tx.last_insert_rowid());
    tx.commit()?;
    Ok(id)
}

/// The tag with this ID.
pub fn get(db: &Database, id: TagId) -> Result<Tag, Error> {
    load(db.conn(), id)
}

/// The tag with this name, ignoring case; `None` when there is none.
pub fn find(db: &Database, name: &TagName) -> Result<Option<Tag>, Error> {
    Ok(find_in(db.conn(), name)?)
}

/// Every tag, by name ignoring ASCII case, then ID.
pub fn list(db: &Database) -> Result<Vec<Tag>, Error> {
    Ok(all(db.conn())?)
}

/// Renames a tag. Changing only the case of its own name is allowed;
/// taking another tag's name, ignoring case, is [`Error::NameTaken`].
pub fn rename(db: &mut Database, id: TagId, name: &TagName) -> Result<(), Error> {
    let tx = db.conn_mut().transaction()?;
    load(&tx, id)?;
    if let Some(existing) = find_in(&tx, name)?
        && existing.id != id
    {
        return Err(Error::NameTaken(existing.id));
    }
    tx.execute(
        "UPDATE tag SET name = ?1 WHERE id = ?2",
        params![name.as_str(), id],
    )?;
    tx.commit()?;
    Ok(())
}

/// Marks a tag private (`true`) or not (`false`).
pub fn set_private(db: &mut Database, id: TagId, private: bool) -> Result<(), Error> {
    let changed = db.conn().execute(
        "UPDATE tag SET private = ?1 WHERE id = ?2",
        params![private, id],
    )?;
    if changed == 0 {
        return Err(Error::NotFound(id));
    }
    Ok(())
}

/// Deletes a tag and its links to transactions, in one DB transaction.
/// Returns how many transactions had it.
pub fn delete(db: &mut Database, id: TagId) -> Result<usize, Error> {
    let tx = db.conn_mut().transaction()?;
    load(&tx, id)?;
    let unlinked = tx.execute("DELETE FROM transaction_tag WHERE tag_id = ?1", [id])?;
    tx.execute("DELETE FROM tag WHERE id = ?1", [id])?;
    tx.commit()?;
    Ok(unlinked)
}

/// Puts a tag on a transaction. Returns `false`, changing nothing, when the
/// transaction already has it.
pub fn add_to_transaction(
    db: &mut Database,
    transaction: TransactionId,
    tag: TagId,
) -> Result<bool, Error> {
    let tx = db.conn_mut().transaction()?;
    require_transaction(&tx, transaction)?;
    load(&tx, tag)?;
    // `transaction_tag`'s primary key is the pair, so only a repeat is
    // ignored; a missing row is still a foreign-key error.
    let added = tx.execute(
        "INSERT OR IGNORE INTO transaction_tag (transaction_id, tag_id) VALUES (?1, ?2)",
        params![transaction, tag],
    )?;
    tx.commit()?;
    Ok(added == 1)
}

/// Takes a tag off a transaction. The tag itself stays. Returns `false`
/// when the transaction didn't have it.
pub fn remove_from_transaction(
    db: &mut Database,
    transaction: TransactionId,
    tag: TagId,
) -> Result<bool, Error> {
    let tx = db.conn_mut().transaction()?;
    require_transaction(&tx, transaction)?;
    load(&tx, tag)?;
    let removed = tx.execute(
        "DELETE FROM transaction_tag WHERE transaction_id = ?1 AND tag_id = ?2",
        params![transaction, tag],
    )?;
    tx.commit()?;
    Ok(removed == 1)
}

/// A transaction's tags, private ones included, by name ignoring ASCII
/// case, then ID. For the app only: anything that leaves the app reads
/// [`public_tags_of`] instead.
pub fn tags_of(db: &Database, transaction: TransactionId) -> Result<Vec<Tag>, Error> {
    let conn = db.conn();
    require_transaction(conn, transaction)?;
    Ok(linked(conn, transaction, false)?)
}

/// A transaction's tags that may leave the app: [`tags_of`] without the
/// private ones.
pub fn public_tags_of(db: &Database, transaction: TransactionId) -> Result<Vec<Tag>, Error> {
    let conn = db.conn();
    require_transaction(conn, transaction)?;
    Ok(linked(conn, transaction, true)?)
}

/// `tags` without the private ones, in the same order: what may leave the
/// app.
pub fn without_private(tags: impl IntoIterator<Item = Tag>) -> Vec<Tag> {
    tags.into_iter().filter(Tag::is_public).collect()
}

const COLUMNS: &str = "tag.id, tag.name, tag.private";

fn from_row(row: &Row<'_>) -> rusqlite::Result<Tag> {
    let name: String = row.get("name")?;
    Ok(Tag {
        id: row.get("id")?,
        // The schema checks less than `TagName` does (ASCII whitespace
        // only), so a failure means the file was changed outside Coffer.
        name: TagName::new(&name).map_err(|_| crate::account::invalid(row, "name"))?,
        private: row.get("private")?,
    })
}

fn load(conn: &Connection, id: TagId) -> Result<Tag, Error> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM tag WHERE id = ?1"),
        [id],
        from_row,
    )
    .optional()?
    .ok_or(Error::NotFound(id))
}

fn all(conn: &Connection) -> rusqlite::Result<Vec<Tag>> {
    // `name` is `COLLATE NOCASE`, so this orders ignoring ASCII case.
    let mut statement = conn.prepare(&format!("SELECT {COLUMNS} FROM tag ORDER BY name, id"))?;
    statement.query_map([], from_row)?.collect()
}

/// The tag named `name` ignoring case. SQL's `NOCASE` folds ASCII only, so
/// this compares in Rust; there are few tags.
fn find_in(conn: &Connection, name: &TagName) -> rusqlite::Result<Option<Tag>> {
    let folded = name.folded();
    Ok(all(conn)?
        .into_iter()
        .find(|tag| tag.name.folded() == folded))
}

fn linked(
    conn: &Connection,
    transaction: TransactionId,
    public_only: bool,
) -> rusqlite::Result<Vec<Tag>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM transaction_tag JOIN tag ON tag.id = transaction_tag.tag_id \
         WHERE transaction_tag.transaction_id = ?1 AND (?2 = 0 OR tag.private = 0) \
         ORDER BY tag.name, tag.id"
    ))?;
    statement
        .query_map(params![transaction, public_only], from_row)?
        .collect()
}

fn require_transaction(conn: &Connection, id: TransactionId) -> Result<(), Error> {
    conn.query_row("SELECT 1 FROM \"transaction\" WHERE id = ?1", [id], |_| {
        Ok(())
    })
    .optional()?
    .ok_or(Error::TransactionNotFound(id))
}

/// Deletes `transaction`'s tag links, if any, and returns how many there
/// were. The tags stay. For whoever deletes a transaction (a manual row):
/// its links go first, or the foreign key refuses the delete. Opens no
/// transaction of its own.
pub fn clear(conn: &Connection, transaction: TransactionId) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM transaction_tag WHERE transaction_id = ?1",
        [transaction],
    )
}

/// Deletes the tag links of every transaction of import `batch` (mirrored
/// rows included) and returns how many there were. For undo, before it
/// deletes the batch's transactions. Opens no transaction of its own.
pub fn clear_batch(conn: &Connection, batch: BatchId) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM transaction_tag WHERE transaction_id IN \
         (SELECT id FROM \"transaction\" WHERE batch_id = ?1)",
        [batch],
    )
}

/// The ID of the tag named `name` ignoring case, creating it (not private)
/// when there is none. For rules' `add_tag`, which name tags rather than
/// refer to them. Opens no transaction of its own.
pub(crate) fn find_or_create(conn: &Connection, name: &TagName) -> rusqlite::Result<TagId> {
    if let Some(tag) = find_in(conn, name)? {
        return Ok(tag.id);
    }
    conn.execute("INSERT INTO tag (name) VALUES (?1)", [name.as_str()])?;
    Ok(TagId::new(conn.last_insert_rowid()))
}

/// Whether `names` repeat a name, ignoring case.
pub(crate) fn has_repeat(names: &[TagName]) -> bool {
    let mut seen = HashSet::new();
    !names.iter().all(|name| seen.insert(name.folded()))
}

#[cfg(test)]
mod tests;
