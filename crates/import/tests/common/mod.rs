//! Shared helpers for the end-to-end import tests (M1.19): a fixture file
//! goes through an importer, then `import::preview` / `commit` into a
//! scratch database, and the tests read back what was stored.
//!
//! Each test binary uses a different subset, hence `dead_code`.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use coffer_core::account::{self, AccountKind, NewAccount};
use coffer_core::db::Database;
use coffer_core::id::AccountId;
use coffer_core::import::{self, CommitSummary, Committed, ImportPreview, RowOutcome};
use coffer_core::money::Currency;
use coffer_core::statement::{SourceLocation, Statement};
use coffer_import::ofx::OfxImporter;
use coffer_import::preset::Presets;
use coffer_import::{AnyImporter, SourceFile};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use tempfile::TempDir;

/// `fixtures/<path>`.
pub fn fixture_path(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(path)
}

/// Reads `fixtures/<path>` as the importer would.
pub fn fixture(path: &str) -> SourceFile {
    SourceFile::read(&fixture_path(path)).unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// A fixture's bytes, for tests that re-encode or re-wrap it.
pub fn fixture_bytes(path: &str) -> Vec<u8> {
    std::fs::read(fixture_path(path)).unwrap()
}

/// The test presets in `fixtures/csv/mappings/`, one per generic CSV
/// fixture shape (`fixtures/README.md`).
pub fn mappings() -> Presets {
    let root = fixture_path("csv/mappings");
    let mut files: Vec<(String, String)> = Vec::new();
    for region in std::fs::read_dir(&root).unwrap() {
        let region = region.unwrap();
        for file in std::fs::read_dir(region.path()).unwrap() {
            let file = file.unwrap().path();
            let path = format!(
                "{}/{}",
                region.file_name().to_string_lossy(),
                file.file_name().unwrap().to_string_lossy()
            );
            files.push((path, std::fs::read_to_string(&file).unwrap()));
        }
    }
    Presets::from_files(
        files
            .iter()
            .map(|(path, text)| (path.as_str(), text.as_str())),
    )
    .unwrap_or_else(|error| panic!("{error}"))
}

/// One test preset's TOML, for tests that vary it.
pub fn mapping_toml(path: &str) -> String {
    std::fs::read_to_string(fixture_path("csv/mappings").join(path)).unwrap()
}

/// Reads `fixtures/<file>` with the test preset `preset`.
pub fn read_with(preset: &str, file: &str) -> Statement {
    let presets = mappings();
    let preset = presets
        .get(preset)
        .unwrap_or_else(|| panic!("no test preset {preset}"));
    preset
        .importer()
        .import(&fixture(file))
        .unwrap_or_else(|error| panic!("{file}: {error}"))
}

/// Reads `fixtures/ofx/<file>`, a file with one statement.
pub fn read_ofx(file: &str) -> Statement {
    OfxImporter::new()
        .import(&fixture(&format!("ofx/{file}")))
        .unwrap_or_else(|error| panic!("{file}: {error}"))
}

/// Reads statement `index` (from 0) of `fixtures/ofx/<file>`.
pub fn read_ofx_statement(file: &str, index: usize) -> Statement {
    OfxImporter::for_statement(index)
        .import(&fixture(&format!("ofx/{file}")))
        .unwrap_or_else(|error| panic!("{file}: {error}"))
}

/// A scratch database in its own directory.
pub struct Scratch {
    pub db: Database,
    _dir: TempDir,
}

impl Scratch {
    pub fn new() -> Scratch {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("coffer.db")).unwrap();
        Scratch { db, _dir: dir }
    }

    pub fn account(&mut self, name: &str, kind: AccountKind, currency: Currency) -> AccountId {
        account::create(&mut self.db, &NewAccount::new(name, kind, currency)).unwrap()
    }

    pub fn current(&mut self, name: &str, currency: Currency) -> AccountId {
        self.account(name, AccountKind::Current, currency)
    }

    pub fn preview(&self, account: AccountId, file: &str, statement: Statement) -> ImportPreview {
        import::preview(&self.db, account, file, statement).unwrap()
    }

    /// Previews and commits, confirming whatever the preview asks for.
    pub fn import(&mut self, account: AccountId, file: &str, statement: Statement) -> Committed {
        let preview = self.preview(account, file, statement);
        let confirm = preview.needs_confirmation();
        import::commit(&mut self.db, &preview, confirm).unwrap()
    }

    /// [`import`](Self::import), expecting a new batch.
    pub fn import_batch(
        &mut self,
        account: AccountId,
        file: &str,
        statement: Statement,
    ) -> CommitSummary {
        match self.import(account, file, statement) {
            Committed::Batch(summary) => summary,
            Committed::NothingNew => panic!("expected a new batch from {file}"),
        }
    }

    /// A read-only connection of its own, for checking what was stored.
    pub fn reader(&self) -> Connection {
        Connection::open_with_flags(self.db.path(), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
    }

    /// A writable connection of its own, for what core has no API for yet
    /// (a lock date, a user's edit).
    pub fn writer(&self) -> Connection {
        let conn = Connection::open(self.db.path()).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        conn
    }

    pub fn count(&self, sql: &str) -> i64 {
        self.reader().query_row(sql, [], |row| row.get(0)).unwrap()
    }

    /// The account's transactions, by ID.
    pub fn stored(&self, account: AccountId) -> Vec<Stored> {
        let reader = self.reader();
        let mut statement = reader
            .prepare(
                "SELECT t.id, t.batch_id, t.date, t.time, t.amount_minor, t.currency, t.merchant, \
                 t.description_raw, t.description_normalized, t.external_id, t.fingerprint, \
                 t.source_category, t.source_type, t.original_amount_minor, \
                 t.original_currency, t.note, t.origin, c.key, t.category_source, \
                 t.transfer_group_id \
                 FROM \"transaction\" t LEFT JOIN category c ON c.id = t.category_id \
                 WHERE t.account_id = ?1 ORDER BY t.id",
            )
            .unwrap();
        statement
            .query_map([account.get()], |row| {
                Ok(Stored {
                    id: row.get(0)?,
                    batch: row.get(1)?,
                    date: row.get(2)?,
                    time: row.get(3)?,
                    amount_minor: row.get(4)?,
                    currency: row.get(5)?,
                    merchant: row.get(6)?,
                    description_raw: row.get(7)?,
                    description_normalized: row.get(8)?,
                    external_id: row.get(9)?,
                    fingerprint: row.get(10)?,
                    source_category: row.get(11)?,
                    source_type: row.get(12)?,
                    original_amount_minor: row.get(13)?,
                    original_currency: row.get(14)?,
                    note: row.get(15)?,
                    origin: row.get(16)?,
                    category: row.get(17)?,
                    category_source: row.get(18)?,
                    transfer_group: row.get(19)?,
                })
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    /// The account's stored balances: `(date, balance_minor, source,
    /// batch_id)`, by date then ID.
    pub fn balances(&self, account: AccountId) -> Vec<(String, i64, String, Option<i64>)> {
        let reader = self.reader();
        let mut statement = reader
            .prepare(
                "SELECT date, balance_minor, source, batch_id FROM balance_snapshot \
                 WHERE account_id = ?1 ORDER BY date, id",
            )
            .unwrap();
        statement
            .query_map([account.get()], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    /// The stored row with this external ID in the account, if any.
    pub fn by_external_id(&self, account: AccountId, id: &str) -> Option<Stored> {
        let found: Option<i64> = self
            .reader()
            .query_row(
                "SELECT id FROM \"transaction\" WHERE account_id = ?1 AND external_id = ?2",
                rusqlite::params![account.get(), id],
                |row| row.get(0),
            )
            .optional()
            .unwrap();
        found.map(|found| {
            self.stored(account)
                .into_iter()
                .find(|row| row.id == found)
                .unwrap()
        })
    }

    /// Sets the account's lock date (`reconciled_through`), which only
    /// M1.27 will have an API for.
    pub fn lock(&self, account: AccountId, through: &str) {
        self.writer()
            .execute(
                "UPDATE account SET reconciled_through = ?2 WHERE id = ?1",
                rusqlite::params![account.get(), through],
            )
            .unwrap();
    }
}

/// One stored transaction, as the commit wrote it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stored {
    pub id: i64,
    pub batch: Option<i64>,
    pub date: String,
    pub time: Option<String>,
    pub amount_minor: i64,
    pub currency: String,
    pub merchant: Option<String>,
    pub description_raw: String,
    pub description_normalized: String,
    pub external_id: Option<String>,
    pub fingerprint: Option<String>,
    pub source_category: Option<String>,
    pub source_type: Option<String>,
    pub original_amount_minor: Option<i64>,
    pub original_currency: Option<String>,
    pub note: Option<String>,
    pub origin: String,
    pub category: Option<String>,
    pub category_source: Option<String>,
    pub transfer_group: Option<i64>,
}

/// `(location, outcome)` for every row of a preview, in file order.
pub fn outcomes(preview: &ImportPreview) -> Vec<(SourceLocation, RowOutcome)> {
    preview
        .rows()
        .map(|row| (row.location, row.outcome.clone()))
        .collect()
}

/// A short name for an outcome, for compact assertions: `new`,
/// `already:source_id`, `already:fingerprint`, `repeated`, or the flag's
/// kind (`flag:unreadable`, …).
pub fn outcome_kind(outcome: &RowOutcome) -> String {
    match outcome {
        RowOutcome::New { .. } => "new".to_owned(),
        RowOutcome::AlreadyImported { matched_by, .. } => match matched_by {
            import::MatchedBy::SourceId => "already:source_id".to_owned(),
            import::MatchedBy::Fingerprint => "already:fingerprint".to_owned(),
        },
        RowOutcome::RepeatedInFile { .. } => "repeated".to_owned(),
        RowOutcome::Flagged(flag) => format!("flag:{}", flag.kind().as_str()),
    }
}

/// `(line or entry number, outcome kind)` for every row.
pub fn outcome_kinds(preview: &ImportPreview) -> Vec<(u64, String)> {
    preview
        .rows()
        .map(|row| {
            let number = match row.location {
                SourceLocation::Line(n) | SourceLocation::Entry(n) => n,
            };
            (number, outcome_kind(row.outcome))
        })
        .collect()
}
