//! The import's reads and writes (M3.4, M3.5): the database, the file and
//! the user's saved presets.
//!
//! As the other stores ([`crate::accounts::store`]), it holds no
//! connection: each call opens the database at the path it was given,
//! creating and migrating it as the CLI does, and checks the restore lock
//! first. The dialog calls these on a background thread: reading a file,
//! previewing and rehearsing the import, the commit (with its backup) and
//! undo. Business rules are core's ([`coffer_core::import`]) and the import
//! crate's; nothing here logs a row's text.

use std::fmt;
use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, Utc};
use coffer_core::account::{self, Account, AccountEdit};
use coffer_core::backup;
use coffer_core::db::Database;
use coffer_core::id::{AccountId, BatchId};
use coffer_core::import::{
    self, BatchStatus, CommitSummary, Committed, Confirmations, ImportPreview,
};
use coffer_core::paths;
use coffer_core::schedule;
use coffer_core::statement::{SourceFormat, Statement};
use coffer_import::SourceFile;
use coffer_import::preset::{Preset, Presets, load_user_presets, save_user_preset};

pub use crate::accounts::store::StoreError;

/// How many imports the File step lists (Paper `6K5-0` draws three).
pub const RECENT_IMPORTS: usize = 3;

/// A restore is replacing the database.
#[derive(Debug)]
struct Restoring;

impl fmt::Display for Restoring {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("A backup is being restored; try again when it's done.")
    }
}

impl std::error::Error for Restoring {}

/// One of the last imports, as the File step lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentImport {
    pub file: String,
    pub account: String,
    /// The preset's name, or the format for a file read without one.
    pub read_with: String,
    /// When, in local time.
    pub date: NaiveDate,
}

/// What the dialog needs before a file is chosen.
#[derive(Clone, Debug)]
pub struct Setup {
    /// The bundled presets and the user's saved ones.
    pub presets: Presets,
    /// Saved presets that couldn't be used, and why.
    pub preset_problems: Vec<String>,
    /// Every account (the session picks those that can take an import).
    pub accounts: Vec<Account>,
    /// The last imports, newest first.
    pub recent: Vec<RecentImport>,
    pub today: NaiveDate,
}

/// The Review step: what the import would do, before anything is written.
#[derive(Clone, Debug)]
pub struct Review {
    pub preview: ImportPreview,
    /// What the commit's own steps would do (categories, pots, duplicates),
    /// from a rehearsal that was rolled back. `None` when nothing is new.
    pub after: Option<CommitSummary>,
}

/// The import's reads and writes on the database at `database`.
#[derive(Clone, Debug)]
pub struct ImportStore {
    database: PathBuf,
}

impl ImportStore {
    pub fn new(database: PathBuf) -> Self {
        Self { database }
    }

    /// Where the user's presets are saved: `presets/` next to the database.
    pub fn presets_dir(&self) -> PathBuf {
        paths::presets_dir(&self.database)
    }

    fn open(&self) -> Result<Database, StoreError> {
        if backup::restoring(&self.database)? {
            return Err(Box::new(Restoring));
        }
        Ok(Database::open(&self.database)?)
    }

    /// The presets (bundled and saved), the accounts and the last imports.
    pub fn setup(&self) -> Result<Setup, StoreError> {
        let (presets, problems) = load_user_presets(&self.presets_dir())?;
        let preset_problems = problems
            .into_iter()
            .map(|problem| {
                tracing::warn!(%problem, "a saved preset can't be used");
                problem.to_string()
            })
            .collect();
        let db = self.open()?;
        let accounts = account::list(&db)?;
        let mut recent = Vec::new();
        for account in &accounts {
            for batch in import::batches(&db, account.id)? {
                if batch.status != BatchStatus::Committed {
                    continue;
                }
                let read_with = match (&batch.preset_id, batch.format) {
                    (Some(id), _) => presets
                        .get(id)
                        .map_or_else(|| id.clone(), |preset| preset.name().to_owned()),
                    (None, SourceFormat::Ofx) => "OFX".to_owned(),
                    (None, _) => "CSV".to_owned(),
                };
                recent.push((
                    batch.imported_at.clone(),
                    batch.id,
                    RecentImport {
                        file: batch.source_file,
                        account: account.name.clone(),
                        read_with,
                        date: local_date(&batch.imported_at),
                    },
                ));
            }
        }
        // Newest first; batch IDs break ties within a millisecond.
        recent.sort_by(|a, b| (&b.0, b.1).cmp(&(&a.0, a.1)));
        recent.truncate(RECENT_IMPORTS);
        Ok(Setup {
            presets,
            preset_problems,
            accounts,
            recent: recent.into_iter().map(|(_, _, recent)| recent).collect(),
            today: schedule::local_today()?,
        })
    }

    /// Reads the file at `path` into memory (its name only is kept).
    pub fn read_file(path: &Path) -> Result<SourceFile, StoreError> {
        SourceFile::read(path).map_err(|error| {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            format!("Couldn't read {name}: {error}.").into()
        })
    }

    /// What importing `statement` into `account` would do: core's preview,
    /// and a rehearsal of the commit's own steps.
    pub fn review(
        &self,
        account: AccountId,
        file_name: &str,
        statement: Statement,
    ) -> Result<Review, StoreError> {
        let db = self.open()?;
        let preview = import::preview(&db, account, file_name, statement)?;
        let after = import::rehearse(&db, &preview)?;
        Ok(Review { preview, after })
    }

    /// Commits `preview` (core takes the backup first).
    pub fn commit(
        &self,
        preview: &ImportPreview,
        confirmed: Confirmations,
    ) -> Result<Committed, StoreError> {
        let mut db = self.open()?;
        Ok(import::commit(&mut db, preview, confirmed)?)
    }

    /// Undoes an import batch.
    pub fn undo(&self, batch: BatchId) -> Result<(), StoreError> {
        let mut db = self.open()?;
        import::undo(&mut db, batch)?;
        Ok(())
    }

    /// Saves `preset` with the user's presets and, when `account` is
    /// given, makes it that account's preset for the next import. Returns
    /// where it was saved.
    pub fn save_preset(
        &self,
        preset: &Preset,
        account: Option<AccountId>,
    ) -> Result<PathBuf, StoreError> {
        // The presets as they are on disk now, so an ID is never reused.
        let (known, _) = load_user_presets(&self.presets_dir())?;
        let path = save_user_preset(&self.presets_dir(), preset, &known)?;
        if let Some(id) = account {
            let mut db = self.open()?;
            if let Some(found) = account::list(&db)?.into_iter().find(|a| a.id == id) {
                let mut edit = AccountEdit::of(&found);
                if found.kind.is_imported() {
                    edit.preset = Some(preset.id().to_owned());
                    account::update(&mut db, id, &edit)?;
                }
            }
        }
        Ok(path)
    }

    /// Writes `preset` as TOML to `path`, wherever the user chose (0600, as
    /// an export): "Save a copy…".
    pub fn save_copy(preset: &Preset, path: &Path) -> Result<(), StoreError> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        use std::io::Write as _;
        let mut file = options.open(path)?;
        file.write_all(preset.to_toml().as_bytes())?;
        file.sync_all()?;
        Ok(())
    }
}

/// The local date of a stored UTC timestamp (`imported_at`); the UTC date
/// if the stamp doesn't read.
fn local_date(stamp: &str) -> NaiveDate {
    match DateTime::parse_from_rfc3339(stamp) {
        Ok(time) => crate::settings::store::local(time.with_timezone(&Utc)).date(),
        Err(_) => stamp
            .get(..10)
            .and_then(|day| NaiveDate::parse_from_str(day, "%Y-%m-%d").ok())
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use coffer_core::account::{AccountKind, NewAccount};
    use coffer_core::money::Currency;
    use coffer_import::preset::PresetParts;
    use tempfile::TempDir;

    use super::*;
    use crate::import::session::Session;

    fn store() -> (TempDir, ImportStore, AccountId) {
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("coffer.db");
        let mut db = Database::open(&database).unwrap();
        let mut new = NewAccount::new("Current account", AccountKind::Current, Currency::GBP);
        new.preset = Some("uk-monzo".to_owned());
        let id = account::create(&mut db, &new).unwrap();
        (dir, ImportStore::new(database), id)
    }

    fn monzo() -> SourceFile {
        SourceFile::read(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/presets/uk/monzo.csv"),
        )
        .unwrap()
    }

    #[test]
    fn reviews_commits_lists_and_undoes_an_import() {
        let (_dir, store, id) = store();
        let setup = store.setup().unwrap();
        assert!(setup.recent.is_empty());
        let session = Session::open(monzo(), &setup.presets, &setup.accounts, None).unwrap();
        assert_eq!(session.account_id(), Some(id));
        let statement = session.statement(&setup.presets, Currency::GBP).unwrap();

        let review = store.review(id, "monzo.csv", statement).unwrap();
        let after = review.after.unwrap();
        assert!(after.transactions > 0);
        assert!(after.categorized_by_preset > 0);
        assert!(after.mirrored > 0, "the fixture has pot transfers");

        let confirm = review.preview.needs_confirmation();
        let Committed::Batch(summary) = store.commit(&review.preview, confirm).unwrap() else {
            panic!("a batch");
        };
        assert_eq!(summary.transactions, after.transactions);

        let setup = store.setup().unwrap();
        assert_eq!(setup.recent.len(), 1);
        assert_eq!(setup.recent[0].file, "monzo.csv");
        assert_eq!(setup.recent[0].account, "Current account");
        assert_eq!(setup.recent[0].read_with, "Monzo");

        store.undo(summary.batch).unwrap();
        assert!(store.setup().unwrap().recent.is_empty());
    }

    #[test]
    fn saving_a_preset_sets_it_on_the_account() {
        let (_dir, store, id) = store();
        let setup = store.setup().unwrap();
        let mut mapping = setup.presets.get("uk-monzo").unwrap().mapping().clone();
        mapping.sub_account_transfer = None;
        let preset = Preset::new(PresetParts {
            id: "uk-my-bank".to_owned(),
            name: "My bank".to_owned(),
            region: "uk".to_owned(),
            mapping,
            ignore: Vec::new(),
            category_map: BTreeMap::new(),
            sub_account_category: None,
        })
        .unwrap();
        let path = store.save_preset(&preset, Some(id)).unwrap();
        assert!(path.starts_with(store.presets_dir()));

        let setup = store.setup().unwrap();
        assert!(
            setup
                .presets
                .get("uk-my-bank")
                .is_some_and(Preset::is_saved)
        );
        let account = setup.accounts.iter().find(|a| a.id == id).unwrap();
        assert_eq!(account.preset.as_deref(), Some("uk-my-bank"));
        // The same ID again is refused.
        assert!(store.save_preset(&preset, None).is_err());

        let copy = store.presets_dir().join("copy.toml");
        ImportStore::save_copy(&preset, &copy).unwrap();
        assert_eq!(std::fs::read_to_string(copy).unwrap(), preset.to_toml());
    }

    #[test]
    fn local_dates_read_the_stamp() {
        let date = local_date("2026-11-09T12:00:00.000Z");
        // Noon UTC is the same day everywhere but the far east and west.
        assert!(
            (date - NaiveDate::from_ymd_opt(2026, 11, 9).unwrap())
                .num_days()
                .abs()
                <= 1
        );
        assert_eq!(
            local_date("not a date"),
            NaiveDate::default(),
            "an unreadable stamp falls back"
        );
    }
}
