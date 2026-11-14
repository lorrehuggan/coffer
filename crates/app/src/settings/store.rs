//! Reading and changing what Settings shows (`docs/ARCHITECTURE.md` →
//! "Decision record: Settings, AI access and backups (built)").
//!
//! As [`crate::accounts::store::AccountStore`]: the app holds no
//! connection, so each call opens the database at the path it was given,
//! creating and migrating it as the CLI does, after checking the restore
//! lock. The rules are core's: [`coffer_core::setting`] (the kill switch),
//! [`coffer_core::mcp_client`], [`coffer_core::access_log`],
//! [`coffer_core::backup`] and [`coffer_core::export`]. The CLI's
//! `ai-access`, `backup`, `restore` and `export` are the reference
//! behaviour; this shows nothing they don't (no result hash).
//!
//! Backing up, checking a backup and restoring run on a background thread
//! (the store is `Send`); the rest is small and runs on the main thread.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chrono::{DateTime, Duration, Utc};
use coffer_core::access_log::activity::{self, Cursor, Query};
use coffer_core::access_log::{self, Outcome};
use coffer_core::backup::detail::{self, Assessment, Contents, Since};
use coffer_core::backup::{self, Backup, Reason, Restored};
use coffer_core::clock;
use coffer_core::db::Database;
use coffer_core::export::{self, Counts, Format, Options};
use coffer_core::mcp_client::{self, AccessLevel, ClientName, ClientStatus};
use coffer_core::{account, schedule, setting, tag};

use super::data::{
    AiData, BackupRow, BackupsData, ClientRow, Clock, ExportChoices, LogFilter, LogPage, LogRow,
    Period, RequestRow,
};
use crate::accounts::store::StoreError;

/// How many calls a page of the access log holds.
pub const LOG_PAGE: u32 = 100;

/// A restore is replacing the database.
#[derive(Debug)]
struct Restoring;

impl std::fmt::Display for Restoring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("A backup is being restored; try again when it's done.")
    }
}

impl std::error::Error for Restoring {}

/// The tools that write to the Inbox, from the MCP server's registry (core
/// doesn't know which tools exist).
fn writing_tools() -> &'static HashSet<&'static str> {
    static TOOLS: OnceLock<HashSet<&'static str>> = OnceLock::new();
    TOOLS.get_or_init(|| {
        coffer_mcp::tools::registry()
            .all()
            .filter(|tool| tool.spec().writes != coffer_mcp::registry::Writes::Nothing)
            .map(|tool| tool.spec().name)
            .collect()
    })
}

/// Whether `tool` writes to the Inbox.
pub fn writes(tool: &str) -> bool {
    writing_tools().contains(tool)
}

/// `time` in the computer's time zone; UTC when that can't be read.
pub fn local(time: DateTime<Utc>) -> chrono::NaiveDateTime {
    clock::local(&[time])
        .ok()
        .and_then(|times| times.into_iter().next())
        .unwrap_or_else(|| time.naive_utc())
}

/// The time now, and today's local date.
pub fn now() -> Result<Clock, StoreError> {
    Ok(Clock {
        now: clock::now()?,
        today: schedule::local_today()?,
    })
}

/// Settings' reads and writes on the database at `database`.
#[derive(Clone, Debug)]
pub struct SettingsStore {
    database: PathBuf,
}

impl SettingsStore {
    pub fn new(database: PathBuf) -> Self {
        Self { database }
    }

    /// The database file.
    pub fn database(&self) -> &Path {
        &self.database
    }

    fn open(&self) -> Result<Database, StoreError> {
        if backup::restoring(&self.database)? {
            return Err(Box::new(Restoring));
        }
        Ok(Database::open(&self.database)?)
    }

    /// The kill switch, the clients and the requests, with what each did
    /// in the 7 days before `clock`. First moves any calls refused while
    /// the database couldn't be opened into the log, as `coffer ai-access`
    /// does (a failure leaves them for next time).
    pub fn load_ai(&self, clock: &Clock) -> Result<AiData, StoreError> {
        let mut db = self.open()?;
        if let Err(error) = access_log::pending::flush(&mut db) {
            tracing::warn!(%error, "couldn't move refused calls into the access log");
        }
        let reader = db.reader()?;
        let week = activity::timestamp(clock.now - Duration::days(7));
        let recent = activity::activity(&reader, Some(&week), writes)?;
        let ever = activity::activity(&reader, None, writes)?;
        let written = activity::written(&reader, &week)?;
        let calls_week = activity::count(
            &reader,
            &Query {
                since: Some(week),
                ..Query::default()
            },
        )?;
        let seen = |at: &Option<String>| at.as_deref().and_then(clock::parse_stored).map(local);
        let mut clients = Vec::new();
        let mut requests = Vec::new();
        for info in mcp_client::list(&reader)? {
            let name = info.name.as_str().to_owned();
            match info.status {
                ClientStatus::Known => clients.push(ClientRow {
                    last_seen: seen(&info.last_seen_at),
                    week: recent.get(&name).cloned().unwrap_or_default(),
                    written: written.get(&name).copied().unwrap_or_default(),
                    enabled: info.enabled,
                    level: info.level,
                    name: info.name,
                }),
                ClientStatus::Requested => requests.push(RequestRow {
                    last_tried: seen(&info.last_seen_at),
                    refused: ever
                        .get(&name)
                        .map_or(0, |activity| activity.refused_total()),
                    name: info.name,
                }),
                ClientStatus::Ignored => {}
            }
        }
        Ok(AiData {
            on: setting::mcp_enabled(&reader)?,
            clients,
            requests,
            calls_week,
            log_clients: ever.into_keys().collect(),
        })
    }

    /// A page of the access log: the calls `filter` selects before `clock`'s
    /// period starts, older than `after` when given.
    pub fn load_log(
        &self,
        filter: &LogFilter,
        clock: &Clock,
        after: Option<Cursor>,
    ) -> Result<LogPage, StoreError> {
        let db = self.open()?;
        let reader = db.reader()?;
        let since = match filter.period {
            // From local midnight: today's date at 00:00, local, in UTC.
            Period::Today => {
                let offset = local(clock.now) - clock.now.naive_utc();
                let midnight = clock.today.and_hms_opt(0, 0, 0).unwrap_or_default() - offset;
                Some(activity::timestamp(midnight.and_utc()))
            }
            Period::Week => Some(activity::timestamp(clock.now - Duration::days(7))),
            Period::Month => Some(activity::timestamp(clock.now - Duration::days(30))),
            Period::All => None,
        };
        let query = Query {
            client: filter.client.clone(),
            since,
            after,
        };
        let mut calls = activity::page(&reader, &query, LOG_PAGE + 1)?;
        let more = calls.len() > LOG_PAGE as usize;
        calls.truncate(LOG_PAGE as usize);
        let times: Vec<DateTime<Utc>> = calls
            .iter()
            .map(|call| clock::parse_stored(&call.at).unwrap_or(clock.now))
            .collect();
        let locals = clock::local(&times)?;
        let rows = calls
            .into_iter()
            .zip(locals)
            .map(|(call, local)| LogRow {
                writes: writes(&call.tool),
                local,
                call,
            })
            .collect();
        Ok(LogPage { rows, more })
    }

    /// Every backup, newest first, with what each holds and why it was
    /// taken.
    pub fn load_backups(&self) -> Result<BackupsData, StoreError> {
        let db = self.open()?;
        let backups = backup::list(db.path())?;
        let contents: Vec<Option<Contents>> = backups
            .iter()
            .map(|backup| match detail::contents(&backup.path) {
                Ok(contents) => Some(contents),
                Err(error) => {
                    tracing::warn!(%error, "couldn't read a backup");
                    None
                }
            })
            .collect();
        let versions: Vec<_> = contents
            .iter()
            .map(|contents| contents.map(|contents| contents.schema_version))
            .collect();
        let details = detail::details(&db, &backups, &versions)?;
        let monthly = detail::monthly(&backups);
        let locals = clock::local(
            &backups
                .iter()
                .map(|backup| backup.taken_at)
                .collect::<Vec<_>>(),
        )?;
        let rows = backups
            .into_iter()
            .zip(contents)
            .zip(details)
            .zip(monthly)
            .zip(locals)
            .map(
                |((((backup, contents), detail), monthly), local)| BackupRow {
                    backup,
                    local,
                    contents,
                    detail,
                    monthly,
                },
            )
            .collect();
        Ok(BackupsData {
            rows,
            supported: Database::supported_schema_version(),
        })
    }

    /// What restoring a backup taken at `taken_at` would undo.
    pub fn since(&self, taken_at: DateTime<Utc>) -> Result<Since, StoreError> {
        let db = self.open()?;
        Ok(detail::since(&db, taken_at)?)
    }

    /// Switches AI access on or off for every client.
    pub fn set_ai_on(&self, on: bool) -> Result<(), StoreError> {
        Ok(setting::set_mcp_enabled(&mut self.open()?, on)?)
    }

    /// Enables a client at `level`.
    pub fn enable(&self, name: &ClientName, level: AccessLevel) -> Result<(), StoreError> {
        mcp_client::enable_at(&mut self.open()?, name, level)?;
        Ok(())
    }

    pub fn set_level(&self, name: &ClientName, level: AccessLevel) -> Result<(), StoreError> {
        Ok(mcp_client::set_level(&mut self.open()?, name, level)?)
    }

    /// Turns a client on (at its kept level) or off.
    pub fn turn(&self, name: &ClientName, on: bool) -> Result<(), StoreError> {
        let mut db = self.open()?;
        if on {
            mcp_client::turn_on(&mut db, name)?;
        } else {
            mcp_client::turn_off(&mut db, name)?;
        }
        Ok(())
    }

    pub fn ignore(&self, name: &ClientName) -> Result<(), StoreError> {
        Ok(mcp_client::ignore(&mut self.open()?, name)?)
    }

    pub fn remove(&self, name: &ClientName) -> Result<(), StoreError> {
        Ok(mcp_client::remove(&mut self.open()?, name)?)
    }

    /// Takes a manual backup ("Back up now"), and reads what it holds.
    pub fn back_up(&self) -> Result<(Backup, Option<Contents>), StoreError> {
        let db = self.open()?;
        let backup = backup::create(&db, Reason::Manual)?;
        drop(db);
        let contents = detail::contents(&backup.path).ok();
        Ok((backup, contents))
    }

    /// The restore checks on `file`, without restoring it.
    pub fn assess(&self, file: &Path) -> Result<Assessment, StoreError> {
        Ok(detail::assess(&self.database, file)?)
    }

    /// Restores `file` over the database (`backup::restore`: checked again,
    /// the current database backed up first, MCP servers refused meanwhile).
    pub fn restore(&self, file: &Path) -> Result<Restored, StoreError> {
        Ok(backup::restore(&self.database, file)?)
    }

    /// Saves a copy of a backup where the user chose.
    pub fn save_copy(&self, from: &Path, to: &Path) -> Result<(), StoreError> {
        Ok(detail::save_copy(&self.database, from, to)?)
    }

    /// What the export dialog's selects offer.
    pub fn export_choices(&self) -> Result<ExportChoices, StoreError> {
        let db = self.open()?;
        let accounts = account::list(&db)?
            .into_iter()
            .filter(|account| account.closed_on.is_none())
            .map(|account| (account.id, account.name))
            .collect();
        let categories = db
            .categories()?
            .iter()
            .map(|category| {
                (
                    category.key().to_string(),
                    category.label().to_owned(),
                    category.is_group(),
                )
            })
            .collect();
        let tags = tag::list(&db)?
            .into_iter()
            .map(|tag| (tag.name.as_str().to_owned(), tag.private))
            .collect();
        Ok(ExportChoices {
            accounts,
            categories,
            tags,
            span: export::date_span(&db)?,
            today: schedule::local_today().ok(),
        })
    }

    /// How many transactions and rows an export with `options` holds.
    pub fn export_count(&self, options: &Options) -> Result<Counts, StoreError> {
        Ok(export::count(&self.open()?, options)?)
    }

    /// Exports to `path`, which the save dialog chose (so an existing file
    /// is replaced). Returns the rows written.
    pub fn export(
        &self,
        format: Format,
        options: &Options,
        path: &Path,
    ) -> Result<usize, StoreError> {
        Ok(export::to_file(&self.open()?, format, options, path, true)?)
    }
}

/// Whether `outcome` is a refusal.
pub fn refused(outcome: Outcome) -> bool {
    !matches!(outcome, Outcome::Ok | Outcome::Error)
}

#[cfg(test)]
mod tests {
    use coffer_core::access_log::NewEntry;
    use tempfile::TempDir;

    use super::*;

    fn store() -> (TempDir, SettingsStore) {
        let dir = TempDir::new().unwrap();
        let store = SettingsStore::new(dir.path().join("coffer.db"));
        (dir, store)
    }

    fn name(name: &str) -> ClientName {
        ClientName::new(name).unwrap()
    }

    #[test]
    fn the_registry_says_which_tools_write() {
        assert!(writes("propose_categorizations"));
        assert!(writes("save_report"));
        assert!(!writes("search_transactions"));
        assert!(!writes("not_a_tool"));
    }

    #[test]
    fn clients_and_requests_are_listed_apart() {
        let (_dir, store) = store();
        let clock = now().unwrap();
        {
            let mut db = Database::open(store.database()).unwrap();
            mcp_client::record_requested(&mut db, &name("my-agent")).unwrap();
            access_log::append(
                &mut db,
                &NewEntry {
                    client: name("my-agent"),
                    tool: "list_accounts".into(),
                    access_level: None,
                    outcome: Outcome::RefusedNotEnabled,
                    purpose: None,
                    args_redacted_json: "{}".into(),
                    row_count: 0,
                    result_hash: None,
                    withheld_count: 0,
                },
            )
            .unwrap();
        }
        store
            .enable(&name("claude-code"), AccessLevel::Standard)
            .unwrap();
        let ai = store.load_ai(&clock).unwrap();
        assert!(ai.on);
        assert_eq!(ai.clients.len(), 1);
        assert_eq!(ai.clients[0].level, AccessLevel::Standard);
        assert_eq!(ai.requests.len(), 1);
        assert_eq!(ai.requests[0].refused, 1);
        assert_eq!(ai.calls_week, 1);

        store.turn(&name("claude-code"), false).unwrap();
        store.set_ai_on(false).unwrap();
        let ai = store.load_ai(&clock).unwrap();
        assert!(!ai.on);
        assert!(!ai.clients[0].enabled);
        // Turned off, it keeps its level.
        assert_eq!(ai.clients[0].level, AccessLevel::Standard);

        store.ignore(&name("my-agent")).unwrap();
        store.remove(&name("claude-code")).unwrap();
        let ai = store.load_ai(&clock).unwrap();
        assert!(ai.clients.is_empty() && ai.requests.is_empty());

        let log = store.load_log(&LogFilter::default(), &clock, None).unwrap();
        assert_eq!(log.rows.len(), 1);
        assert!(!log.more);
    }

    #[test]
    fn backs_up_and_lists_backups() {
        let (_dir, store) = store();
        drop(Database::open(store.database()).unwrap());
        let (backup, contents) = store.back_up().unwrap();
        assert_eq!(contents.unwrap().transactions, 0);
        let data = store.load_backups().unwrap();
        assert_eq!(data.rows.len(), 1);
        assert_eq!(data.rows[0].backup, backup);
        assert!(!data.rows[0].monthly);
        assert!(store.assess(&backup.path).unwrap().is_ready());
        assert!(store.since(backup.taken_at).unwrap().is_empty());
    }

    #[test]
    fn exports_where_asked() {
        let (dir, store) = store();
        drop(Database::open(store.database()).unwrap());
        let choices = store.export_choices().unwrap();
        assert!(choices.span.is_none());
        assert!(!choices.categories.is_empty());
        let options = Options::default();
        assert_eq!(store.export_count(&options).unwrap(), Counts::default());
        let path = dir.path().join("out.csv");
        assert_eq!(store.export(Format::Csv, &options, &path).unwrap(), 0);
        assert!(path.exists());
        assert!(
            store
                .export(Format::Csv, &options, store.database())
                .is_err()
        );
    }
}
