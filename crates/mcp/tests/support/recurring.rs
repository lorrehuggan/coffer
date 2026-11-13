//! Recurring payments for the fixture database (Block D): a synthetic
//! statement imported into its own account through the real import path,
//! and a schedule on it.
//!
//! - [`GYM`]: a fixed monthly payment, the same amount on the 10th of each
//!   month, with a category: detection finds it.
//! - [`ENERGY`]: a variable monthly bill on the 15th: detection finds it.
//! - [`STREAMING`]: a monthly subscription on the 3rd with August's payment
//!   missing and October's 5 days late, confirmed as a schedule
//!   ([`seed_schedule`]), so it is a schedule, not a detected series.
//! - One-off rows around them (noise).
//!
//! All merchants, amounts and dates are made up.

use chrono::NaiveDate;
use coffer_core::account::{self, AccountKind, NewAccount};
use coffer_core::category::CategoryKey;
use coffer_core::db::Database;
use coffer_core::id::{AccountId, ScheduleId};
use coffer_core::import;
use coffer_core::money::{Currency, Money};
use coffer_core::recurring::Cadence;
use coffer_core::schedule::{self, NewSchedule, Tolerance};
use coffer_core::statement::{SourceFormat, SourceLocation, Statement, StatementLine};

use super::Fixture;

/// The fixed monthly payment.
pub const GYM: &str = "Brightlane Gym";
pub const GYM_MINOR: i64 = -3500;
pub const GYM_CATEGORY: &str = "health.fitness";

/// The variable monthly bill, May to October.
pub const ENERGY: &str = "Northgrid Energy";
pub const ENERGY_MINORS: [i64; 6] = [-5420, -6130, -4875, -5890, -6210, -5555];

/// The scheduled subscription.
pub const STREAMING: &str = "Streamfix Media";
pub const STREAMING_MINOR: i64 = -1099;
/// The schedule's amount tolerance, in pence, and its date tolerance.
pub const STREAMING_TOLERANCE: Tolerance = Tolerance {
    amount_minor: 110,
    days: 3,
};

/// The account the rows go into, and the schedule.
#[derive(Debug, Clone, Copy)]
pub struct Recurring {
    pub account: AccountId,
    pub schedule: ScheduleId,
}

pub fn date(month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, month, day).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn line(month: u32, day: u32, minor: i64, merchant: &str, description: &str) -> StatementLine {
    let mut line = StatementLine::new(date(month, day), gbp(minor), description);
    line.merchant = Some(merchant.to_owned());
    line
}

/// The streaming payments: (month, day) of each, May to October with
/// August missing and October late.
pub const STREAMING_PAID: [(u32, u32); 5] = [(5, 3), (6, 3), (7, 3), (9, 3), (10, 8)];

/// The rows: six months of each series and four one-offs, the latest on
/// 20 October (so imports cover the account through then).
pub fn statement() -> Statement {
    let mut lines = Vec::new();
    for (index, month) in (5..=10).enumerate() {
        let mut gym = line(month, 10, GYM_MINOR, GYM, "BRIGHTLANE GYM MEMBERSHIP");
        gym.category = Some(CategoryKey::new(GYM_CATEGORY).unwrap());
        lines.push(gym);
        lines.push(line(
            month,
            15,
            ENERGY_MINORS[index],
            ENERGY,
            "NORTHGRID ENERGY DD",
        ));
    }
    for (month, day) in STREAMING_PAID {
        lines.push(line(
            month,
            day,
            STREAMING_MINOR,
            STREAMING,
            "STREAMFIX MEDIA SUBSCRIPTION",
        ));
    }
    lines.push(line(6, 21, -450, "Corner Deli", "CORNER DELI"));
    lines.push(line(9, 2, -1299, "Pagewell Books", "PAGEWELL BOOKS"));
    lines.push(line(8, 27, 2500, "Tallow Market", "TALLOW MARKET REFUND"));
    lines.push(line(10, 20, -380, "Corner Deli", "CORNER DELI"));
    lines.sort_by_key(|line| line.date);

    let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
    for (index, line) in lines.into_iter().enumerate() {
        statement.push(SourceLocation::Line(index as u64 + 2), Ok(line));
    }
    statement
}

/// Imports [`statement`] into a new account. Detection works without a
/// schedule; [`seed`] adds one.
pub fn seed_rows(fixture: &Fixture) -> AccountId {
    let mut db = fixture.open();
    let account = account::create(
        &mut db,
        &NewAccount::new("Subscriptions", AccountKind::Current, Currency::GBP),
    )
    .unwrap();
    let preview = import::preview(&db, account, "recurring.csv", statement()).unwrap();
    let confirm = preview.needs_confirmation();
    import::commit(&mut db, &preview, confirm).unwrap();
    account
}

/// The streaming schedule: monthly from 3 May, which matches the rows
/// already imported.
pub fn seed_schedule(db: &mut Database, account: AccountId) -> ScheduleId {
    schedule::create(
        db,
        &NewSchedule {
            account,
            merchant_display: STREAMING.to_owned(),
            category: None,
            cadence: Cadence::Monthly,
            start_date: date(5, 3),
            amount: gbp(STREAMING_MINOR),
            tolerance: STREAMING_TOLERANCE,
            series_key: None,
        },
    )
    .unwrap()
}

/// [`seed_rows`], then [`seed_schedule`].
pub fn seed(fixture: &Fixture) -> Recurring {
    let account = seed_rows(fixture);
    let schedule = seed_schedule(&mut fixture.open(), account);
    Recurring { account, schedule }
}
