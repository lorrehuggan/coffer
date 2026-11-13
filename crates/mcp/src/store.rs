//! The MCP server's only writer (`AGENTS.md` → Invariants: MCP writes go to
//! the inbox only).
//!
//! [`Store`] owns the server's one read-write connection and exposes three
//! inserts and nothing else:
//!
//! - a suggestion and a report, through [`Inbox`], which only a tool
//!   registered with [`Writes::Inbox`](crate::registry::Writes::Inbox)
//!   gets. The source is always the server's client name, never an
//!   argument;
//! - an access-log row, which only the server's call wrapper writes
//!   ([`crate::server`]).
//!
//! Every query runs on the separate read-only [`Reader`](coffer_core::db::Reader).

use std::sync::{Mutex, MutexGuard};

use coffer_core::access_log::{self, NewEntry};
use coffer_core::db::Database;
use coffer_core::inbox::{self, NewReport, NewSuggestion};
use coffer_core::mcp_client::ClientName;

use crate::registry::ToolError;

/// The server's read-write connection, behind exactly three inserts.
pub struct Store {
    db: Mutex<Database>,
    client: ClientName,
}

/// Why the access log couldn't be written.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Log(#[from] access_log::Error),

    #[error("the database connection is unusable after a panic")]
    Poisoned,
}

impl Store {
    pub(crate) fn new(db: Database, client: ClientName) -> Self {
        Store {
            db: Mutex::new(db),
            client,
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, Database>, StoreError> {
        self.db.lock().map_err(|_| StoreError::Poisoned)
    }

    /// Appends a call to the access log. For the server's wrapper only.
    pub(crate) fn log(&self, entry: &NewEntry) -> Result<i64, StoreError> {
        Ok(access_log::append(&mut *self.lock()?, entry)?)
    }

    /// The inbox, filed as this server's client.
    pub(crate) fn inbox(&self) -> Inbox<'_> {
        Inbox { store: self }
    }

    // Block B: recording a requested client (`mcp_client`, status
    // `requested`) and `last_seen_at` are the server's own records, like
    // the log. If B writes them, they go here as crate-private methods,
    // never on `Inbox`.
}

/// The inbox writes a tool may make: a pending suggestion or a saved
/// report, from the calling client.
pub struct Inbox<'a> {
    store: &'a Store,
}

impl Inbox<'_> {
    /// Files a pending suggestion and returns its ID.
    pub fn suggest(&self, suggestion: &NewSuggestion) -> Result<i64, ToolError> {
        let mut db = self
            .store
            .lock()
            .map_err(|_| ToolError::Internal("the database connection is unusable"))?;
        inbox::insert_suggestion(&mut db, &self.store.client, suggestion).map_err(ToolError::core)
    }

    /// Saves a report and returns its ID.
    pub fn save_report(&self, report: &NewReport) -> Result<i64, ToolError> {
        let mut db = self
            .store
            .lock()
            .map_err(|_| ToolError::Internal("the database connection is unusable"))?;
        inbox::insert_report(&mut db, &self.store.client, report).map_err(ToolError::core)
    }
}
