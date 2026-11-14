//! The inbox: suggestions and saved reports from AI clients (ROADMAP M2.3,
//! M2.7, M2.14; `docs/ARCHITECTURE.md` → "MCP surface").
//!
//! [`insert_suggestion`] and [`insert_report`] are the only writes an MCP
//! tool can make (`AGENTS.md` → Invariants), and they reach a tool only
//! through `coffer_mcp::store`. Nothing here changes a transaction, rule,
//! category or account when a suggestion is filed: it waits as `pending`
//! until the user approves or rejects it in the app or the CLI.
//!
//! # Filing
//!
//! - **Typed payloads.** Each [`SuggestionKind`] has one strict JSON shape
//!   ([`payload`]): parsed into a [`Payload`], its references checked against
//!   the database, and stored in canonical form. A payload that fails is
//!   refused ([`Error::Invalid`]) and nothing is written.
//! - **Caps.** A rationale is at most [`MAX_RATIONALE_CHARS`] characters, a
//!   categorisation at most [`payload::MAX_ITEMS`] items, and each source
//!   has at most [`MAX_PENDING_PER_SOURCE`] pending suggestions
//!   ([`Error::TooManyPending`]): the user clears some first. Each source
//!   has at most [`MAX_REPORTS_PER_SOURCE`] saved reports
//!   ([`Error::TooManyReports`]) until the user deletes some
//!   ([`delete_reports`], `coffer inbox delete-report`).
//! - **Once.** A pending suggestion of the same kind with the same canonical
//!   payload, from any source, is the same suggestion: filing it again
//!   returns the existing ID ([`Filed::duplicate`]) and writes nothing. The
//!   schema enforces it (`V13__inbox.sql`).
//! - **The source** is the calling client's name, set by the server, never
//!   taken from the client's arguments; Coffer's own suggestions use
//!   [`COFFER_SOURCE`] (`own`).
//!
//! Payloads, rationales and report bodies are untrusted text: stored as
//! given, stripped when shown, never logged.
//!
//! # Deciding
//!
//! Approving applies the suggestion through the existing core APIs and
//! marks it `approved` in the same DB transaction; rejecting marks it
//! `rejected` with an optional reason ([`approve`], [`reject`]).
//! [`impact`] reports what approving would change, against today's data.
//!
//! # Deleting reports
//!
//! Only the user deletes saved reports ([`delete_reports`]): by ID or by
//! client, all or nothing. No MCP tool can.

mod approve;
mod impact;
pub mod own;
pub mod payload;
pub mod preview;
mod reports;

use std::fmt;
use std::str::FromStr;

use rusqlite::types::{FromSql, FromSqlResult, ToSqlOutput, ValueRef};
use rusqlite::{Connection, OptionalExtension, ToSql, TransactionBehavior, params};

use crate::db::Database;
use crate::id::ChangeBatchId;
use crate::mcp_client::{ClientName, RESERVED_CLIENT_NAME};
use crate::rule::{UnknownKey, key_enum};

pub use approve::{
    Applied, ApproveError, ApproveOptions, Approved, approve, approve_all, reject, reject_all,
};
pub use impact::{
    AliasImpact, CategorizationImpact, Detail, Impact, LinkImpact, RuleImpact, TransferImpact,
    WIDE_MERCHANTS, impact,
};
pub use payload::{Invalid, Payload};
pub use reports::{
    ReportHeading, ReportSelection, delete_reports, report_counts, reports_to_delete,
};

/// The longest rationale, in characters.
pub const MAX_RATIONALE_CHARS: usize = 1_000;

/// How many pending suggestions one source may have at once.
pub const MAX_PENDING_PER_SOURCE: usize = 200;

/// The longest report title, in characters.
pub const MAX_REPORT_TITLE_CHARS: usize = 200;

/// The longest report period (e.g. `2026-10`, `2026-Q3`), in characters.
pub const MAX_REPORT_PERIOD_CHARS: usize = 32;

/// The longest report body, in characters.
pub const MAX_REPORT_BODY_CHARS: usize = 100_000;

/// How many saved reports one source may have. Reports are never decided
/// like suggestions, so this caps the total: the user deletes old ones in
/// Coffer ([`delete_reports`]) before the client can save more.
pub const MAX_REPORTS_PER_SOURCE: usize = 500;

/// The longest reason the user gives for a rejection, in characters (the
/// schema checks the same).
pub const MAX_REASON_CHARS: usize = 500;

/// The source of the suggestions Coffer files itself (M2.14). No MCP
/// client can have this name.
pub const COFFER_SOURCE: &str = RESERVED_CLIENT_NAME;

key_enum! {
    /// What a suggestion proposes, as `suggestion.kind` stores it.
    SuggestionKind, "suggestion kind" {
        /// Categories for transactions.
        Categorization => "categorization",
        /// A new rule.
        Rule => "rule",
        /// Two transactions that are one transfer.
        TransferPair => "transfer_pair",
        /// A display name for a merchant.
        MerchantAlias => "merchant_alias",
        /// A refund linked to its purchase.
        RefundLink => "refund_link",
    }
}

key_enum! {
    /// Where a suggestion stands, as `suggestion.status` stores it.
    SuggestionStatus, "suggestion status" {
        /// Waiting for the user.
        Pending => "pending",
        /// The user approved it and it was applied.
        Approved => "approved",
        /// The user rejected it.
        Rejected => "rejected",
    }
}

/// A `suggestion` row's ID.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct SuggestionId(i64);

impl SuggestionId {
    /// Wraps a stored ID, e.g. one the user typed. Whether it exists is the
    /// database's question.
    pub const fn new(id: i64) -> Self {
        SuggestionId(id)
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

impl fmt::Display for SuggestionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl ToSql for SuggestionId {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        self.0.to_sql()
    }
}

impl FromSql for SuggestionId {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        i64::column_result(value).map(SuggestionId)
    }
}

/// A suggestion to file as `pending`.
#[derive(Debug, Clone, PartialEq)]
pub struct NewSuggestion {
    pub kind: SuggestionKind,
    /// What to change, as JSON in the kind's shape ([`payload`]).
    pub payload: serde_json::Value,
    /// The client's reason, untrusted.
    pub rationale: Option<String>,
}

/// A review to save, shown as untrusted, AI-authored text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewReport {
    /// The period it covers, e.g. `2026-10`. Not blank.
    pub period: String,
    /// Not blank.
    pub title: String,
    /// A markdown subset, rendered without HTML, images or live links.
    pub body_markdown: String,
}

/// What filing a suggestion did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Filed {
    pub id: SuggestionId,
    /// An identical suggestion was already pending: `id` is that one, and
    /// nothing was written.
    pub duplicate: bool,
}

/// Why a suggestion or report wasn't stored or read, or a decision wasn't
/// made. Messages name IDs, keys and limits, never the text.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The payload, the rationale or the report doesn't fit (`payload`).
    #[error(transparent)]
    Invalid(#[from] Invalid),

    #[error("the rationale is longer than {max} characters", max = MAX_RATIONALE_CHARS)]
    RationaleTooLong,

    #[error(
        "there are already {max} pending suggestions from this client: the user needs to \
         review some before more can be filed",
        max = MAX_PENDING_PER_SOURCE
    )]
    TooManyPending,

    #[error(
        "this client has saved {max} reports, the most one client may keep; no more can be \
         saved until the user deletes some in Coffer (`coffer inbox delete-report`)"
    )]
    TooManyReports { max: usize },

    #[error("a report's {field} can't be blank")]
    BlankReportField { field: &'static str },

    #[error("a report's {field} is longer than {max} characters")]
    ReportTooLong { field: &'static str, max: usize },

    #[error("there is no suggestion {0}")]
    NotFound(SuggestionId),

    #[error("there is no report {0}")]
    ReportNotFound(i64),

    #[error("report {0} is named twice")]
    ReportNamedTwice(i64),

    #[error("suggestion {0} is stored with a payload that doesn't parse")]
    Corrupt(SuggestionId),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

// Filing ------------------------------------------------------------------------

/// Files a pending suggestion from `source` and returns its ID: the
/// existing one when an identical suggestion is pending (module docs).
pub fn insert_suggestion(
    db: &mut Database,
    source: &ClientName,
    suggestion: &NewSuggestion,
) -> Result<i64, Error> {
    Ok(file_suggestion(db, source, suggestion)?.id.get())
}

/// [`insert_suggestion`], saying whether it was a duplicate.
pub fn file_suggestion(
    db: &mut Database,
    source: &ClientName,
    suggestion: &NewSuggestion,
) -> Result<Filed, Error> {
    let payload = Payload::parse(suggestion.kind, &suggestion.payload)?;
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let filed = file_in(
        &tx,
        source.as_str(),
        &payload,
        suggestion.rationale.as_deref(),
    )?;
    tx.commit()?;
    Ok(filed)
}

/// Files `payload` from `source` inside the caller's DB transaction:
/// checks its references, the rationale and the source's pending cap, and
/// returns the existing ID for an identical pending suggestion.
pub(crate) fn file_in(
    conn: &Connection,
    source: &str,
    payload: &Payload,
    rationale: Option<&str>,
) -> Result<Filed, Error> {
    if rationale.is_some_and(|text| text.chars().count() > MAX_RATIONALE_CHARS) {
        return Err(Error::RationaleTooLong);
    }
    payload.check(conn)?;
    let kind = payload.kind();
    let json = payload.canonical_json();
    if let Some(id) = pending_identical(conn, kind, &json)? {
        return Ok(Filed {
            id,
            duplicate: true,
        });
    }
    let pending: i64 = conn.query_row(
        "SELECT COUNT(*) FROM suggestion WHERE source_client = ?1 AND status = 'pending'",
        [source],
        |row| row.get(0),
    )?;
    if usize::try_from(pending).unwrap_or(usize::MAX) >= MAX_PENDING_PER_SOURCE {
        return Err(Error::TooManyPending);
    }
    conn.execute(
        "INSERT INTO suggestion (kind, payload_json, rationale, source_client) \
         VALUES (?1, ?2, ?3, ?4)",
        params![kind.as_str(), json, rationale, source],
    )?;
    let id = SuggestionId::new(conn.last_insert_rowid());
    tracing::debug!(suggestion = %id, kind = kind.as_str(), "filed a suggestion");
    Ok(Filed {
        id,
        duplicate: false,
    })
}

/// The pending suggestion of `kind` with canonical payload `json`, if any.
pub(crate) fn pending_identical(
    conn: &Connection,
    kind: SuggestionKind,
    json: &str,
) -> rusqlite::Result<Option<SuggestionId>> {
    conn.query_row(
        "SELECT id FROM suggestion WHERE status = 'pending' AND kind = ?1 AND payload_json = ?2",
        params![kind.as_str(), json],
        |row| row.get(0),
    )
    .optional()
}

/// Saves a report from `source` and returns its ID. A source that already
/// has [`MAX_REPORTS_PER_SOURCE`] reports is refused
/// ([`Error::TooManyReports`]) and nothing is written.
pub fn insert_report(
    db: &mut Database,
    source: &ClientName,
    report: &NewReport,
) -> Result<i64, Error> {
    for (field, text, max) in [
        ("period", &report.period, MAX_REPORT_PERIOD_CHARS),
        ("title", &report.title, MAX_REPORT_TITLE_CHARS),
        ("body", &report.body_markdown, MAX_REPORT_BODY_CHARS),
    ] {
        if field != "body" && text.trim().is_empty() {
            return Err(Error::BlankReportField { field });
        }
        if text.chars().count() > max {
            return Err(Error::ReportTooLong { field, max });
        }
    }
    // Count and insert in one write transaction, so two servers for the
    // same client can't both pass the cap.
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let saved: i64 = tx.query_row(
        "SELECT COUNT(*) FROM report WHERE source_client = ?1",
        [source.as_str()],
        |row| row.get(0),
    )?;
    if usize::try_from(saved).unwrap_or(usize::MAX) >= MAX_REPORTS_PER_SOURCE {
        return Err(Error::TooManyReports {
            max: MAX_REPORTS_PER_SOURCE,
        });
    }
    tx.execute(
        "INSERT INTO report (period, title, body_markdown, source_client) \
         VALUES (?1, ?2, ?3, ?4)",
        params![
            report.period,
            report.title,
            report.body_markdown,
            source.as_str(),
        ],
    )?;
    let id = tx.last_insert_rowid();
    tx.commit()?;
    Ok(id)
}

// Reading -----------------------------------------------------------------------

/// A stored suggestion.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub id: SuggestionId,
    pub kind: SuggestionKind,
    pub payload: Payload,
    /// The client's reason: untrusted, sanitise before showing.
    pub rationale: Option<String>,
    /// The client's name, or [`COFFER_SOURCE`].
    pub source_client: String,
    pub created_at: String,
    pub status: SuggestionStatus,
    pub decided_at: Option<String>,
    /// The user's reason for a rejection.
    pub decision_reason: Option<String>,
    /// The change batch an approval recorded, or the rule batch that
    /// settled it.
    pub change_batch: Option<ChangeBatchId>,
    /// What else an approval made, by ID (`suggestion.applied_json`).
    pub applied: Option<serde_json::Value>,
}

/// Which suggestions [`list`] returns. The default is all of them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Filter {
    pub status: Option<SuggestionStatus>,
    pub kind: Option<SuggestionKind>,
}

const COLUMNS: &str = "id, kind, payload_json, rationale, source_client, created_at, status, \
     decided_at, decision_reason, change_batch_id, applied_json";

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<Suggestion, Error>> {
    let id: SuggestionId = row.get("id")?;
    let kind: String = row.get("kind")?;
    let status: String = row.get("status")?;
    let payload: String = row.get("payload_json")?;
    let applied: Option<String> = row.get("applied_json")?;
    let (Ok(kind), Ok(status)) = (
        SuggestionKind::from_str(&kind),
        SuggestionStatus::from_str(&status),
    ) else {
        return Ok(Err(Error::Corrupt(id)));
    };
    let Ok(payload) = Payload::from_stored(kind, &payload) else {
        return Ok(Err(Error::Corrupt(id)));
    };
    let applied = match applied.map(|json| serde_json::from_str(&json)).transpose() {
        Ok(applied) => applied,
        Err(_) => return Ok(Err(Error::Corrupt(id))),
    };
    Ok(Ok(Suggestion {
        id,
        kind,
        payload,
        rationale: row.get("rationale")?,
        source_client: row.get("source_client")?,
        created_at: row.get("created_at")?,
        status,
        decided_at: row.get("decided_at")?,
        decision_reason: row.get("decision_reason")?,
        change_batch: row.get("change_batch_id")?,
        applied,
    }))
}

/// Suggestion `id`.
pub fn get(db: &Database, id: SuggestionId) -> Result<Suggestion, Error> {
    get_in(db.conn(), id)
}

/// [`get`] on any connection (inside a DB transaction).
pub(crate) fn get_in(conn: &Connection, id: SuggestionId) -> Result<Suggestion, Error> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM suggestion WHERE id = ?1"),
        [id],
        read_row,
    )
    .optional()?
    .ok_or(Error::NotFound(id))?
}

/// The suggestions `filter` selects, oldest first.
pub fn list(db: &Database, filter: &Filter) -> Result<Vec<Suggestion>, Error> {
    let mut statement = db.conn().prepare(&format!(
        "SELECT {COLUMNS} FROM suggestion \
         WHERE (?1 IS NULL OR status = ?1) AND (?2 IS NULL OR kind = ?2) ORDER BY id"
    ))?;
    let rows = statement
        .query_map(
            params![
                filter.status.map(SuggestionStatus::as_str),
                filter.kind.map(SuggestionKind::as_str)
            ],
            read_row,
        )?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter().collect()
}

/// How many suggestions the Inbox's left panel counts: by status, and the
/// pending ones by kind and by source.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts {
    pub pending: usize,
    pub approved: usize,
    pub rejected: usize,
    /// Pending suggestions by kind; kinds with none are left out.
    pub pending_by_kind: std::collections::BTreeMap<SuggestionKind, usize>,
    /// Suggestions by source (a client's name or [`COFFER_SOURCE`]) with
    /// how many are pending: every source that filed one, any status.
    pub by_source: std::collections::BTreeMap<String, usize>,
}

impl Counts {
    /// Suggestions with `status`.
    pub fn with_status(&self, status: SuggestionStatus) -> usize {
        match status {
            SuggestionStatus::Pending => self.pending,
            SuggestionStatus::Approved => self.approved,
            SuggestionStatus::Rejected => self.rejected,
        }
    }
}

/// The Inbox's counts ([`Counts`]).
pub fn counts(db: &Database) -> Result<Counts, Error> {
    let mut counts = Counts::default();
    let mut statement = db.conn().prepare(
        "SELECT status, kind, source_client, COUNT(*) FROM suggestion \
         GROUP BY status, kind, source_client",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let status: String = row.get(0)?;
        let kind: String = row.get(1)?;
        let source: String = row.get(2)?;
        let count = usize::try_from(row.get::<_, i64>(3)?).unwrap_or(0);
        let (Ok(status), Ok(kind)) = (
            SuggestionStatus::from_str(&status),
            SuggestionKind::from_str(&kind),
        ) else {
            continue;
        };
        let pending = counts.by_source.entry(source).or_default();
        match status {
            SuggestionStatus::Pending => {
                counts.pending += count;
                *pending += count;
                *counts.pending_by_kind.entry(kind).or_default() += count;
            }
            SuggestionStatus::Approved => counts.approved += count,
            SuggestionStatus::Rejected => counts.rejected += count,
        }
    }
    Ok(counts)
}

#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;
