//! The AI access log (ROADMAP M2.12; `docs/ARCHITECTURE.md` → "MCP surface",
//! "Access log").
//!
//! The MCP server appends one `mcp_access_log` row (V11) per tool call as
//! its own audit record: who called which tool, at which access level, with
//! what outcome, the client's stated purpose, the arguments with free text
//! redacted, how many rows went out, a hash of the result and how many
//! descriptions were withheld. Never the returned data. This isn't a tool
//! write (`AGENTS.md` → Invariants): no tool can reach it, and the client's
//! only input is the untrusted `purpose` string.
//!
//! Rows are only appended (and later deleted when they expire, Block B);
//! the schema refuses edits.

use std::fmt;
use std::str::FromStr;

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef};
use rusqlite::{ToSql, params};

use crate::db::{Database, Reader};
use crate::mcp_client::{AccessLevel, ClientName};
use crate::rule::{UnknownKey, key_enum};

/// The longest `purpose` stored, in characters. A longer one is cut:
/// refusing it would leave the call unlogged.
pub const MAX_PURPOSE_LEN: usize = 500;

key_enum! {
    /// How a call ended, as `mcp_access_log.outcome` stores it.
    Outcome, "access log outcome" {
        /// The tool ran and returned its result.
        Ok => "ok",
        /// Refused: MCP access is switched off (the kill switch).
        RefusedOff => "refused_off",
        /// Refused: the client isn't enabled.
        RefusedNotEnabled => "refused_not_enabled",
        /// Refused: the tool is above the client's access level.
        RefusedLevel => "refused_level",
        /// The tool failed and returned an error, no data.
        Error => "error",
    }
}

impl ToSql for Outcome {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(ValueRef::Text(
            self.as_str().as_bytes(),
        )))
    }
}

impl FromSql for Outcome {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Outcome> {
        value
            .as_str()?
            .parse()
            .map_err(|error| FromSqlError::Other(Box::new(error)))
    }
}

/// A call to record. A refused or failed call has no rows, hash or
/// withheld count (the schema checks it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEntry {
    pub client: ClientName,
    /// The registered tool's name.
    pub tool: String,
    /// The client's level at the time of the call, `None` when it has none.
    pub access_level: Option<AccessLevel>,
    pub outcome: Outcome,
    /// The client's stated purpose, untrusted; cut to [`MAX_PURPOSE_LEN`].
    pub purpose: Option<String>,
    /// The arguments as JSON, free text already redacted.
    pub args_redacted_json: String,
    pub row_count: u64,
    pub result_hash: Option<String>,
    pub withheld_count: u64,
}

/// A recorded call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggedCall {
    pub id: i64,
    /// UTC, `YYYY-MM-DDTHH:MM:SS.SSSZ`.
    pub at: String,
    pub client: String,
    pub tool: String,
    pub access_level: Option<AccessLevel>,
    pub outcome: Outcome,
    pub purpose: Option<String>,
    pub args_redacted_json: String,
    pub row_count: u64,
    pub result_hash: Option<String>,
    pub withheld_count: u64,
}

/// Why appending or reading the log failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("a count is too large to store")]
    CountTooLarge,

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// Appends a call to the log and returns its row ID.
pub fn append(db: &mut Database, entry: &NewEntry) -> Result<i64, Error> {
    let count = |n: u64| i64::try_from(n).map_err(|_| Error::CountTooLarge);
    let purpose = entry
        .purpose
        .as_deref()
        .map(|purpose| purpose.chars().take(MAX_PURPOSE_LEN).collect::<String>());
    db.conn().execute(
        "INSERT INTO mcp_access_log (client, tool, access_level, outcome, purpose, \
         args_redacted_json, row_count, result_hash, withheld_count) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            entry.client.as_str(),
            entry.tool,
            entry.access_level,
            entry.outcome,
            purpose,
            entry.args_redacted_json,
            count(entry.row_count)?,
            entry.result_hash,
            count(entry.withheld_count)?,
        ],
    )?;
    Ok(db.conn().last_insert_rowid())
}

/// The latest `limit` calls, newest first.
pub fn recent(reader: &Reader, limit: u32) -> Result<Vec<LoggedCall>, Error> {
    let mut statement = reader.conn().prepare(
        "SELECT id, at, client, tool, access_level, outcome, purpose, args_redacted_json, \
         row_count, result_hash, withheld_count \
         FROM mcp_access_log ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = statement.query_map([limit], |row| {
        Ok(LoggedCall {
            id: row.get(0)?,
            at: row.get(1)?,
            client: row.get(2)?,
            tool: row.get(3)?,
            access_level: row.get(4)?,
            outcome: row.get(5)?,
            purpose: row.get(6)?,
            args_redacted_json: row.get(7)?,
            row_count: row.get::<_, i64>(8)?.unsigned_abs(),
            result_hash: row.get(9)?,
            withheld_count: row.get::<_, i64>(10)?.unsigned_abs(),
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn entry(outcome: Outcome) -> NewEntry {
        NewEntry {
            client: ClientName::new("claude-code").unwrap(),
            tool: "list_categories".to_owned(),
            access_level: Some(AccessLevel::Aggregate),
            outcome,
            purpose: None,
            args_redacted_json: "{}".to_owned(),
            row_count: 0,
            result_hash: None,
            withheld_count: 0,
        }
    }

    #[test]
    fn appends_and_reads_back() {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
        let ok = NewEntry {
            purpose: Some("x".repeat(MAX_PURPOSE_LEN + 10)),
            row_count: 12,
            result_hash: Some("ab12".to_owned()),
            ..entry(Outcome::Ok)
        };
        let first = append(&mut db, &ok).unwrap();
        let second = append(&mut db, &entry(Outcome::RefusedLevel)).unwrap();
        assert!(second > first);

        let calls = recent(&db.reader().unwrap(), 10).unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].outcome, Outcome::RefusedLevel);
        let logged = &calls[1];
        assert_eq!(logged.client, "claude-code");
        assert_eq!(logged.tool, "list_categories");
        assert_eq!(logged.access_level, Some(AccessLevel::Aggregate));
        assert_eq!(logged.row_count, 12);
        assert_eq!(logged.result_hash.as_deref(), Some("ab12"));
        assert_eq!(
            logged.purpose.as_ref().unwrap().chars().count(),
            MAX_PURPOSE_LEN
        );
        assert_eq!(logged.at.len(), 24, "{}", logged.at);
    }

    #[test]
    fn a_refused_call_carries_no_result() {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
        for refused in [
            NewEntry {
                row_count: 1,
                ..entry(Outcome::RefusedOff)
            },
            NewEntry {
                result_hash: Some("ab12".to_owned()),
                ..entry(Outcome::Error)
            },
            NewEntry {
                withheld_count: 1,
                ..entry(Outcome::RefusedNotEnabled)
            },
        ] {
            assert!(append(&mut db, &refused).is_err(), "{refused:?}");
        }
    }

    #[test]
    fn rows_are_read_only() {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
        let id = append(&mut db, &entry(Outcome::Ok)).unwrap();
        assert!(
            db.conn()
                .execute(
                    "UPDATE mcp_access_log SET outcome = 'error' WHERE id = ?1",
                    [id]
                )
                .is_err()
        );
        // Expired rows are deleted.
        db.conn()
            .execute("DELETE FROM mcp_access_log WHERE id = ?1", [id])
            .unwrap();
    }
}
