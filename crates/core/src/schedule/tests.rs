//! Tests for schedules against a real database. Synthetic data only.

use chrono::NaiveDate;
use rusqlite::params;
use tempfile::TempDir;

use super::*;
use crate::account::{AccountKind, NewAccount};
use crate::exclusion::{self, ExclusionReason};
use crate::import::{self, CommitSummary, Committed};
use crate::manual::{self, ManualTransaction};
use crate::statement::{SourceFormat, SourceLocation, Statement, StatementLine};

const MERCHANT: &str = "Streamco";

struct Fixture {
    db: Database,
    account: AccountId,
    _dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
        let account = account::create(
            &mut db,
            &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        Fixture {
            db,
            account,
            _dir: dir,
        }
    }

    fn other_account(&mut self, name: &str) -> AccountId {
        account::create(
            &mut self.db,
            &NewAccount::new(name, AccountKind::Current, Currency::GBP),
        )
        .unwrap()
    }

    /// Imports `lines` into `account`, confirming whatever the preview asks
    /// for.
    fn import_into(&mut self, account: AccountId, lines: Vec<StatementLine>) -> CommitSummary {
        let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
        for (index, line) in lines.into_iter().enumerate() {
            statement.push(SourceLocation::Line(index as u64 + 2), Ok(line));
        }
        let preview = import::preview(&self.db, account, "statement.csv", statement).unwrap();
        let confirm = preview.needs_confirmation();
        match import::commit(&mut self.db, &preview, confirm).unwrap() {
            Committed::Batch(summary) => summary,
            Committed::NothingNew => panic!("expected a new batch"),
        }
    }

    fn import(&mut self, lines: Vec<StatementLine>) -> CommitSummary {
        self.import_into(self.account, lines)
    }

    /// A monthly schedule for [`MERCHANT`] from `start`: -£10.00, ±£0.50,
    /// ±3 days.
    fn monthly(&self, start: NaiveDate) -> NewSchedule {
        NewSchedule {
            account: self.account,
            merchant_display: MERCHANT.to_owned(),
            category: None,
            cadence: Cadence::Monthly,
            start_date: start,
            amount: gbp(-1000),
            tolerance: Tolerance {
                amount_minor: 50,
                days: 3,
            },
            series_key: None,
        }
    }

    fn create(&mut self, schedule: &NewSchedule) -> ScheduleId {
        create(&mut self.db, schedule).unwrap()
    }

    fn view(&self, id: ScheduleId, today: NaiveDate) -> ScheduleView {
        view(&self.db, id, today).unwrap()
    }

    /// `(occurrence date, state)` for every listed occurrence.
    fn states(&self, id: ScheduleId, today: NaiveDate) -> Vec<(NaiveDate, OccurrenceState)> {
        self.view(id, today)
            .occurrences
            .iter()
            .map(|occurrence| (occurrence.date, occurrence.state))
            .collect()
    }

    /// The transaction on `date` with `minor`.
    fn transaction(&self, date: NaiveDate, minor: i64) -> TransactionId {
        self.db
            .conn()
            .query_row(
                "SELECT id FROM \"transaction\" WHERE date = ?1 AND amount_minor = ?2",
                params![date_key(date), minor],
                |row| row.get(0),
            )
            .unwrap()
    }

    /// `(schedule, occurrence date, transaction)` for every match.
    fn matches(&self) -> Vec<(ScheduleId, NaiveDate, TransactionId)> {
        let mut statement = self
            .db
            .conn()
            .prepare(
                "SELECT schedule_id, occurrence_date, transaction_id FROM schedule_match \
                 ORDER BY occurrence_date, schedule_id",
            )
            .unwrap();
        statement
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    account::date(row, "occurrence_date")?,
                    row.get(2)?,
                ))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn lock(&self, through: Option<NaiveDate>) {
        self.db
            .conn()
            .execute(
                "UPDATE account SET reconciled_through = ?1 WHERE id = ?2",
                params![through.map(date_key), self.account],
            )
            .unwrap();
    }
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

/// A payment to `merchant`.
fn paid(day: NaiveDate, minor: i64, merchant: &str) -> StatementLine {
    let mut line = StatementLine::new(day, gbp(minor), format!("{merchant} payment"));
    line.merchant = Some(merchant.to_owned());
    line
}

/// A row for another merchant: it only moves how far imports cover.
fn other(day: NaiveDate) -> StatementLine {
    paid(day, -123, "Corner Shop")
}

use OccurrenceState::{Late, Matched, Missed, Skipped, Upcoming};

// Matching at import --------------------------------------------------------

#[test]
fn an_import_matches_and_undoing_it_opens_the_occurrences_again() {
    let mut f = Fixture::new();
    let id = f.create(&f.monthly(date(2026, 1, 15)));
    let today = date(2026, 3, 1);
    let rows = || {
        vec![
            paid(date(2026, 1, 15), -1000, MERCHANT),
            paid(date(2026, 2, 16), -1000, MERCHANT),
        ]
    };

    let summary = f.import(rows());
    assert_eq!(summary.schedule_matches, 2);
    assert_eq!(
        f.states(id, today),
        [
            (date(2026, 1, 15), Matched),
            (date(2026, 2, 15), Matched),
            (date(2026, 3, 15), Upcoming),
        ]
    );
    let view = f.view(id, today);
    assert_eq!(view.next, Some(date(2026, 3, 15)));
    assert_eq!(view.covered_through, Some(date(2026, 2, 16)));
    let payment = view.occurrences[1].payment.clone().unwrap();
    assert_eq!(payment.date, date(2026, 2, 16));
    assert_eq!(payment.amount, gbp(-1000));
    assert_eq!(payment.matched_by, MatchedBy::Auto);
    assert_eq!(payment.transaction, f.transaction(date(2026, 2, 16), -1000));

    import::undo(&mut f.db, summary.batch).unwrap();
    assert!(f.matches().is_empty());
    let view = f.view(id, today);
    assert_eq!(view.covered_through, None);
    assert!(view.occurrences.iter().all(|o| o.state == Upcoming));
    assert!(view.occurrences.iter().all(|o| o.payment.is_none()));

    let summary = f.import(rows());
    assert_eq!(summary.schedule_matches, 2);
    assert_eq!(
        f.states(id, today)[..2],
        [(date(2026, 1, 15), Matched), (date(2026, 2, 15), Matched)]
    );
}

#[test]
fn an_import_matches_only_its_own_accounts_schedules() {
    let mut f = Fixture::new();
    let id = f.create(&f.monthly(date(2026, 1, 15)));
    let savings = f.other_account("Savings");
    let summary = f.import_into(savings, vec![paid(date(2026, 1, 15), -1000, MERCHANT)]);
    assert_eq!(summary.schedule_matches, 0);
    assert_eq!(
        f.states(id, date(2026, 1, 10)),
        [(date(2026, 1, 15), Upcoming)]
    );
}

#[test]
fn an_occurrence_is_missed_only_once_imports_cover_it() {
    let mut f = Fixture::new();
    let id = f.create(&f.monthly(date(2026, 1, 15)));
    f.import(vec![
        paid(date(2026, 1, 15), -1000, MERCHANT),
        other(date(2026, 2, 17)),
    ]);
    // 15 February plus 3 days is 18 February; imports reach the 17th.
    let late_march = date(2026, 3, 30);
    assert_eq!(
        f.states(id, late_march)[1..3],
        [(date(2026, 2, 15), Upcoming), (date(2026, 3, 15), Upcoming)]
    );

    // A manual row doesn't cover anything.
    manual::create(
        &mut f.db,
        &ManualTransaction::new(f.account, date(2026, 3, 20), gbp(-500), "Cash"),
    )
    .unwrap();
    assert_eq!(f.states(id, late_march)[1].1, Upcoming);

    f.import(vec![other(date(2026, 2, 18))]);
    assert_eq!(
        f.view(id, late_march).covered_through,
        Some(date(2026, 2, 18))
    );
    assert_eq!(
        f.states(id, late_march)[1..3],
        [(date(2026, 2, 15), Missed), (date(2026, 3, 15), Upcoming)]
    );
    // Not before the tolerance has passed, whatever imports cover.
    assert_eq!(f.states(id, date(2026, 2, 18))[1].1, Upcoming);
    assert_eq!(f.states(id, date(2026, 2, 19))[1].1, Missed);
}

#[test]
fn a_payment_after_the_tolerance_is_late() {
    let mut f = Fixture::new();
    let id = f.create(&f.monthly(date(2026, 1, 10)));
    let summary = f.import(vec![
        paid(date(2026, 1, 7), -1000, MERCHANT),  // 3 days early
        paid(date(2026, 2, 13), -1000, MERCHANT), // 3 days late
        paid(date(2026, 3, 14), -1000, MERCHANT), // 4 days late
    ]);
    assert_eq!(summary.schedule_matches, 3);
    assert_eq!(
        f.states(id, date(2026, 3, 20)),
        [
            (date(2026, 1, 10), Matched),
            (date(2026, 2, 10), Matched),
            (date(2026, 3, 10), Late),
            (date(2026, 4, 10), Upcoming),
        ]
    );
    let view = f.view(id, date(2026, 3, 20));
    assert_eq!(
        view.occurrences[2].payment.as_ref().unwrap().date,
        date(2026, 3, 14)
    );
}

#[test]
fn month_end_anchors_fall_on_the_last_day_of_short_months() {
    let mut f = Fixture::new();
    let id = f.create(&f.monthly(date(2026, 1, 31)));
    f.import(vec![
        paid(date(2026, 1, 31), -1000, MERCHANT),
        paid(date(2026, 2, 28), -1000, MERCHANT),
        paid(date(2026, 3, 31), -1000, MERCHANT),
        paid(date(2028, 2, 29), -1000, MERCHANT),
    ]);
    let matched: Vec<NaiveDate> = f.matches().into_iter().map(|(_, day, _)| day).collect();
    assert_eq!(
        matched,
        [
            date(2026, 1, 31),
            date(2026, 2, 28),
            date(2026, 3, 31),
            date(2028, 2, 29)
        ]
    );
    let states = f.states(id, date(2028, 3, 1));
    assert!(states.contains(&(date(2028, 2, 29), Matched)));
    assert!(states.contains(&(date(2026, 4, 30), Missed)));
    assert_eq!(states.last(), Some(&(date(2028, 3, 31), Upcoming)));
}

#[test]
fn only_a_payment_like_the_schedule_matches() {
    let mut f = Fixture::new();
    let mut schedule = f.monthly(date(2026, 1, 15));
    let id = f.create(&schedule);
    // Wrong sign within the amount tolerance, on another schedule.
    schedule.start_date = date(2026, 6, 15);
    schedule.tolerance.amount_minor = 5000;
    let wide = f.create(&schedule);
    let summary = f.import(vec![
        paid(date(2026, 1, 15), -1051, MERCHANT),    // 51p over
        paid(date(2026, 2, 15), -1000, "Otherco"),   // another merchant
        paid(date(2026, 3, 15), -950, MERCHANT),     // 50p under: fits
        paid(date(2026, 4, 15), -1000, " streamco"), // normalised: fits
        paid(date(2026, 6, 15), 1000, MERCHANT),     // money in
    ]);
    assert_eq!(summary.schedule_matches, 2);
    assert_eq!(
        f.matches(),
        [
            (
                id,
                date(2026, 3, 15),
                f.transaction(date(2026, 3, 15), -950)
            ),
            (
                id,
                date(2026, 4, 15),
                f.transaction(date(2026, 4, 15), -1000)
            ),
        ]
    );
    assert_eq!(
        f.states(wide, date(2026, 6, 1)),
        [(date(2026, 6, 15), Upcoming)]
    );
}

#[test]
fn a_second_payment_in_a_window_matches_nothing() {
    let mut f = Fixture::new();
    let id = f.create(&f.monthly(date(2026, 1, 15)));
    let summary = f.import(vec![
        paid(date(2026, 1, 15), -1000, MERCHANT),
        paid(date(2026, 1, 20), -1000, MERCHANT),
        other(date(2026, 2, 25)),
    ]);
    assert_eq!(summary.schedule_matches, 1);
    assert_eq!(
        f.states(id, date(2026, 3, 1)),
        [
            (date(2026, 1, 15), Matched),
            (date(2026, 2, 15), Missed),
            (date(2026, 3, 15), Upcoming),
        ]
    );
}

#[test]
fn excluded_and_transfer_rows_never_match() {
    let mut f = Fixture::new();
    f.import(vec![
        paid(date(2026, 1, 15), -1000, MERCHANT),
        paid(date(2026, 2, 15), -1000, MERCHANT),
        paid(date(2026, 3, 15), -1000, MERCHANT),
        paid(date(2026, 4, 15), -1000, MERCHANT),
    ]);
    let excluded = f.transaction(date(2026, 1, 15), -1000);
    exclusion::exclude(&mut f.db, excluded, ExclusionReason::Disputed, None).unwrap();
    let conn = f.db.conn();
    for (status, day) in [("suggested", 2), ("confirmed", 3), ("rejected", 4)] {
        conn.execute("INSERT INTO transfer_group (status) VALUES (?1)", [status])
            .unwrap();
        let group = conn.last_insert_rowid();
        conn.execute(
            "UPDATE \"transaction\" SET transfer_group_id = ?1 WHERE id = ?2",
            params![group, f.transaction(date(2026, day, 15), -1000)],
        )
        .unwrap();
    }

    let id = f.create(&f.monthly(date(2026, 1, 15)));
    // Only the row in a rejected pair is an ordinary row.
    assert_eq!(
        f.states(id, date(2026, 4, 30)),
        [
            (date(2026, 1, 15), Missed),
            (date(2026, 2, 15), Missed),
            (date(2026, 3, 15), Missed),
            (date(2026, 4, 15), Matched),
            (date(2026, 5, 15), Upcoming),
        ]
    );
}

#[test]
fn a_transfer_pair_found_at_import_doesnt_match() {
    let mut f = Fixture::new();
    let savings = f.other_account("Savings");
    f.import_into(savings, vec![paid(date(2026, 1, 15), 1000, "Current")]);
    let id = f.create(&f.monthly(date(2026, 1, 15)));
    let summary = f.import(vec![paid(date(2026, 1, 15), -1000, MERCHANT)]);
    assert_eq!(
        summary.transfer_pairs, 1,
        "the rows should pair as a transfer"
    );
    assert_eq!(summary.schedule_matches, 0);
    assert_eq!(
        f.states(id, date(2026, 1, 10)),
        [(date(2026, 1, 15), Upcoming)]
    );
}

#[test]
fn the_lower_schedule_id_wins_a_row() {
    let mut f = Fixture::new();
    let first = f.create(&f.monthly(date(2026, 1, 15)));
    let second = f.create(&f.monthly(date(2026, 1, 14)));
    let summary = f.import(vec![paid(date(2026, 1, 15), -1000, MERCHANT)]);
    assert_eq!(summary.schedule_matches, 1);
    assert_eq!(f.matches()[0].0, first);
    assert_eq!(
        f.states(second, date(2026, 1, 1)),
        [(date(2026, 1, 14), Upcoming)]
    );
}

// Creating ------------------------------------------------------------------

#[test]
fn create_matches_history_after_the_lock_date() {
    let mut f = Fixture::new();
    f.import(vec![
        paid(date(2026, 1, 15), -1000, MERCHANT),
        paid(date(2026, 2, 15), -1000, MERCHANT),
        paid(date(2026, 3, 15), -1000, MERCHANT),
    ]);
    f.lock(Some(date(2026, 2, 20)));
    let id = f.create(&f.monthly(date(2026, 1, 15)));
    assert_eq!(
        f.states(id, date(2026, 4, 1)),
        [
            (date(2026, 1, 15), Missed),
            (date(2026, 2, 15), Missed),
            (date(2026, 3, 15), Matched),
            (date(2026, 4, 15), Upcoming),
        ]
    );

    // Without a lock, everything from the start.
    f.lock(None);
    let mut again = f.monthly(date(2026, 1, 15));
    again.merchant_display = "Corner Shop".to_owned();
    again.amount = gbp(-123);
    f.import(vec![other(date(2026, 1, 16)), other(date(2026, 2, 14))]);
    let shop = f.create(&again);
    assert_eq!(
        f.states(shop, date(2026, 3, 1))[..2],
        [(date(2026, 1, 15), Matched), (date(2026, 2, 15), Matched)]
    );
}

#[test]
fn create_stores_what_it_was_given() {
    let mut f = Fixture::new();
    let mut schedule = f.monthly(date(2026, 1, 15));
    schedule.merchant_display = "  Streamco ".to_owned();
    schedule.category = Some(CategoryKey::new("bills").unwrap());
    let id = f.create(&schedule);
    let stored = get(&f.db, id).unwrap();
    assert_eq!(
        stored,
        Schedule {
            id,
            account: f.account,
            merchant_display: MERCHANT.to_owned(),
            category: schedule.category.clone(),
            cadence: Cadence::Monthly,
            start_date: date(2026, 1, 15),
            amount: gbp(-1000),
            tolerance: schedule.tolerance,
            status: ScheduleStatus::Active,
            series_key: None,
        }
    );
    assert_eq!(list(&f.db).unwrap(), [stored]);
}

#[test]
fn create_refuses_a_bad_schedule() {
    let mut f = Fixture::new();
    let good = f.monthly(date(2026, 1, 15));
    let refused = |f: &mut Fixture, change: &dyn Fn(&mut NewSchedule)| {
        let mut schedule = good.clone();
        change(&mut schedule);
        create(&mut f.db, &schedule).unwrap_err()
    };
    assert!(matches!(
        refused(&mut f, &|s| s.account = AccountId::new(999)),
        Error::AccountNotFound(_)
    ));
    let house = account::create(
        &mut f.db,
        &NewAccount::new("House", AccountKind::Asset, Currency::GBP),
    )
    .unwrap();
    assert!(matches!(
        refused(&mut f, &|s| s.account = house),
        Error::HoldsValuations(a) if a == house
    ));
    assert!(matches!(
        refused(&mut f, &|s| s.amount = gbp(0)),
        Error::ZeroAmount
    ));
    assert!(matches!(
        refused(&mut f, &|s| s.amount = Money::new(-1000, Currency::EUR)),
        Error::CurrencyMismatch { .. }
    ));
    assert!(matches!(
        refused(&mut f, &|s| s.merchant_display = " \t".to_owned()),
        Error::EmptyMerchant
    ));
    assert!(matches!(
        refused(&mut f, &|s| s.tolerance.days = 14),
        Error::ToleranceTooWide {
            days: 14,
            max: 13,
            ..
        }
    ));
    assert!(matches!(
        refused(&mut f, &|s| s.tolerance.amount_minor = -1),
        Error::NegativeTolerance
    ));
    assert!(matches!(
        refused(&mut f, &|s| s.category =
            Some(CategoryKey::new("no_such").unwrap())),
        Error::CategoryNotFound(_)
    ));
    assert!(list(&f.db).unwrap().is_empty());
    // The widest tolerance is fine.
    let mut widest = good.clone();
    widest.tolerance.days = 13;
    f.create(&widest);
}

#[test]
#[ignore = "needs recurring detection (Block D lead runs it after merge)"]
fn confirm_creates_a_schedule_from_a_detected_series() {
    let mut f = Fixture::new();
    f.import(vec![
        paid(date(2026, 1, 15), -1000, MERCHANT),
        paid(date(2026, 2, 15), -1000, MERCHANT),
        paid(date(2026, 3, 15), -1000, MERCHANT),
        paid(date(2026, 4, 15), -1000, MERCHANT),
    ]);
    let series = recurring::find(&f.db).unwrap();
    assert_eq!(series.len(), 1);
    let tolerance = Tolerance {
        amount_minor: 7,
        days: 2,
    };
    let id = confirm(&mut f.db, &series[0].key, Some(tolerance)).unwrap();
    let stored = get(&f.db, id).unwrap();
    assert_eq!(stored.tolerance, tolerance);
    assert_eq!(stored.series_key.as_ref(), Some(&series[0].key));
    assert_eq!(f.matches().len(), 4);
    // Now scheduled, so no longer suggested.
    assert!(recurring::find(&f.db).unwrap().is_empty());
    let missing: SeriesKey = "000000000000".parse().unwrap();
    assert!(matches!(
        confirm(&mut f.db, &missing, None),
        Err(Error::Recurring(recurring::Error::SeriesNotFound(_)))
    ));
}

// Skips and the user's matches -----------------------------------------------

#[test]
fn skipping_and_unskipping() {
    let mut f = Fixture::new();
    let id = f.create(&f.monthly(date(2026, 1, 15)));
    f.import(vec![paid(date(2026, 1, 15), -1000, MERCHANT)]);
    let feb = date(2026, 2, 15);

    skip(&mut f.db, id, feb).unwrap();
    assert!(matches!(skip(&mut f.db, id, feb), Err(Error::AlreadySkipped(d)) if d == feb));
    assert_eq!(f.states(id, date(2026, 3, 1))[1], (feb, Skipped));
    // A payment in a skipped occurrence's window matches nothing.
    let summary = f.import(vec![paid(date(2026, 2, 15), -1000, MERCHANT)]);
    assert_eq!(summary.schedule_matches, 0);
    let row = f.transaction(feb, -1000);
    assert!(matches!(
        match_occurrence(&mut f.db, id, feb, row),
        Err(Error::AlreadySkipped(_))
    ));

    unskip(&mut f.db, id, feb).unwrap();
    assert!(matches!(
        unskip(&mut f.db, id, feb),
        Err(Error::NotSkipped(_))
    ));
    assert_eq!(f.states(id, date(2026, 2, 1))[1], (feb, Upcoming));

    // A matched occurrence can't be skipped.
    let jan = date(2026, 1, 15);
    assert!(matches!(
        skip(&mut f.db, id, jan),
        Err(Error::AlreadyMatched(_))
    ));
    // Only occurrences.
    for not_one in [date(2026, 2, 16), date(2025, 12, 15)] {
        assert!(matches!(
            skip(&mut f.db, id, not_one),
            Err(Error::NotAnOccurrence { date, .. }) if date == not_one
        ));
    }
    // Skipping ahead is fine; it shows once the occurrence is reached.
    skip(&mut f.db, id, date(2026, 4, 15)).unwrap();
    assert_eq!(f.view(id, date(2026, 2, 1)).occurrences.len(), 2);
    assert_eq!(
        f.states(id, date(2026, 4, 1))[2..],
        [
            (date(2026, 3, 15), Upcoming),
            (date(2026, 4, 15), Skipped),
            (date(2026, 5, 15), Upcoming)
        ]
    );
    assert_eq!(f.view(id, date(2026, 4, 1)).next, Some(date(2026, 5, 15)));
}

#[test]
fn the_user_matches_and_unmatches() {
    let mut f = Fixture::new();
    let savings = f.other_account("Savings");
    f.import_into(savings, vec![paid(date(2026, 2, 1), -2222, "Elsewhere")]);
    f.import(vec![
        paid(date(2026, 1, 15), -1000, MERCHANT),
        paid(date(2026, 2, 3), -4321, "Odd name"),
    ]);
    let id = f.create(&f.monthly(date(2026, 1, 15)));
    let jan = date(2026, 1, 15);
    let feb = date(2026, 2, 15);
    let odd = f.transaction(date(2026, 2, 3), -4321);
    let auto = f.transaction(jan, -1000);

    assert!(matches!(
        match_occurrence(&mut f.db, id, feb, TransactionId::new(999)),
        Err(Error::TransactionNotFound(_))
    ));
    let elsewhere = f.transaction(date(2026, 2, 1), -2222);
    assert!(matches!(
        match_occurrence(&mut f.db, id, feb, elsewhere),
        Err(Error::WrongAccount { .. })
    ));
    assert!(matches!(
        match_occurrence(&mut f.db, id, feb, auto),
        Err(Error::TransactionTaken(t)) if t == auto
    ));
    assert!(matches!(
        match_occurrence(&mut f.db, id, jan, odd),
        Err(Error::AlreadyMatched(_))
    ));
    assert!(matches!(
        match_occurrence(&mut f.db, id, date(2026, 2, 14), odd),
        Err(Error::NotAnOccurrence { .. })
    ));
    assert!(matches!(
        match_occurrence(&mut f.db, ScheduleId::new(999), feb, odd),
        Err(Error::NotFound(_))
    ));

    // Any amount, merchant or date.
    match_occurrence(&mut f.db, id, feb, odd).unwrap();
    let view = f.view(id, date(2026, 3, 1));
    let payment = view.occurrences[1].payment.clone().unwrap();
    assert_eq!(payment.matched_by, MatchedBy::User);
    assert_eq!(payment.transaction, odd);
    assert_eq!(view.occurrences[1].state, Matched);

    unmatch(&mut f.db, id, jan).unwrap();
    assert!(matches!(
        unmatch(&mut f.db, id, jan),
        Err(Error::NotMatched(_))
    ));
    assert_eq!(f.states(id, date(2026, 1, 1))[0], (jan, Upcoming));
    // The row is free again, for the user; an import doesn't take it back.
    match_occurrence(&mut f.db, id, jan, auto).unwrap();
}

#[test]
fn lock_dates_freeze_occurrences_on_or_before_them() {
    let mut f = Fixture::new();
    let id = f.create(&f.monthly(date(2026, 1, 15)));
    f.import(vec![
        paid(date(2026, 1, 15), -1000, MERCHANT),
        paid(date(2026, 2, 15), -1000, MERCHANT),
        other(date(2026, 3, 20)),
    ]);
    let (jan, feb, mar) = (date(2026, 1, 15), date(2026, 2, 15), date(2026, 3, 15));
    let feb_row = f.transaction(feb, -1000);

    f.lock(Some(feb));
    let locked = |result: Result<(), Error>, at: NaiveDate| {
        assert!(
            matches!(result, Err(Error::Locked { date, through }) if date == at && through == feb),
            "{result:?}"
        );
    };
    locked(unmatch(&mut f.db, id, feb), feb);
    locked(unmatch(&mut f.db, id, jan), jan);
    assert!(matches!(
        delete(&mut f.db, id),
        Err(Error::Locked { date, .. }) if date == jan
    ));
    // After the lock, fine.
    skip(&mut f.db, id, mar).unwrap();
    unskip(&mut f.db, id, mar).unwrap();

    f.lock(Some(date(2026, 1, 31)));
    unmatch(&mut f.db, id, feb).unwrap();
    skip(&mut f.db, id, feb).unwrap();
    f.lock(Some(feb));
    locked(unskip(&mut f.db, id, feb), feb);
    f.lock(Some(date(2026, 1, 31)));
    unskip(&mut f.db, id, feb).unwrap();
    f.lock(Some(feb));
    locked(skip(&mut f.db, id, feb), feb);
    locked(match_occurrence(&mut f.db, id, feb, feb_row), feb);

    // A lock before every decision lets the schedule go.
    f.lock(Some(date(2026, 1, 1)));
    assert_eq!(
        delete(&mut f.db, id).unwrap(),
        Deleted {
            matches: 1,
            skips: 0
        }
    );
}

// Status --------------------------------------------------------------------

#[test]
fn pause_resume_and_end() {
    let mut f = Fixture::new();
    let id = f.create(&f.monthly(date(2026, 1, 15)));
    f.import(vec![paid(date(2026, 1, 15), -1000, MERCHANT)]);
    skip(&mut f.db, id, date(2026, 3, 15)).unwrap();

    assert!(matches!(
        resume(&mut f.db, id),
        Err(Error::AlreadyInStatus(_, ScheduleStatus::Active))
    ));
    pause(&mut f.db, id).unwrap();
    assert!(matches!(
        pause(&mut f.db, id),
        Err(Error::AlreadyInStatus(_, ScheduleStatus::Paused))
    ));
    assert_eq!(get(&f.db, id).unwrap().status, ScheduleStatus::Paused);

    // Paused: no matching, and only decided occurrences are listed.
    let summary = f.import(vec![
        paid(date(2026, 2, 15), -1000, MERCHANT),
        other(date(2026, 4, 30)),
    ]);
    assert_eq!(summary.schedule_matches, 0);
    let view = f.view(id, date(2026, 5, 1));
    assert_eq!(view.next, None);
    assert_eq!(
        f.states(id, date(2026, 5, 1)),
        [(date(2026, 1, 15), Matched), (date(2026, 3, 15), Skipped)]
    );
    // Its amount can still change.
    let tolerance = Tolerance {
        amount_minor: 0,
        days: 1,
    };
    update(&mut f.db, id, gbp(-1100), tolerance).unwrap();

    // Resuming derives the rest again; what was imported meanwhile isn't
    // matched.
    resume(&mut f.db, id).unwrap();
    assert_eq!(
        f.states(id, date(2026, 5, 1)),
        [
            (date(2026, 1, 15), Matched),
            (date(2026, 2, 15), Missed),
            (date(2026, 3, 15), Skipped),
            (date(2026, 4, 15), Missed),
            (date(2026, 5, 15), Upcoming),
        ]
    );

    pause(&mut f.db, id).unwrap();
    end(&mut f.db, id).unwrap();
    assert!(matches!(
        end(&mut f.db, id),
        Err(Error::AlreadyInStatus(_, ScheduleStatus::Ended))
    ));
    assert!(matches!(resume(&mut f.db, id), Err(Error::Ended(_))));
    assert!(matches!(pause(&mut f.db, id), Err(Error::Ended(_))));
    assert!(matches!(
        update(&mut f.db, id, gbp(-1000), tolerance),
        Err(Error::Ended(_))
    ));
    assert!(matches!(
        skip(&mut f.db, id, date(2026, 2, 15)),
        Err(Error::Ended(_))
    ));
    let row = f.transaction(date(2026, 2, 15), -1000);
    assert!(matches!(
        match_occurrence(&mut f.db, id, date(2026, 2, 15), row),
        Err(Error::Ended(_))
    ));
    let summary = f.import(vec![paid(date(2026, 5, 15), -1100, MERCHANT)]);
    assert_eq!(summary.schedule_matches, 0);
    assert_eq!(f.view(id, date(2026, 6, 1)).occurrences.len(), 2);
    // Its history can still be corrected.
    unskip(&mut f.db, id, date(2026, 3, 15)).unwrap();
    assert_eq!(f.view(id, date(2026, 6, 1)).occurrences.len(), 1);

    // An active schedule ends too.
    let other_id = f.create(&f.monthly(date(2026, 1, 16)));
    end(&mut f.db, other_id).unwrap();
}

#[test]
fn update_changes_the_amount_and_tolerance() {
    let mut f = Fixture::new();
    let id = f.create(&f.monthly(date(2026, 1, 15)));
    f.import(vec![paid(date(2026, 1, 15), -1000, MERCHANT)]);
    let tolerance = Tolerance {
        amount_minor: 10,
        days: 5,
    };
    assert!(matches!(
        update(&mut f.db, id, gbp(0), tolerance),
        Err(Error::ZeroAmount)
    ));
    assert!(matches!(
        update(&mut f.db, id, Money::new(-1200, Currency::EUR), tolerance),
        Err(Error::CurrencyMismatch { .. })
    ));
    assert!(matches!(
        update(
            &mut f.db,
            id,
            gbp(-1200),
            Tolerance {
                amount_minor: 10,
                days: 14
            }
        ),
        Err(Error::ToleranceTooWide { .. })
    ));
    update(&mut f.db, id, gbp(-1200), tolerance).unwrap();
    let stored = get(&f.db, id).unwrap();
    assert_eq!((stored.amount, stored.tolerance), (gbp(-1200), tolerance));
    // The existing match stays; new imports match the new amount.
    assert_eq!(f.matches().len(), 1);
    let summary = f.import(vec![
        paid(date(2026, 2, 10), -1205, MERCHANT),
        paid(date(2026, 3, 15), -1000, MERCHANT),
    ]);
    assert_eq!(summary.schedule_matches, 1);
}

#[test]
fn delete_removes_matches_and_skips() {
    let mut f = Fixture::new();
    let id = f.create(&f.monthly(date(2026, 1, 15)));
    f.import(vec![
        paid(date(2026, 1, 15), -1000, MERCHANT),
        paid(date(2026, 2, 15), -1000, MERCHANT),
    ]);
    skip(&mut f.db, id, date(2026, 3, 15)).unwrap();
    end(&mut f.db, id).unwrap();
    assert_eq!(
        delete(&mut f.db, id).unwrap(),
        Deleted {
            matches: 2,
            skips: 1
        }
    );
    assert!(matches!(get(&f.db, id), Err(Error::NotFound(_))));
    assert!(matches!(
        view(&f.db, id, date(2026, 1, 1)),
        Err(Error::NotFound(_))
    ));
    assert!(matches!(delete(&mut f.db, id), Err(Error::NotFound(_))));
    assert!(f.matches().is_empty());
    // The rows are free for another schedule.
    f.create(&f.monthly(date(2026, 1, 15)));
    assert_eq!(f.matches().len(), 2);
}

#[test]
fn deleting_a_matched_manual_row_clears_its_match() {
    let mut f = Fixture::new();
    let mut entry = ManualTransaction::new(f.account, date(2026, 1, 15), gbp(-1000), "Paid");
    entry.merchant = Some(MERCHANT.to_owned());
    let row = manual::create(&mut f.db, &entry).unwrap();
    let id = f.create(&f.monthly(date(2026, 1, 15)));
    assert_eq!(f.matches(), [(id, date(2026, 1, 15), row)]);

    manual::delete(&mut f.db, row).unwrap();
    assert!(f.matches().is_empty());
    assert_eq!(
        f.states(id, date(2026, 1, 1)),
        [(date(2026, 1, 15), Upcoming)]
    );
}

// Views ---------------------------------------------------------------------

#[test]
fn overview_lists_every_schedule_on_either_connection() {
    let mut f = Fixture::new();
    let first = f.create(&f.monthly(date(2026, 1, 15)));
    let mut weekly = f.monthly(date(2026, 1, 5));
    weekly.cadence = Cadence::Weekly;
    weekly.tolerance.days = 1;
    weekly.merchant_display = "Corner Shop".to_owned();
    weekly.amount = gbp(-123);
    let second = f.create(&weekly);
    f.import(vec![
        paid(date(2026, 1, 15), -1000, MERCHANT),
        other(date(2026, 1, 13)),
    ]);
    let today = date(2026, 1, 20);
    let views = overview(&f.db, today).unwrap();
    assert_eq!(
        views.iter().map(|v| v.schedule.id).collect::<Vec<_>>(),
        [first, second]
    );
    assert_eq!(views[0], f.view(first, today));
    assert_eq!(
        f.states(second, today),
        [
            (date(2026, 1, 5), Missed),
            (date(2026, 1, 12), Matched),
            (date(2026, 1, 19), Upcoming),
            (date(2026, 1, 26), Upcoming),
        ]
    );
    assert_eq!(views[1].next, Some(date(2026, 1, 26)));
    let reader = f.db.reader().unwrap();
    assert_eq!(overview_with_reader(&reader, today).unwrap(), views);
}

#[test]
fn a_start_in_the_future_lists_one_occurrence() {
    let mut f = Fixture::new();
    let id = f.create(&f.monthly(date(2026, 6, 1)));
    let view = f.view(id, date(2026, 1, 1));
    assert_eq!(view.next, Some(date(2026, 6, 1)));
    assert_eq!(
        f.states(id, date(2026, 1, 1)),
        [(date(2026, 6, 1), Upcoming)]
    );
}

#[test]
fn a_far_future_today_lists_a_bounded_number() {
    let mut f = Fixture::new();
    let mut weekly = f.monthly(date(2000, 1, 3));
    weekly.cadence = Cadence::Weekly;
    weekly.tolerance.days = 0;
    let id = f.create(&weekly);
    let view = f.view(id, date(9000, 1, 1));
    assert_eq!(view.occurrences.len(), MAX_LISTED_OCCURRENCES);
    assert_eq!(view.next, None);
}

// Occurrence arithmetic -----------------------------------------------------

#[test]
fn windows_agree_with_counting_from_the_start() {
    for (cadence, start, days) in [
        (Cadence::Monthly, date(2020, 1, 31), 13),
        (Cadence::Monthly, date(2020, 2, 29), 3),
        (Cadence::Weekly, date(2020, 1, 1), 3),
        (Cadence::FourWeekly, date(2020, 1, 1), 2),
        (Cadence::Quarterly, date(2020, 11, 30), 44),
        (Cadence::Yearly, date(2020, 2, 29), 181),
    ] {
        let schedule = Schedule {
            id: ScheduleId::new(1),
            account: AccountId::new(1),
            merchant_display: MERCHANT.to_owned(),
            category: None,
            cadence,
            start_date: start,
            amount: gbp(-1),
            tolerance: Tolerance {
                amount_minor: 0,
                days,
            },
            status: ScheduleStatus::Active,
            series_key: None,
        };
        let shift = Days::new(u64::from(days));
        let window_start = |k: u32| cadence.nth(start, k).unwrap() - shift;
        let mut k = 0;
        let mut day = window_start(0) - Days::new(1);
        assert_eq!(window_occurrence(&schedule, day), None);
        assert!(check_occurrence(&schedule, day).is_err());
        day = window_start(0);
        while day < date(2032, 1, 1) {
            while window_start(k + 1) <= day {
                k += 1;
            }
            assert_eq!(
                window_occurrence(&schedule, day),
                cadence.nth(start, k),
                "{cadence} from {start} on {day}"
            );
            assert_eq!(
                check_occurrence(&schedule, day).is_ok(),
                (0..=k + 1).any(|i| cadence.nth(start, i) == Some(day)),
                "{cadence} from {start} on {day}"
            );
            day = day + Days::new(1);
        }
    }
}
