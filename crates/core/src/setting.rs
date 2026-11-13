//! App-wide settings in the `setting` table (V11): the MCP kill switch and
//! the per-database key for merchant pseudonyms, which is never exposed
//! (ROADMAP M2.11; `docs/ARCHITECTURE.md` → "Decision record: Settings, AI
//! access and backups (2026-11-10)").
//!
//! Each key has its own typed functions; the table stores text and core
//! parses and checks it. Reads take a [`Reader`], so the MCP server can read
//! the kill switch on every call. Writes take `&mut Database`, which only the
//! app and the CLI hold: the server never writes a setting.
//!
//! - [`MCP_ENABLED`]: the kill switch. A missing row means on: MCP access is
//!   still off until the user enables a client ([`crate::mcp_client`]), and
//!   switching it off refuses every client's next call.
//! - [`PSEUDONYM_KEY`]: 32 random bytes, hex in the table, for merchant
//!   tokens at `pseudonymous` ([`PseudonymKey`]). Created by
//!   [`ensure_pseudonym_key`] when a client is first enabled; never shown,
//!   logged or sent anywhere.

use std::fmt;

use rusqlite::{OptionalExtension, params};

use crate::db::{Database, Reader};

/// The kill switch: `"on"` or `"off"`. Missing means on.
pub const MCP_ENABLED: &str = "mcp.enabled";

/// The per-database key for merchant pseudonyms: 64 lowercase hex digits.
pub const PSEUDONYM_KEY: &str = "mcp.pseudonym_key";

/// The pseudonym key's length in bytes.
pub const PSEUDONYM_KEY_LEN: usize = 32;

/// Why reading or writing a setting failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The stored value isn't one the key allows. The message names the
    /// key, never the value (the pseudonym key is secret).
    #[error("the stored value of setting `{key}` isn't valid")]
    Invalid { key: &'static str },

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// Whether MCP access is switched on (the kill switch). On when it was
/// never set. Read on every MCP call.
pub fn mcp_enabled(reader: &Reader) -> Result<bool, Error> {
    match get(reader.conn(), MCP_ENABLED)?.as_deref() {
        None | Some("on") => Ok(true),
        Some("off") => Ok(false),
        Some(_) => Err(Error::Invalid { key: MCP_ENABLED }),
    }
}

/// Switches MCP access on or off for every client. The next call from a
/// client that is already connected sees it.
pub fn set_mcp_enabled(db: &mut Database, on: bool) -> Result<(), Error> {
    set(db.conn(), MCP_ENABLED, if on { "on" } else { "off" })
}

/// The key for merchant pseudonyms. Its bytes never leave core: `Debug`
/// doesn't print them, and there is no `Display`, `Serialize` or accessor
/// outside this crate.
#[derive(Clone, PartialEq, Eq)]
pub struct PseudonymKey([u8; PSEUDONYM_KEY_LEN]);

impl PseudonymKey {
    /// The key's bytes, for the keyed hash in [`crate::envelope`].
    pub(crate) fn bytes(&self) -> &[u8; PSEUDONYM_KEY_LEN] {
        &self.0
    }

    fn parse(hex: &str) -> Option<PseudonymKey> {
        if hex.len() != PSEUDONYM_KEY_LEN * 2 {
            return None;
        }
        let mut bytes = [0; PSEUDONYM_KEY_LEN];
        let (pairs, _) = hex.as_bytes().as_chunks::<2>();
        for (byte, pair) in bytes.iter_mut().zip(pairs) {
            let pair = std::str::from_utf8(pair).ok()?;
            *byte = u8::from_str_radix(pair, 16).ok()?;
        }
        Some(PseudonymKey(bytes))
    }
}

impl fmt::Debug for PseudonymKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PseudonymKey(..)")
    }
}

/// The pseudonym key, `None` until a client has been enabled.
pub fn pseudonym_key(reader: &Reader) -> Result<Option<PseudonymKey>, Error> {
    get(reader.conn(), PSEUDONYM_KEY)?
        .map(|hex| PseudonymKey::parse(&hex).ok_or(Error::Invalid { key: PSEUDONYM_KEY }))
        .transpose()
}

/// Creates the pseudonym key if there is none; an existing key is kept, so
/// tokens stay stable. [`crate::mcp_client::enable`] calls it.
///
/// The bytes come from SQLite's `randomblob`, which draws on the operating
/// system's entropy: no new dependency for one random key.
pub fn ensure_pseudonym_key(db: &mut Database) -> Result<(), Error> {
    ensure_pseudonym_key_on(db.conn())
}

pub(crate) fn ensure_pseudonym_key_on(conn: &rusqlite::Connection) -> Result<(), Error> {
    conn.execute(
        "INSERT INTO setting (key, value) \
         VALUES (?1, lower(hex(randomblob(?2)))) \
         ON CONFLICT (key) DO NOTHING",
        params![PSEUDONYM_KEY, PSEUDONYM_KEY_LEN as i64],
    )?;
    Ok(())
}

fn get(conn: &rusqlite::Connection, key: &str) -> Result<Option<String>, Error> {
    Ok(conn
        .query_row("SELECT value FROM setting WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?)
}

fn set(conn: &rusqlite::Connection, key: &str, value: &str) -> Result<(), Error> {
    conn.execute(
        "INSERT INTO setting (key, value) VALUES (?1, ?2) \
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn db() -> (TempDir, Database) {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("coffer.db")).unwrap();
        (dir, db)
    }

    #[test]
    fn the_kill_switch_defaults_to_on() {
        let (_dir, mut db) = db();
        let reader = db.reader().unwrap();
        assert!(mcp_enabled(&reader).unwrap());
        set_mcp_enabled(&mut db, false).unwrap();
        assert!(!mcp_enabled(&reader).unwrap());
        set_mcp_enabled(&mut db, true).unwrap();
        assert!(mcp_enabled(&reader).unwrap());
    }

    #[test]
    fn a_bad_kill_switch_value_is_an_error() {
        let (_dir, db) = db();
        set(db.conn(), MCP_ENABLED, "maybe").unwrap();
        assert!(matches!(
            mcp_enabled(&db.reader().unwrap()),
            Err(Error::Invalid { key: MCP_ENABLED })
        ));
    }

    #[test]
    fn the_pseudonym_key_is_created_once_and_kept() {
        let (_dir, mut db) = db();
        let reader = db.reader().unwrap();
        assert_eq!(pseudonym_key(&reader).unwrap(), None);
        ensure_pseudonym_key(&mut db).unwrap();
        let key = pseudonym_key(&reader).unwrap().unwrap();
        assert_ne!(key.bytes(), &[0; PSEUDONYM_KEY_LEN]);
        ensure_pseudonym_key(&mut db).unwrap();
        assert_eq!(pseudonym_key(&reader).unwrap(), Some(key.clone()));
        assert_eq!(format!("{key:?}"), "PseudonymKey(..)");
    }

    #[test]
    fn keys_differ_between_databases() {
        let (_a, mut a) = db();
        let (_b, mut b) = db();
        ensure_pseudonym_key(&mut a).unwrap();
        ensure_pseudonym_key(&mut b).unwrap();
        assert_ne!(
            pseudonym_key(&a.reader().unwrap()).unwrap(),
            pseudonym_key(&b.reader().unwrap()).unwrap()
        );
    }

    #[test]
    fn a_malformed_key_is_an_error() {
        let (_dir, db) = db();
        set(db.conn(), PSEUDONYM_KEY, "not hex").unwrap();
        assert!(matches!(
            pseudonym_key(&db.reader().unwrap()),
            Err(Error::Invalid { key: PSEUDONYM_KEY })
        ));
    }
}
