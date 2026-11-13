//! Storing merchant aliases, and setting a name on one transaction.
//!
//! Every write is one database transaction, and every alias change
//! recomputes the derived names in it. An alias is validated as an
//! [`AliasDefinition`] before it is stored, and again when it is read back.

use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{AliasDefinition, AliasError, AliasKind, AliasSet, DisplayName, MerchantAlias};
use crate::db::Database;
use crate::id::{MerchantAliasId, TransactionId};

/// Why storing or reading an alias, or naming a transaction, failed.
///
/// Messages never contain a pattern, a name or transaction text.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// There is no alias with this ID.
    #[error("there is no merchant name {0}")]
    NotFound(MerchantAliasId),

    /// Another alias already matches the same text the same way (plain
    /// patterns ignoring case and repeated whitespace).
    #[error("merchant name {0} already matches this text")]
    Duplicate(MerchantAliasId),

    /// A stored alias no longer validates.
    #[error("merchant name {id} is stored with {problem}")]
    Invalid {
        /// The alias.
        id: MerchantAliasId,
        /// What is wrong with it.
        problem: InvalidAlias,
    },

    /// There is no transaction with this ID.
    #[error("there is no transaction {0}")]
    TransactionNotFound(TransactionId),

    /// The database refused the operation.
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// What is wrong with a stored alias ([`Error::Invalid`]).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidAlias {
    /// `match_kind` isn't an [`AliasKind`] key.
    #[error("an unknown match kind")]
    Kind,

    /// The pattern or the name doesn't validate.
    #[error("an invalid definition: {0}")]
    Definition(AliasError),
}

/// Stores a new alias and recomputes every derived name with it, in one
/// database transaction. Returns the alias's ID and how many transactions'
/// names changed.
///
/// An alias that matches the same text the same way as a stored one is
/// [`Error::Duplicate`].
pub fn add(db: &mut Database, alias: &AliasDefinition) -> Result<(MerchantAliasId, usize), Error> {
    let tx = db.conn_mut().transaction()?;
    let added = add_in(&tx, alias)?;
    tx.commit()?;
    Ok(added)
}

/// [`add`] inside the caller's DB transaction.
pub(crate) fn add_in(
    tx: &Connection,
    alias: &AliasDefinition,
) -> Result<(MerchantAliasId, usize), Error> {
    refuse_duplicate(tx, alias, None)?;
    tx.execute(
        "INSERT INTO merchant_alias (match_kind, pattern, display_name) VALUES (?1, ?2, ?3)",
        params![
            alias.kind().as_str(),
            alias.pattern(),
            alias.name().as_str()
        ],
    )?;
    let id = MerchantAliasId::new(tx.last_insert_rowid());
    let changed = AliasSet::load(tx)?.recompute_all(tx)?;
    Ok((id, changed))
}

/// Replaces an alias's definition, keeping its ID (and so its place among
/// regexes), and recomputes every derived name, in one database
/// transaction. Returns how many transactions' names changed.
pub fn edit(
    db: &mut Database,
    id: MerchantAliasId,
    alias: &AliasDefinition,
) -> Result<usize, Error> {
    let tx = db.conn_mut().transaction()?;
    refuse_duplicate(&tx, alias, Some(id))?;
    let updated = tx.execute(
        "UPDATE merchant_alias SET match_kind = ?1, pattern = ?2, display_name = ?3 \
         WHERE id = ?4",
        params![
            alias.kind().as_str(),
            alias.pattern(),
            alias.name().as_str(),
            id
        ],
    )?;
    if updated == 0 {
        return Err(Error::NotFound(id));
    }
    let changed = AliasSet::load(&tx)?.recompute_all(&tx)?;
    tx.commit()?;
    Ok(changed)
}

/// Deletes an alias and recomputes every derived name, in one database
/// transaction. Returns how many transactions' names changed (back to their
/// cleaned text, or to another alias).
pub fn delete(db: &mut Database, id: MerchantAliasId) -> Result<usize, Error> {
    let tx = db.conn_mut().transaction()?;
    if tx.execute("DELETE FROM merchant_alias WHERE id = ?1", [id])? == 0 {
        return Err(Error::NotFound(id));
    }
    let changed = AliasSet::load(&tx)?.recompute_all(&tx)?;
    tx.commit()?;
    Ok(changed)
}

/// The alias with this ID, validated.
pub fn get(db: &Database, id: MerchantAliasId) -> Result<MerchantAlias, Error> {
    let conn = db.conn();
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM merchant_alias WHERE id = ?1"),
        [id],
        RawAlias::from_row,
    )
    .optional()?
    .ok_or(Error::NotFound(id))?
    .validate()
}

/// Every alias, by ID (the order regexes are tried in), validated.
pub fn list(db: &Database) -> Result<Vec<MerchantAlias>, Error> {
    load_all(db.conn())
}

/// Sets the name the user typed on one transaction
/// (`merchant_display_source = 'user'`). Recomputes leave it alone until
/// [`clear_user_name`]. The bank's text is unchanged.
pub fn set_user_name(
    db: &mut Database,
    transaction: TransactionId,
    name: &DisplayName,
) -> Result<(), Error> {
    let updated = db.conn().execute(
        "UPDATE \"transaction\" SET merchant_display = ?1, merchant_display_source = 'user' \
         WHERE id = ?2",
        params![name.as_str(), transaction],
    )?;
    if updated == 0 {
        return Err(Error::TransactionNotFound(transaction));
    }
    Ok(())
}

/// Drops the name set on one transaction (by the user or a rule) and
/// derives it again from the bank's text and the aliases, in one database
/// transaction.
pub fn clear_user_name(db: &mut Database, transaction: TransactionId) -> Result<(), Error> {
    let tx = db.conn_mut().transaction()?;
    let updated = tx.execute(
        "UPDATE \"transaction\" SET merchant_display_source = NULL WHERE id = ?1",
        [transaction],
    )?;
    if updated == 0 {
        return Err(Error::TransactionNotFound(transaction));
    }
    AliasSet::load(&tx)?.apply_to_transaction(&tx, transaction)?;
    tx.commit()?;
    Ok(())
}

/// Every alias by ID, validated.
pub(super) fn load_all(conn: &Connection) -> Result<Vec<MerchantAlias>, Error> {
    let mut statement =
        conn.prepare(&format!("SELECT {COLUMNS} FROM merchant_alias ORDER BY id"))?;
    let raw = statement
        .query_map([], RawAlias::from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    raw.into_iter().map(RawAlias::validate).collect()
}

/// Refuses `alias` when a stored alias other than `except` has the same
/// match key.
fn refuse_duplicate(
    conn: &Connection,
    alias: &AliasDefinition,
    except: Option<MerchantAliasId>,
) -> Result<(), Error> {
    let key = alias.match_key();
    match load_all(conn)?
        .into_iter()
        .find(|stored| Some(stored.id) != except && stored.definition.match_key() == key)
    {
        Some(stored) => Err(Error::Duplicate(stored.id)),
        None => Ok(()),
    }
}

const COLUMNS: &str = "id, match_kind, pattern, display_name";

/// A `merchant_alias` row before validation.
struct RawAlias {
    id: MerchantAliasId,
    kind: String,
    pattern: String,
    name: String,
}

impl RawAlias {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<RawAlias> {
        Ok(RawAlias {
            id: row.get("id")?,
            kind: row.get("match_kind")?,
            pattern: row.get("pattern")?,
            name: row.get("display_name")?,
        })
    }

    fn validate(self) -> Result<MerchantAlias, Error> {
        let invalid = |problem| Error::Invalid {
            id: self.id,
            problem,
        };
        let kind: AliasKind = self.kind.parse().map_err(|_| invalid(InvalidAlias::Kind))?;
        let definition = DisplayName::new(&self.name)
            .and_then(|name| AliasDefinition::new(kind, &self.pattern, name))
            .map_err(|err| invalid(InvalidAlias::Definition(err)))?;
        Ok(MerchantAlias::new(self.id, definition))
    }
}
