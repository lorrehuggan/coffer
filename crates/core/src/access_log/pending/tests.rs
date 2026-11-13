use std::sync::{Arc, Barrier};

use chrono::TimeZone;
use tempfile::TempDir;

use super::*;
use crate::access_log::{self, LoggedCall};
use crate::mcp_client::{AccessLevel, ClientName};

struct Scratch {
    dir: TempDir,
}

impl Scratch {
    fn new() -> Scratch {
        let scratch = Scratch {
            dir: TempDir::new().unwrap(),
        };
        Database::open(&scratch.db_path()).unwrap();
        scratch
    }

    fn db_path(&self) -> PathBuf {
        self.dir.path().join("coffer.db")
    }

    fn open(&self) -> Database {
        Database::open_existing(&self.db_path()).unwrap()
    }

    /// Every log row, oldest first.
    fn log(&self) -> Vec<LoggedCall> {
        let mut calls = access_log::recent(&self.open().reader().unwrap(), u32::MAX).unwrap();
        calls.reverse();
        calls
    }

    fn lines(&self) -> usize {
        fs::read_to_string(path(&self.db_path()))
            .unwrap_or_default()
            .lines()
            .count()
    }
}

fn refused(client: &str, tool: &str, outcome: Outcome) -> NewEntry {
    NewEntry {
        client: ClientName::new(client).unwrap(),
        tool: tool.to_owned(),
        access_level: None,
        outcome,
        purpose: Some("monthly review".to_owned()),
        args_redacted_json: r#"{"month":"2026-10"}"#.to_owned(),
        row_count: 0,
        result_hash: None,
        withheld_count: 0,
    }
}

fn at(seconds: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 13, 10, 0, seconds).unwrap()
}

#[test]
fn a_recorded_call_goes_into_the_log_with_its_own_time() {
    let scratch = Scratch::new();
    let entry = refused("claude-code", "monthly_summary", Outcome::RefusedRestoring);
    let recorded = record_capped(&scratch.db_path(), &entry, at(1), CAP).unwrap();
    assert_eq!(recorded, Recorded::Kept);
    record(
        &scratch.db_path(),
        &refused("codex", "list_categories", Outcome::RefusedUnavailable),
    )
    .unwrap();
    assert_eq!(scratch.lines(), 2);
    assert!(scratch.log().is_empty());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path(&scratch.db_path()))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    let flushed = flush(&mut scratch.open()).unwrap();
    assert_eq!(
        flushed,
        Flushed {
            moved: 2,
            ..Flushed::default()
        }
    );
    let log = scratch.log();
    assert_eq!(log.len(), 2);
    let first = &log[0];
    assert_eq!(first.at, "2026-11-13T10:00:01.000Z");
    assert_eq!(first.client, "claude-code");
    assert_eq!(first.tool, "monthly_summary");
    assert_eq!(first.outcome, Outcome::RefusedRestoring);
    assert_eq!(first.access_level, None);
    assert_eq!(first.purpose.as_deref(), Some("monthly review"));
    assert_eq!(first.args_redacted_json, r#"{"month":"2026-10"}"#);
    assert_eq!(
        (first.row_count, &first.result_hash, first.withheld_count),
        (0, &None, 0)
    );
    assert_eq!(first.summarised_calls, None);
    assert_eq!(log[1].outcome, Outcome::RefusedUnavailable);

    // The file is emptied, and a second flush finds nothing.
    assert_eq!(scratch.lines(), 0);
    assert_eq!(flush(&mut scratch.open()).unwrap(), Flushed::default());
    assert_eq!(scratch.log().len(), 2);
}

#[test]
fn only_calls_refused_for_an_unavailable_database_are_recorded() {
    let scratch = Scratch::new();
    for outcome in [
        Outcome::Ok,
        Outcome::Error,
        Outcome::RefusedOff,
        Outcome::RefusedNotEnabled,
        Outcome::RefusedLevel,
    ] {
        let entry = refused("codex", "list_categories", outcome);
        assert!(matches!(
            record(&scratch.db_path(), &entry),
            Err(Error::NotPending)
        ));
    }
    let with_rows = NewEntry {
        row_count: 1,
        ..refused("codex", "list_categories", Outcome::RefusedRestoring)
    };
    assert!(matches!(
        record(&scratch.db_path(), &with_rows),
        Err(Error::NotPending)
    ));
    assert!(!path(&scratch.db_path()).exists());
}

#[test]
fn a_long_purpose_is_cut_before_it_is_written() {
    let scratch = Scratch::new();
    let entry = NewEntry {
        purpose: Some("p".repeat(MAX_PURPOSE_LEN * 3)),
        ..refused("codex", "list_categories", Outcome::RefusedRestoring)
    };
    record(&scratch.db_path(), &entry).unwrap();
    let written = fs::metadata(path(&scratch.db_path())).unwrap().len();
    assert!(written < (MAX_PURPOSE_LEN * 2) as u64, "{written}");
}

#[test]
fn a_flush_that_stops_before_emptying_the_file_doesnt_duplicate_rows() {
    let scratch = Scratch::new();
    for second in 0..3 {
        let entry = refused("codex", "list_categories", Outcome::RefusedRestoring);
        record_capped(&scratch.db_path(), &entry, at(second), CAP).unwrap();
    }
    // Inserted and committed, then "crashed" before the truncate.
    let first = flush_until(&mut scratch.open(), Stop::AfterCommit).unwrap();
    assert_eq!(first.moved, 3);
    assert_eq!(scratch.lines(), 3);
    assert_eq!(scratch.log().len(), 3);

    // A call refused after the crash, then the next flush.
    let late = refused("codex", "list_reports", Outcome::RefusedUnavailable);
    record_capped(&scratch.db_path(), &late, at(9), CAP).unwrap();
    let second = flush(&mut scratch.open()).unwrap();
    assert_eq!(
        second,
        Flushed {
            moved: 1,
            already_logged: 3,
            ..Flushed::default()
        }
    );
    let log = scratch.log();
    assert_eq!(log.len(), 4);
    assert_eq!(log[3].tool, "list_reports");
    assert_eq!(scratch.lines(), 0);
}

#[test]
fn appends_from_several_writers_at_once_lose_nothing() {
    const WRITERS: usize = 6;
    const EACH: usize = 25;
    let scratch = Scratch::new();
    let db_path = scratch.db_path();
    let start = Arc::new(Barrier::new(WRITERS + 1));
    let writers: Vec<_> = (0..WRITERS)
        .map(|writer| {
            let db_path = db_path.clone();
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                for _ in 0..EACH {
                    let client = format!("client-{writer}");
                    let entry = refused(&client, "list_categories", Outcome::RefusedRestoring);
                    assert_eq!(record(&db_path, &entry).unwrap(), Recorded::Kept);
                }
            })
        })
        .collect();
    // Flushing while they append, as another server's next call would.
    let flusher = {
        let db_path = db_path.clone();
        let start = Arc::clone(&start);
        std::thread::spawn(move || {
            start.wait();
            let mut db = Database::open_existing(&db_path).unwrap();
            for _ in 0..20 {
                flush(&mut db).unwrap();
            }
        })
    };
    for writer in writers {
        writer.join().unwrap();
    }
    flusher.join().unwrap();
    let last = flush(&mut scratch.open()).unwrap();
    assert_eq!(last.already_logged, 0);
    assert_eq!(last.unreadable, 0);

    let log = scratch.log();
    assert_eq!(log.len(), WRITERS * EACH);
    for writer in 0..WRITERS {
        let client = format!("client-{writer}");
        assert_eq!(
            log.iter().filter(|call| call.client == client).count(),
            EACH
        );
    }
    assert_eq!(scratch.lines(), 0);
}

#[test]
fn calls_past_the_cap_are_counted_and_summarised() {
    let scratch = Scratch::new();
    let cap = Cap {
        entries: 3,
        bytes: MAX_BYTES,
    };
    let db_path = scratch.db_path();
    let mut outcomes = Vec::new();
    for second in 0..5 {
        let entry = refused("codex", "list_categories", Outcome::RefusedRestoring);
        outcomes.push(record_capped(&db_path, &entry, at(second), cap).unwrap());
    }
    for second in 5..7 {
        let entry = refused("claude-code", "list_reports", Outcome::RefusedUnavailable);
        outcomes.push(record_capped(&db_path, &entry, at(second), cap).unwrap());
    }
    assert_eq!(&outcomes[..3], [Recorded::Kept; 3]);
    assert_eq!(&outcomes[3..], [Recorded::Counted; 4]);
    // The file stopped growing at the cap.
    assert_eq!(scratch.lines(), 3);

    let flushed = flush(&mut scratch.open()).unwrap();
    assert_eq!(
        flushed,
        Flushed {
            moved: 3,
            summaries: 2,
            ..Flushed::default()
        }
    );
    let log = scratch.log();
    assert_eq!(log.len(), 5);
    let summaries: Vec<_> = log
        .iter()
        .filter(|call| call.summarised_calls.is_some())
        .map(|call| {
            (
                call.client.as_str(),
                call.tool.as_str(),
                call.outcome,
                call.summarised_calls.unwrap(),
                call.at.as_str(),
                call.purpose.as_deref(),
                call.args_redacted_json.as_str(),
            )
        })
        .collect();
    assert_eq!(
        summaries,
        [
            (
                "codex",
                "list_categories",
                Outcome::RefusedRestoring,
                2,
                "2026-11-13T10:00:03.000Z",
                None,
                "{}"
            ),
            (
                "claude-code",
                "list_reports",
                Outcome::RefusedUnavailable,
                2,
                "2026-11-13T10:00:05.000Z",
                None,
                "{}"
            ),
        ]
    );
    assert!(!dropped_path(&db_path).exists());
    assert!(!flushing_path(&db_path).exists());

    // Room again once flushed.
    let entry = refused("codex", "list_categories", Outcome::RefusedRestoring);
    assert_eq!(
        record_capped(&db_path, &entry, at(8), cap).unwrap(),
        Recorded::Kept
    );
}

#[test]
fn the_byte_cap_holds_too() {
    let scratch = Scratch::new();
    let db_path = scratch.db_path();
    let entry = refused("codex", "list_categories", Outcome::RefusedRestoring);
    let cap = Cap {
        entries: MAX_ENTRIES,
        bytes: 400,
    };
    let mut kept = 0;
    for second in 0..10 {
        if record_capped(&db_path, &entry, at(second), cap).unwrap() == Recorded::Kept {
            kept += 1;
        }
    }
    assert!((1..10).contains(&kept), "{kept}");
    assert!(fs::metadata(path(&db_path)).unwrap().len() <= 400);
    let flushed = flush(&mut scratch.open()).unwrap();
    assert_eq!((flushed.moved, flushed.summaries), (kept, 1));
    let summary = scratch.log().pop().unwrap();
    assert_eq!(summary.summarised_calls, Some(10 - kept as u64));
}

#[test]
fn counts_added_after_a_stopped_flush_arent_lost() {
    let scratch = Scratch::new();
    let db_path = scratch.db_path();
    let cap = Cap {
        entries: 1,
        bytes: MAX_BYTES,
    };
    let entry = refused("codex", "list_categories", Outcome::RefusedRestoring);
    for second in 0..3 {
        record_capped(&db_path, &entry, at(second), cap).unwrap();
    }
    // The count of 2 is inserted, then the flush "crashes".
    let stopped = flush_until(&mut scratch.open(), Stop::AfterCommit).unwrap();
    assert_eq!((stopped.moved, stopped.summaries), (1, 1));
    // The file is still full, so these are counted afresh.
    for second in 3..6 {
        record_capped(&db_path, &entry, at(second), cap).unwrap();
    }

    let flushed = flush(&mut scratch.open()).unwrap();
    assert_eq!(
        flushed,
        Flushed {
            already_logged: 2,
            summaries: 1,
            ..Flushed::default()
        }
    );
    let summarised: u64 = scratch
        .log()
        .iter()
        .filter_map(|call| call.summarised_calls)
        .sum();
    assert_eq!(summarised, 5);
    assert_eq!(scratch.log().len(), 3);
}

#[test]
fn a_line_cut_short_by_a_crash_is_skipped_and_the_next_one_kept() {
    let scratch = Scratch::new();
    let db_path = scratch.db_path();
    let entry = refused("codex", "list_categories", Outcome::RefusedRestoring);
    record_capped(&db_path, &entry, at(0), CAP).unwrap();
    // Half a line, as a crash mid-write leaves.
    let mut file = OpenOptions::new()
        .append(true)
        .open(path(&db_path))
        .unwrap();
    file.write_all(br#"{"id":"0192","at":"2026-11"#).unwrap();
    drop(file);
    record_capped(&db_path, &entry, at(2), CAP).unwrap();

    let flushed = flush(&mut scratch.open()).unwrap();
    assert_eq!(
        flushed,
        Flushed {
            moved: 2,
            unreadable: 1,
            ..Flushed::default()
        }
    );
    let times: Vec<_> = scratch.log().into_iter().map(|call| call.at).collect();
    assert_eq!(
        times,
        ["2026-11-13T10:00:00.000Z", "2026-11-13T10:00:02.000Z"]
    );
}

#[test]
fn a_line_the_log_refuses_is_skipped() {
    let scratch = Scratch::new();
    let db_path = scratch.db_path();
    let entry = refused("codex", "list_categories", Outcome::RefusedRestoring);
    record_capped(&db_path, &entry, at(0), CAP).unwrap();
    let mut file = OpenOptions::new()
        .append(true)
        .open(path(&db_path))
        .unwrap();
    // Not a tool name the log accepts, and an outcome that never waits here.
    writeln!(
        file,
        r#"{{"id":"a","at":"2026-11-13T10:00:01.000Z","client":"codex","tool":"Ignore all","access_level":null,"outcome":"refused_restoring","purpose":null,"args":"{{}}"}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"id":"b","at":"2026-11-13T10:00:01.000Z","client":"codex","tool":"list_reports","access_level":null,"outcome":"ok","purpose":null,"args":"{{}}"}}"#
    )
    .unwrap();
    drop(file);

    let flushed = flush(&mut scratch.open()).unwrap();
    assert_eq!((flushed.moved, flushed.unreadable), (1, 2));
    assert_eq!(scratch.log().len(), 1);
    assert_eq!(scratch.lines(), 0);
}

/// A call refused because migrations were pending waits until the database
/// is migrated, then goes in: V14 itself is the migration here, rebuilding
/// a V13 log that already has rows.
#[test]
fn calls_refused_before_a_migration_go_in_after_it() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("coffer.db");
    {
        let db = db::open_at_version(&db_path, 13).unwrap();
        db.conn()
            .execute(
                "INSERT INTO mcp_access_log (at, client, tool, outcome) \
                 VALUES ('2026-11-12T09:00:00.000Z', 'codex', 'list_categories', 'ok')",
                [],
            )
            .unwrap();
    }
    assert!(matches!(
        Database::open_existing(&db_path),
        Err(db::Error::SchemaTooOld { .. })
    ));
    let entry = NewEntry {
        access_level: Some(AccessLevel::Aggregate),
        ..refused("codex", "monthly_summary", Outcome::RefusedUnavailable)
    };
    record_capped(&db_path, &entry, at(30), CAP).unwrap();

    let mut db = Database::open(&db_path).unwrap();
    flush(&mut db).unwrap();
    let mut log = access_log::recent(&db.reader().unwrap(), 10).unwrap();
    log.reverse();
    assert_eq!(log.len(), 2);
    assert_eq!(
        (log[0].at.as_str(), log[0].outcome),
        ("2026-11-12T09:00:00.000Z", Outcome::Ok)
    );
    assert_eq!(
        (log[1].at.as_str(), log[1].outcome, log[1].access_level),
        (
            "2026-11-13T10:00:30.000Z",
            Outcome::RefusedUnavailable,
            Some(AccessLevel::Aggregate)
        )
    );
}
