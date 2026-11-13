//! `coffer backup` and `coffer restore` against scratch data dirs. Nothing
//! here touches the real data dir.

use std::path::{Path, PathBuf};

use clap::Parser;
use coffer_core::account;
use coffer_core::backup::{self, Reason};
use coffer_core::db::Database;
use tempfile::TempDir;

use super::*;

/// A scratch data dir; its database doesn't exist until a command creates
/// it.
struct Scratch {
    _dir: TempDir,
    db: PathBuf,
}

impl Scratch {
    fn new() -> Scratch {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("coffer.db");
        Scratch { _dir: dir, db }
    }

    /// With one account, "Monzo".
    fn with_account() -> Scratch {
        let scratch = Scratch::new();
        scratch.add_account("Monzo");
        scratch
    }

    fn add_account(&self, name: &str) {
        self.ok(&[
            "accounts",
            "add",
            "--name",
            name,
            "--kind",
            "current",
            "--currency",
            "GBP",
        ]);
    }

    fn run(&self, args: &[&str]) -> Result<String, String> {
        let cli = Cli::try_parse_from(std::iter::once("coffer").chain(args.iter().copied()))
            .unwrap_or_else(|error| panic!("{args:?} doesn't parse: {error}"));
        let mut out = Vec::new();
        let result = super::run(cli.command.expect("a subcommand"), &self.db, &mut out);
        let out = String::from_utf8(out).unwrap();
        result.map(|()| out).map_err(|error| format!("{error:#}"))
    }

    fn ok(&self, args: &[&str]) -> String {
        self.run(args)
            .unwrap_or_else(|error| panic!("{args:?} failed: {error}"))
    }

    fn err(&self, args: &[&str]) -> String {
        match self.run(args) {
            Ok(out) => panic!("{args:?} succeeded:\n{out}"),
            Err(error) => error,
        }
    }

    fn account_names(&self) -> Vec<String> {
        let db = Database::open(&self.db).unwrap();
        account::list(&db)
            .unwrap()
            .into_iter()
            .map(|account| account.name)
            .collect()
    }

    fn backups(&self) -> Vec<backup::Backup> {
        backup::list(&self.db).unwrap()
    }
}

#[test]
fn parses_backup_and_restore() {
    let cli = Cli::try_parse_from(["coffer", "backup"]).unwrap();
    assert!(matches!(
        cli.command,
        Some(Command::Backup(BackupArgs { list: false }))
    ));
    let cli = Cli::try_parse_from(["coffer", "backup", "--list"]).unwrap();
    assert!(matches!(
        cli.command,
        Some(Command::Backup(BackupArgs { list: true }))
    ));

    let cli = Cli::try_parse_from(["coffer", "restore", "some.db"]).unwrap();
    let Some(Command::Restore(args)) = cli.command else {
        panic!("not restore");
    };
    assert_eq!(args.file, PathBuf::from("some.db"));
    assert!(!args.yes);
    let cli = Cli::try_parse_from(["coffer", "restore", "some.db", "--yes"]).unwrap();
    assert!(matches!(
        cli.command,
        Some(Command::Restore(RestoreArgs { yes: true, .. }))
    ));

    assert!(Cli::try_parse_from(["coffer", "restore"]).is_err());
}

#[test]
fn backup_needs_a_database() {
    let scratch = Scratch::new();

    let error = scratch.err(&["backup"]);

    assert!(error.contains("no Coffer database"), "{error}");
    assert!(!scratch.db.exists());
    assert!(!backup::backups_dir(&scratch.db).exists());
}

#[test]
fn backup_takes_a_manual_backup() {
    let scratch = Scratch::with_account();

    let out = scratch.ok(&["backup"]);

    let backups = scratch.backups();
    assert_eq!(backups.len(), 1);
    assert_eq!(backups[0].reason, Reason::Manual);
    assert!(
        out.starts_with(&format!(
            "Backed up the database to {} (",
            backups[0].path.display()
        )),
        "{out}"
    );
    assert!(out.trim_end().ends_with("KB).") || out.trim_end().ends_with("MB)."));
}

#[test]
fn backup_list() {
    let scratch = Scratch::with_account();
    let out = scratch.ok(&["backup", "--list"]);
    assert!(out.starts_with("No backups in "), "{out}");

    scratch.ok(&["backup"]);
    scratch.ok(&["backup"]);
    let out = scratch.ok(&["backup", "--list"]);

    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].starts_with("Backups in "), "{out}");
    assert!(lines[1].starts_with("TAKEN (UTC)"), "{out}");
    let names: Vec<String> = scratch
        .backups()
        .iter()
        .map(|b| b.path.file_name().unwrap().to_str().unwrap().to_owned())
        .collect();
    assert_eq!(names.len(), 2);
    // Newest first.
    assert!(lines[2].ends_with(&names[0]), "{out}");
    assert!(lines[3].ends_with(&names[1]), "{out}");
    assert!(lines[2].contains(" manual "), "{out}");
    assert!(lines[4].starts_with("Automatic backups"), "{out}");
}

#[test]
fn backup_list_works_without_a_database() {
    let scratch = Scratch::new();
    let out = scratch.ok(&["backup", "--list"]);
    assert!(out.starts_with("No backups in "), "{out}");
    assert!(!scratch.db.exists());
}

#[test]
fn restore_without_yes_only_checks() {
    let scratch = Scratch::with_account();
    scratch.ok(&["backup"]);
    let backup = scratch.backups().remove(0);
    scratch.add_account("Euro");
    let before = std::fs::read(&scratch.db).unwrap();

    let out = scratch.ok(&["restore", backup.path.to_str().unwrap()]);

    let version = Database::supported_schema_version();
    assert!(
        out.starts_with(&format!("Backup: {}\n", backup.path.display())),
        "{out}"
    );
    assert!(out.contains(", by hand\n"), "{out}");
    assert!(
        out.contains(&format!("  schema version {version}\n")),
        "{out}"
    );
    assert!(out.contains("passed SQLite's integrity check"), "{out}");
    assert!(
        out.contains(&format!("schema version {version}\nRestoring replaces it")),
        "{out}"
    );
    assert!(
        out.trim_end()
            .ends_with("Nothing was restored. Run the command again with --yes to restore it."),
        "{out}"
    );
    assert_eq!(std::fs::read(&scratch.db).unwrap(), before);
    assert_eq!(scratch.backups(), [backup]);
}

#[test]
fn restore_with_yes_restores_by_file_name() {
    let scratch = Scratch::with_account();
    scratch.ok(&["backup"]);
    let backup = scratch.backups().remove(0);
    scratch.add_account("Euro");
    let name = backup
        .path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    let out = scratch.ok(&["restore", &name, "--yes"]);

    assert_eq!(scratch.account_names(), ["Monzo"]);
    let backups = scratch.backups();
    let pre_restore = &backups[0];
    assert_eq!(pre_restore.reason, Reason::Restore);
    assert!(
        out.contains(&format!(
            "Backed up the current database to {}.\nRestored {}.\n",
            pre_restore.path.display(),
            backup.path.display()
        )),
        "{out}"
    );

    // And back again.
    let pre_restore = pre_restore.path.to_str().unwrap().to_owned();
    scratch.ok(&["restore", &pre_restore, "--yes"]);
    assert_eq!(scratch.account_names(), ["Monzo", "Euro"]);
}

/// AI access-log rows written after the backup are kept, and the output
/// says how many.
#[test]
fn restore_keeps_the_ai_access_log() {
    use coffer_core::access_log::{self, NewEntry, Outcome};
    use coffer_core::mcp_client::ClientName;

    let scratch = Scratch::with_account();
    scratch.ok(&["backup"]);
    let backup = scratch.backups().remove(0);
    for _ in 0..2 {
        access_log::append(
            &mut Database::open(&scratch.db).unwrap(),
            &NewEntry {
                client: ClientName::new("codex").unwrap(),
                tool: "list_categories".to_owned(),
                access_level: None,
                outcome: Outcome::RefusedNotEnabled,
                purpose: None,
                args_redacted_json: "{}".to_owned(),
                row_count: 0,
                result_hash: None,
                withheld_count: 0,
            },
        )
        .unwrap();
    }

    let out = scratch.ok(&["restore", backup.path.to_str().unwrap(), "--yes"]);

    assert!(
        out.ends_with("Kept 2 AI access-log entries newer than the backup.\n"),
        "{out}"
    );
    let db = Database::open(&scratch.db).unwrap();
    assert_eq!(
        access_log::recent(&db.reader().unwrap(), 10).unwrap().len(),
        2
    );
}

#[test]
fn restore_warns_about_a_restore_that_stopped_and_goes_ahead() {
    let scratch = Scratch::with_account();
    scratch.ok(&["backup"]);
    let backup = scratch.backups().remove(0);
    scratch.add_account("Euro");
    // What a restore killed part-way leaves: its marker, in a lock file
    // nobody holds.
    let lock = backup::restore_lock_path(&scratch.db);
    std::fs::write(
        &lock,
        format!("2026-11-12T10:00:00.000Z\n{}\n", backup.path.display()),
    )
    .unwrap();
    let warning = format!(
        "Warning: an earlier restore from {} started 2026-11-12 10:00:00 UTC stopped before \
         it finished.",
        backup.path.display()
    );

    let out = scratch.ok(&["restore", backup.path.to_str().unwrap()]);
    assert!(out.contains(&warning), "{out}");

    let out = scratch.ok(&["restore", backup.path.to_str().unwrap(), "--yes"]);
    assert!(out.contains(&warning), "{out}");
    assert_eq!(scratch.account_names(), ["Monzo"]);
    // Cleared by the restore that finished.
    let out = scratch.ok(&["restore", backup.path.to_str().unwrap()]);
    assert!(!out.contains("Warning"), "{out}");
}

#[test]
fn restore_refuses_while_another_restore_holds_the_lock() {
    let scratch = Scratch::with_account();
    scratch.ok(&["backup"]);
    let backup = scratch.backups().remove(0);
    let lock = backup::restore_lock_path(&scratch.db);
    let held = std::fs::File::create(&lock).unwrap();
    held.lock().unwrap();

    let error = scratch.err(&["restore", backup.path.to_str().unwrap(), "--yes"]);

    assert!(
        error.contains(&format!(
            "another restore is running (it holds the lock on {}); wait for it to finish",
            lock.display()
        )),
        "{error}"
    );
    drop(held);
    scratch.ok(&["restore", backup.path.to_str().unwrap(), "--yes"]);
}

#[test]
fn restore_refuses_a_file_that_isnt_a_backup() {
    let scratch = Scratch::with_account();
    let notes = scratch._dir.path().join("notes.db");
    std::fs::write(&notes, "not a database ".repeat(10)).unwrap();
    let before = std::fs::read(&scratch.db).unwrap();

    let error = scratch.err(&["restore", notes.to_str().unwrap(), "--yes"]);

    assert!(error.contains("isn't a SQLite database"), "{error}");
    assert_eq!(std::fs::read(&scratch.db).unwrap(), before);
    assert!(scratch.backups().is_empty());
}

#[test]
fn restore_names_a_missing_backup() {
    let scratch = Scratch::with_account();

    let error = scratch.err(&["restore", "coffer-20261111T153012.345Z-import.db"]);

    assert!(
        error.starts_with("there is no backup coffer-20261111T153012.345Z-import.db"),
        "{error}"
    );
}

#[test]
fn import_takes_an_automatic_backup_and_a_dry_run_doesnt() {
    let scratch = Scratch::with_account();
    let monzo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/presets/uk/monzo.csv")
        .display()
        .to_string();
    scratch.ok(&["import", &monzo, "--dry-run"]);
    assert!(backup::list(&scratch.db).unwrap().is_empty());

    scratch.ok(&["import", &monzo]);
    let backups = backup::list(&scratch.db).unwrap();
    assert_eq!(backups.len(), 1);
    assert_eq!(backups[0].reason, Reason::Import);

    // Nothing new: no backup.
    scratch.ok(&["import", &monzo]);
    assert_eq!(backup::list(&scratch.db).unwrap().len(), 1);
}
