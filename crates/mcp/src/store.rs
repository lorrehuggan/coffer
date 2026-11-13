//! The MCP server's only writer (`AGENTS.md` → Invariants: MCP writes go to
//! the inbox only).
//!
//! [`Store`] owns the server's one read-write connection and exposes four
//! inserts and nothing else:
//!
//! - a suggestion and a report, through [`Inbox`], which only a tool
//!   registered with [`Writes::Inbox`](crate::registry::Writes::Inbox)
//!   gets. The source is always the server's client name, never an
//!   argument;
//! - an access-log row, which only the server's call wrapper writes
//!   ([`crate::server`]);
//! - the server's own client as `requested`, when it has no `mcp_client`
//!   row yet ([`crate::access::for_request`]). It never enables a client or
//!   changes a level.
//!
//! Every query runs on the separate read-only [`Reader`](coffer_core::db::Reader).

use std::sync::{Mutex, MutexGuard};

use coffer_core::access_log::{self, NewEntry};
use coffer_core::db::Database;
use coffer_core::inbox::{self, Filed, NewReport, NewSuggestion};
use coffer_core::mcp_client::{self, ClientName};

use crate::registry::ToolError;

/// The server's read-write connection, behind exactly four inserts.
pub struct Store {
    db: Mutex<Database>,
    client: ClientName,
}

/// Why the access log or the client's request couldn't be written.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Log(#[from] access_log::Error),

    #[error(transparent)]
    Client(#[from] mcp_client::Error),

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

    /// The client this server runs for.
    pub(crate) fn client(&self) -> &ClientName {
        &self.client
    }

    /// Records this server's client as `requested` if it has no row yet,
    /// and returns whether a row was created. The name is always the
    /// server's own, never an argument. For the access policy only
    /// ([`crate::access::for_request`]).
    pub(crate) fn record_requested(&self) -> Result<bool, StoreError> {
        Ok(mcp_client::record_requested(
            &mut *self.lock()?,
            &self.client,
        )?)
    }
}

/// The inbox writes a tool may make: a pending suggestion or a saved
/// report, from the calling client.
pub struct Inbox<'a> {
    store: &'a Store,
}

impl Inbox<'_> {
    /// Files a pending suggestion and returns its ID: the existing one when
    /// an identical suggestion is already pending.
    pub fn suggest(&self, suggestion: &NewSuggestion) -> Result<i64, ToolError> {
        Ok(self.file(suggestion)?.id.get())
    }

    /// Files a pending suggestion, saying whether an identical one was
    /// already pending. A payload core refuses is
    /// [`ToolError::InvalidArguments`], with core's message (IDs, keys and
    /// limits, never the client's text), and nothing is written.
    pub fn file(&self, suggestion: &NewSuggestion) -> Result<Filed, ToolError> {
        let mut db = self
            .store
            .lock()
            .map_err(|_| ToolError::Internal("the database connection is unusable"))?;
        inbox::file_suggestion(&mut db, &self.store.client, suggestion).map_err(refusal)
    }

    /// Saves a report and returns its ID.
    pub fn save_report(&self, report: &NewReport) -> Result<i64, ToolError> {
        let mut db = self
            .store
            .lock()
            .map_err(|_| ToolError::Internal("the database connection is unusable"))?;
        inbox::insert_report(&mut db, &self.store.client, report).map_err(refusal)
    }
}

/// What the client is told when core refuses an inbox write: why, for
/// what the client sent; the generic error for anything else.
fn refusal(error: inbox::Error) -> ToolError {
    match error {
        inbox::Error::Invalid(_)
        | inbox::Error::RationaleTooLong
        | inbox::Error::TooManyPending
        | inbox::Error::TooManyReports { .. }
        | inbox::Error::BlankReportField { .. }
        | inbox::Error::ReportTooLong { .. } => ToolError::InvalidArguments(error.to_string()),
        error => ToolError::core(error),
    }
}
