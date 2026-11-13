//! Calls the MCP server refused because it couldn't open the database,
//! waiting to go into the access log (decision record "M2: MCP server
//! (2026-11-13)", F.8).
//!
//! The access log is in the database. While a restore holds the restore
//! lock, while migrations are pending, or when the lock can't be checked,
//! the server can't open it, so it refuses the call and [`record`]s it here
//! instead: one line in `<database>-access-pending`, next to the database.
//! The next time something opens the database to write, [`flush`] moves the
//! lines into `mcp_access_log` with their own times. The server flushes on
//! every call and listing that opens the database, `coffer ai-access`
//! before it shows anything, and `backup::restore` once the restored
//! database is open.
//!
//! **What a line holds.** Exactly what the log row would: a JSON object with
//! the entry's ID, the call's time, the client, the tool, the outcome
//! ([`Outcome::RefusedRestoring`] or [`Outcome::RefusedUnavailable`]), the
//! client's cleaned purpose (cut to [`MAX_PURPOSE_LEN`]) and the arguments,
//! already redacted by the server. A refused call read nothing, so there is
//! no row count, result hash or withheld count. Nothing here is logged at
//! `info` or above.
//!
//! **Files.** `0600` when created. Every append is synced before
//! [`record`] returns. Several `coffer mcp` processes (one per client) may
//! record and flush at once: each holds the operating system's exclusive
//! lock on the pending file (`File::lock`, std) for its whole append or
//! flush, so lines never interleave and a flush never empties a file under
//! an append. A line cut short by a crash is ended before the next one is
//! written, and skipped when flushed.
//!
//! **No duplicates.** Each line gets a UUIDv7 when recorded. [`flush`]
//! inserts every line in one transaction, as `pending_id` (unique, V14) with
//! `ON CONFLICT (pending_id) DO NOTHING`, commits, and only then empties the
//! file. If it stops between the commit and the truncate (crash, kill,
//! power loss), the next flush inserts the same lines again and every one is
//! ignored: the IDs make the insert idempotent, so the order of the steps
//! alone needn't be atomic.
//!
//! **The cap.** The file holds at most [`MAX_ENTRIES`] lines and
//! [`MAX_BYTES`]. A call that doesn't fit is only counted, per client, tool
//! and outcome, in `<database>-access-pending-dropped` (a small JSON file
//! replaced whole through a temporary file and a rename, so it's never
//! half-written). A flush turns each count into one summary row
//! (`summarised_calls`, timed at the first such call, no arguments). Each
//! count has its own UUIDv7 as its `pending_id` too; before inserting, the
//! flush renames the counts file to `…-dropped-flushing`, so counts added
//! after a crash go into a new file with new IDs instead of into one whose
//! rows are already in.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use rusqlite::{ErrorCode, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{MAX_PURPOSE_LEN, NewEntry, Outcome};
use crate::db::{self, Database};

/// The most calls the pending file holds. Calls past it are only counted.
pub const MAX_ENTRIES: usize = 1_000;

/// The largest the pending file grows, in bytes. A call that would take it
/// past this is only counted.
pub const MAX_BYTES: u64 = 1024 * 1024;

/// The pending file for the database at `db_path`:
/// `<database>-access-pending`, next to it.
pub fn path(db_path: &Path) -> PathBuf {
    db::sidecar(db_path, "-access-pending")
}

/// Where calls past the cap are counted.
fn dropped_path(db_path: &Path) -> PathBuf {
    db::sidecar(db_path, "-access-pending-dropped")
}

/// The counts a flush is inserting (renamed from [`dropped_path`]).
fn flushing_path(db_path: &Path) -> PathBuf {
    db::sidecar(db_path, "-access-pending-dropped-flushing")
}

/// Where new counts are written before they replace [`dropped_path`].
fn dropped_temp_path(db_path: &Path) -> PathBuf {
    db::sidecar(db_path, "-access-pending-dropped-tmp")
}

/// What [`record`] did with a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recorded {
    /// It's in the pending file.
    Kept,
    /// The file was full: the call was only counted.
    Counted,
}

/// What [`flush`] moved into the log.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Flushed {
    /// Calls inserted, one row each.
    pub moved: usize,
    /// Calls whose row was already in (a flush stopped before it emptied
    /// the file).
    pub already_logged: usize,
    /// Summary rows inserted for calls past the cap.
    pub summaries: usize,
    /// Lines or counts that couldn't be read or didn't fit the log's
    /// schema (e.g. a line a crash cut short), skipped.
    pub unreadable: usize,
}

/// Why a call couldn't be recorded or the pending file flushed. Messages
/// name files, never what is in them.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("only calls refused because the database was unavailable wait in the pending file")]
    NotPending,

    #[error("couldn't {action} {path}: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

fn io_error(action: &'static str, path: &Path) -> impl FnOnce(io::Error) -> Error {
    let path = path.to_path_buf();
    move |source| Error::Io {
        action,
        path,
        source,
    }
}

/// One call in the pending file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Line {
    id: String,
    at: String,
    client: String,
    tool: String,
    access_level: Option<String>,
    outcome: String,
    purpose: Option<String>,
    /// The redacted arguments, as the JSON text the log stores.
    args: String,
}

/// The calls past the cap, counted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Counts {
    groups: Vec<Count>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Count {
    id: String,
    client: String,
    tool: String,
    outcome: String,
    calls: u64,
    first_at: String,
}

/// The limits [`record`] keeps to; smaller ones in tests.
#[derive(Debug, Clone, Copy)]
struct Cap {
    entries: usize,
    bytes: u64,
}

const CAP: Cap = Cap {
    entries: MAX_ENTRIES,
    bytes: MAX_BYTES,
};

/// Records `entry`, a call refused because the database at `db_path`
/// couldn't be opened, in the pending file, timed now. Its outcome must be
/// [`Outcome::is_unavailable`], and it carries no result. Returns once the
/// line is synced to disk; waits while another process appends or flushes.
pub fn record(db_path: &Path, entry: &NewEntry) -> Result<Recorded, Error> {
    record_capped(db_path, entry, DateTime::from(SystemTime::now()), CAP)
}

fn record_capped(
    db_path: &Path,
    entry: &NewEntry,
    at: DateTime<Utc>,
    cap: Cap,
) -> Result<Recorded, Error> {
    if !entry.outcome.is_unavailable()
        || entry.row_count != 0
        || entry.result_hash.is_some()
        || entry.withheld_count != 0
    {
        return Err(Error::NotPending);
    }
    let line = Line {
        id: Uuid::now_v7().to_string(),
        at: timestamp(at),
        client: entry.client.as_str().to_owned(),
        tool: entry.tool.clone(),
        access_level: entry.access_level.map(|level| level.as_str().to_owned()),
        outcome: entry.outcome.as_str().to_owned(),
        purpose: entry
            .purpose
            .as_deref()
            .map(|purpose| purpose.chars().take(MAX_PURPOSE_LEN).collect()),
        args: entry.args_redacted_json.clone(),
    };
    let mut json = serde_json::to_string(&line).map_err(|error| Error::Io {
        action: "encode an entry for",
        path: path(db_path),
        source: io::Error::other(error),
    })?;
    json.push('\n');

    let pending = path(db_path);
    let (mut file, created) = open_locked(&pending)?;
    let mut held = Vec::new();
    let len = file.metadata().map_err(io_error("read", &pending))?.len();
    if len < cap.bytes {
        file.read_to_end(&mut held)
            .map_err(io_error("read", &pending))?;
    }
    let cut_short = held.last().is_some_and(|last| *last != b'\n');
    let lines = held
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .count();
    let needed = json.len() as u64 + u64::from(cut_short);
    if len >= cap.bytes || lines >= cap.entries || len + needed > cap.bytes {
        count_dropped(db_path, &line)?;
        return Ok(Recorded::Counted);
    }

    if cut_short {
        // A crash cut the last line short: end it, so this one stays whole.
        json.insert(0, '\n');
    }
    file.write_all(json.as_bytes())
        .and_then(|()| file.sync_data())
        .map_err(io_error("write", &pending))?;
    if created {
        sync_parent(&pending)?;
    }
    Ok(Recorded::Kept)
}

/// Counts `line`, which didn't fit, in the counts file. The pending file's
/// lock is held.
fn count_dropped(db_path: &Path, line: &Line) -> Result<(), Error> {
    let path = dropped_path(db_path);
    let mut counts = match read_counts(&path) {
        Ok(counts) => counts.unwrap_or_default(),
        Err(Unreadable) => {
            // Replaced whole by a rename, so this isn't a crash's doing.
            tracing::warn!("the access log's dropped-call counts were unreadable; starting again");
            Counts::default()
        }
    };
    let same = |count: &&mut Count| {
        count.client == line.client && count.tool == line.tool && count.outcome == line.outcome
    };
    match counts.groups.iter_mut().find(same) {
        Some(count) => count.calls += 1,
        None => counts.groups.push(Count {
            id: Uuid::now_v7().to_string(),
            client: line.client.clone(),
            tool: line.tool.clone(),
            outcome: line.outcome.clone(),
            calls: 1,
            first_at: line.at.clone(),
        }),
    }
    let json = serde_json::to_vec(&counts).map_err(|error| Error::Io {
        action: "encode",
        path: path.clone(),
        source: io::Error::other(error),
    })?;
    let temp = dropped_temp_path(db_path);
    let mut file = private_options()
        .write(true)
        .truncate(true)
        .open(&temp)
        .map_err(io_error("create", &temp))?;
    file.write_all(&json)
        .and_then(|()| file.sync_all())
        .map_err(io_error("write", &temp))?;
    drop(file);
    fs::rename(&temp, &path).map_err(io_error("replace", &path))?;
    sync_parent(&path)
}

/// Moves the calls waiting in the pending file of `db` into its access log
/// (see the module docs) and empties the file. Cheap when there are none:
/// the files are checked before anything is locked. Waits while another
/// process appends or flushes.
pub fn flush(db: &mut Database) -> Result<Flushed, Error> {
    flush_until(db, Stop::Never)
}

/// Where a test stops [`flush_until`], as a crash would.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    Never,
    #[cfg(test)]
    AfterCommit,
}

fn flush_until(db: &mut Database, stop: Stop) -> Result<Flushed, Error> {
    let db_path = db.path().to_path_buf();
    let pending = path(&db_path);
    let dropped = dropped_path(&db_path);
    let flushing = flushing_path(&db_path);
    let waiting = match fs::metadata(&pending) {
        Ok(metadata) => metadata.len() > 0,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(io_error("read", &pending)(error)),
    };
    if !waiting && !exists(&dropped)? && !exists(&flushing)? {
        return Ok(Flushed::default());
    }

    let (mut file, _) = open_locked(&pending)?;
    let mut flushed = Flushed::default();
    // At most twice: counts left by a stopped flush first, then the
    // current ones. Nobody else can add any while the lock is held.
    loop {
        if !exists(&flushing)? && exists(&dropped)? {
            fs::rename(&dropped, &flushing).map_err(io_error("rename", &dropped))?;
            sync_parent(&flushing)?;
        }
        let counts = match read_counts(&flushing) {
            Ok(counts) => counts,
            Err(Unreadable) => {
                flushed.unreadable += 1;
                None
            }
        };
        let mut held = Vec::new();
        file.rewind()
            .and_then(|()| file.read_to_end(&mut held))
            .map_err(io_error("read", &pending))?;

        insert(db, &held, counts.as_ref(), &mut flushed)?;
        if stop != Stop::Never {
            return Ok(flushed);
        }

        file.set_len(0)
            .and_then(|()| file.sync_all())
            .map_err(io_error("empty", &pending))?;
        if exists(&flushing)? {
            fs::remove_file(&flushing).map_err(io_error("remove", &flushing))?;
            sync_parent(&flushing)?;
        }
        if !exists(&dropped)? {
            break;
        }
    }
    if flushed.unreadable > 0 {
        tracing::warn!(
            skipped = flushed.unreadable,
            "skipped unreadable entries in the access log's pending file"
        );
    }
    Ok(flushed)
}

/// Inserts the pending lines in `held` and the `counts` in one transaction.
fn insert(
    db: &mut Database,
    held: &[u8],
    counts: Option<&Counts>,
    flushed: &mut Flushed,
) -> Result<(), Error> {
    let tx = db.conn_mut().transaction()?;
    {
        let mut statement = tx.prepare(
            "INSERT INTO mcp_access_log (at, client, tool, access_level, outcome, purpose, \
             args_redacted_json, pending_id, summarised_calls) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
             ON CONFLICT (pending_id) DO NOTHING",
        )?;
        let mut run = |values: &[&dyn rusqlite::ToSql]| -> Result<Inserted, Error> {
            match statement.execute(values) {
                Ok(0) => Ok(Inserted::AlreadyThere),
                Ok(_) => Ok(Inserted::New),
                // A value the log's schema refuses: SQLite undoes only this
                // statement, and the transaction goes on.
                Err(error) if error.sqlite_error_code() == Some(ErrorCode::ConstraintViolation) => {
                    Ok(Inserted::Refused)
                }
                Err(error) => Err(error.into()),
            }
        };

        for raw in held
            .split(|byte| *byte == b'\n')
            .filter(|raw| !raw.is_empty())
        {
            let Some(line) = serde_json::from_slice::<Line>(raw)
                .ok()
                .filter(|line| is_pending_outcome(&line.outcome))
            else {
                flushed.unreadable += 1;
                continue;
            };
            let inserted = run(params![
                line.at,
                line.client,
                line.tool,
                line.access_level,
                line.outcome,
                line.purpose,
                line.args,
                line.id,
                None::<i64>,
            ])?;
            flushed.count(inserted, false);
        }
        for count in counts
            .map(|counts| counts.groups.as_slice())
            .unwrap_or_default()
        {
            let Some(calls) = i64::try_from(count.calls)
                .ok()
                .filter(|_| is_pending_outcome(&count.outcome))
            else {
                flushed.unreadable += 1;
                continue;
            };
            let inserted = run(params![
                count.first_at,
                count.client,
                count.tool,
                None::<String>,
                count.outcome,
                None::<String>,
                "{}",
                count.id,
                calls,
            ])?;
            flushed.count(inserted, true);
        }
    }
    tx.commit()?;
    Ok(())
}

/// What inserting one pending line or count did.
enum Inserted {
    New,
    AlreadyThere,
    /// The log's schema refused it.
    Refused,
}

impl Flushed {
    fn count(&mut self, inserted: Inserted, summary: bool) {
        match inserted {
            Inserted::New if summary => self.summaries += 1,
            Inserted::New => self.moved += 1,
            Inserted::AlreadyThere => self.already_logged += 1,
            Inserted::Refused => self.unreadable += 1,
        }
    }
}

fn is_pending_outcome(outcome: &str) -> bool {
    outcome
        .parse::<Outcome>()
        .is_ok_and(Outcome::is_unavailable)
}

/// The counts file couldn't be parsed.
struct Unreadable;

fn read_counts(path: &Path) -> Result<Option<Counts>, Unreadable> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            tracing::debug!(%error, "couldn't read the dropped-call counts");
            return Err(Unreadable);
        }
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| Unreadable)
}

/// Opens (creating it `0600` if needed) and locks the pending file, and
/// says whether it was created.
fn open_locked(path: &Path) -> Result<(File, bool), Error> {
    let created = !exists(path)?;
    let file = private_options()
        .read(true)
        .append(true)
        .open(path)
        .map_err(io_error("open", path))?;
    file.lock().map_err(io_error("lock", path))?;
    Ok((file, created))
}

fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.create(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
}

fn exists(path: &Path) -> Result<bool, Error> {
    path.try_exists().map_err(io_error("check", path))
}

fn sync_parent(path: &Path) -> Result<(), Error> {
    match path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        Some(dir) => crate::backup::sync_dir(dir).map_err(io_error("sync", dir)),
        None => Ok(()),
    }
}

/// UTC, as the log stores it: `YYYY-MM-DDTHH:MM:SS.SSSZ`.
fn timestamp(at: DateTime<Utc>) -> String {
    at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

#[cfg(test)]
mod tests;
