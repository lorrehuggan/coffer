//! Deleting saved reports: the user's way to make room under a client's
//! [`MAX_REPORTS_PER_SOURCE`](super::MAX_REPORTS_PER_SOURCE).
//!
//! Only the user deletes reports, in the CLI (`coffer inbox delete-report`)
//! or later the app. No MCP tool reaches this module: `coffer_mcp::store`
//! exposes inserts only (`AGENTS.md` → Invariants).
//!
//! - [`reports_to_delete`] says what a [`ReportSelection`] would delete,
//!   changing nothing; [`delete_reports`] deletes it, all or nothing, in
//!   one DB transaction, and returns what it deleted.
//! - An unknown or repeated ID refuses the whole selection.
//! - Nothing else refers to a report by ID: no table has a foreign key to
//!   `report`, suggestions and change batches never name one, and the
//!   access log keeps only a hash of `save_report`'s result. `report.id` is
//!   `AUTOINCREMENT`, so a deleted report's ID is never given to a new one,
//!   and an ID a client or the log remembers can't come to mean another
//!   report.
//!
//! Titles and periods are an AI client's words: returned to the caller to
//! show (stripped), never logged.

use std::collections::BTreeSet;

use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};

use super::Error;
use crate::db::Database;
use crate::mcp_client::ClientName;

/// Which saved reports to delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportSelection {
    /// These reports, in this order. Each must exist and be named once.
    Ids(Vec<i64>),
    /// Every report `source` saved or, with `before`, those saved before
    /// that day began (UTC, as `created_at` is stored). None is not an
    /// error: the selection is empty.
    Source {
        source: ClientName,
        before: Option<NaiveDate>,
    },
}

/// A saved report without its body: what deleting it shows. `period` and
/// `title` are the client's words, untrusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportHeading {
    pub id: i64,
    pub period: String,
    pub title: String,
    /// The client that saved it.
    pub source_client: String,
    /// When it was saved: a UTC timestamp, `YYYY-MM-DDTHH:MM:SS.sssZ`.
    pub created_at: String,
}

/// The reports `selection` names, as [`delete_reports`] would delete them.
/// Writes nothing.
pub fn reports_to_delete(
    db: &Database,
    selection: &ReportSelection,
) -> Result<Vec<ReportHeading>, Error> {
    select(db.conn(), selection)
}

/// Deletes the reports `selection` names, all of them or none (an unknown
/// or repeated ID refuses them all), in one DB transaction, and returns
/// what it deleted. Deleting can't be undone.
pub fn delete_reports(
    db: &mut Database,
    selection: &ReportSelection,
) -> Result<Vec<ReportHeading>, Error> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let reports = select(&tx, selection)?;
    {
        let mut delete = tx.prepare_cached("DELETE FROM report WHERE id = ?1")?;
        for report in &reports {
            delete.execute([report.id])?;
        }
    }
    tx.commit()?;
    tracing::debug!(count = reports.len(), "deleted saved reports");
    Ok(reports)
}

/// How many reports each source has saved, by source name.
pub fn report_counts(db: &Database) -> Result<Vec<(String, usize)>, Error> {
    let mut statement = db.conn().prepare_cached(
        "SELECT source_client, COUNT(*) FROM report GROUP BY source_client ORDER BY source_client",
    )?;
    let counts = statement
        .query_map([], |row| {
            let count: i64 = row.get(1)?;
            Ok((row.get(0)?, usize::try_from(count).unwrap_or(usize::MAX)))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(counts)
}

const COLUMNS: &str = "id, period, title, source_client, created_at";

fn heading(row: &Row<'_>) -> rusqlite::Result<ReportHeading> {
    Ok(ReportHeading {
        id: row.get(0)?,
        period: row.get(1)?,
        title: row.get(2)?,
        source_client: row.get(3)?,
        created_at: row.get(4)?,
    })
}

fn select(conn: &Connection, selection: &ReportSelection) -> Result<Vec<ReportHeading>, Error> {
    match selection {
        ReportSelection::Ids(ids) => {
            let mut seen = BTreeSet::new();
            let mut statement =
                conn.prepare_cached(&format!("SELECT {COLUMNS} FROM report WHERE id = ?1"))?;
            let mut reports = Vec::with_capacity(ids.len());
            for &id in ids {
                if !seen.insert(id) {
                    return Err(Error::ReportNamedTwice(id));
                }
                let report = statement
                    .query_row([id], heading)
                    .optional()?
                    .ok_or(Error::ReportNotFound(id))?;
                reports.push(report);
            }
            Ok(reports)
        }
        ReportSelection::Source { source, before } => {
            let before = before.map(|day| format!("{}T00:00:00.000Z", day.format("%Y-%m-%d")));
            let mut statement = conn.prepare_cached(&format!(
                "SELECT {COLUMNS} FROM report \
                 WHERE source_client = ?1 AND (?2 IS NULL OR created_at < ?2) \
                 ORDER BY created_at, id"
            ))?;
            let reports = statement
                .query_map(params![source.as_str(), before], heading)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(reports)
        }
    }
}

#[cfg(test)]
#[path = "reports_tests.rs"]
mod tests;
