//! The inbox: suggestions and saved reports from AI clients (ROADMAP M2.3,
//! M2.7; `docs/ARCHITECTURE.md` → "MCP surface").
//!
//! These two inserts are the only writes an MCP tool can make (`AGENTS.md`
//! → Invariants), and they reach a tool only through `coffer_mcp::store`.
//! Nothing here changes a transaction, rule, category or account: a
//! suggestion waits as `pending` until the user approves or rejects it in
//! the app or the CLI.
//!
//! The source is always the calling client's name, set by the server, never
//! taken from the client's arguments. Payloads, rationales and report
//! bodies are untrusted text, stored as given and stripped when shown.
//!
//! Wave 0 has the bare inserts. Block E adds the typed payload per kind
//! (and checks it here, before the insert), the app's own suggestions
//! (source `coffer`), and approve and reject.

use std::fmt;
use std::str::FromStr;

use rusqlite::params;

use crate::db::Database;
use crate::mcp_client::ClientName;
use crate::rule::{UnknownKey, key_enum};

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

/// A suggestion to file as `pending`.
#[derive(Debug, Clone, PartialEq)]
pub struct NewSuggestion {
    pub kind: SuggestionKind,
    /// What to change, as JSON. Block E checks its shape per kind.
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

/// Why a suggestion or report wasn't stored.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// Files a pending suggestion from `source` and returns its ID.
pub fn insert_suggestion(
    db: &mut Database,
    source: &ClientName,
    suggestion: &NewSuggestion,
) -> Result<i64, Error> {
    db.conn().execute(
        "INSERT INTO suggestion (kind, payload_json, rationale, source_client) \
         VALUES (?1, ?2, ?3, ?4)",
        params![
            suggestion.kind.as_str(),
            suggestion.payload.to_string(),
            suggestion.rationale,
            source.as_str(),
        ],
    )?;
    Ok(db.conn().last_insert_rowid())
}

/// Saves a report from `source` and returns its ID.
pub fn insert_report(
    db: &mut Database,
    source: &ClientName,
    report: &NewReport,
) -> Result<i64, Error> {
    db.conn().execute(
        "INSERT INTO report (period, title, body_markdown, source_client) \
         VALUES (?1, ?2, ?3, ?4)",
        params![
            report.period,
            report.title,
            report.body_markdown,
            source.as_str(),
        ],
    )?;
    Ok(db.conn().last_insert_rowid())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn files_pending_suggestions_and_reports() {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
        let client = ClientName::new("claude-code").unwrap();
        let id = insert_suggestion(
            &mut db,
            &client,
            &NewSuggestion {
                kind: SuggestionKind::Categorization,
                payload: serde_json::json!({ "transaction_id": 1 }),
                rationale: Some("Looks like a café".to_owned()),
            },
        )
        .unwrap();
        let (status, source): (String, String) = db
            .conn()
            .query_row(
                "SELECT status, source_client FROM suggestion WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            (status.as_str(), source.as_str()),
            ("pending", "claude-code")
        );

        insert_report(
            &mut db,
            &client,
            &NewReport {
                period: "2026-10".to_owned(),
                title: "October".to_owned(),
                body_markdown: "Spending was down.".to_owned(),
            },
        )
        .unwrap();
        let blank = NewReport {
            period: "2026-10".to_owned(),
            title: " ".to_owned(),
            body_markdown: String::new(),
        };
        assert!(insert_report(&mut db, &client, &blank).is_err());
    }
}
