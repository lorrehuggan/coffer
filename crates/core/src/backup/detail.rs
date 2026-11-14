//! What the app's Backups screen shows about each backup (ROADMAP M3.16;
//! `docs/DESIGN.md` → "Settings" → **Backups**): what a backup holds, why
//! it was taken, which group it's listed in, what restoring it would undo,
//! and the restore checks one by one.
//!
//! A backup's metadata lives in its file name (decision record 2026-11-10,
//! point 6), so everything else is read: from the backup itself, opened
//! read-only and `immutable` so nothing (not even a `-wal` or `-shm`) is
//! written beside it, or from the current database, whose import batches,
//! change batches and suggestions carry their times.

use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags};

use super::{Backup, Error, KEEP_RECENT, Reason, RestoreCheck};
use crate::clock;
use crate::db::{self, Database, SchemaVersion};

/// What a backup holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Contents {
    pub schema_version: SchemaVersion,
    /// Every transaction row, excluded ones and pot mirrors included.
    pub transactions: u64,
    /// Every account, closed ones included.
    pub accounts: u64,
    pub rules: u64,
}

/// `path` as an SQLite URI that opens it read-only and `immutable`: no
/// locks, no journal, nothing written next to it.
fn immutable_uri(path: &Path) -> String {
    let mut uri = String::from("file:");
    for c in path.to_string_lossy().chars() {
        match c {
            '%' => uri.push_str("%25"),
            '?' => uri.push_str("%3F"),
            '#' => uri.push_str("%23"),
            c => uri.push(c),
        }
    }
    uri.push_str("?immutable=1");
    uri
}

/// What the backup at `path` holds, read without changing the file.
pub fn contents(path: &Path) -> Result<Contents, Error> {
    let conn = Connection::open_with_flags(
        immutable_uri(path),
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.pragma_update(None, "trusted_schema", false)?;
    let schema_version = db::schema_version_on(&conn)?;
    let count = |table: &str| -> Result<u64, Error> {
        let count: i64 = conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })?;
        Ok(count.unsigned_abs())
    };
    Ok(Contents {
        schema_version,
        transactions: count("\"transaction\"")?,
        accounts: count("account")?,
        rules: count("rule")?,
    })
}

/// For each of `backups` (as [`super::list`] returns them, newest first),
/// whether it's kept as a month's backup rather than as a recent one: an
/// automatic backup outside the [`KEEP_RECENT`] most recent automatic ones,
/// which rotation keeps only as the newest of its month. Manual backups
/// are never monthly.
pub fn monthly(backups: &[Backup]) -> Vec<bool> {
    let mut automatic = 0;
    backups
        .iter()
        .map(|backup| {
            if !backup.reason.is_automatic() {
                return false;
            }
            automatic += 1;
            automatic > KEEP_RECENT
        })
        .collect()
}

/// Why a backup was taken, with what can be read about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detail {
    /// Before an import: the file imported, when the current database
    /// still has its batch (committed or undone).
    Import { file: Option<String> },
    /// Before a database update: from the backup's schema to the next one.
    Migration {
        from: Option<SchemaVersion>,
        to: Option<SchemaVersion>,
    },
    /// Before a restore.
    Restore,
    /// Asked for.
    Manual,
}

/// What each of `backups` (newest first) was taken before. `versions` are
/// their schema versions where read ([`contents`]), in the same order;
/// `db` is the current database.
///
/// An import backup is taken just before its import commits, so its import
/// is the first batch the current database recorded between it and the
/// next newer backup. A migration backup's update went to the schema of the
/// next newer backup, or the current database's when it's the newest.
pub fn details(
    db: &Database,
    backups: &[Backup],
    versions: &[Option<SchemaVersion>],
) -> Result<Vec<Detail>, Error> {
    let current = db.schema_version()?;
    let mut first_import = db.conn().prepare(
        "SELECT source_file FROM import_batch \
         WHERE imported_at >= ?1 AND (?2 IS NULL OR imported_at < ?2) \
         ORDER BY imported_at, id LIMIT 1",
    )?;
    backups
        .iter()
        .enumerate()
        .map(|(index, backup)| {
            let newer = index.checked_sub(1).map(|newer| &backups[newer]);
            Ok(match backup.reason {
                Reason::Import => {
                    let from = clock::stored(backup.taken_at);
                    let to = newer.map(|newer| clock::stored(newer.taken_at));
                    let file = first_import
                        .query_map(rusqlite::params![from, to], |row| row.get(0))?
                        .next()
                        .transpose()?;
                    Detail::Import { file }
                }
                Reason::Migration => Detail::Migration {
                    from: versions.get(index).copied().flatten(),
                    to: match newer {
                        Some(_) => versions.get(index - 1).copied().flatten(),
                        None => Some(current),
                    },
                },
                Reason::Restore => Detail::Restore,
                Reason::Manual => Detail::Manual,
            })
        })
        .collect()
}

/// An import committed after a backup was taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSince {
    /// The imported file's name.
    pub file: String,
    /// The transactions it added.
    pub rows: u64,
}

/// What restoring a backup taken at a given time would undo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Since {
    /// Imports committed since, oldest first.
    pub imports: Vec<ImportSince>,
    /// Changes to many transactions made since and not undone (rule
    /// applies, bulk edits, duplicate merges).
    pub edits: u64,
    /// Inbox suggestions filed since, whatever has happened to them.
    pub suggestions: u64,
}

impl Since {
    pub fn is_empty(&self) -> bool {
        self.imports.is_empty() && self.edits == 0 && self.suggestions == 0
    }
}

/// What the current database has gained since `taken_at`.
pub fn since(db: &Database, taken_at: DateTime<Utc>) -> Result<Since, Error> {
    let at = clock::stored(taken_at);
    let conn = db.conn();
    let imports = conn
        .prepare(
            "SELECT b.source_file, \
                    (SELECT count(*) FROM \"transaction\" t WHERE t.batch_id = b.id) \
             FROM import_batch b WHERE b.imported_at >= ?1 AND b.status = 'committed' \
             ORDER BY b.imported_at, b.id",
        )?
        .query_map([&at], |row| {
            Ok(ImportSince {
                file: row.get(0)?,
                rows: row.get::<_, i64>(1)?.unsigned_abs(),
            })
        })?
        .collect::<Result<_, _>>()?;
    let count = |sql: &str| -> Result<u64, Error> {
        let count: i64 = conn.query_row(sql, [&at], |row| row.get(0))?;
        Ok(count.unsigned_abs())
    };
    Ok(Since {
        imports,
        edits: count(
            "SELECT count(*) FROM change_batch WHERE created_at >= ?1 AND status = 'applied'",
        )?,
        suggestions: count("SELECT count(*) FROM suggestion WHERE created_at >= ?1")?,
    })
}

/// Saves a copy of the backup at `from` as `to`, a new file readable only
/// by the user (`0600`, as backups are), for "Save a copy…". An existing
/// `to` is replaced (the save dialog has asked), written beside it first
/// and renamed over it; the database itself and its sidecar files are
/// refused.
pub fn save_copy(db_path: &Path, from: &Path, to: &Path) -> Result<(), Error> {
    let same = |a: &Path, b: &Path| match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    for suffix in ["", "-wal", "-shm", "-journal"] {
        if same(&db::sidecar(db_path, suffix), to) {
            return Err(Error::Io {
                action: "save a copy over",
                path: to.to_path_buf(),
                source: std::io::Error::other("it is Coffer's database"),
            });
        }
    }
    if same(from, to) {
        return Ok(());
    }
    let name = to
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let partial = to.with_file_name(format!(".{name}.coffer-copy.tmp"));
    let _ = std::fs::remove_file(&partial);
    super::create_private_file(&partial)?;
    let copied = super::copy_into(from, &partial)
        .and_then(|()| super::finish_file(&partial))
        .and_then(|()| {
            std::fs::rename(&partial, to).map_err(|source| Error::Io {
                action: "save",
                path: to.to_path_buf(),
                source,
            })
        });
    if copied.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    copied
}

/// What the restore checks found, check by check: the file is intact
/// (SQLite's integrity check), then this version of Coffer can read it
/// (Coffer's migration history, a schema no newer than this build's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assessment {
    /// Both passed: it can be restored.
    Ready(RestoreCheck),
    /// The file isn't a SQLite database, or fails the integrity check. The
    /// second check didn't run.
    Damaged { problem: String },
    /// Intact, but not a Coffer database.
    NotCoffer,
    /// Intact, but from a newer Coffer.
    TooNew {
        found: SchemaVersion,
        supported: SchemaVersion,
    },
    /// Intact, but its migrations differ from this build's.
    OtherHistory,
}

impl Assessment {
    pub fn is_ready(&self) -> bool {
        matches!(self, Assessment::Ready(_))
    }
}

/// Runs [`super::check`] on `backup` and sorts its answer into the checks.
/// Errors that aren't a check failing (the file can't be read, the data
/// directory can't be written) are returned as they are.
pub fn assess(db_path: &Path, backup: &Path) -> Result<Assessment, Error> {
    match super::check(db_path, backup) {
        Ok(check) => Ok(Assessment::Ready(check)),
        Err(Error::NotADatabase { .. }) => Ok(Assessment::Damaged {
            problem: "it isn't a database".to_owned(),
        }),
        Err(Error::Integrity { count, first, .. }) => Ok(Assessment::Damaged {
            problem: format!(
                "{count} {}; the first: {first}",
                if count == 1 { "problem" } else { "problems" }
            ),
        }),
        Err(Error::NotCoffer { .. }) => Ok(Assessment::NotCoffer),
        Err(Error::Incompatible {
            source: db::Error::SchemaTooNew { found, supported },
            ..
        }) => Ok(Assessment::TooNew { found, supported }),
        Err(Error::Incompatible { .. }) => Ok(Assessment::OtherHistory),
        Err(other) => Err(other),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::TimeZone as _;
    use tempfile::TempDir;

    use super::*;
    use crate::backup;

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, day, hour, 0, 0).unwrap()
    }

    fn named(reason: Reason, taken_at: DateTime<Utc>) -> Backup {
        Backup {
            path: backup::file_name(reason, taken_at).into(),
            reason,
            taken_at,
            size: 0,
        }
    }

    #[test]
    fn reads_what_a_backup_holds_without_writing_beside_it() {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("coffer.db")).unwrap();
        db.conn()
            .execute(
                "INSERT INTO account (name, kind, currency) VALUES ('Monzo', 'current', 'GBP')",
                [],
            )
            .unwrap();
        let taken = backup::create(&db, Reason::Manual).unwrap();
        let before: Vec<_> = fs::read_dir(taken.path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        let contents = contents(&taken.path).unwrap();
        assert_eq!(
            contents.schema_version,
            Database::supported_schema_version()
        );
        assert_eq!(
            (contents.accounts, contents.transactions, contents.rules),
            (1, 0, 0)
        );
        let after: Vec<_> = fs::read_dir(taken.path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(before, after);

        assert!(super::contents(&dir.path().join("missing.db")).is_err());
    }

    #[test]
    fn automatic_backups_past_the_recent_ones_are_monthly() {
        let mut backups: Vec<Backup> = (0..12)
            .map(|n| named(Reason::Import, at(20 - n, 9)))
            .collect();
        backups.insert(3, named(Reason::Manual, at(17, 12)));
        let monthly = monthly(&backups);
        assert_eq!(monthly.iter().filter(|monthly| **monthly).count(), 2);
        assert!(!monthly[3]);
        assert!(monthly[11] && monthly[12]);
    }

    #[test]
    fn names_the_import_and_the_update() {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("coffer.db")).unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO account (id, name, kind, currency) VALUES (1, 'Monzo', 'current', 'GBP'); \
                 INSERT INTO import_batch (account_id, source_file, format, imported_at) \
                     VALUES (1, 'monzo-october.csv', 'csv', '2026-11-02T09:00:00.120Z'), \
                            (1, 'monzo-november.csv', 'csv', '2026-11-10T14:02:00.500Z');",
            )
            .unwrap();
        let current = db.schema_version().unwrap();
        let backups = vec![
            named(Reason::Import, at(10, 14)),
            named(Reason::Manual, at(9, 9)),
            named(Reason::Migration, at(5, 8)),
            named(Reason::Import, at(2, 9)),
            named(Reason::Import, at(1, 9)),
        ];
        let versions = [Some(current), Some(current), Some(6), Some(6), Some(6)];
        let details = details(&db, &backups, &versions).unwrap();
        assert_eq!(
            details,
            [
                Detail::Import {
                    file: Some("monzo-november.csv".into())
                },
                Detail::Manual,
                Detail::Migration {
                    from: Some(6),
                    to: Some(current)
                },
                Detail::Import {
                    file: Some("monzo-october.csv".into())
                },
                // Its import didn't commit before the next backup.
                Detail::Import { file: None },
            ]
        );
    }

    #[test]
    fn counts_what_restoring_would_undo() {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("coffer.db")).unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO account (id, name, kind, currency) VALUES (1, 'Monzo', 'current', 'GBP'); \
                 INSERT INTO import_batch (id, account_id, source_file, format, imported_at) \
                     VALUES (1, 1, 'old.csv', 'csv', '2026-11-01T09:00:00.000Z'), \
                            (2, 1, 'monzo-november.csv', 'csv', '2026-11-10T14:02:00.500Z'); \
                 INSERT INTO change_batch (kind, created_at) \
                     VALUES ('bulk_edit', '2026-11-11T10:00:00.000Z'), \
                            ('bulk_edit', '2026-10-01T10:00:00.000Z'); \
                 INSERT INTO suggestion (kind, payload_json, source_client, created_at) \
                     VALUES ('categorization', '[1]', 'claude-code', '2026-11-12T10:00:00.000Z');",
            )
            .unwrap();
        for n in 0..3 {
            db.conn()
                .execute(
                    "INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, origin, \
                     currency, description_raw, description_normalized, fingerprint) \
                     VALUES (1, 2, '2026-11-09', ?1, 'imported', 'GBP', 'x', 'X', ?2)",
                    rusqlite::params![-100 - n, format!("f{n}")],
                )
                .unwrap();
        }
        let since = since(&db, at(10, 14)).unwrap();
        assert_eq!(
            since.imports,
            [ImportSince {
                file: "monzo-november.csv".into(),
                rows: 3
            }]
        );
        assert_eq!((since.edits, since.suggestions), (1, 1));
        assert!(super::since(&db, at(13, 0)).unwrap().is_empty());
    }

    #[test]
    fn saves_a_private_copy_but_never_over_the_database() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("coffer.db");
        let db = Database::open(&db_path).unwrap();
        let taken = backup::create(&db, Reason::Manual).unwrap();
        let copy = dir.path().join("elsewhere.db");
        save_copy(&db_path, &taken.path, &copy).unwrap();
        assert_eq!(fs::read(&copy).unwrap(), fs::read(&taken.path).unwrap());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&copy).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        // Replaced when asked again.
        save_copy(&db_path, &taken.path, &copy).unwrap();
        let before = fs::read(&db_path).unwrap();
        assert!(save_copy(&db_path, &taken.path, &db_path).is_err());
        assert_eq!(fs::read(&db_path).unwrap(), before);
    }

    #[test]
    fn sorts_check_failures_into_the_checks() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("coffer.db");
        let db = Database::open(&db_path).unwrap();
        let good = backup::create(&db, Reason::Manual).unwrap();
        drop(db);
        assert!(assess(&db_path, &good.path).unwrap().is_ready());

        let junk = dir.path().join("junk.db");
        fs::write(&junk, b"not a database at all, just some bytes......").unwrap();
        assert!(matches!(
            assess(&db_path, &junk).unwrap(),
            Assessment::Damaged { .. }
        ));

        let empty = dir.path().join("empty.db");
        Connection::open(&empty)
            .unwrap()
            .execute_batch("CREATE TABLE t (x INTEGER)")
            .unwrap();
        assert_eq!(assess(&db_path, &empty).unwrap(), Assessment::NotCoffer);

        assert!(assess(&db_path, &dir.path().join("missing.db")).is_err());
    }
}
