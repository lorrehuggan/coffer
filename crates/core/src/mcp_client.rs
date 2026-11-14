//! MCP clients and their access levels (ROADMAP M2.11; `docs/ARCHITECTURE.md`
//! → "MCP surface" and "Decision record: Settings, AI access and backups
//! (2026-11-10)").
//!
//! A client is the name `coffer mcp --client <name>` was started with
//! ([`ClientName`]); `mcp_client` (V11) records whether the user enabled it
//! and at which [`AccessLevel`]. The MCP server registers only the tools at
//! or below a client's level.
//!
//! A client's lifecycle ([`ClientStatus`]):
//!
//! - **requested**: the server saw a name it didn't know and recorded it
//!   ([`record_requested`], the one write the server makes here). Not
//!   enabled; every call is refused.
//! - **known**: the user answered by enabling it ([`enable`], which starts
//!   at `aggregate`, or [`enable_at`], the app's Enable dialog, at the level
//!   chosen). [`revoke`] keeps it known but not enabled, its level back at
//!   `aggregate`; [`turn_off`] and [`turn_on`] (the app's per-client switch,
//!   M3.15) keep its level.
//! - **ignored**: the user hid the request ([`ignore`]). Not enabled.
//!
//! [`remove`] deletes a client's row: its level is forgotten, its log,
//! suggestions and reports stay, and a later call is a new request.
//!
//! Only the app and the CLI, which hold `&mut Database`, enable a client or
//! change its level; the server holds a [`Reader`] for everything but
//! [`record_requested`]. The kill switch is [`crate::setting::mcp_enabled`].

use std::fmt;
use std::str::FromStr;

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef};
use rusqlite::{OptionalExtension, Row, ToSql, params};

use crate::db::{Database, Reader};
use crate::rule::{UnknownKey, key_enum};

key_enum! {
    /// How much of the data a client may see, as `mcp_client.access_level`
    /// stores it. Ordered: each level sees everything the levels before it
    /// see.
    AccessLevel, "access level" {
        /// Totals only: no transaction rows, no writes but `save_report`.
        Aggregate => "aggregate",
        /// Transaction rows, with each merchant replaced by a stable
        /// keyed-hash token.
        Pseudonymous => "pseudonymous",
        /// The full surface.
        Standard => "standard",
    }
}

impl ToSql for AccessLevel {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(ValueRef::Text(
            self.as_str().as_bytes(),
        )))
    }
}

impl FromSql for AccessLevel {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<AccessLevel> {
        value
            .as_str()?
            .parse()
            .map_err(|error| FromSqlError::Other(Box::new(error)))
    }
}

/// The longest client name, in characters.
pub const MAX_CLIENT_NAME_LEN: usize = 64;

/// The name the app's own suggestions carry (`suggestion.source_client`),
/// which no MCP client may use.
pub const RESERVED_CLIENT_NAME: &str = "coffer";

/// A client's name: 1 to [`MAX_CLIENT_NAME_LEN`] ASCII letters, digits,
/// `.`, `_` or `-`, and not [`RESERVED_CLIENT_NAME`] in any case. The schema
/// checks the same.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClientName(String);

impl ClientName {
    /// Checks `name`. The error doesn't repeat it.
    pub fn new(name: &str) -> Result<ClientName, ClientNameError> {
        if name.is_empty() || name.len() > MAX_CLIENT_NAME_LEN {
            return Err(ClientNameError::Length {
                max: MAX_CLIENT_NAME_LEN,
            });
        }
        if !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(ClientNameError::Characters);
        }
        if name.eq_ignore_ascii_case(RESERVED_CLIENT_NAME) {
            return Err(ClientNameError::Reserved);
        }
        Ok(ClientName(name.to_owned()))
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ClientName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ClientName {
    type Err = ClientNameError;

    fn from_str(name: &str) -> Result<ClientName, ClientNameError> {
        ClientName::new(name)
    }
}

/// Why a client name was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClientNameError {
    #[error("a client name is 1 to {max} characters")]
    Length { max: usize },
    #[error("a client name has only ASCII letters, digits, '.', '_' and '-'")]
    Characters,
    #[error("`{RESERVED_CLIENT_NAME}` is reserved for Coffer's own suggestions")]
    Reserved,
}

/// Why reading or changing a client failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No client has this name.
    #[error("there is no AI client called `{0}`")]
    NotFound(ClientName),

    /// The client isn't enabled, so it has no level to change.
    #[error("the AI client `{0}` isn't enabled; enable it first")]
    NotEnabled(ClientName),

    /// The client only asked for access (or was ignored): it's enabled by
    /// choosing a level, not turned back on.
    #[error("the AI client `{0}` hasn't been enabled yet; enable it and choose its level")]
    NeverEnabled(ClientName),

    #[error(transparent)]
    Setting(#[from] crate::setting::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

key_enum! {
    /// Where a client's request stands, as `mcp_client.status` stores it.
    ClientStatus, "client status" {
        /// The server saw the name; the user hasn't answered.
        Requested => "requested",
        /// The user enabled it (it may since have been revoked).
        Known => "known",
        /// The user hid the request without enabling it.
        Ignored => "ignored",
    }
}

impl FromSql for ClientStatus {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<ClientStatus> {
        value
            .as_str()?
            .parse()
            .map_err(|error| FromSqlError::Other(Box::new(error)))
    }
}

/// A client as the app and `coffer ai-access list` show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientInfo {
    pub name: ClientName,
    pub status: ClientStatus,
    pub enabled: bool,
    /// The stored level. It only applies while [`ClientInfo::enabled`].
    pub level: AccessLevel,
    /// UTC, `YYYY-MM-DDTHH:MM:SS.SSSZ`.
    pub created_at: String,
    /// The latest of the client's request and its latest logged call, UTC;
    /// `None` when neither is known.
    pub last_seen_at: Option<String>,
}

/// What a client may do now, from its row alone (the kill switch is
/// separate: [`crate::setting::mcp_enabled`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientAccess {
    /// No row: the server records it with [`record_requested`].
    Unknown,
    /// Asked for access; the user hasn't answered.
    Requested,
    /// The user hid the request.
    Ignored,
    /// Enabled once, then revoked.
    Revoked,
    /// Enabled at this level.
    Enabled(AccessLevel),
}

impl ClientAccess {
    /// The level the client may use now, `None` when it may use no tool.
    pub fn level(self) -> Option<AccessLevel> {
        match self {
            ClientAccess::Enabled(level) => Some(level),
            _ => None,
        }
    }
}

const SELECT_CLIENT: &str = "SELECT c.name, c.status, c.enabled, c.access_level, c.created_at, \
     NULLIF(max(coalesce(c.last_seen_at, ''), \
                coalesce((SELECT max(l.at) FROM mcp_access_log l WHERE l.client = c.name), '')), '') \
     FROM mcp_client c";

fn client_info(row: &Row<'_>) -> rusqlite::Result<ClientInfo> {
    let name: String = row.get(0)?;
    Ok(ClientInfo {
        name: ClientName::new(&name).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        status: row.get(1)?,
        enabled: row.get(2)?,
        level: row.get(3)?,
        created_at: row.get(4)?,
        last_seen_at: row.get(5)?,
    })
}

/// Every client that has asked for access or been enabled, by name.
pub fn list(reader: &Reader) -> Result<Vec<ClientInfo>, Error> {
    let mut statement = reader
        .conn()
        .prepare(&format!("{SELECT_CLIENT} ORDER BY c.name"))?;
    let rows = statement.query_map([], client_info)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The client called `name`, `None` when it has no row.
pub fn get(reader: &Reader, name: &ClientName) -> Result<Option<ClientInfo>, Error> {
    get_on(reader.conn(), name)
}

fn get_on(conn: &rusqlite::Connection, name: &ClientName) -> Result<Option<ClientInfo>, Error> {
    Ok(conn
        .query_row(
            &format!("{SELECT_CLIENT} WHERE c.name = ?1"),
            [name.as_str()],
            client_info,
        )
        .optional()?)
}

/// What `name` may do now. Read by the MCP server on every call.
pub fn access(reader: &Reader, name: &ClientName) -> Result<ClientAccess, Error> {
    let row: Option<(ClientStatus, bool, AccessLevel)> = reader
        .conn()
        .query_row(
            "SELECT status, enabled, access_level FROM mcp_client WHERE name = ?1",
            [name.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    Ok(match row {
        None => ClientAccess::Unknown,
        Some((_, true, level)) => ClientAccess::Enabled(level),
        Some((ClientStatus::Requested, false, _)) => ClientAccess::Requested,
        Some((ClientStatus::Ignored, false, _)) => ClientAccess::Ignored,
        Some((ClientStatus::Known, false, _)) => ClientAccess::Revoked,
    })
}

/// Records `name` as a client that asked for access: a `requested` row,
/// not enabled. A name that already has a row is left as it is. Returns
/// whether a row was created.
///
/// The only write the MCP server makes outside the inbox and its own log:
/// it never enables a client or changes a level.
pub fn record_requested(db: &mut Database, name: &ClientName) -> Result<bool, Error> {
    let created = db.conn().execute(
        "INSERT INTO mcp_client (name, status, enabled, last_seen_at) \
         VALUES (?1, 'requested', 0, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) \
         ON CONFLICT (name) DO NOTHING",
        [name.as_str()],
    )?;
    Ok(created > 0)
}

/// Enables `name`, creating its row if the server hasn't seen it yet. A
/// client that wasn't enabled starts at `aggregate`; one that already is
/// keeps its level. Creates the pseudonym key on first use
/// ([`crate::setting::ensure_pseudonym_key`]), so the server never writes
/// settings.
pub fn enable(db: &mut Database, name: &ClientName) -> Result<ClientInfo, Error> {
    let tx = db.conn_mut().transaction()?;
    tx.execute(
        "INSERT INTO mcp_client (name, status, enabled, access_level) \
         VALUES (?1, 'known', 1, 'aggregate') \
         ON CONFLICT (name) DO UPDATE SET \
             access_level = CASE WHEN enabled = 1 THEN access_level ELSE 'aggregate' END, \
             status = 'known', \
             enabled = 1",
        [name.as_str()],
    )?;
    crate::setting::ensure_pseudonym_key_on(&tx)?;
    let info = get_on(&tx, name)?.ok_or_else(|| Error::NotFound(name.clone()))?;
    tx.commit()?;
    Ok(info)
}

/// Sets an enabled client's level. Lowering it applies to the client's
/// next call; raising it too, without restarting the server.
pub fn set_level(db: &mut Database, name: &ClientName, level: AccessLevel) -> Result<(), Error> {
    let changed = db.conn().execute(
        "UPDATE mcp_client SET access_level = ?2 WHERE name = ?1 AND enabled = 1",
        params![name.as_str(), level],
    )?;
    if changed == 0 {
        return Err(missing_or_disabled(db, name));
    }
    Ok(())
}

/// Revokes `name`: it stays known, but isn't enabled, and its level goes
/// back to `aggregate` for when it is enabled again.
pub fn revoke(db: &mut Database, name: &ClientName) -> Result<(), Error> {
    let changed = db.conn().execute(
        "UPDATE mcp_client SET enabled = 0, access_level = 'aggregate' \
         WHERE name = ?1 AND status = 'known'",
        [name.as_str()],
    )?;
    if changed == 0 {
        return Err(missing_or_disabled(db, name));
    }
    Ok(())
}

/// Hides `name`'s request without enabling it. An enabled client is
/// revoked as well.
pub fn ignore(db: &mut Database, name: &ClientName) -> Result<(), Error> {
    let changed = db.conn().execute(
        "UPDATE mcp_client SET status = 'ignored', enabled = 0, access_level = 'aggregate' \
         WHERE name = ?1",
        [name.as_str()],
    )?;
    if changed == 0 {
        return Err(Error::NotFound(name.clone()));
    }
    Ok(())
}

/// Enables `name` at `level`: the app's Enable dialog, which chooses the
/// level as it enables (starting from `aggregate`, the least shared). As
/// [`enable`] followed by [`set_level`], in one transaction.
pub fn enable_at(
    db: &mut Database,
    name: &ClientName,
    level: AccessLevel,
) -> Result<ClientInfo, Error> {
    let tx = db.conn_mut().transaction()?;
    tx.execute(
        "INSERT INTO mcp_client (name, status, enabled, access_level) \
         VALUES (?1, 'known', 1, ?2) \
         ON CONFLICT (name) DO UPDATE SET \
             access_level = excluded.access_level, status = 'known', enabled = 1",
        params![name.as_str(), level],
    )?;
    crate::setting::ensure_pseudonym_key_on(&tx)?;
    let info = get_on(&tx, name)?.ok_or_else(|| Error::NotFound(name.clone()))?;
    tx.commit()?;
    Ok(info)
}

/// Turns an enabled client off (the app's per-client switch): its next call
/// is refused, and its level is kept for [`turn_on`]. Unlike [`revoke`],
/// which sets the level back to `aggregate`.
pub fn turn_off(db: &mut Database, name: &ClientName) -> Result<(), Error> {
    let changed = db.conn().execute(
        "UPDATE mcp_client SET enabled = 0 WHERE name = ?1 AND enabled = 1",
        [name.as_str()],
    )?;
    if changed == 0 {
        return Err(missing_or_disabled(db, name));
    }
    Ok(())
}

/// Turns a client the user enabled before back on, at the level it had.
/// A client that has only asked (`requested`) or was ignored is refused:
/// it's enabled through [`enable_at`], where the user chooses its level.
/// Turning on an enabled client changes nothing.
pub fn turn_on(db: &mut Database, name: &ClientName) -> Result<(), Error> {
    let tx = db.conn_mut().transaction()?;
    let changed = tx.execute(
        "UPDATE mcp_client SET enabled = 1 WHERE name = ?1 AND status = 'known'",
        [name.as_str()],
    )?;
    if changed == 0 {
        return Err(match get_on(&tx, name)? {
            Some(_) => Error::NeverEnabled(name.clone()),
            None => Error::NotFound(name.clone()),
        });
    }
    crate::setting::ensure_pseudonym_key_on(&tx)?;
    tx.commit()?;
    Ok(())
}

/// Removes `name`'s row: it isn't enabled any more and its level is
/// forgotten. Its access-log rows, suggestions and reports stay (they name
/// the client as text, not by a key). If it calls again, the server records
/// it as `requested`, as for any name it hasn't seen.
pub fn remove(db: &mut Database, name: &ClientName) -> Result<(), Error> {
    let removed = db
        .conn()
        .execute("DELETE FROM mcp_client WHERE name = ?1", [name.as_str()])?;
    if removed == 0 {
        return Err(Error::NotFound(name.clone()));
    }
    Ok(())
}

fn missing_or_disabled(db: &Database, name: &ClientName) -> Error {
    match get_on(db.conn(), name) {
        Ok(Some(_)) => Error::NotEnabled(name.clone()),
        Ok(None) => Error::NotFound(name.clone()),
        Err(error) => error,
    }
}

/// The access level stored for `client`, `None` when it has no row,
/// whether or not it is enabled. The server uses [`access`].
pub fn stored_level(reader: &Reader, client: &ClientName) -> Result<Option<AccessLevel>, Error> {
    Ok(reader
        .conn()
        .query_row(
            "SELECT access_level FROM mcp_client WHERE name = ?1",
            [client.as_str()],
            |row| row.get(0),
        )
        .optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_ordered() {
        assert!(AccessLevel::Aggregate < AccessLevel::Pseudonymous);
        assert!(AccessLevel::Pseudonymous < AccessLevel::Standard);
        for level in AccessLevel::ALL {
            assert_eq!(level.as_str().parse::<AccessLevel>().unwrap(), *level);
        }
        assert!("admin".parse::<AccessLevel>().is_err());
    }

    fn db() -> (tempfile::TempDir, Database) {
        let dir = tempfile::TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("coffer.db")).unwrap();
        (dir, db)
    }

    fn name(name: &str) -> ClientName {
        ClientName::new(name).unwrap()
    }

    #[test]
    fn a_requested_client_is_not_enabled() {
        let (_dir, mut db) = db();
        let reader = db.reader().unwrap();
        let codex = name("codex");
        assert_eq!(access(&reader, &codex).unwrap(), ClientAccess::Unknown);
        assert!(record_requested(&mut db, &codex).unwrap());
        assert!(!record_requested(&mut db, &codex).unwrap());
        assert_eq!(access(&reader, &codex).unwrap(), ClientAccess::Requested);
        let info = get(&reader, &codex).unwrap().unwrap();
        assert_eq!(info.status, ClientStatus::Requested);
        assert!(!info.enabled);
        assert!(info.last_seen_at.is_some());
        // The server never writes a setting: no key yet.
        assert_eq!(crate::setting::pseudonym_key(&reader).unwrap(), None);
    }

    #[test]
    fn enabling_starts_at_aggregate_and_creates_the_key() {
        let (_dir, mut db) = db();
        let reader = db.reader().unwrap();
        let codex = name("codex");
        record_requested(&mut db, &codex).unwrap();
        let info = enable(&mut db, &codex).unwrap();
        assert_eq!(
            (info.status, info.enabled, info.level),
            (ClientStatus::Known, true, AccessLevel::Aggregate)
        );
        assert!(crate::setting::pseudonym_key(&reader).unwrap().is_some());

        set_level(&mut db, &codex, AccessLevel::Standard).unwrap();
        // Enabling again keeps the level.
        assert_eq!(
            enable(&mut db, &codex).unwrap().level,
            AccessLevel::Standard
        );
        assert_eq!(
            access(&reader, &codex).unwrap(),
            ClientAccess::Enabled(AccessLevel::Standard)
        );

        // A name the server never saw can be enabled ahead of time.
        let claude = name("claude-code");
        assert_eq!(
            enable(&mut db, &claude).unwrap().level,
            AccessLevel::Aggregate
        );
        let names: Vec<_> = list(&reader)
            .unwrap()
            .into_iter()
            .map(|info| info.name)
            .collect();
        assert_eq!(names, [claude, codex]);
    }

    #[test]
    fn revoke_and_ignore_take_access_away() {
        let (_dir, mut db) = db();
        let reader = db.reader().unwrap();
        let codex = name("codex");
        enable(&mut db, &codex).unwrap();
        set_level(&mut db, &codex, AccessLevel::Pseudonymous).unwrap();
        revoke(&mut db, &codex).unwrap();
        assert_eq!(access(&reader, &codex).unwrap(), ClientAccess::Revoked);
        assert!(matches!(
            set_level(&mut db, &codex, AccessLevel::Standard),
            Err(Error::NotEnabled(_))
        ));
        // Enabled again, it starts over at aggregate.
        assert_eq!(
            enable(&mut db, &codex).unwrap().level,
            AccessLevel::Aggregate
        );

        ignore(&mut db, &codex).unwrap();
        assert_eq!(access(&reader, &codex).unwrap(), ClientAccess::Ignored);
        assert!(matches!(revoke(&mut db, &codex), Err(Error::NotEnabled(_))));

        let ghost = name("ghost");
        assert!(matches!(ignore(&mut db, &ghost), Err(Error::NotFound(_))));
        assert!(matches!(revoke(&mut db, &ghost), Err(Error::NotFound(_))));
        assert!(matches!(
            set_level(&mut db, &ghost, AccessLevel::Standard),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn enable_at_chooses_the_level() {
        let (_dir, mut db) = db();
        let reader = db.reader().unwrap();
        let agent = name("my-agent");
        record_requested(&mut db, &agent).unwrap();
        let info = enable_at(&mut db, &agent, AccessLevel::Pseudonymous).unwrap();
        assert_eq!(
            (info.status, info.enabled, info.level),
            (ClientStatus::Known, true, AccessLevel::Pseudonymous)
        );
        assert!(crate::setting::pseudonym_key(&reader).unwrap().is_some());
        // An ignored client can be enabled again, at the level chosen.
        ignore(&mut db, &agent).unwrap();
        assert_eq!(
            enable_at(&mut db, &agent, AccessLevel::Aggregate)
                .unwrap()
                .level,
            AccessLevel::Aggregate
        );
    }

    #[test]
    fn turning_off_keeps_the_level() {
        let (_dir, mut db) = db();
        let reader = db.reader().unwrap();
        let codex = name("codex");
        enable_at(&mut db, &codex, AccessLevel::Standard).unwrap();
        turn_off(&mut db, &codex).unwrap();
        assert_eq!(access(&reader, &codex).unwrap(), ClientAccess::Revoked);
        assert_eq!(
            stored_level(&reader, &codex).unwrap(),
            Some(AccessLevel::Standard)
        );
        assert!(matches!(
            turn_off(&mut db, &codex),
            Err(Error::NotEnabled(_))
        ));
        turn_on(&mut db, &codex).unwrap();
        assert_eq!(
            access(&reader, &codex).unwrap(),
            ClientAccess::Enabled(AccessLevel::Standard)
        );
        // Idempotent.
        turn_on(&mut db, &codex).unwrap();

        // A request is answered by enabling it, not by turning it on.
        let agent = name("my-agent");
        record_requested(&mut db, &agent).unwrap();
        assert!(matches!(
            turn_on(&mut db, &agent),
            Err(Error::NeverEnabled(_))
        ));
        assert_eq!(access(&reader, &agent).unwrap(), ClientAccess::Requested);
        assert!(matches!(
            turn_on(&mut db, &name("ghost")),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn removing_forgets_the_client_but_not_its_log() {
        let (_dir, mut db) = db();
        let reader = db.reader().unwrap();
        let codex = name("codex");
        enable_at(&mut db, &codex, AccessLevel::Standard).unwrap();
        crate::access_log::append(
            &mut db,
            &crate::access_log::NewEntry {
                client: codex.clone(),
                tool: "list_accounts".into(),
                access_level: Some(AccessLevel::Standard),
                outcome: crate::access_log::Outcome::Ok,
                purpose: None,
                args_redacted_json: "{}".into(),
                row_count: 2,
                result_hash: Some("ab".into()),
                withheld_count: 0,
            },
        )
        .unwrap();
        remove(&mut db, &codex).unwrap();
        assert_eq!(access(&reader, &codex).unwrap(), ClientAccess::Unknown);
        assert_eq!(get(&reader, &codex).unwrap(), None);
        assert_eq!(crate::access_log::recent(&reader, 10).unwrap().len(), 1);
        assert!(matches!(remove(&mut db, &codex), Err(Error::NotFound(_))));
        // Calling again, it is a new request.
        assert!(record_requested(&mut db, &codex).unwrap());
        assert_eq!(access(&reader, &codex).unwrap(), ClientAccess::Requested);
    }

    #[test]
    fn client_names_are_checked() {
        for name in ["claude-code", "codex", "Claude.Desktop_2", &"a".repeat(64)] {
            assert_eq!(ClientName::new(name).unwrap().as_str(), name);
        }
        assert_eq!(
            ClientName::new(""),
            Err(ClientNameError::Length { max: 64 })
        );
        assert_eq!(
            ClientName::new(&"a".repeat(65)),
            Err(ClientNameError::Length { max: 64 })
        );
        for name in ["claude code", "claude\u{202e}", "é", "a/b", "a\n"] {
            assert_eq!(ClientName::new(name), Err(ClientNameError::Characters));
        }
        for name in ["coffer", "Coffer", "COFFER"] {
            assert_eq!(ClientName::new(name), Err(ClientNameError::Reserved));
        }
    }
}
