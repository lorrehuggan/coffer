use chrono::NaiveDateTime;
use rusqlite::OpenFlags;
use tempfile::TempDir;

use super::*;
use crate::account::{self, AccountKind, NewAccount};
use crate::money::Currency;

fn at(stamp: &str) -> DateTime<Utc> {
    NaiveDateTime::parse_from_str(stamp, "%Y-%m-%d %H:%M:%S%.3f")
        .unwrap_or_else(|err| panic!("{stamp}: {err}"))
        .and_utc()
}

fn db_path(dir: &TempDir) -> PathBuf {
    dir.path().join("data").join("coffer.db")
}

/// A database with one account, `name`, closed again.
fn database_with(path: &Path, name: &str) {
    let mut db = Database::open(path).unwrap();
    account::create(
        &mut db,
        &NewAccount::new(name, AccountKind::Current, Currency::GBP),
    )
    .unwrap();
}

/// The account names in the database file at `path`, read without changing
/// it.
fn account_names(path: &Path) -> Vec<String> {
    let conn = Connection::open_with_flags(
        format!("file:{}?immutable=1", path.display()),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .unwrap();
    let mut statement = conn
        .prepare("SELECT name FROM account ORDER BY id")
        .unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// The names in `dir`, sorted.
fn dir_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// No temporary file of ours (`.coffer-*`) in the data or backups dir.
fn assert_no_scratch_files(db_path: &Path) {
    for dir in [
        db_path.parent().unwrap().to_path_buf(),
        backups_dir(db_path),
    ] {
        if !dir.exists() {
            continue;
        }
        let names = dir_names(&dir);
        assert!(
            names.iter().all(|name| !name.starts_with(".coffer-")),
            "temporary files left in {}: {names:?}",
            dir.display()
        );
    }
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

// Names ---------------------------------------------------------------------

#[test]
fn names_round_trip_and_sort_by_time() {
    let taken_at = at("2026-11-11 15:30:12.345");
    let name = file_name(Reason::Import, taken_at);
    assert_eq!(name, "coffer-20261111T153012.345Z-import.db");
    assert_eq!(parse_file_name(&name), Some((Reason::Import, taken_at)));

    for reason in Reason::ALL {
        let name = file_name(reason, taken_at);
        assert_eq!(parse_file_name(&name), Some((reason, taken_at)), "{name}");
    }

    let earlier = file_name(Reason::Manual, at("2026-11-11 15:30:12.344"));
    let later = file_name(Reason::Import, at("2026-11-11 15:30:12.345"));
    assert!(earlier < later);
}

#[test]
fn other_names_dont_parse() {
    for name in [
        "coffer.db",
        "coffer-20261111T153012.345Z-import.db-wal",
        "coffer-20261111T153012.345Z-import.sqlite",
        "coffer-20261111T153012.345Z-weekly.db",
        "coffer-20261111T153012Z-import.db",
        "coffer-20261111T153012.3456Z-import.db",
        "coffer-20261311T153012.345Z-import.db",
        "coffer-20261111T153012.345-import.db",
        "Coffer-20261111T153012.345Z-import.db",
        ".coffer-partial-1-0.db",
        "notes.txt",
    ] {
        assert_eq!(parse_file_name(name), None, "{name}");
    }
}

#[test]
fn only_manual_backups_are_kept_forever() {
    assert!(Reason::Import.is_automatic());
    assert!(Reason::Migration.is_automatic());
    assert!(Reason::Restore.is_automatic());
    assert!(!Reason::Manual.is_automatic());
}

#[test]
fn backups_sit_next_to_the_database() {
    assert_eq!(
        backups_dir(Path::new("/data/coffer.db")),
        Path::new("/data/backups")
    );
    assert_eq!(backups_dir(Path::new("coffer.db")), Path::new("backups"));
}

// Creating ------------------------------------------------------------------

#[test]
fn create_writes_a_private_copy_of_the_database() {
    let dir = TempDir::new().unwrap();
    let path = db_path(&dir);
    database_with(&path, "Everyday");
    let db = Database::open(&path).unwrap();

    let backup = create(&db, Reason::Manual).unwrap();

    assert_eq!(backup.reason, Reason::Manual);
    assert_eq!(backup.path.parent().unwrap(), backups_dir(&path));
    assert_eq!(backup.size, fs::metadata(&backup.path).unwrap().len());
    assert!(backup.size > 0);
    #[cfg(unix)]
    {
        assert_eq!(mode(&backup.path), 0o600);
        assert_eq!(mode(&backups_dir(&path)), 0o700);
    }
    assert_eq!(account_names(&backup.path), ["Everyday"]);
    assert_eq!(list(&path).unwrap(), std::slice::from_ref(&backup));
    // Only the backup: no temporary or sidecar file.
    assert_eq!(
        dir_names(&backups_dir(&path)),
        [backup.path.file_name().unwrap().to_str().unwrap()]
    );

    // It's a valid Coffer database at this build's version.
    let conn = db::connect(&backup.path).unwrap();
    assert_eq!(
        db::check_schema(&conn).unwrap(),
        Database::supported_schema_version()
    );
}

#[test]
fn backups_taken_at_once_get_distinct_names() {
    let dir = TempDir::new().unwrap();
    let path = db_path(&dir);
    let db = Database::open(&path).unwrap();

    let names: HashSet<PathBuf> = (0..5)
        .map(|_| create(&db, Reason::Manual).unwrap().path)
        .collect();

    assert_eq!(names.len(), 5);
    assert_eq!(list(&path).unwrap().len(), 5);
}

#[test]
fn create_from_path_reads_an_older_database_without_changing_it() {
    let dir = TempDir::new().unwrap();
    let path = db_path(&dir);
    drop(db::open_at_version(&path, 2).unwrap());
    let before = fs::read(&path).unwrap();

    let backup = create_from_path(&path, Reason::Migration).unwrap();

    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(backup.reason, Reason::Migration);
    let conn = db::connect(&backup.path).unwrap();
    assert_eq!(db::check_schema(&conn).unwrap(), 2);
}

#[test]
fn create_with_connection_uses_the_open_connection() {
    let dir = TempDir::new().unwrap();
    let path = db_path(&dir);
    database_with(&path, "Everyday");
    let db = Database::open(&path).unwrap();

    let backup = create_with_connection(db.conn(), &path, Reason::Migration).unwrap();

    assert_eq!(account_names(&backup.path), ["Everyday"]);
}

#[test]
fn automatic_backups_rotate_and_manual_ones_stay() {
    let dir = TempDir::new().unwrap();
    let path = db_path(&dir);
    let db = Database::open(&path).unwrap();
    let manual = create(&db, Reason::Manual).unwrap();
    let foreign = backups_dir(&path).join("my-copy.db");
    fs::write(&foreign, "not ours").unwrap();

    let imports: Vec<Backup> = (0..KEEP_RECENT + 2)
        .map(|_| create(&db, Reason::Import).unwrap())
        .collect();

    let left = list(&path).unwrap();
    assert!(left.contains(&manual));
    let automatic: Vec<&Backup> = left.iter().filter(|b| b.reason.is_automatic()).collect();
    assert_eq!(automatic.len(), KEEP_RECENT);
    // The newest ten, newest first.
    let newest: Vec<&Backup> = imports.iter().rev().take(KEEP_RECENT).collect();
    assert_eq!(automatic, newest);
    assert!(foreign.exists());
}

// Rotation ------------------------------------------------------------------

fn backup(reason: Reason, stamp: &str) -> Backup {
    let taken_at = at(stamp);
    Backup {
        path: PathBuf::from(file_name(reason, taken_at)),
        reason,
        taken_at,
        size: 0,
    }
}

fn deleted<'a>(backups: &'a [Backup], now: &str) -> Vec<&'a str> {
    let mut names: Vec<&str> = to_delete(backups, at(now))
        .into_iter()
        .map(|b| b.path.to_str().unwrap())
        .collect();
    names.sort();
    names
}

/// `count` import backups on consecutive days from `first` (a date).
fn daily(first: &str, count: usize) -> Vec<Backup> {
    let start = at(&format!("{first} 12:00:00.000"));
    (0..count)
        .map(|day| {
            let taken_at = start + TimeDelta::days(day as i64);
            backup(
                Reason::Import,
                &taken_at.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            )
        })
        .collect()
}

#[test]
fn keeps_exactly_ten_recent() {
    let ten = daily("2026-11-01", 10);
    assert!(deleted(&ten, "2026-11-20 00:00:00.000").is_empty());

    let eleven = daily("2026-11-01", 11);
    assert_eq!(
        deleted(&eleven, "2026-11-20 00:00:00.000"),
        [file_name(Reason::Import, at("2026-11-01 12:00:00.000"))]
    );
}

#[test]
fn keeps_the_newest_of_each_earlier_month() {
    let mut backups = daily("2026-11-05", 10);
    backups.extend([
        backup(Reason::Import, "2026-10-31 23:59:59.999"),
        backup(Reason::Migration, "2026-10-01 00:00:00.000"),
        backup(Reason::Restore, "2026-09-15 08:00:00.000"),
        backup(Reason::Import, "2026-09-10 08:00:00.000"),
        // A year boundary: December and January are different months.
        backup(Reason::Import, "2025-12-31 23:59:59.999"),
        backup(Reason::Import, "2025-12-01 00:00:00.000"),
        backup(Reason::Import, "2025-01-01 00:00:00.000"),
    ]);

    let mut expected = [
        file_name(Reason::Migration, at("2026-10-01 00:00:00.000")),
        file_name(Reason::Import, at("2026-09-10 08:00:00.000")),
        file_name(Reason::Import, at("2025-12-01 00:00:00.000")),
    ];
    expected.sort();
    assert_eq!(deleted(&backups, "2026-11-20 00:00:00.000"), expected);
}

#[test]
fn the_current_month_keeps_only_the_recent_ten() {
    // 12 this month: the two oldest go, though no other month has any.
    let backups = daily("2026-11-01", 12);
    assert_eq!(deleted(&backups, "2026-11-30 23:00:00.000").len(), 2);

    // A month later, the same files: November is now an earlier month, so
    // its newest is kept (it's also among the ten).
    assert_eq!(deleted(&backups, "2026-12-01 00:00:00.000").len(), 2);
}

#[test]
fn a_months_newest_counts_even_when_it_is_among_the_ten() {
    // Ten recent ones, the oldest of which is October's newest: October
    // keeps no second backup.
    let mut backups = daily("2026-10-31", 10);
    backups.push(backup(Reason::Import, "2026-10-30 12:00:00.000"));
    assert_eq!(
        deleted(&backups, "2026-11-20 00:00:00.000"),
        [file_name(Reason::Import, at("2026-10-30 12:00:00.000"))]
    );
}

#[test]
fn manual_backups_are_never_selected_or_counted() {
    let mut backups: Vec<Backup> = (1..=20)
        .map(|day| backup(Reason::Manual, &format!("2026-11-{day:02} 09:00:00.000")))
        .collect();
    backups.push(backup(Reason::Manual, "2020-01-01 00:00:00.000"));
    assert!(deleted(&backups, "2026-11-30 00:00:00.000").is_empty());

    // Manual ones newer than the automatic ones don't push them out.
    backups.extend(daily("2026-10-01", 10));
    assert!(deleted(&backups, "2026-11-30 00:00:00.000").is_empty());
}

#[test]
fn ties_are_broken_by_name_whatever_the_order() {
    let mut backups = daily("2026-11-01", 10);
    // Same time as the oldest, different reasons.
    backups.push(backup(Reason::Restore, "2026-11-01 12:00:00.000"));
    backups.push(backup(Reason::Migration, "2026-11-01 12:00:00.000"));

    let forward: Vec<String> = deleted(&backups, "2026-11-20 00:00:00.000")
        .into_iter()
        .map(str::to_owned)
        .collect();
    backups.reverse();
    let backward = deleted(&backups, "2026-11-20 00:00:00.000");

    assert_eq!(forward.len(), 2);
    assert_eq!(forward, backward);
}

#[test]
fn rotate_only_touches_backup_names() {
    let dir = TempDir::new().unwrap();
    let path = db_path(&dir);
    let backups = backups_dir(&path);
    fs::create_dir_all(&backups).unwrap();
    let mut automatic = Vec::new();
    for day in 1..=14 {
        let name = file_name(
            Reason::Import,
            at(&format!("2026-11-{day:02} 09:00:00.000")),
        );
        fs::write(backups.join(&name), "").unwrap();
        automatic.push(name);
    }
    let manual = file_name(Reason::Manual, at("2026-01-01 09:00:00.000"));
    let foreign = [
        "coffer-latest.db",
        "notes.txt",
        "coffer-20261101T090000.000Z-import.db.bak",
    ];
    fs::write(backups.join(&manual), "").unwrap();
    for name in foreign {
        fs::write(backups.join(name), "").unwrap();
    }
    // A directory with a backup name isn't a backup either.
    let directory = file_name(Reason::Import, at("2020-01-01 00:00:00.000"));
    fs::create_dir(backups.join(&directory)).unwrap();

    let removed = rotate(&path, at("2026-11-20 00:00:00.000")).unwrap();

    let removed: Vec<String> = removed
        .iter()
        .map(|p| p.file_name().unwrap().to_str().unwrap().to_owned())
        .collect();
    assert_eq!(removed.len(), 4);
    assert!(removed.iter().all(|name| automatic[..4].contains(name)));
    let left = dir_names(&backups);
    assert!(left.contains(&manual));
    assert!(left.contains(&directory));
    for name in foreign {
        assert!(left.contains(&name.to_owned()), "{name}");
    }
}

#[test]
fn listing_without_a_directory_is_empty() {
    let dir = TempDir::new().unwrap();
    assert!(list(&db_path(&dir)).unwrap().is_empty());
}

// Restoring -----------------------------------------------------------------

/// A data dir whose database holds "Current", and a manual backup that
/// holds "Backed up" (taken before the account was renamed).
struct Setup {
    _dir: TempDir,
    path: PathBuf,
    backup: PathBuf,
}

impl Setup {
    fn new() -> Setup {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        database_with(&path, "Backed up");
        let backup = {
            let db = Database::open(&path).unwrap();
            create(&db, Reason::Manual).unwrap().path
        };
        Database::open(&path)
            .unwrap()
            .conn()
            .execute("UPDATE account SET name = 'Current'", [])
            .unwrap();
        Setup {
            _dir: dir,
            path,
            backup,
        }
    }

    /// A writable copy of the backup elsewhere, to break.
    fn copy_of_backup(&self, name: &str) -> PathBuf {
        let copy = self._dir.path().join(name);
        fs::copy(&self.backup, &copy).unwrap();
        copy
    }

    /// Asserts a refused restore changed nothing: the database still holds
    /// "Current", no backup was added and no temporary file is left.
    fn assert_untouched(&self, before: &[u8], backups_before: usize) {
        assert_eq!(fs::read(&self.path).unwrap(), before);
        assert_eq!(account_names(&self.path), ["Current"]);
        assert_eq!(list(&self.path).unwrap().len(), backups_before);
        assert_no_scratch_files(&self.path);
    }
}

#[test]
fn restore_swaps_in_the_backup_after_backing_up_the_current_database() {
    let setup = Setup::new();
    let backup_bytes = fs::read(&setup.backup).unwrap();

    let restored = restore(&setup.path, &setup.backup).unwrap();

    assert_eq!(account_names(&setup.path), ["Backed up"]);
    assert_eq!(restored.from_version, Database::supported_schema_version());
    assert_eq!(restored.migrated_to, None);
    let pre_restore = restored.pre_restore.unwrap();
    assert_eq!(pre_restore.reason, Reason::Restore);
    assert_eq!(account_names(&pre_restore.path), ["Current"]);
    assert_eq!(list(&setup.path).unwrap()[0], pre_restore);

    // The backup itself is unchanged, with nothing left beside it.
    assert_eq!(fs::read(&setup.backup).unwrap(), backup_bytes);
    for suffix in ["-wal", "-shm", "-journal"] {
        assert!(!db::sidecar(&setup.backup, suffix).exists());
    }
    assert_no_scratch_files(&setup.path);
    #[cfg(unix)]
    assert_eq!(mode(&setup.path), 0o600);
    // It opens as an ordinary database again.
    let db = Database::open(&setup.path).unwrap();
    assert_eq!(account::list(&db).unwrap()[0].name, "Backed up");
}

#[test]
fn restore_migrates_an_older_backup() {
    let setup = Setup::new();
    let old_dir = TempDir::new().unwrap();
    let old = old_dir.path().join("old.db");
    {
        let db = db::open_at_version(&old, 2).unwrap();
        db.conn()
            .execute(
                "INSERT INTO account (name, kind, currency) VALUES ('Old', 'current', 'GBP')",
                [],
            )
            .unwrap();
    }

    let checked = check(&setup.path, &old).unwrap();
    assert_eq!(checked.schema_version, 2);
    assert_eq!(checked.named, None);
    assert_eq!(
        checked.current,
        Current::At(Database::supported_schema_version())
    );

    let restored = restore(&setup.path, &old).unwrap();

    assert_eq!(restored.from_version, 2);
    assert_eq!(
        restored.migrated_to,
        Some(Database::supported_schema_version())
    );
    assert!(restored.pre_restore.is_some());
    let db = Database::open(&setup.path).unwrap();
    assert_eq!(
        db.schema_version().unwrap(),
        Database::supported_schema_version()
    );
    assert_eq!(account_names(&setup.path), ["Old"]);
}

#[test]
fn restore_without_a_current_database_takes_no_backup() {
    let setup = Setup::new();
    let elsewhere = TempDir::new().unwrap();
    let path = elsewhere.path().join("new").join("coffer.db");

    assert_eq!(
        check(&path, &setup.backup).unwrap().current,
        Current::Missing
    );
    let restored = restore(&path, &setup.backup).unwrap();

    assert_eq!(restored.pre_restore, None);
    assert_eq!(account_names(&path), ["Backed up"]);
    #[cfg(unix)]
    assert_eq!(mode(path.parent().unwrap()), 0o700);
}

#[test]
fn check_changes_nothing() {
    let setup = Setup::new();
    let before = fs::read(&setup.path).unwrap();
    let backup_bytes = fs::read(&setup.backup).unwrap();

    let checked = check(&setup.path, &setup.backup).unwrap();

    let name = setup.backup.file_name().unwrap().to_str().unwrap();
    assert_eq!(checked.named, parse_file_name(name));
    assert_eq!(checked.named.unwrap().0, Reason::Manual);
    assert_eq!(fs::read(&setup.backup).unwrap(), backup_bytes);
    setup.assert_untouched(&before, 1);
}

#[test]
fn a_corrupted_backup_is_refused() {
    let setup = Setup::new();
    let corrupt = setup.copy_of_backup("corrupt.db");
    {
        let mut bytes = fs::read(&corrupt).unwrap();
        let page_size = usize::from(u16::from_be_bytes([bytes[16], bytes[17]]));
        // Overwrite the start of a b-tree page past the schema page.
        let page = (bytes.len() / page_size / 2).max(2) - 1;
        bytes[page * page_size..page * page_size + 64].fill(0xA5);
        fs::write(&corrupt, bytes).unwrap();
    }
    let before = fs::read(&setup.path).unwrap();

    let err = restore(&setup.path, &corrupt).unwrap_err();

    assert!(matches!(err, Error::Integrity { .. }), "{err:?}");
    setup.assert_untouched(&before, 1);
    assert!(matches!(
        check(&setup.path, &corrupt),
        Err(Error::Integrity { .. })
    ));
}

#[test]
fn a_newer_backup_is_refused() {
    let setup = Setup::new();
    let newer = setup.copy_of_backup("newer.db");
    Connection::open(&newer)
        .unwrap()
        .execute(
            "INSERT INTO refinery_schema_history (version, name, applied_on, checksum) \
             VALUES (?1, 'from_the_future', '', '0')",
            [Database::supported_schema_version() + 1],
        )
        .unwrap();
    let before = fs::read(&setup.path).unwrap();

    let err = restore(&setup.path, &newer).unwrap_err();

    assert!(
        matches!(
            &err,
            Error::Incompatible {
                source: db::Error::SchemaTooNew { .. },
                ..
            }
        ),
        "{err:?}"
    );
    setup.assert_untouched(&before, 1);
}

#[test]
fn a_backup_with_a_different_history_is_refused() {
    let setup = Setup::new();
    let edited = setup.copy_of_backup("edited.db");
    Connection::open(&edited)
        .unwrap()
        .execute(
            "UPDATE refinery_schema_history SET checksum = '1' WHERE version = 1",
            [],
        )
        .unwrap();
    let before = fs::read(&setup.path).unwrap();

    let err = restore(&setup.path, &edited).unwrap_err();

    assert!(
        matches!(
            &err,
            Error::Incompatible {
                source: db::Error::MigrationHistory { version: 1, .. },
                ..
            }
        ),
        "{err:?}"
    );
    setup.assert_untouched(&before, 1);
}

#[test]
fn a_file_that_isnt_a_coffer_database_is_refused() {
    let setup = Setup::new();
    let before = fs::read(&setup.path).unwrap();

    let text = setup._dir.path().join("notes.db");
    fs::write(
        &text,
        "not a database, and long enough to have a header ".repeat(4),
    )
    .unwrap();
    let err = restore(&setup.path, &text).unwrap_err();
    assert!(matches!(err, Error::NotADatabase { .. }), "{err:?}");

    let other = setup._dir.path().join("other.db");
    Connection::open(&other)
        .unwrap()
        .execute("CREATE TABLE t (x)", [])
        .unwrap();
    let err = restore(&setup.path, &other).unwrap_err();
    assert!(matches!(err, Error::NotCoffer { .. }), "{err:?}");

    let missing = setup._dir.path().join("missing.db");
    let err = restore(&setup.path, &missing).unwrap_err();
    assert!(matches!(err, Error::Io { .. }), "{err:?}");

    setup.assert_untouched(&before, 1);
}

#[test]
fn restore_refuses_while_the_database_is_open() {
    let setup = Setup::new();
    let open = Database::open(&setup.path).unwrap();

    let err = restore(&setup.path, &setup.backup).unwrap_err();

    assert!(matches!(err, Error::InUse { .. }), "{err:?}");
    assert_eq!(account::list(&open).unwrap()[0].name, "Current");
    assert_eq!(account_names(&setup.path), ["Current"]);
    assert_no_scratch_files(&setup.path);
}

#[test]
fn restore_refuses_a_damaged_current_database() {
    let setup = Setup::new();
    fs::write(&setup.path, "damaged ".repeat(64)).unwrap();
    let before = fs::read(&setup.path).unwrap();

    assert_eq!(
        check(&setup.path, &setup.backup).unwrap().current,
        Current::Unreadable
    );
    let err = restore(&setup.path, &setup.backup).unwrap_err();

    assert!(matches!(err, Error::PreRestoreBackup { .. }), "{err:?}");
    assert_eq!(fs::read(&setup.path).unwrap(), before);
    assert_no_scratch_files(&setup.path);
}

#[test]
fn restore_refuses_a_wal_without_its_database() {
    let setup = Setup::new();
    fs::remove_file(&setup.path).unwrap();
    let wal = db::sidecar(&setup.path, "-wal");
    fs::write(&wal, "pages").unwrap();

    let err = restore(&setup.path, &setup.backup).unwrap_err();

    assert!(matches!(err, Error::OrphanSidecar { .. }), "{err:?}");
    assert!(!setup.path.exists());
    assert_eq!(fs::read(&wal).unwrap(), b"pages");
}
