//! Local backups (M1.28): `VACUUM INTO` snapshots of the database, and
//! restoring one.
//!
//! Backups live in `backups/` next to the database file (so `<data
//! dir>/backups`, and wherever a test or `COFFER_DATA_DIR` puts the
//! database). The directory is `0700` when Coffer creates it; every backup
//! is `0600`.
//!
//! **Metadata lives in the file name only**: the UTC time and the
//! [`Reason`], as in `coffer-20261111T153012.345Z-import.db`. There is no
//! table for them, because restoring would roll the table back. Names sort
//! by time and parse back ([`parse_file_name`]); anything else in the
//! directory isn't a backup and is never touched.
//!
//! **Rotation.** Automatic backups (before an import commit, a migration or
//! a restore) rotate: the [`KEEP_RECENT`] most recent are kept, plus the
//! newest of each earlier calendar month (UTC). Manual backups (`coffer
//! backup`, "Back up now") are never deleted automatically.
//!
//! **Restore** checks a copy of the backup (SQLite's `integrity_check`,
//! Coffer's migration history, a schema no newer than this build's) before
//! anything is swapped, backs the current database up (reason
//! [`Reason::Restore`]), swaps the checked copy in with a rename, and opens
//! it with [`Database::open`], which migrates an older schema. A refused
//! restore leaves the current database as it was.
//!
//! Nothing here reads or logs transaction contents; errors name files,
//! versions and SQLite's own messages only.

#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

use chrono::{DateTime, Datelike, NaiveDateTime, TimeDelta, Utc};
use rusqlite::Connection;

use crate::db::{self, Database, SchemaVersion};

/// The backups directory's name, next to the database file.
pub const BACKUPS_DIR: &str = "backups";

/// How many of the most recent automatic backups rotation keeps (besides
/// one per earlier month). A default, not an invariant (`ARCHITECTURE.md`
/// → "Decision record: feature research", point 4).
pub const KEEP_RECENT: usize = 10;

const NAME_PREFIX: &str = "coffer-";
const NAME_SUFFIX: &str = ".db";
/// UTC, to the millisecond: `20261111T153012.345Z`.
const STAMP_FORMAT: &str = "%Y%m%dT%H%M%S%.3fZ";

/// Why a backup couldn't be taken, listed or restored. Messages name files
/// and schema versions, never anything stored in the database.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("couldn't {action} {}: {source}", path.display())]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error(transparent)]
    Database(#[from] db::Error),

    #[error("{} isn't a SQLite database", path.display())]
    NotADatabase { path: PathBuf },

    #[error("{} isn't a Coffer database: it has no migration history", path.display())]
    NotCoffer { path: PathBuf },

    #[error(
        "{} failed SQLite's integrity check ({count} {}; the first: {first})",
        path.display(),
        if *count == 1 { "problem" } else { "problems" }
    )]
    Integrity {
        path: PathBuf,
        count: usize,
        first: String,
    },

    /// From a newer Coffer ([`db::Error::SchemaTooNew`]), or with a migration
    /// history that differs from this build's ([`db::Error::MigrationHistory`]).
    #[error("{} can't be restored: {source}", path.display())]
    Incompatible {
        path: PathBuf,
        #[source]
        source: db::Error,
    },

    #[error(
        "the current database {} couldn't be backed up, so nothing was restored ({source}); \
         if it is damaged, move it and its -wal and -shm files aside first",
        path.display()
    )]
    PreRestoreBackup {
        path: PathBuf,
        #[source]
        source: Box<Error>,
    },

    #[error(
        "{} is still open in another program (its -wal file remains); \
         close Coffer and anything else using it, then restore again",
        path.display()
    )]
    InUse { path: PathBuf },

    /// The backup is in place, but opening (and migrating) it failed. It is
    /// at its old version: migrations run in one transaction.
    #[error(
        "the backup was restored to {}, but it couldn't be opened ({source}){}",
        path.display(),
        match pre_restore {
            Some(backup) => format!("; the database it replaced is in {}", backup.display()),
            None => String::new(),
        }
    )]
    RestoredButNotOpened {
        path: PathBuf,
        pre_restore: Option<PathBuf>,
        #[source]
        source: db::Error,
    },

    #[error(
        "{} exists without its database {}; it may hold changes, so move it aside \
         before restoring",
        sidecar.display(),
        database.display()
    )]
    OrphanSidecar { sidecar: PathBuf, database: PathBuf },
}

fn io_error(action: &'static str, path: &Path) -> impl FnOnce(io::Error) -> Error {
    let path = path.to_path_buf();
    move |source| Error::Io {
        action,
        path,
        source,
    }
}

/// Why a backup was taken. Its [`key`](Reason::key) is in the file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    /// Before an import commit.
    Import,
    /// Before pending migrations ran on an existing database.
    Migration,
    /// Before a restore replaced the database.
    Restore,
    /// Asked for: `coffer backup`, the app's "Back up now".
    Manual,
}

impl Reason {
    pub const ALL: [Reason; 4] = [
        Reason::Import,
        Reason::Migration,
        Reason::Restore,
        Reason::Manual,
    ];

    /// The stable key in the file name.
    pub fn key(self) -> &'static str {
        match self {
            Reason::Import => "import",
            Reason::Migration => "migration",
            Reason::Restore => "restore",
            Reason::Manual => "manual",
        }
    }

    pub fn from_key(key: &str) -> Option<Reason> {
        Self::ALL.into_iter().find(|reason| reason.key() == key)
    }

    /// Whether Coffer took it by itself. Only automatic backups rotate.
    pub fn is_automatic(self) -> bool {
        self != Reason::Manual
    }
}

/// A backup file in the backups directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backup {
    pub path: PathBuf,
    pub reason: Reason,
    /// When it was taken (UTC, to the millisecond), from its name.
    pub taken_at: DateTime<Utc>,
    /// Its size in bytes.
    pub size: u64,
}

/// The backups directory for the database at `db_path`: `backups/` next
/// to it.
pub fn backups_dir(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(BACKUPS_DIR)
}

/// A backup's file name: `coffer-20261111T153012.345Z-import.db`.
/// `taken_at` is written to the millisecond.
pub fn file_name(reason: Reason, taken_at: DateTime<Utc>) -> String {
    format!(
        "{NAME_PREFIX}{}-{}{NAME_SUFFIX}",
        taken_at.format(STAMP_FORMAT),
        reason.key()
    )
}

/// The reason and time in a backup's file name, or `None` when `name` isn't
/// exactly one [`file_name`] writes.
pub fn parse_file_name(name: &str) -> Option<(Reason, DateTime<Utc>)> {
    let rest = name.strip_prefix(NAME_PREFIX)?.strip_suffix(NAME_SUFFIX)?;
    let (stamp, key) = rest.split_once('-')?;
    let reason = Reason::from_key(key)?;
    let taken_at = NaiveDateTime::parse_from_str(stamp, STAMP_FORMAT)
        .ok()?
        .and_utc();
    // Only the canonical spelling: no other digit counts or forms.
    (file_name(reason, taken_at) == name).then_some((reason, taken_at))
}

/// Takes a backup of `db` now, and rotates the automatic ones when
/// `reason` is automatic.
///
/// `VACUUM INTO` can't run inside an open transaction: callers take the
/// backup before they `BEGIN` (`import::commit` takes it before its
/// transaction). The snapshot is written to a hidden temporary file in the
/// backups directory, synced, and only then given its backup name, so a
/// half-written file never looks like a backup.
///
/// A failed rotation doesn't fail the backup: the new backup is there, and
/// the files rotation couldn't delete are logged and tried again next time.
pub fn create(db: &Database, reason: Reason) -> Result<Backup, Error> {
    let backup = snapshot(db.conn(), db.path(), reason)?;
    rotate_after(db.path(), reason);
    Ok(backup)
}

/// Takes a backup of the database file at `db_path` through a connection of
/// its own, for when there is no [`Database`] to pass to [`create`]:
///
/// - **before migrations** (the intended call site is `Database::open`, on
///   an existing database whose schema is older than this build's, after
///   `migrations::check` and before `migrations::run`), with
///   [`Reason::Migration`]: it reads the file at its old version and
///   changes nothing in it;
/// - before a restore ([`restore`] does this).
///
/// The file must exist and be a SQLite database. Nobody may hold a write
/// transaction open on it for longer than the busy timeout.
pub fn create_from_path(db_path: &Path, reason: Reason) -> Result<Backup, Error> {
    let conn = db::connect(db_path)?;
    conn.busy_timeout(db::BUSY_TIMEOUT)?;
    conn.pragma_update(None, "trusted_schema", false)?;
    let backup = snapshot(&conn, db_path, reason)?;
    drop(conn);
    rotate_after(db_path, reason);
    Ok(backup)
}

/// Like [`create_from_path`], on a connection the caller already has open
/// to `db_path` (e.g. `Database::open`'s, before it runs migrations). No
/// transaction may be open on `conn`.
// Unused until `Database::open` calls it before migrations (M1.28 wiring).
pub(crate) fn create_with_connection(
    conn: &Connection,
    db_path: &Path,
    reason: Reason,
) -> Result<Backup, Error> {
    let backup = snapshot(conn, db_path, reason)?;
    rotate_after(db_path, reason);
    Ok(backup)
}

fn rotate_after(db_path: &Path, reason: Reason) {
    if !reason.is_automatic() {
        return;
    }
    if let Err(err) = rotate(db_path, now()) {
        tracing::warn!(error = %err, "couldn't rotate old backups");
    }
}

fn snapshot(conn: &Connection, db_path: &Path, reason: Reason) -> Result<Backup, Error> {
    let dir = backups_dir(db_path);
    db::create_private_dir(&dir).map_err(io_error("create", &dir))?;

    let partial = Scratch::create(&dir, "partial")?;
    let target = partial.path.to_str().ok_or_else(|| Error::Io {
        action: "write a backup to",
        path: dir.clone(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "the path isn't valid UTF-8"),
    })?;
    // Into the empty `0600` file `Scratch::create` made: SQLite writes into
    // an existing empty file and keeps its mode.
    conn.execute("VACUUM INTO ?1", [target])?;
    finish_file(&partial.path)?;

    let (path, taken_at) = partial.publish(&dir, reason)?;
    sync_dir(&dir).map_err(io_error("sync", &dir))?;
    let size = fs::metadata(&path).map_err(io_error("read", &path))?.len();
    tracing::debug!(backup = %path.display(), "took a backup");
    Ok(Backup {
        path,
        reason,
        taken_at,
        size,
    })
}

/// The backups of the database at `db_path`, newest first. Files in the
/// backups directory whose names don't parse ([`parse_file_name`]) aren't
/// listed. No directory means no backups.
pub fn list(db_path: &Path) -> Result<Vec<Backup>, Error> {
    let dir = backups_dir(db_path);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(io_error("read", &dir)(err)),
    };
    let mut backups = Vec::new();
    for entry in entries {
        let entry = entry.map_err(io_error("read", &dir))?;
        let Some((reason, taken_at)) = entry.file_name().to_str().and_then(parse_file_name) else {
            continue;
        };
        let path = entry.path();
        // Not through symlinks: a link named like a backup isn't one.
        let metadata = fs::symlink_metadata(&path).map_err(io_error("read", &path))?;
        if !metadata.is_file() {
            continue;
        }
        backups.push(Backup {
            path,
            reason,
            taken_at,
            size: metadata.len(),
        });
    }
    backups.sort_by(newest_first);
    Ok(backups)
}

fn newest_first(a: &Backup, b: &Backup) -> std::cmp::Ordering {
    (b.taken_at, &b.path).cmp(&(a.taken_at, &a.path))
}

/// Which of `backups` rotation deletes at `now`: the automatic backups that
/// are neither among the [`KEEP_RECENT`] most recent automatic ones nor the
/// newest automatic backup of a calendar month (UTC) before `now`'s. Manual
/// backups are never selected. Equal times are ordered by path, so the
/// choice doesn't depend on the order of `backups`.
pub fn to_delete(backups: &[Backup], now: DateTime<Utc>) -> Vec<&Backup> {
    let mut automatic: Vec<&Backup> = backups
        .iter()
        .filter(|backup| backup.reason.is_automatic())
        .collect();
    automatic.sort_by(|a, b| newest_first(a, b));

    let this_month = (now.year(), now.month());
    let mut months_kept = HashSet::new();
    let mut delete = Vec::new();
    for (index, backup) in automatic.into_iter().enumerate() {
        let month = (backup.taken_at.year(), backup.taken_at.month());
        let recent = index < KEEP_RECENT;
        // Newest first, so the first one seen in a month is its newest.
        let monthly = month < this_month && months_kept.insert(month);
        if !recent && !monthly {
            delete.push(backup);
        }
    }
    delete
}

/// Deletes the automatic backups [`to_delete`] selects at `now`, and
/// returns their paths. Only files that parse as backup names are ever
/// considered. One that is already gone (another process rotated) is
/// skipped; if others can't be deleted, the rest still are and the first
/// error is returned.
pub fn rotate(db_path: &Path, now: DateTime<Utc>) -> Result<Vec<PathBuf>, Error> {
    let backups = list(db_path)?;
    let mut deleted = Vec::new();
    let mut first_error = None;
    for backup in to_delete(&backups, now) {
        match fs::remove_file(&backup.path) {
            Ok(()) => deleted.push(backup.path.clone()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => {
                first_error.get_or_insert_with(|| io_error("delete", &backup.path)(err));
            }
        }
    }
    match first_error {
        Some(err) => Err(err),
        None => Ok(deleted),
    }
}

/// What restoring a backup would do: [`check`]'s answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreCheck {
    /// The backup file as given.
    pub path: PathBuf,
    /// The reason and time in its name, when it has a backup name.
    pub named: Option<(Reason, DateTime<Utc>)>,
    /// The backup's schema version.
    pub schema_version: SchemaVersion,
    /// This build's schema version: a backup below it is migrated after
    /// restoring.
    pub supported_version: SchemaVersion,
    /// The database a restore would replace.
    pub current: Current,
}

/// The database a restore replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Current {
    /// No file (or an empty one): nothing to back up first.
    Missing,
    /// A database at this schema version.
    At(SchemaVersion),
    /// A file that SQLite can't read. A restore would fail to back it up.
    Unreadable,
}

/// What a restore did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restored {
    /// The restored backup's schema version.
    pub from_version: SchemaVersion,
    /// The version it was migrated to, when it was older than this build's.
    pub migrated_to: Option<SchemaVersion>,
    /// The backup of the database it replaced, when there was one.
    pub pre_restore: Option<Backup>,
}

/// Checks `backup` as [`restore`] would, without restoring it, and reports
/// the database it would replace. Changes nothing: the checks run on a
/// temporary copy next to the database, which is removed again. (The data
/// directory is created, `0700`, if it doesn't exist.)
pub fn check(db_path: &Path, backup: &Path) -> Result<RestoreCheck, Error> {
    let (staged, schema_version) = stage(db_path, backup)?;
    drop(staged);
    let current = if is_non_empty_file(db_path)? {
        match db::peek_schema_version(db_path) {
            Ok(version) => Current::At(version),
            Err(db::Error::Sqlite(_)) => Current::Unreadable,
            Err(err) => return Err(err.into()),
        }
    } else {
        Current::Missing
    };
    Ok(RestoreCheck {
        path: backup.to_path_buf(),
        named: backup
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(parse_file_name),
        schema_version,
        supported_version: Database::supported_schema_version(),
        current,
    })
}

/// Restores `backup` over the database at `db_path`, then opens it with
/// [`Database::open`] so an older schema is migrated.
///
/// In order:
///
/// 1. `backup` is copied next to the database (`0600`), and the copy is
///    checked: a SQLite database that passes `PRAGMA integrity_check`, with
///    Coffer's migration history matching this build's and a schema no
///    newer. The user's file is only ever read, so the check can't change
///    it (or leave `-wal`/`-shm` files beside it).
/// 2. The current database, if there is one, is checkpointed and closed:
///    if its `-wal` or `-shm` stays, another program has it open and
///    nothing is restored. Then it is backed up ([`Reason::Restore`]). If
///    it can't be (e.g. it's damaged), nothing is restored: move it aside
///    to restore over it.
/// 3. It is checkpointed again and its sidecar files go.
/// 4. The checked copy is renamed over the database, and the directory
///    synced.
///
/// Any failure up to step 4 leaves the current database untouched and no
/// temporary file behind. If [`Database::open`] then fails (e.g. a
/// migration), the error is [`Error::RestoredButNotOpened`]: the backup is
/// in place at its old version (migrations are one transaction) and the
/// pre-restore backup holds what it replaced.
///
/// No other Coffer process may have the database open during a restore:
/// one that does keeps reading the replaced file. Step 3 notices an open
/// connection in WAL mode, but it can't rule one out.
// TODO(M2, lead): running MCP servers refuse calls while a restore swaps
// the files, and reopen the database afterwards (ARCHITECTURE.md →
// "Decision record: Settings, AI access and backups", point 6).
pub fn restore(db_path: &Path, backup: &Path) -> Result<Restored, Error> {
    let exists = is_non_empty_file(db_path)?;
    if !db_path.exists() {
        for suffix in ["-wal", "-journal"] {
            let sidecar = db::sidecar(db_path, suffix);
            if sidecar.exists() {
                return Err(Error::OrphanSidecar {
                    sidecar,
                    database: db_path.to_path_buf(),
                });
            }
        }
    }

    let (staged, from_version) = stage(db_path, backup)?;

    // Refuse an open database before backing it up, and again after (the
    // backup's own connection has come and gone).
    let pre_restore = if exists {
        let backup = release(db_path)
            .and_then(|()| create_from_path(db_path, Reason::Restore))
            .map_err(|source| match source {
                Error::InUse { .. } => source,
                source => Error::PreRestoreBackup {
                    path: db_path.to_path_buf(),
                    source: Box::new(source),
                },
            })?;
        Some(backup)
    } else {
        None
    };
    if db_path.exists() {
        release(db_path)?;
    }
    // A leftover `-shm` without a `-wal` holds nothing; SQLite rebuilds it.
    remove_if_exists(&db::sidecar(db_path, "-shm"))?;

    fs::rename(&staged.path, db_path).map_err(io_error("replace", db_path))?;
    staged.disarm();
    if let Some(dir) = db_path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        sync_dir(dir).map_err(io_error("sync", dir))?;
    }

    let opened = Database::open(db_path).and_then(|db| db.schema_version());
    let version = opened.map_err(|source| Error::RestoredButNotOpened {
        path: db_path.to_path_buf(),
        pre_restore: pre_restore.as_ref().map(|backup| backup.path.clone()),
        source,
    })?;
    Ok(Restored {
        from_version,
        migrated_to: (version != from_version).then_some(version),
        pre_restore,
    })
}

/// Copies `backup` next to the database and checks the copy. Returns the
/// copy (removed on drop unless disarmed), self-contained in rollback
/// journal mode with no sidecar files, and its schema version.
fn stage(db_path: &Path, backup: &Path) -> Result<(Scratch, SchemaVersion), Error> {
    let dir = db_path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    db::create_private_dir(dir).map_err(io_error("create", dir))?;

    let staged = Scratch::create(dir, "restore")?;
    copy_into(backup, &staged.path)?;
    // A copied live database may have committed pages in its -wal only.
    let wal = db::sidecar(backup, "-wal");
    if wal.exists() {
        let staged_wal = db::sidecar(&staged.path, "-wal");
        create_private_file(&staged_wal)?;
        copy_into(&wal, &staged_wal)?;
    }

    let version = check_copy(&staged.path, backup)?;
    for suffix in ["-wal", "-shm", "-journal"] {
        remove_if_exists(&db::sidecar(&staged.path, suffix))?;
    }
    finish_file(&staged.path)?;
    Ok((staged, version))
}

/// The checks on the staged copy at `copy` of `backup` (named in errors).
fn check_copy(copy: &Path, backup: &Path) -> Result<SchemaVersion, Error> {
    let not_a_database = |err: rusqlite::Error| match err.sqlite_error_code() {
        Some(rusqlite::ErrorCode::NotADatabase) => Error::NotADatabase {
            path: backup.to_path_buf(),
        },
        Some(rusqlite::ErrorCode::DatabaseCorrupt) => Error::Integrity {
            path: backup.to_path_buf(),
            count: 1,
            first: err.to_string(),
        },
        _ => Error::Sqlite(err),
    };

    let conn = db::connect(copy)?;
    conn.busy_timeout(db::BUSY_TIMEOUT)?;
    conn.pragma_update(None, "trusted_schema", false)
        .map_err(not_a_database)?;

    let problems: Vec<String> = conn
        .prepare("PRAGMA integrity_check")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get(0))?
                .collect::<Result<_, _>>()
        })
        .map_err(not_a_database)?;
    if problems != ["ok"] {
        return Err(Error::Integrity {
            path: backup.to_path_buf(),
            count: problems.len(),
            first: problems.into_iter().next().unwrap_or_default(),
        });
    }

    let version = db::check_schema(&conn).map_err(|err| match err {
        db::Error::SchemaTooNew { .. } | db::Error::MigrationHistory { .. } => {
            Error::Incompatible {
                path: backup.to_path_buf(),
                source: err,
            }
        }
        db::Error::Sqlite(err) => not_a_database(err),
        other => Error::Database(other),
    })?;
    if version == 0 {
        return Err(Error::NotCoffer {
            path: backup.to_path_buf(),
        });
    }

    // Fold any -wal into the file; `Database::open` switches it back.
    let mode: String =
        conn.pragma_update_and_check(None, "journal_mode", "delete", |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("delete") {
        return Err(Error::Io {
            action: "fold the -wal into",
            path: copy.to_path_buf(),
            source: io::Error::other(format!("SQLite kept it in {mode} mode")),
        });
    }
    Ok(version)
}

/// Checkpoints the database at `db_path` and closes it again, so its
/// `-wal` and `-shm` go. If they remain, another connection has it open.
fn release(db_path: &Path) -> Result<(), Error> {
    let conn = db::connect(db_path)?;
    conn.busy_timeout(db::BUSY_TIMEOUT)?;
    conn.pragma_update(None, "trusted_schema", false)?;
    // Reading rolls back a hot journal; the checkpoint empties the -wal.
    conn.query_row("SELECT count(*) FROM sqlite_schema", [], |_| Ok(()))?;
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    drop(conn);
    for suffix in ["-wal", "-journal"] {
        if db::sidecar(db_path, suffix).exists() {
            return Err(Error::InUse {
                path: db_path.to_path_buf(),
            });
        }
    }
    Ok(())
}

/// A file Coffer is writing that isn't a backup (or the database) yet:
/// hidden, `0600`, and removed with its sidecars when dropped unless
/// [`disarm`](Scratch::disarm)ed or published.
struct Scratch {
    path: PathBuf,
    armed: bool,
}

impl Scratch {
    /// Creates `.coffer-<kind>-<pid>-<n>.db` in `dir`, empty, `0600`. The
    /// name never parses as a backup's.
    fn create(dir: &Path, kind: &str) -> Result<Scratch, Error> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        loop {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = dir.join(format!(".coffer-{kind}-{}-{n}.db", std::process::id()));
            match create_private_file(&path) {
                Ok(()) => return Ok(Scratch { path, armed: true }),
                // Left by a crashed process that had the same ID.
                Err(Error::Io { source, .. }) if source.kind() == io::ErrorKind::AlreadyExists => {}
                Err(err) => return Err(err),
            }
        }
    }

    fn disarm(mut self) {
        self.armed = false;
    }

    /// Gives the file its backup name in `dir`, without replacing a file
    /// that has that name: within a process times strictly increase, and a
    /// name another process took moves on to the next millisecond.
    fn publish(self, dir: &Path, reason: Reason) -> Result<(PathBuf, DateTime<Utc>), Error> {
        loop {
            let taken_at = next_stamp();
            let target = dir.join(file_name(reason, taken_at));
            match fs::hard_link(&self.path, &target) {
                Ok(()) => {
                    // `self` drops here and removes the temporary name.
                    return Ok((target, taken_at));
                }
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
                // A file system without hard links: rename, after checking.
                Err(_) => {
                    if target.try_exists().map_err(io_error("read", &target))? {
                        continue;
                    }
                    fs::rename(&self.path, &target).map_err(io_error("name", &target))?;
                    self.disarm();
                    return Ok((target, taken_at));
                }
            }
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        for path in [
            self.path.clone(),
            db::sidecar(&self.path, "-wal"),
            db::sidecar(&self.path, "-shm"),
            db::sidecar(&self.path, "-journal"),
        ] {
            if let Err(err) = fs::remove_file(&path)
                && err.kind() != io::ErrorKind::NotFound
            {
                tracing::warn!(file = %path.display(), error = %err, "couldn't remove a temporary file");
            }
        }
    }
}

/// The current time, to the millisecond.
fn now() -> DateTime<Utc> {
    let now = DateTime::<Utc>::from(SystemTime::now());
    DateTime::from_timestamp_millis(now.timestamp_millis()).unwrap_or(now)
}

/// A backup time: now, or a millisecond after the last one this process
/// gave out, so two backups never get the same name.
fn next_stamp() -> DateTime<Utc> {
    static LAST: Mutex<Option<DateTime<Utc>>> = Mutex::new(None);
    let now = now();
    let mut last = LAST.lock().unwrap_or_else(PoisonError::into_inner);
    let stamp = match *last {
        Some(previous) if now <= previous => previous + TimeDelta::milliseconds(1),
        _ => now,
    };
    *last = Some(stamp);
    stamp
}

fn create_private_file(path: &Path) -> Result<(), Error> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path).map_err(io_error("create", path))?;
    Ok(())
}

/// Copies `from` into the existing file `to` (keeping `to`'s mode).
fn copy_into(from: &Path, to: &Path) -> Result<(), Error> {
    let mut source = File::open(from).map_err(io_error("read", from))?;
    let mut target = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(to)
        .map_err(io_error("write", to))?;
    io::copy(&mut source, &mut target).map_err(io_error("copy", from))?;
    Ok(())
}

/// Makes sure `file` is `0600` and on disk.
fn finish_file(file: &Path) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(file, fs::Permissions::from_mode(0o600))
            .map_err(io_error("protect", file))?;
    }
    File::open(file)
        .and_then(|file| file.sync_all())
        .map_err(io_error("sync", file))
}

/// Syncs a directory, so a rename in it survives a crash.
fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(dir)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

fn remove_if_exists(path: &Path) -> Result<(), Error> {
    match fs::remove_file(path) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => Err(io_error("remove", path)(err)),
        _ => Ok(()),
    }
}

fn is_non_empty_file(path: &Path) -> Result<bool, Error> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_file() && metadata.len() > 0),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(io_error("read", path)(err)),
    }
}
