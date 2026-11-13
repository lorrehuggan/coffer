//! MCP clients and their access levels (ROADMAP M2.11; `docs/ARCHITECTURE.md`
//! → "MCP surface" and "Decision record: Settings, AI access and backups
//! (2026-11-10)").
//!
//! A client is the name `coffer mcp --client <name>` was started with
//! ([`ClientName`]); `mcp_client` (V11) records whether the user enabled it
//! and at which [`AccessLevel`]. The MCP server registers only the tools at
//! or below a client's level.
//!
//! Wave 0 reads a client's level and nothing else. Block B (access control)
//! adds the rest here: recording a requested client, enabling, ignoring and
//! changing the level from the CLI and the app, and the kill switch
//! ([`crate::setting`]).

use std::fmt;
use std::str::FromStr;

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef};
use rusqlite::{OptionalExtension, ToSql};

use crate::db::Reader;
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

/// Why reading a client failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// The access level stored for `client`, `None` when it has no row.
///
/// Wave 0 reads the level whether or not the client is enabled. Block B
/// replaces this with the full policy (enabled, requested, ignored).
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
