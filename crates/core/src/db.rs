//! The SQLite database.
//!
//! One file, `coffer.db` in the data directory ([`crate::paths`]), shared by
//! the app, the CLI and the MCP server. There are two ways to open it:
//!
//! - [`Database::open`] (the app and the CLI) creates the database if needed
//!   and brings it up to date;
//! - [`Database::open_existing`] (`coffer mcp`) only opens a database that
//!   is already up to date, and never creates or migrates one.
//!
//! An existing database file is checked before anything is changed: it must
//! be a SQLite database, not from a newer Coffer, and its applied migrations
//! must match this build's. A file that fails is left exactly as it was: no
//! permission change, no WAL switch, no write. Then both make sure that:
//!
//! - the database file and its `-wal`/`-shm` files are `0600` (Unix): only
//!   the user can read them. [`Database::open`] also creates the data
//!   directory (`0700`) and the file (`0600`) when they don't exist;
//! - the database is in WAL mode, so the app and the MCP server can read
//!   while one of them writes, with a busy timeout instead of instant
//!   `SQLITE_BUSY` errors;
//! - writes start with `BEGIN IMMEDIATE` (the connection's default), so two
//!   writers queue on the busy timeout instead of failing mid-transaction;
//! - foreign keys are enforced, commits are fully synced and the schema is
//!   untrusted (`trusted_schema = OFF`).
//!
//! [`Database::open`] then applies every pending migration
//! (`crates/core/migrations/`), all in one transaction.
//!
//! One connection per process, no pool (`docs/ARCHITECTURE.md` →
//! Dependencies).

#[cfg(test)]
mod category_tests;
mod migrations;
#[cfg(test)]
mod schema_tests;

use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::{Connection, OpenFlags, TransactionBehavior};

use crate::category::CategoryTree;
use crate::paths::{self, NoDataDir};

/// A schema version: the number of the last migration applied, `0` for a
/// database no migration has touched.
pub type SchemaVersion = i64;

/// How long a statement waits for another process's lock before failing
/// with `SQLITE_BUSY`. Imports commit in one transaction, so a reader or a
/// second writer may wait for a whole import.
const BUSY_TIMEOUT: Duration = Duration::from_secs(10);

/// Why the database couldn't be opened.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    NoDataDir(#[from] NoDataDir),

    #[error("couldn't prepare {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("SQLite wouldn't switch the database to WAL mode (it reports {0:?})")]
    NotWal(String),

    #[error(
        "the database is at schema version {found}, newer than this version of Coffer \
         supports ({supported}); update Coffer to open it"
    )]
    SchemaTooNew {
        found: SchemaVersion,
        supported: SchemaVersion,
    },

    #[error(
        "the database's migration history doesn't match this version of Coffer: \
         migration {version} {problem}"
    )]
    MigrationHistory {
        version: SchemaVersion,
        problem: HistoryProblem,
    },

    #[error("couldn't migrate the database: {0}")]
    Migration(#[source] Box<refinery::Error>),

    #[error(
        "there is no Coffer database at {}; open Coffer, or run a `coffer` command, \
         to create it",
        path.display()
    )]
    NoDatabase { path: PathBuf },

    #[error(
        "the database is at schema version {found}, older than this version of Coffer \
         needs ({supported}); open Coffer, or run a `coffer` command, to update it"
    )]
    SchemaTooOld {
        found: SchemaVersion,
        supported: SchemaVersion,
    },
}

/// How a database's applied migrations differ from this build's
/// ([`Error::MigrationHistory`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryProblem {
    /// It was applied with a different name or SQL than this build's
    /// migration of the same version: a shipped migration was edited.
    Changed,
    /// It was applied, but this build has no migration with its version.
    Unknown,
    /// This build has it, but it was never applied although later ones
    /// were.
    Skipped,
}

impl std::fmt::Display for HistoryProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Changed => "was changed after it was applied",
            Self::Unknown => "was applied but is unknown to this build",
            Self::Skipped => "was never applied, though later ones were",
        })
    }
}

/// An open, configured and migrated Coffer database.
#[derive(Debug)]
pub struct Database {
    conn: Connection,
    path: PathBuf,
}

impl Database {
    /// Opens the database in the data directory ([`paths::database_path`]),
    /// creating it if needed.
    pub fn open_default() -> Result<Self, Error> {
        Self::open(&paths::database_path()?)
    }

    /// Opens the database at `path`, creating the file and its directory if
    /// needed, and applies pending migrations. See the module docs for what
    /// is checked and set.
    pub fn open(path: &Path) -> Result<Self, Error> {
        Self::open_with(path, &migrations::embedded())
    }

    fn open_with(path: &Path, migrations: &[refinery::Migration]) -> Result<Self, Error> {
        // An empty file is what `prepare_files` itself creates (and what a
        // concurrent open may have just created): nothing to protect yet.
        if is_non_empty_file(path).map_err(io_error(path))? {
            migrations::check(&inspect(path)?, migrations)?;
        }
        prepare_files(path)?;
        let mut conn = connect(path)?;
        configure(&mut conn)?;
        migrations::run(&mut conn, migrations)?;
        Ok(Self {
            conn,
            path: path.to_path_buf(),
        })
    }

    /// Opens the database in the data directory ([`paths::database_path`])
    /// without creating or migrating it. See [`Database::open_existing`].
    pub fn open_existing_default() -> Result<Self, Error> {
        Self::open_existing(&paths::database_path()?)
    }

    /// Opens the database at `path` only if it exists and is at exactly
    /// the schema version this build supports. For `coffer mcp`, which must
    /// never create or migrate the database: the app and the CLI do that.
    ///
    /// Fails with [`Error::NoDatabase`] when there is no file,
    /// [`Error::SchemaTooOld`] when migrations are pending (including an
    /// empty file), [`Error::SchemaTooNew`] for a database from a newer
    /// Coffer and [`Error::MigrationHistory`] when its applied migrations
    /// differ from this build's. A refused file is left untouched.
    ///
    /// The connection is read-write (MCP tools write the inbox tables) and
    /// configured like [`Database::open`]'s. Once the file has passed those
    /// checks, it also gets the same two fixes `open` makes, because both
    /// keep a verified Coffer database as every Coffer process expects it
    /// and neither touches its contents:
    ///
    /// - loose permissions on the file and its `-wal`/`-shm` are tightened
    ///   to `0600`: the MCP server may be the only Coffer process running
    ///   for a long time, and leaving financial data world-readable until
    ///   the app next opens it helps nobody;
    /// - a database not in WAL mode (e.g. copied or restored with another
    ///   tool) is switched to WAL, which `open` would do anyway. Without it
    ///   the MCP server's reads would block the app's writes. The journal
    ///   mode is a header setting, not the schema or the data.
    pub fn open_existing(path: &Path) -> Result<Self, Error> {
        Self::open_existing_with(path, &migrations::embedded())
    }

    fn open_existing_with(path: &Path, migrations: &[refinery::Migration]) -> Result<Self, Error> {
        match std::fs::metadata(path) {
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                return Err(Error::NoDatabase {
                    path: path.to_path_buf(),
                });
            }
            Err(err) => return Err(io_error(path)(err)),
        }
        let supported = migrations::latest_version(migrations);
        let up_to_date = |found| {
            if found < supported {
                Err(Error::SchemaTooOld { found, supported })
            } else {
                Ok(())
            }
        };
        up_to_date(migrations::check(&inspect(path)?, migrations)?)?;

        restrict_files_to_owner(path)?;
        let mut conn = connect(path)?;
        configure(&mut conn)?;
        // Again on the real connection, in case another process changed the
        // file since the check.
        up_to_date(migrations::check(&conn, migrations)?)?;
        Ok(Self {
            conn,
            path: path.to_path_buf(),
        })
    }

    /// The database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The schema version of this database. After [`Database::open`] it is
    /// [`Database::supported_schema_version`].
    pub fn schema_version(&self) -> Result<SchemaVersion, Error> {
        Ok(migrations::current_version(&self.conn)?)
    }

    /// The newest schema version this build of Coffer knows: the number of
    /// its last migration.
    pub fn supported_schema_version() -> SchemaVersion {
        migrations::latest_version(&migrations::embedded())
    }

    /// Every category, as a two-level tree that looks categories up by key
    /// ([`CategoryTree`]).
    pub fn categories(&self) -> Result<CategoryTree, Error> {
        Ok(CategoryTree::load(&self.conn)?)
    }

    /// The connection, for core's own modules. It isn't public: every write
    /// goes through core's API, which keeps the invariants the schema can't
    /// check alone.
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    pub(crate) fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
}

/// Opens a read-write connection to an existing file. Not
/// `SQLITE_OPEN_CREATE`: [`prepare_files`] creates the file with private
/// permissions, and SQLite gives the -wal and -shm files the main file's
/// mode.
fn connect(path: &Path) -> Result<Connection, Error> {
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?)
}

/// A connection for checking an existing file before anything changes it:
/// `query_only`, and the journal mode is left as it is.
///
/// Read-write rather than `SQLITE_OPEN_READ_ONLY`: a read-only connection to
/// a WAL database creates the `-wal`/`-shm` files but can't remove them on
/// close, whereas the last read-write connection to close removes them.
/// Reading checks the file is a SQLite database (`SQLITE_NOTADB` otherwise).
fn inspect(path: &Path) -> Result<Connection, Error> {
    let conn = connect(path)?;
    conn.busy_timeout(BUSY_TIMEOUT)?;
    conn.pragma_update(None, "query_only", true)?;
    conn.pragma_update(None, "trusted_schema", false)?;
    Ok(conn)
}

/// Sets the per-connection options. `journal_mode` is stored in the file;
/// the rest must be set on every connection.
fn configure(conn: &mut Connection) -> Result<(), Error> {
    conn.busy_timeout(BUSY_TIMEOUT)?;
    enable_wal(conn)?;

    // NORMAL is the usual pairing with WAL, but it can lose the last commits
    // on power loss. Writes are rare here (imports, edits), so pay for FULL.
    conn.pragma_update(None, "synchronous", "FULL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    // Schema objects (views, triggers) may not call functions with side
    // effects; a tampered file can't run anything through them.
    conn.pragma_update(None, "trusted_schema", false)?;

    conn.set_transaction_behavior(TransactionBehavior::Immediate);
    Ok(())
}

/// Switches the file to WAL mode (a no-op when it already is).
///
/// SQLite answers `SQLITE_BUSY` here without calling the busy handler when
/// another connection holds a lock, e.g. the app and the CLI opening a new
/// database at the same moment. So retry until the busy timeout runs out.
fn enable_wal(conn: &Connection) -> Result<(), Error> {
    let deadline = Instant::now() + BUSY_TIMEOUT;
    let mode: String = loop {
        match conn.pragma_update_and_check(None, "journal_mode", "wal", |row| row.get(0)) {
            Ok(mode) => break mode,
            Err(err)
                if err.sqlite_error_code() == Some(rusqlite::ErrorCode::DatabaseBusy)
                    && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(err) => return Err(err.into()),
        }
    };
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(Error::NotWal(mode));
    }
    Ok(())
}

/// Creates the data directory and the database file with private
/// permissions, and tightens permissions on existing files that are looser.
fn prepare_files(path: &Path) -> Result<(), Error> {
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        create_private_dir(dir).map_err(io_error(dir))?;
    }

    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path).map_err(io_error(path))?;

    restrict_files_to_owner(path)
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> Error {
    let path = path.to_path_buf();
    move |source| Error::Io { path, source }
}

/// Whether `path` is a file with something in it. A missing file isn't.
fn is_non_empty_file(path: &Path) -> io::Result<bool> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_file() && metadata.len() > 0),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

/// Tightens loose permissions on the database file and its `-wal`/`-shm`.
fn restrict_files_to_owner(path: &Path) -> Result<(), Error> {
    for file in [
        path.to_path_buf(),
        sidecar(path, "-wal"),
        sidecar(path, "-shm"),
    ] {
        restrict_to_owner(&file).map_err(io_error(&file))?;
    }
    Ok(())
}

/// `coffer.db` → `coffer.db-wal`, `coffer.db-shm`.
fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(suffix);
    PathBuf::from(name)
}

/// Creates `dir` as `0700` if it doesn't exist. An existing directory is
/// left alone: it may be one the user chose (`COFFER_DATA_DIR`).
fn create_private_dir(dir: &Path) -> io::Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    match builder.create(dir) {
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists && dir.is_dir() => Ok(()),
        result => result,
    }
}

/// Sets `0600` on `file` if it exists and group or others have any access.
#[cfg(unix)]
fn restrict_to_owner(file: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mode = match std::fs::metadata(file) {
        Ok(metadata) => metadata.permissions().mode(),
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    if mode & 0o077 != 0 {
        tracing::warn!(
            file = %file.display(),
            mode = format_args!("{:o}", mode & 0o777),
            "database file was readable by others; restricting it to the owner"
        );
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn restrict_to_owner(_file: &Path) -> io::Result<()> {
    // Windows is community-supported (ARCHITECTURE.md → Platforms); the
    // per-user data dir's ACL is what protects the file there.
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};

    use refinery::Migration;
    use tempfile::TempDir;

    use super::*;

    fn migration(name: &str, sql: &str) -> Migration {
        Migration::unapplied(name, sql).unwrap()
    }

    fn v1() -> Migration {
        migration("V1__first", "CREATE TABLE first (id INTEGER PRIMARY KEY);")
    }

    fn v2() -> Migration {
        migration(
            "V2__second",
            "CREATE TABLE second (id INTEGER PRIMARY KEY);",
        )
    }

    fn v3() -> Migration {
        migration("V3__third", "CREATE TABLE third (id INTEGER PRIMARY KEY);")
    }

    fn db_path(dir: &TempDir) -> PathBuf {
        dir.path().join("data").join(paths::DATABASE_FILE)
    }

    fn table_exists(db: &Database, name: &str) -> bool {
        db.conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)",
                [name],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn creates_the_directory_and_the_database() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);

        let db = Database::open(&path).unwrap();

        assert!(path.is_file());
        assert_eq!(db.path(), path);
        assert_eq!(
            db.schema_version().unwrap(),
            Database::supported_schema_version()
        );
    }

    #[test]
    fn reopening_is_a_no_op() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);

        drop(Database::open_with(&path, &[v1(), v2()]).unwrap());
        let db = Database::open_with(&path, &[v1(), v2()]).unwrap();

        assert_eq!(db.schema_version().unwrap(), 2);
    }

    #[test]
    fn connection_settings() {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&db_path(&dir)).unwrap();
        let pragma = |name: &str| -> String {
            db.conn
                .pragma_query_value(None, name, |row| row.get::<_, rusqlite::types::Value>(0))
                .map(|value| match value {
                    rusqlite::types::Value::Integer(n) => n.to_string(),
                    rusqlite::types::Value::Text(text) => text,
                    other => panic!("unexpected {name} value {other:?}"),
                })
                .unwrap()
        };

        assert_eq!(pragma("journal_mode"), "wal");
        assert_eq!(pragma("synchronous"), "2", "FULL");
        assert_eq!(pragma("foreign_keys"), "1");
        assert_eq!(pragma("trusted_schema"), "0");
        assert_eq!(pragma("busy_timeout"), BUSY_TIMEOUT.as_millis().to_string());
    }

    #[test]
    fn writes_begin_immediate() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        let mut first = Database::open_with(&path, &[v1()]).unwrap();
        let second = Database::open_with(&path, &[v1()]).unwrap();
        second.conn.busy_timeout(Duration::ZERO).unwrap();

        // A deferred transaction would take no lock until its first write.
        let tx = first.conn.transaction().unwrap();
        let err = second
            .conn
            .execute("INSERT INTO first (id) VALUES (1)", [])
            .unwrap_err();
        assert_eq!(
            err.sqlite_error_code(),
            Some(rusqlite::ErrorCode::DatabaseBusy)
        );
        drop(tx);
    }

    #[cfg(unix)]
    mod permissions {
        use std::os::unix::fs::PermissionsExt;

        use super::*;

        fn mode(path: &Path) -> u32 {
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777
        }

        #[test]
        fn new_files_are_private() {
            let dir = TempDir::new().unwrap();
            let path = db_path(&dir);

            // Keep the connection open so the -wal and -shm files exist.
            let _db = Database::open_with(&path, &[v1()]).unwrap();

            assert_eq!(mode(path.parent().unwrap()), 0o700);
            for file in [path.clone(), sidecar(&path, "-wal"), sidecar(&path, "-shm")] {
                assert!(file.exists(), "{} missing", file.display());
                assert_eq!(mode(&file), 0o600, "{}", file.display());
            }
        }

        #[test]
        fn loose_permissions_are_tightened() {
            let dir = TempDir::new().unwrap();
            let path = db_path(&dir);
            drop(Database::open_with(&path, &[v1()]).unwrap());
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

            let _db = Database::open_with(&path, &[v1()]).unwrap();

            assert_eq!(mode(&path), 0o600);
        }

        #[test]
        fn an_existing_directory_is_left_alone() {
            let dir = TempDir::new().unwrap();
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();

            let _db = Database::open(&dir.path().join(paths::DATABASE_FILE)).unwrap();

            assert_eq!(mode(dir.path()), 0o755);
        }
    }

    #[test]
    fn applies_new_migrations_on_open() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        drop(Database::open_with(&path, &[v1()]).unwrap());

        let db = Database::open_with(&path, &[v1(), v2()]).unwrap();

        assert_eq!(db.schema_version().unwrap(), 2);
        assert!(table_exists(&db, "first"));
        assert!(table_exists(&db, "second"));
    }

    #[test]
    fn a_failed_migration_applies_nothing() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        let broken = migration("V2__broken", "CREATE TABLE second (;");

        let err = Database::open_with(&path, &[v1(), broken]).unwrap_err();
        assert!(matches!(err, Error::Migration(_)), "{err:?}");

        let db = Database::open_with(&path, &[]).unwrap();
        assert_eq!(db.schema_version().unwrap(), 0);
        assert!(!table_exists(&db, "first"));
    }

    /// A file's contents and (on Unix) mode, to check a refused open
    /// changed nothing.
    fn snapshot(path: &Path) -> (Vec<u8>, u32) {
        #[cfg(unix)]
        let mode = std::os::unix::fs::PermissionsExt::mode(
            &std::fs::metadata(path).unwrap().permissions(),
        ) & 0o777;
        #[cfg(not(unix))]
        let mode = 0;
        (std::fs::read(path).unwrap(), mode)
    }

    #[cfg(unix)]
    fn set_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    fn assert_no_sidecars(path: &Path) {
        for suffix in ["-wal", "-shm", "-journal"] {
            let file = sidecar(path, suffix);
            assert!(!file.exists(), "{} was left behind", file.display());
        }
    }

    /// A closed database at version 2 in rollback-journal mode with loose
    /// permissions: what a newer Coffer might leave, and everything an
    /// open could wrongly change.
    fn newer_database(dir: &TempDir) -> PathBuf {
        let path = db_path(dir);
        drop(Database::open_with(&path, &[v1(), v2()]).unwrap());
        let conn = Connection::open(&path).unwrap();
        let mode: String = conn
            .pragma_update_and_check(None, "journal_mode", "delete", |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "delete");
        drop(conn);
        #[cfg(unix)]
        set_mode(&path, 0o644);
        assert_no_sidecars(&path);
        path
    }

    /// SQLite's header: bytes 18 and 19 are 2 in WAL mode, 1 otherwise.
    fn is_wal(path: &Path) -> bool {
        std::fs::read(path).unwrap()[18] == 2
    }

    #[test]
    fn refuses_a_newer_schema_and_leaves_it_alone() {
        let dir = TempDir::new().unwrap();
        let path = newer_database(&dir);
        let before = snapshot(&path);

        let err = Database::open_with(&path, &[v1()]).unwrap_err();

        assert!(
            matches!(
                err,
                Error::SchemaTooNew {
                    found: 2,
                    supported: 1
                }
            ),
            "{err:?}"
        );
        assert_eq!(snapshot(&path), before);
        assert!(!is_wal(&path), "the journal mode was changed");
        assert_no_sidecars(&path);
    }

    #[test]
    fn refuses_a_newer_schema_in_wal_mode_without_leaving_files() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        drop(Database::open_with(&path, &[v1(), v2()]).unwrap());
        let before = snapshot(&path);

        let err = Database::open_with(&path, &[v1()]).unwrap_err();

        assert!(matches!(err, Error::SchemaTooNew { .. }), "{err:?}");
        assert_eq!(snapshot(&path), before);
        assert_no_sidecars(&path);
    }

    #[test]
    fn refuses_an_edited_migration_and_leaves_it_alone() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        drop(Database::open_with(&path, &[v1()]).unwrap());
        let before = snapshot(&path);
        let edited = migration("V1__first", "CREATE TABLE first (id INTEGER);");

        let err = Database::open_with(&path, &[edited]).unwrap_err();

        assert!(
            matches!(
                err,
                Error::MigrationHistory {
                    version: 1,
                    problem: HistoryProblem::Changed
                }
            ),
            "{err:?}"
        );
        assert_eq!(snapshot(&path), before);
        assert_no_sidecars(&path);
    }

    #[test]
    fn refuses_a_file_that_is_not_a_database_and_leaves_it_alone() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("notes.txt");
        let contents = "this is not a database, and it is long enough to have a header\n";
        std::fs::write(&path, contents).unwrap();
        #[cfg(unix)]
        set_mode(&path, 0o644);
        let before = snapshot(&path);

        let err = Database::open(&path).unwrap_err();

        assert!(
            matches!(&err, Error::Sqlite(err)
                if err.sqlite_error_code() == Some(rusqlite::ErrorCode::NotADatabase)),
            "{err:?}"
        );
        assert_eq!(snapshot(&path), before);
        assert_eq!(before.1, if cfg!(unix) { 0o644 } else { 0 });
        assert_no_sidecars(&path);

        let err = Database::open_existing(&path).unwrap_err();
        assert!(matches!(err, Error::Sqlite(_)), "{err:?}");
        assert_eq!(snapshot(&path), before);
        assert_no_sidecars(&path);
    }

    #[test]
    fn an_empty_file_becomes_a_database() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "").unwrap();

        let db = Database::open_with(&path, &[v1()]).unwrap();

        assert_eq!(db.schema_version().unwrap(), 1);
    }

    mod open_existing {
        use super::*;

        #[test]
        fn opens_an_up_to_date_database() {
            let dir = TempDir::new().unwrap();
            let path = db_path(&dir);
            drop(Database::open_with(&path, &[v1(), v2()]).unwrap());

            let db = Database::open_existing_with(&path, &[v1(), v2()]).unwrap();

            assert_eq!(db.path(), path);
            assert_eq!(db.schema_version().unwrap(), 2);
            db.conn
                .execute("INSERT INTO second (id) VALUES (1)", [])
                .unwrap();
        }

        #[test]
        fn opens_the_embedded_schema() {
            let dir = TempDir::new().unwrap();
            let path = db_path(&dir);
            drop(Database::open(&path).unwrap());

            let db = Database::open_existing(&path).unwrap();

            assert_eq!(
                db.schema_version().unwrap(),
                Database::supported_schema_version()
            );
        }

        #[test]
        fn configures_the_connection_like_open() {
            let dir = TempDir::new().unwrap();
            let path = newer_database(&dir);
            // At version 2, so up to date for these migrations; not in WAL
            // mode and readable by others (see `newer_database`).

            let db = Database::open_existing_with(&path, &[v1(), v2()]).unwrap();
            let pragma = |name: &str| -> String {
                db.conn
                    .pragma_query_value(None, name, |row| row.get::<_, rusqlite::types::Value>(0))
                    .map(|value| match value {
                        rusqlite::types::Value::Integer(n) => n.to_string(),
                        rusqlite::types::Value::Text(text) => text,
                        other => panic!("unexpected {name} value {other:?}"),
                    })
                    .unwrap()
            };

            assert_eq!(pragma("journal_mode"), "wal");
            assert_eq!(pragma("synchronous"), "2", "FULL");
            assert_eq!(pragma("foreign_keys"), "1");
            assert_eq!(pragma("trusted_schema"), "0");
            assert_eq!(pragma("query_only"), "0");
            assert_eq!(pragma("busy_timeout"), BUSY_TIMEOUT.as_millis().to_string());
            #[cfg(unix)]
            assert_eq!(snapshot(&path).1, 0o600);
        }

        #[test]
        fn writes_begin_immediate() {
            let dir = TempDir::new().unwrap();
            let path = db_path(&dir);
            drop(Database::open_with(&path, &[v1()]).unwrap());
            let mut first = Database::open_existing_with(&path, &[v1()]).unwrap();
            let second = Database::open_existing_with(&path, &[v1()]).unwrap();
            second.conn.busy_timeout(Duration::ZERO).unwrap();

            let tx = first.conn.transaction().unwrap();
            let err = second
                .conn
                .execute("INSERT INTO first (id) VALUES (1)", [])
                .unwrap_err();
            assert_eq!(
                err.sqlite_error_code(),
                Some(rusqlite::ErrorCode::DatabaseBusy)
            );
            drop(tx);
        }

        #[test]
        fn refuses_a_missing_database_without_creating_it() {
            let dir = TempDir::new().unwrap();
            let path = db_path(&dir);

            let err = Database::open_existing_with(&path, &[v1()]).unwrap_err();

            assert!(
                matches!(&err, Error::NoDatabase { path: missing } if *missing == path),
                "{err:?}"
            );
            assert!(!path.exists());
            assert!(!path.parent().unwrap().exists());
        }

        #[test]
        fn refuses_an_older_schema_and_leaves_it_alone() {
            let dir = TempDir::new().unwrap();
            let path = newer_database(&dir);
            let before = snapshot(&path);

            let err = Database::open_existing_with(&path, &[v1(), v2(), v3()]).unwrap_err();

            assert!(
                matches!(
                    err,
                    Error::SchemaTooOld {
                        found: 2,
                        supported: 3
                    }
                ),
                "{err:?}"
            );
            assert_eq!(snapshot(&path), before);
            assert!(!is_wal(&path));
            assert_no_sidecars(&path);
        }

        #[test]
        fn refuses_an_empty_file_and_leaves_it_alone() {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join(paths::DATABASE_FILE);
            std::fs::write(&path, "").unwrap();
            #[cfg(unix)]
            set_mode(&path, 0o644);
            let before = snapshot(&path);

            let err = Database::open_existing_with(&path, &[v1()]).unwrap_err();

            assert!(
                matches!(
                    err,
                    Error::SchemaTooOld {
                        found: 0,
                        supported: 1
                    }
                ),
                "{err:?}"
            );
            assert_eq!(snapshot(&path), before);
            assert_no_sidecars(&path);
        }

        #[test]
        fn refuses_a_newer_schema_and_leaves_it_alone() {
            let dir = TempDir::new().unwrap();
            let path = newer_database(&dir);
            let before = snapshot(&path);

            let err = Database::open_existing_with(&path, &[v1()]).unwrap_err();

            assert!(
                matches!(
                    err,
                    Error::SchemaTooNew {
                        found: 2,
                        supported: 1
                    }
                ),
                "{err:?}"
            );
            assert_eq!(snapshot(&path), before);
            assert!(!is_wal(&path));
            assert_no_sidecars(&path);
        }

        #[test]
        fn refuses_an_edited_migration_and_leaves_it_alone() {
            let dir = TempDir::new().unwrap();
            let path = newer_database(&dir);
            let before = snapshot(&path);
            let edited = migration("V2__second", "CREATE TABLE second (id TEXT);");

            let err = Database::open_existing_with(&path, &[v1(), edited]).unwrap_err();

            assert!(
                matches!(
                    err,
                    Error::MigrationHistory {
                        version: 2,
                        problem: HistoryProblem::Changed
                    }
                ),
                "{err:?}"
            );
            assert_eq!(snapshot(&path), before);
            assert_no_sidecars(&path);
        }
    }

    #[test]
    fn concurrent_opens_migrate_once() {
        let dir = TempDir::new().unwrap();
        let path = Arc::new(db_path(&dir));
        let threads = 8;
        let barrier = Arc::new(Barrier::new(threads));

        let handles: Vec<_> = (0..threads)
            .map(|_| {
                let path = Arc::clone(&path);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    Database::open_with(&path, &[v1(), v2()]).map(|db| db.schema_version())
                })
            })
            .collect();

        for handle in handles {
            assert_eq!(handle.join().unwrap().unwrap().unwrap(), 2);
        }
    }
}
