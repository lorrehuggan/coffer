//! `coffer schedules` against scratch data dirs and synthetic rows imported
//! through core. Nothing here touches the real data dir.
//!
//! The seeded account has two monthly series, May to October 2026: a fixed
//! gym membership on the 10th and a variable energy bill on the 15th, plus
//! one-off rows (the latest on 20 October).

use std::path::PathBuf;

use chrono::NaiveDate;
use clap::Parser;
use coffer_core::account::{self, AccountKind, NewAccount};
use coffer_core::db::Database;
use coffer_core::id::{AccountId, ScheduleId};
use coffer_core::import;
use coffer_core::money::{Currency, Money};
use coffer_core::recurring::{self, RecurringSeries, SeriesKey};
use coffer_core::schedule::{self, OccurrenceState, ScheduleStatus, Tolerance};
use coffer_core::statement::{SourceFormat, SourceLocation, Statement, StatementLine};
use tempfile::TempDir;

use super::*;

const GYM: &str = "Brightlane Gym";
const ENERGY: &str = "Northgrid Energy";
const ENERGY_MINORS: [i64; 6] = [-5420, -6130, -4875, -5890, -6210, -5555];

fn date(month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, month, day).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn line(month: u32, day: u32, minor: i64, merchant: &str) -> StatementLine {
    let mut line = StatementLine::new(date(month, day), gbp(minor), merchant.to_uppercase());
    line.merchant = Some(merchant.to_owned());
    line
}

fn statement() -> Statement {
    let mut lines = Vec::new();
    for (index, month) in (5..=10).enumerate() {
        lines.push(line(month, 10, -3500, GYM));
        lines.push(line(month, 15, ENERGY_MINORS[index], ENERGY));
    }
    lines.push(line(6, 21, -450, "Corner Deli"));
    lines.push(line(9, 2, -1299, "Pagewell Books"));
    lines.push(line(10, 20, -380, "Corner Deli"));
    lines.sort_by_key(|line| line.date);
    let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
    for (index, line) in lines.into_iter().enumerate() {
        statement.push(SourceLocation::Line(index as u64 + 2), Ok(line));
    }
    statement
}

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

    /// A GBP account with the synthetic rows imported.
    fn seeded() -> (Scratch, AccountId) {
        let scratch = Scratch::new();
        let mut db = scratch.open();
        let account = account::create(
            &mut db,
            &NewAccount::new("Everyday", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        let preview = import::preview(&db, account, "recurring.csv", statement()).unwrap();
        let confirm = preview.needs_confirmation();
        import::commit(&mut db, &preview, confirm).unwrap();
        (scratch, account)
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

    fn open(&self) -> Database {
        Database::open(&self.db).unwrap()
    }

    /// The detected series of `merchant`.
    fn series(&self, merchant: &str) -> RecurringSeries {
        recurring::find(&self.open())
            .unwrap()
            .into_iter()
            .find(|series| series.merchant_display == merchant)
            .unwrap_or_else(|| panic!("{merchant} isn't detected"))
    }

    /// Confirms the gym series and returns its key and the schedule's ID.
    fn confirm_gym(&self) -> (RecurringSeries, ScheduleId) {
        let series = self.series(GYM);
        let out = self.ok(&["schedules", "confirm", series.key.as_str()]);
        assert!(out.starts_with("Confirmed schedule "), "{out}");
        let schedules = schedule::list(&self.open()).unwrap();
        assert_eq!(schedules.len(), 1);
        (series, schedules[0].id)
    }

    fn state(&self, id: ScheduleId, on: NaiveDate) -> Option<OccurrenceState> {
        let today = schedule::local_today().unwrap();
        schedule::view(&self.open(), id, today)
            .unwrap()
            .occurrences
            .into_iter()
            .find(|occurrence| occurrence.date == on)
            .map(|occurrence| occurrence.state)
    }

    fn status(&self, id: ScheduleId) -> ScheduleStatus {
        schedule::get(&self.open(), id).unwrap().status
    }
}

fn parse(args: &[&str]) -> SchedulesCommand {
    let cli = Cli::try_parse_from(std::iter::once("coffer").chain(args.iter().copied()))
        .unwrap_or_else(|error| panic!("{args:?} doesn't parse: {error}"));
    match cli.command {
        Some(Command::Schedules(SchedulesArgs { command })) => command,
        other => panic!("not a schedules command: {other:?}"),
    }
}

fn parse_err(args: &[&str]) -> String {
    match Cli::try_parse_from(std::iter::once("coffer").chain(args.iter().copied())) {
        Ok(cli) => panic!("{args:?} parsed: {:?}", cli.command),
        Err(error) => error.to_string(),
    }
}

const KEY: &str = "0123456789ab";

fn key() -> SeriesKey {
    KEY.parse().unwrap()
}

// Parsing ----------------------------------------------------------------------

#[test]
fn parses_detect_list_and_show() {
    assert!(matches!(
        parse(&["schedules", "detect"]),
        SchedulesCommand::Detect
    ));
    assert!(matches!(
        parse(&["schedules", "list"]),
        SchedulesCommand::List
    ));
    let SchedulesCommand::Show(args) = parse(&["schedules", "show", "4"]) else {
        panic!("not show");
    };
    assert_eq!(args.id, 4);
    // A subcommand is required.
    parse_err(&["schedules"]);
    parse_err(&["schedules", "show"]);
    parse_err(&["schedules", "show", "four"]);
}

#[test]
fn parses_confirm() {
    let SchedulesCommand::Confirm(args) = parse(&["schedules", "confirm", KEY]) else {
        panic!("not confirm");
    };
    assert_eq!(args.key, key());
    assert_eq!(args.tolerance, None);
    assert_eq!(args.tolerance_days, None);

    let SchedulesCommand::Confirm(args) = parse(&[
        "schedules",
        "confirm",
        "0123456789AB",
        "--tolerance",
        "1.50",
        "--tolerance-days",
        "4",
    ]) else {
        panic!("not confirm");
    };
    assert_eq!(args.key, key(), "keys are read in either case");
    assert_eq!(args.tolerance.as_deref(), Some("1.50"));
    assert_eq!(args.tolerance_days, Some(4));

    // A negative tolerance reaches the command, which refuses it.
    let SchedulesCommand::Confirm(args) =
        parse(&["schedules", "confirm", KEY, "--tolerance", "-1"])
    else {
        panic!("not confirm");
    };
    assert_eq!(args.tolerance.as_deref(), Some("-1"));

    let error = parse_err(&["schedules", "confirm", "Streamfix"]);
    assert!(error.contains("12 lowercase hex digits"), "{error}");
    parse_err(&["schedules", "confirm", KEY, "--tolerance-days", "-1"]);
    parse_err(&["schedules", "confirm"]);
}

#[test]
fn parses_occurrence_commands() {
    let SchedulesCommand::Skip(args) = parse(&["schedules", "skip", "2", "2026-11-03"]) else {
        panic!("not skip");
    };
    assert_eq!((args.id, args.date), (2, date(11, 3)));
    let SchedulesCommand::Unskip(args) = parse(&["schedules", "unskip", "2", "2026-11-03"]) else {
        panic!("not unskip");
    };
    assert_eq!((args.id, args.date), (2, date(11, 3)));
    let SchedulesCommand::Match(args) = parse(&["schedules", "match", "2", "2026-10-03", "57"])
    else {
        panic!("not match");
    };
    assert_eq!((args.id, args.date, args.transaction), (2, date(10, 3), 57));
    let SchedulesCommand::Unmatch(args) = parse(&["schedules", "unmatch", "2", "2026-10-03"])
    else {
        panic!("not unmatch");
    };
    assert_eq!((args.id, args.date), (2, date(10, 3)));

    let error = parse_err(&["schedules", "skip", "2", "03/11/2026"]);
    assert!(error.contains("YYYY-MM-DD"), "{error}");
    parse_err(&["schedules", "skip", "2"]);
    parse_err(&["schedules", "match", "2", "2026-10-03"]);
}

#[test]
fn parses_status_commands_and_delete() {
    let SchedulesCommand::Pause(args) = parse(&["schedules", "pause", "3"]) else {
        panic!("not pause");
    };
    assert_eq!(args.id, 3);
    let SchedulesCommand::Resume(args) = parse(&["schedules", "resume", "3"]) else {
        panic!("not resume");
    };
    assert_eq!(args.id, 3);
    let SchedulesCommand::End(args) = parse(&["schedules", "end", "3"]) else {
        panic!("not end");
    };
    assert_eq!(args.id, 3);

    let SchedulesCommand::Delete(args) = parse(&["schedules", "delete", "3"]) else {
        panic!("not delete");
    };
    assert_eq!(args.id, 3);
    assert!(!args.yes);
    let SchedulesCommand::Delete(args) = parse(&["schedules", "delete", "3", "--yes"]) else {
        panic!("not delete");
    };
    assert!(args.yes);
    let SchedulesCommand::Delete(args) = parse(&["schedules", "delete", "-y", "3"]) else {
        panic!("not delete");
    };
    assert!(args.yes);
}

#[test]
fn parses_dismiss_and_undismiss() {
    let SchedulesCommand::Dismiss(args) = parse(&["schedules", "dismiss", KEY]) else {
        panic!("not dismiss");
    };
    assert_eq!(args.key, key());
    let SchedulesCommand::Undismiss(args) = parse(&["schedules", "undismiss", KEY]) else {
        panic!("not undismiss");
    };
    assert_eq!(args.key, key());
    parse_err(&["schedules", "dismiss", "abc"]);
}

// Running ----------------------------------------------------------------------

#[test]
fn needs_an_existing_database() {
    let scratch = Scratch::new();
    let error = scratch.err(&["schedules", "list"]);
    assert!(error.contains("no Coffer database"), "{error}");
    assert!(!scratch.db.exists(), "the database was created");
}

#[test]
fn detect_lists_the_series_with_their_keys() {
    let (scratch, account) = Scratch::seeded();
    let gym = scratch.series(GYM);
    let energy = scratch.series(ENERGY);

    let out = scratch.ok(&["schedules", "detect"]);
    let header = out.lines().next().unwrap();
    for column in [
        "KEY", "ACCOUNT", "MERCHANT", "CADENCE", "AMOUNT", "KIND", "COUNT", "LAST", "NEXT",
    ] {
        assert!(header.contains(column), "{header}");
    }
    let gym_line = out
        .lines()
        .find(|line| line.contains(GYM))
        .unwrap_or_else(|| panic!("no gym line:\n{out}"));
    assert!(gym_line.starts_with(gym.key.as_str()), "{gym_line}");
    for cell in [
        &account.to_string(),
        "monthly",
        "-35.00 GBP",
        "fixed",
        "6",
        "2026-10-10",
        &gym.next_expected.to_string(),
    ] {
        assert!(gym_line.contains(cell), "{cell:?} in {gym_line}");
    }
    assert!(!gym_line.contains("lapsed"), "{gym_line}");
    let energy_line = out.lines().find(|line| line.contains(ENERGY)).unwrap();
    assert!(
        energy_line.starts_with(energy.key.as_str()),
        "{energy_line}"
    );
    assert!(energy_line.contains("variable"), "{energy_line}");
    assert!(
        energy_line.contains(&format!("{} GBP", energy.typical.to_decimal_string())),
        "{energy_line}"
    );
    assert!(!out.contains("Corner Deli"), "{out}");
    assert!(out.contains("coffer schedules confirm <key>"), "{out}");
}

#[test]
fn confirm_list_and_show() {
    let (scratch, _) = Scratch::seeded();
    let (gym, id) = scratch.confirm_gym();
    let created = schedule::get(&scratch.open(), id).unwrap();
    assert_eq!(created.series_key.as_ref(), Some(&gym.key));
    assert_eq!(created.tolerance, Tolerance::default_for(&gym));
    assert_eq!(created.amount, gbp(-3500));

    // Confirmed, it isn't detected any more.
    let out = scratch.ok(&["schedules", "detect"]);
    assert!(!out.contains(GYM), "{out}");
    assert!(out.contains(ENERGY), "{out}");

    let out = scratch.ok(&["schedules", "list"]);
    let header = out.lines().next().unwrap();
    for column in [
        "ID",
        "MERCHANT",
        "CADENCE",
        "AMOUNT",
        "±TOLERANCE",
        "±DAYS",
        "STATUS",
        "NEXT",
        "MISSED",
        "LATE",
    ] {
        assert!(header.contains(column), "{header}");
    }
    let row = out.lines().nth(1).unwrap();
    assert!(row.starts_with(&id.to_string()), "{row}");
    for cell in [GYM, "monthly", "-35.00 GBP", "±3.50", "±3", "active"] {
        assert!(row.contains(cell), "{cell:?} in {row}");
    }

    let out = scratch.ok(&["schedules", "show", &id.to_string()]);
    assert!(
        out.starts_with(&format!("Schedule {id}: {GYM}, monthly")),
        "{out}"
    );
    assert!(out.contains("Imported through 2026-10-20."), "{out}");
    for month in 5..=10 {
        let on = date(month, 10).to_string();
        let row = out
            .lines()
            .find(|line| line.starts_with(&on))
            .unwrap_or_else(|| panic!("no {on} row:\n{out}"));
        assert!(row.contains("matched"), "{row}");
        assert!(row.contains("-35.00 GBP"), "{row}");
    }

    let error = scratch.err(&["schedules", "show", "999"]);
    assert!(error.contains("no schedule 999"), "{error}");
}

#[test]
fn confirm_takes_a_tolerance() {
    let (scratch, _) = Scratch::seeded();
    let energy = scratch.series(ENERGY);
    let key = energy.key.to_string();

    let error = scratch.err(&["schedules", "confirm", &key, "--tolerance", "-1"]);
    assert!(error.contains("can't be negative"), "{error}");
    let error = scratch.err(&["schedules", "confirm", &key, "--tolerance", "1.234"]);
    assert!(error.contains("isn't an amount in GBP"), "{error}");
    let error = scratch.err(&["schedules", "confirm", &key, "--tolerance-days", "20"]);
    assert!(error.contains("at most 13 days"), "{error}");
    let error = scratch.err(&["schedules", "confirm", KEY, "--tolerance", "1"]);
    assert!(error.contains("no recurring series"), "{error}");
    assert!(schedule::list(&scratch.open()).unwrap().is_empty());

    // Only the days: the amount keeps its default.
    scratch.ok(&["schedules", "confirm", &key, "--tolerance-days", "5"]);
    let schedules = schedule::list(&scratch.open()).unwrap();
    assert_eq!(
        schedules[0].tolerance,
        Tolerance {
            amount_minor: Tolerance::default_for(&energy).amount_minor,
            days: 5,
        }
    );

    // Both.
    let gym = scratch.series(GYM);
    scratch.ok(&[
        "schedules",
        "confirm",
        gym.key.as_str(),
        "--tolerance",
        "2.5",
        "--tolerance-days",
        "1",
    ]);
    let schedules = schedule::list(&scratch.open()).unwrap();
    assert_eq!(
        schedules[1].tolerance,
        Tolerance {
            amount_minor: 250,
            days: 1,
        }
    );
}

#[test]
fn skip_and_unskip_an_occurrence() {
    let (scratch, _) = Scratch::seeded();
    let (_, id) = scratch.confirm_gym();
    let id_text = id.to_string();
    let today = schedule::local_today().unwrap();
    let next = schedule::view(&scratch.open(), id, today)
        .unwrap()
        .next
        .expect("an active schedule has a next occurrence");
    let next_text = next.to_string();

    let out = scratch.ok(&["schedules", "skip", &id_text, &next_text]);
    assert!(
        out.contains(&format!("Skipped the {next} occurrence")),
        "{out}"
    );
    assert_eq!(scratch.state(id, next), Some(OccurrenceState::Skipped));
    let error = scratch.err(&["schedules", "skip", &id_text, &next_text]);
    assert!(error.contains("already skipped"), "{error}");

    scratch.ok(&["schedules", "unskip", &id_text, &next_text]);
    assert_eq!(scratch.state(id, next), Some(OccurrenceState::Upcoming));
    let error = scratch.err(&["schedules", "unskip", &id_text, &next_text]);
    assert!(error.contains("isn't skipped"), "{error}");

    let error = scratch.err(&["schedules", "skip", &id_text, "2026-10-11"]);
    assert!(error.contains("isn't an occurrence"), "{error}");
}

#[test]
fn unmatch_and_match_an_occurrence() {
    let (scratch, _) = Scratch::seeded();
    let (gym, id) = scratch.confirm_gym();
    let id_text = id.to_string();
    let october = date(10, 10);
    let paid = *gym.transactions.last().unwrap();

    let out = scratch.ok(&["schedules", "unmatch", &id_text, "2026-10-10"]);
    assert!(out.contains("Removed the match"), "{out}");
    // Due, past the tolerance, imported through 20 October: missed.
    assert_eq!(scratch.state(id, october), Some(OccurrenceState::Missed));
    let error = scratch.err(&["schedules", "unmatch", &id_text, "2026-10-10"]);
    assert!(error.contains("isn't matched"), "{error}");

    let out = scratch.ok(&[
        "schedules",
        "match",
        &id_text,
        "2026-10-10",
        &paid.to_string(),
    ]);
    assert!(
        out.contains(&format!("Matched transaction {paid}")),
        "{out}"
    );
    assert_eq!(scratch.state(id, october), Some(OccurrenceState::Matched));

    // September, unmatched: October's payment is taken, and there is no
    // transaction 99999.
    scratch.ok(&["schedules", "unmatch", &id_text, "2026-09-10"]);
    let error = scratch.err(&[
        "schedules",
        "match",
        &id_text,
        "2026-09-10",
        &paid.to_string(),
    ]);
    assert!(error.contains("already fulfils"), "{error}");
    let error = scratch.err(&["schedules", "match", &id_text, "2026-09-10", "99999"]);
    assert!(error.contains("no transaction 99999"), "{error}");
}

#[test]
fn pause_resume_and_end() {
    let (scratch, _) = Scratch::seeded();
    let (_, id) = scratch.confirm_gym();
    let id_text = id.to_string();

    let out = scratch.ok(&["schedules", "pause", &id_text]);
    assert!(out.contains(&format!("Paused schedule {id}")), "{out}");
    assert_eq!(scratch.status(id), ScheduleStatus::Paused);
    let list = scratch.ok(&["schedules", "list"]);
    assert!(list.contains("paused"), "{list}");
    let error = scratch.err(&["schedules", "pause", &id_text]);
    assert!(error.contains("already paused"), "{error}");

    scratch.ok(&["schedules", "resume", &id_text]);
    assert_eq!(scratch.status(id), ScheduleStatus::Active);

    let out = scratch.ok(&["schedules", "end", &id_text]);
    assert!(out.contains(&format!("Ended schedule {id}")), "{out}");
    assert_eq!(scratch.status(id), ScheduleStatus::Ended);
    let list = scratch.ok(&["schedules", "list"]);
    assert!(list.contains("ended"), "{list}");
    let error = scratch.err(&["schedules", "resume", &id_text]);
    assert!(error.contains("has ended"), "{error}");

    let error = scratch.err(&["schedules", "pause", "999"]);
    assert!(error.contains("no schedule 999"), "{error}");
}

#[test]
fn delete_needs_yes() {
    let (scratch, _) = Scratch::seeded();
    let (_, id) = scratch.confirm_gym();
    let id_text = id.to_string();
    let today = schedule::local_today().unwrap();
    let next = schedule::view(&scratch.open(), id, today)
        .unwrap()
        .next
        .unwrap();
    scratch.ok(&["schedules", "skip", &id_text, &next.to_string()]);

    let out = scratch.ok(&["schedules", "delete", &id_text]);
    assert!(
        out.contains("removes the schedule, 6 matched payments and 1 skip"),
        "{out}"
    );
    assert!(out.contains("Nothing was deleted"), "{out}");
    assert!(schedule::get(&scratch.open(), id).is_ok());

    let out = scratch.ok(&["schedules", "delete", &id_text, "--yes"]);
    assert!(
        out.contains(&format!(
            "Deleted schedule {id}, 6 matched payments and 1 skip."
        )),
        "{out}"
    );
    assert!(schedule::list(&scratch.open()).unwrap().is_empty());
    let out = scratch.ok(&["schedules", "list"]);
    assert!(out.starts_with("No schedules yet."), "{out}");
    // The series is offered again.
    let out = scratch.ok(&["schedules", "detect"]);
    assert!(out.contains(GYM), "{out}");

    let error = scratch.err(&["schedules", "delete", &id_text, "--yes"]);
    assert!(error.contains(&format!("no schedule {id}")), "{error}");
}

#[test]
fn dismiss_and_undismiss_a_series() {
    let (scratch, _) = Scratch::seeded();
    let energy = scratch.series(ENERGY);
    let key = energy.key.to_string();

    let out = scratch.ok(&["schedules", "dismiss", &key]);
    assert!(out.contains("as not recurring"), "{out}");
    let out = scratch.ok(&["schedules", "detect"]);
    assert!(!out.contains(ENERGY), "{out}");
    assert!(out.contains(GYM), "{out}");
    let out = scratch.ok(&["schedules", "dismiss", &key]);
    assert!(out.contains("already marked"), "{out}");

    let out = scratch.ok(&["schedules", "undismiss", &key]);
    assert!(out.contains("no longer marked"), "{out}");
    let out = scratch.ok(&["schedules", "detect"]);
    assert!(out.contains(ENERGY), "{out}");
    let out = scratch.ok(&["schedules", "undismiss", &key]);
    assert!(out.contains("isn't marked"), "{out}");
}
