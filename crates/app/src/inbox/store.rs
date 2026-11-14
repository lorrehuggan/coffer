//! Reading and deciding suggestions in the database (`docs/ARCHITECTURE.md`
//! → "Decision record: Inbox"), the way [`crate::accounts::store`] does
//! accounts: no connection is held between calls. Each call opens the
//! database at its path (creating and migrating it, as the CLI does) after
//! checking the restore lock, so a restore can swap the file.
//!
//! Everything it reads and every decision is core's
//! ([`coffer_core::inbox`]): approving and rejecting go through
//! `approve`/`approve_all`/`reject_all`, which check each suggestion again
//! against today's data, and the previews are `inbox::preview`, computed
//! when read (decision record "Inbox impact preview", point 6).

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use coffer_core::backup;
use coffer_core::db::Database;
use coffer_core::inbox::preview::{self, Covered, Preview, Subject};
use coffer_core::inbox::{
    self, ApproveOptions, Approved, Counts, Filter, Suggestion, SuggestionId, SuggestionStatus,
};
use coffer_core::mcp_client::{self, ClientStatus};
use coffer_core::reporting::palette::{Palette, category_palette};
use coffer_core::schedule;

use crate::accounts::store::StoreError;

/// A restore is replacing the database.
#[derive(Debug)]
struct Restoring;

impl std::fmt::Display for Restoring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("A backup is being restored; try again when it's done.")
    }
}

impl std::error::Error for Restoring {}

/// One suggestion as the Inbox lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub suggestion: Suggestion,
    /// What it names, as the rows are now.
    pub subject: Subject,
    /// What approving it would change: pending suggestions only.
    pub preview: Option<Preview>,
    /// Why there is no preview for a pending suggestion: what it names no
    /// longer fits (core's message: IDs and keys, never text).
    pub refused: Option<String>,
}

impl Entry {
    pub fn id(&self) -> SuggestionId {
        self.suggestion.id
    }

    /// Whether "Approve all" leaves it out: a wide rule (decision record
    /// "Inbox impact preview", point 3), or one that can't be approved now.
    pub fn needs_own_approval(&self) -> bool {
        match &self.preview {
            Some(preview) => {
                preview.impact.wide()
                    || preview.impact.blocked.is_some()
                    // The Inbox applies a rule to history too.
                    || matches!(
                        &preview.impact.detail,
                        coffer_core::inbox::Detail::Rule(rule) if rule.history_blocked.is_some()
                    )
            }
            None => true,
        }
    }
}

/// What the Inbox shows: the counts, the sources, and the suggestions with
/// one status.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InboxData {
    pub counts: Counts,
    /// The sources the panel lists: every client the user enabled, every
    /// source that filed a suggestion, and Coffer itself, by name.
    pub sources: Vec<String>,
    pub status: Option<SuggestionStatus>,
    /// The suggestions with `status`, oldest first.
    pub entries: Vec<Entry>,
    pub palette: Palette,
    /// When the data was read, for "2h ago".
    pub read_at: Option<DateTime<Utc>>,
}

/// The suggestions in the database at `database`.
#[derive(Clone, Debug)]
pub struct InboxStore {
    database: PathBuf,
}

impl InboxStore {
    pub fn new(database: PathBuf) -> Self {
        Self { database }
    }

    fn open(&self) -> Result<Database, StoreError> {
        if backup::restoring(&self.database)? {
            return Err(Box::new(Restoring));
        }
        Ok(Database::open(&self.database)?)
    }

    /// The counts and the suggestions with `status`; pending ones with
    /// their previews.
    pub fn load(&self, status: SuggestionStatus) -> Result<InboxData, StoreError> {
        let db = self.open()?;
        load(&db, status)
    }

    /// The pending rule that would settle categorisation `id`, if any.
    pub fn covered_by(&self, id: SuggestionId) -> Result<Option<Covered>, StoreError> {
        let db = self.open()?;
        Ok(preview::covered_by(&db, id)?)
    }

    /// Approves `ids` together, all or none. A rule is also applied to
    /// past transactions, never over a category the user set (decision
    /// record "Inbox impact preview", point 1).
    pub fn approve(&self, ids: &[SuggestionId]) -> Result<Vec<Approved>, StoreError> {
        let mut db = self.open()?;
        Ok(inbox::approve_all(
            &mut db,
            ids,
            ApproveOptions {
                apply_to_history: true,
            },
        )?)
    }

    /// Rejects `ids` together, all or none.
    pub fn reject(&self, ids: &[SuggestionId]) -> Result<(), StoreError> {
        let mut db = self.open()?;
        Ok(inbox::reject_all(&mut db, ids, None)?)
    }
}

/// [`InboxStore::load`] on an open database: also what tests and the
/// snapshot use.
pub fn load(db: &Database, status: SuggestionStatus) -> Result<InboxData, StoreError> {
    let counts = inbox::counts(db)?;
    let mut sources: Vec<String> = mcp_client::list(&db.reader()?)?
        .into_iter()
        .filter(|client| client.status == ClientStatus::Known)
        .map(|client| client.name.as_str().to_owned())
        .collect();
    for source in counts.by_source.keys() {
        if !sources.contains(source) {
            sources.push(source.clone());
        }
    }
    sources.retain(|source| source != inbox::COFFER_SOURCE);
    sources.sort();
    sources.push(inbox::COFFER_SOURCE.to_owned());

    let suggestions = inbox::list(
        db,
        &Filter {
            status: Some(status),
            kind: None,
        },
    )?;
    let mut entries = Vec::with_capacity(suggestions.len());
    for suggestion in suggestions {
        let subject = preview::describe(db, &suggestion)?;
        let (preview, refused) = if status == SuggestionStatus::Pending {
            match preview::preview(db, suggestion.id) {
                Ok(preview) => (Some(preview), None),
                Err(error) => (None, Some(error.to_string())),
            }
        } else {
            (None, None)
        };
        entries.push(Entry {
            suggestion,
            subject,
            preview,
            refused,
        });
    }
    Ok(InboxData {
        counts,
        sources,
        status: Some(status),
        entries,
        palette: category_palette(&db.reader()?, schedule::local_today()?)?,
        read_at: Some(DateTime::<Utc>::from(std::time::SystemTime::now())),
    })
}
