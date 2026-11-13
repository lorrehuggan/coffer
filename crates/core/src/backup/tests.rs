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
    let lock = restore_lock_path(db_path);
    assert!(
        !restoring(db_path).unwrap(),
        "the restore lock is still held at {}",
        lock.display()
    );
    // Emptied when the restore ended, so it isn't reported as interrupted.
    if lock.exists() {
        assert_eq!(fs::read(&lock).unwrap(), b"", "{}", lock.display());
    }
}

/// Holds the restore lock for `db_path` as a running restore in another
/// process would, until dropped.
fn hold_restore_lock(db_path: &Path) -> File {
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(restore_lock_path(db_path))
        .unwrap();
    file.lock().unwrap();
    file
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
fn restore_waits_for_a_running_call_to_close_its_connection() {
    let setup = Setup::new();
    let open = Database::open(&setup.path).unwrap();
    let path = setup.path.clone();
    let closer = std::thread::spawn(move || {
        // A call that is running when the restore starts: it sees the lock
        // when it ends, and closes its connection.
        while !restoring(&path).unwrap() {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(200));
        drop(open);
    });

    restore(&setup.path, &setup.backup).unwrap();
    closer.join().unwrap();

    assert_eq!(account_names(&setup.path), ["Backed up"]);
    assert_no_scratch_files(&setup.path);
}

#[test]
fn a_held_restore_lock_refuses_a_second_restore() {
    let setup = Setup::new();
    let before = fs::read(&setup.path).unwrap();
    let held = hold_restore_lock(&setup.path);
    assert!(restoring(&setup.path).unwrap());
    // A running restore isn't an interrupted one.
    assert_eq!(check(&setup.path, &setup.backup).unwrap().interrupted, None);

    let err = restore(&setup.path, &setup.backup).unwrap_err();

    assert!(matches!(err, Error::RestoreLocked { .. }), "{err:?}");
    let message = err.to_string();
    assert!(message.contains("coffer.db-restoring"), "{message}");
    assert!(!message.contains("delete"), "{message}");
    // Someone else's lock is still held; nothing was swapped or backed up.
    assert!(restoring(&setup.path).unwrap());
    drop(held);
    setup.assert_untouched(&before, 1);

    // Once it's released, the restore goes ahead.
    restore(&setup.path, &setup.backup).unwrap();
    assert_eq!(account_names(&setup.path), ["Backed up"]);
}

#[test]
fn a_restore_holds_its_lock_and_releases_it_when_done() {
    let setup = Setup::new();
    let lock = RestoreLock::acquire(&setup.path, &setup.backup).unwrap();
    assert!(restoring(&setup.path).unwrap());
    // The marker: when it started, and from which backup.
    let marker = fs::read_to_string(restore_lock_path(&setup.path)).unwrap();
    assert!(
        marker.ends_with(&format!("\n{}\n", setup.backup.display())),
        "{marker}"
    );
    // A second lock can't be taken beside it.
    assert!(matches!(
        RestoreLock::acquire(&setup.path, &setup.backup),
        Err(Error::RestoreLocked { .. })
    ));

    drop(lock);
    assert!(!restoring(&setup.path).unwrap());
    assert_eq!(fs::read(restore_lock_path(&setup.path)).unwrap(), b"");
    assert_eq!(check(&setup.path, &setup.backup).unwrap().interrupted, None);
}

#[test]
fn a_lock_file_nobody_holds_isnt_restoring() {
    let setup = Setup::new();
    assert!(!restoring(&setup.path).unwrap(), "no lock file");
    // Left by a restore that crashed after taking the lock.
    RestoreLock::acquire(&setup.path, &setup.backup)
        .unwrap()
        .crash();
    assert!(restore_lock_path(&setup.path).exists());
    assert!(!restoring(&setup.path).unwrap(), "a lock file nobody holds");
}

#[test]
fn a_restore_over_a_stopped_restores_lock_goes_ahead_and_clears_it() {
    let setup = Setup::new();
    let started = now();
    RestoreLock::acquire(&setup.path, &setup.backup)
        .unwrap()
        .crash();

    // `check` reports it, for the CLI's warning.
    let interrupted = check(&setup.path, &setup.backup)
        .unwrap()
        .interrupted
        .expect("the stopped restore is reported");
    assert_eq!(interrupted.backup.as_deref(), Some(setup.backup.as_path()));
    let reported = interrupted.started.unwrap();
    assert!(reported >= started && reported <= now(), "{reported}");

    // The restore goes ahead, backing up the current database first.
    let restored = restore(&setup.path, &setup.backup).unwrap();
    assert_eq!(account_names(&setup.path), ["Backed up"]);
    assert_eq!(
        account_names(&restored.pre_restore.unwrap().path),
        ["Current"]
    );
    assert_no_scratch_files(&setup.path);
    assert_eq!(check(&setup.path, &setup.backup).unwrap().interrupted, None);
}

#[test]
fn a_lock_file_coffer_didnt_write_is_reported_as_interrupted() {
    let setup = Setup::new();
    fs::write(restore_lock_path(&setup.path), "not a marker").unwrap();

    let interrupted = check(&setup.path, &setup.backup).unwrap().interrupted;
    assert_eq!(
        interrupted,
        Some(Interrupted {
            started: None,
            backup: None
        })
    );
    restore(&setup.path, &setup.backup).unwrap();
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

// When backups are taken (M1.28 wiring) ---------------------------------------

/// A one-row CSV statement, with an unreadable row too when `flagged`.
fn statement(flagged: bool) -> crate::statement::Statement {
    use crate::money::Money;
    use crate::statement::{Field, RowProblem, SourceFormat, SourceLocation, StatementLine};
    let mut statement = crate::statement::Statement::new(SourceFormat::Csv, Currency::GBP);
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 2).unwrap();
    statement.push(
        SourceLocation::Line(2),
        Ok(StatementLine::new(
            date,
            Money::new(-340, Currency::GBP),
            "SYNTHETIC CAFE",
        )),
    );
    if flagged {
        statement.push(
            SourceLocation::Line(3),
            Err(RowProblem::Missing(Field::Date)),
        );
    }
    statement
}

/// Transactions in the database file at `path`: a backup, or the live
/// database (read with its WAL, so not `immutable`).
fn transactions_in(path: &Path) -> i64 {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    conn.query_row("SELECT COUNT(*) FROM \"transaction\"", [], |row| row.get(0))
        .unwrap()
}

/// An older database (schema 2) with one account, closed again.
fn older_database(path: &Path) {
    let db = db::open_at_version(path, 2).unwrap();
    db.conn()
        .execute(
            "INSERT INTO account (name, kind, currency) VALUES ('Old', 'current', 'GBP')",
            [],
        )
        .unwrap();
}

#[test]
fn opening_an_older_database_backs_it_up_before_migrating() {
    let dir = TempDir::new().unwrap();
    let path = db_path(&dir);
    older_database(&path);
    assert!(
        list(&path).unwrap().is_empty(),
        "a new database isn't backed up"
    );

    let db = Database::open(&path).unwrap();
    assert_eq!(
        db.schema_version().unwrap(),
        Database::supported_schema_version()
    );
    let backups = list(&path).unwrap();
    assert_eq!(backups.len(), 1);
    assert_eq!(backups[0].reason, Reason::Migration);
    // The backup is the database as it was before the migrations.
    assert_eq!(db::peek_schema_version(&backups[0].path).unwrap(), 2);
    assert_eq!(account_names(&backups[0].path), ["Old"]);
    drop(db);

    // Up to date: opening it again takes no backup.
    drop(Database::open(&path).unwrap());
    assert_eq!(list(&path).unwrap().len(), 1);
}

#[test]
fn without_its_backup_a_database_isnt_migrated() {
    let dir = TempDir::new().unwrap();
    let path = db_path(&dir);
    older_database(&path);
    // A file where the backups directory should be: the backup fails.
    fs::write(backups_dir(&path), b"in the way").unwrap();

    let error = Database::open(&path).unwrap_err();
    assert!(matches!(error, db::Error::Backup(_)), "{error}");
    assert_eq!(db::peek_schema_version(&path).unwrap(), 2);
    assert!(
        dir_names(path.parent().unwrap())
            .iter()
            .all(|name| !name.starts_with(".coffer-"))
    );
}

#[test]
fn an_import_is_backed_up_before_it_commits() {
    use crate::import::{self, Committed, Confirmations};
    let dir = TempDir::new().unwrap();
    let path = db_path(&dir);
    let mut db = Database::open(&path).unwrap();
    let account = account::create(
        &mut db,
        &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
    )
    .unwrap();

    // A preview writes nothing, and a commit refused for a missing
    // confirmation takes no backup either.
    let preview = import::preview(&db, account, "a.csv", statement(true)).unwrap();
    let refused = import::commit(&mut db, &preview, Confirmations::default());
    assert!(matches!(refused, Err(import::Error::NotConfirmed(_))));
    assert!(list(&path).unwrap().is_empty());

    let committed = import::commit(&mut db, &preview, preview.needs_confirmation()).unwrap();
    assert!(matches!(committed, Committed::Batch(_)));
    let backups = list(&path).unwrap();
    assert_eq!(backups.len(), 1);
    assert_eq!(backups[0].reason, Reason::Import);
    // Taken before the commit: the import's row isn't in it.
    assert_eq!(transactions_in(&backups[0].path), 0);
    assert_eq!(transactions_in(&path), 1);

    // Nothing new: no backup.
    let again = import::preview(&db, account, "a.csv", statement(true)).unwrap();
    let committed = import::commit(&mut db, &again, again.needs_confirmation()).unwrap();
    assert_eq!(committed, Committed::NothingNew);
    assert_eq!(list(&path).unwrap().len(), 1);
}

#[test]
fn without_its_backup_nothing_is_imported() {
    use crate::import;
    let dir = TempDir::new().unwrap();
    let path = db_path(&dir);
    let mut db = Database::open(&path).unwrap();
    let account = account::create(
        &mut db,
        &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
    )
    .unwrap();
    fs::write(backups_dir(&path), b"in the way").unwrap();

    let preview = import::preview(&db, account, "a.csv", statement(false)).unwrap();
    let error = import::commit(&mut db, &preview, preview.needs_confirmation()).unwrap_err();
    assert!(matches!(error, import::Error::Backup(_)), "{error}");
    assert_eq!(transactions_in(&path), 0);
    let batches: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM import_batch", [], |row| row.get(0))
        .unwrap();
    assert_eq!(batches, 0);
}

/// Adds an access-log row at `at` (`strftime` modifiers on now, or a
/// timestamp), from `client`.
fn log_row(conn: &Connection, client: &str, at: &str) {
    let at = if at.starts_with(['-', '+']) {
        conn.query_row(
            "SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?1)",
            [at],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
    } else {
        at.to_owned()
    };
    conn.execute(
        "INSERT INTO mcp_access_log (at, client, tool, outcome) \
         VALUES (?1, ?2, 'list_categories', 'ok')",
        [&at, client],
    )
    .unwrap();
}

/// The log of the database at `path`, oldest first: (client, outcome,
/// pending ID, summarised calls).
fn log_of(path: &Path) -> Vec<(String, String, Option<String>, Option<i64>)> {
    let db = Database::open(path).unwrap();
    let mut statement = db
        .conn()
        .prepare(
            "SELECT client, outcome, pending_id, summarised_calls FROM mcp_access_log \
             ORDER BY at, id",
        )
        .unwrap();
    statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn clients(path: &Path) -> Vec<String> {
    log_of(path).into_iter().map(|row| row.0).collect()
}

/// Restoring an older backup doesn't roll the access log back: rows
/// written since the backup are carried over, once, however often the
/// backup is restored.
#[test]
fn restore_keeps_access_log_rows_newer_than_the_backup() {
    let dir = TempDir::new().unwrap();
    let path = db_path(&dir);
    let backup = {
        let db = Database::open(&path).unwrap();
        log_row(db.conn(), "in-backup", "-2 hours");
        create(&db, Reason::Manual).unwrap().path
    };
    {
        let db = Database::open(&path).unwrap();
        log_row(db.conn(), "after-1", "-1 hours");
        db.conn()
            .execute(
                "INSERT INTO mcp_access_log (at, client, tool, outcome, pending_id, \
                 summarised_calls) VALUES (strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-30 minutes'), \
                 'after-2', 'list_reports', 'refused_restoring', 'p-1', 12)",
                [],
            )
            .unwrap();
    }

    let restored = restore(&path, &backup).unwrap();
    assert_eq!(restored.log_rows_carried, Some(2));
    let expected = vec![
        ("in-backup".to_owned(), "ok".to_owned(), None, None),
        ("after-1".to_owned(), "ok".to_owned(), None, None),
        (
            "after-2".to_owned(),
            "refused_restoring".to_owned(),
            Some("p-1".to_owned()),
            Some(12),
        ),
    ];
    assert_eq!(log_of(&path), expected);

    // Again: the rows are in both now, and nothing is doubled.
    let again = restore(&path, &backup).unwrap();
    assert_eq!(again.log_rows_carried, Some(2));
    assert_eq!(log_of(&path), expected);
}

/// A backup from before the access log existed (V2) is restored over a
/// database at an older schema (V13, its log without V14's columns): the
/// backup is migrated first, then the replaced log copied in. Rows past the
/// 12-month retention, the backup's or the carried ones, go.
#[test]
fn restore_keeps_the_access_log_across_older_schemas() {
    let dir = TempDir::new().unwrap();
    let path = db_path(&dir);
    let old = dir.path().join("old.db");
    drop(db::open_at_version(&old, 2).unwrap());
    {
        let db = db::open_at_version(&path, 13).unwrap();
        log_row(db.conn(), "expired", "-13 months");
        log_row(db.conn(), "kept-1", "-11 months");
        log_row(db.conn(), "kept-2", "-1 days");
    }

    let restored = restore(&path, &old).unwrap();
    assert_eq!(restored.from_version, 2);
    assert_eq!(
        restored.migrated_to,
        Some(Database::supported_schema_version())
    );
    assert_eq!(restored.log_rows_carried, Some(3));
    assert_eq!(clients(&path), ["kept-1", "kept-2"]);
    // Taken at V13, before the swap.
    let pre_restore = restored.pre_restore.unwrap().path;
    assert_eq!(db::peek_schema_version(&pre_restore).unwrap(), 13);
}

/// A replaced database from before the log existed has nothing to carry,
/// and the backup's own log is kept as it is.
#[test]
fn restore_over_a_database_without_a_log_keeps_the_backups() {
    let dir = TempDir::new().unwrap();
    let path = db_path(&dir);
    let backup_dir = TempDir::new().unwrap();
    let backup = backup_dir.path().join("backup.db");
    {
        let db = Database::open(&backup).unwrap();
        log_row(db.conn(), "in-backup", "-1 days");
    }
    drop(db::open_at_version(&path, 2).unwrap());

    let restored = restore(&path, &backup).unwrap();
    assert_eq!(restored.log_rows_carried, Some(0));
    assert_eq!(clients(&path), ["in-backup"]);
}
