//! Schedules: confirmed recurring series with an expected date, amount and
//! tolerance (ROADMAP M2.10). Imports fulfil occurrences; missed or late
//! payments are flagged.
//!
//! Block D: the schedule tables come with migration V12
//! (`migrations/V12__schedule.sql`).
//!
//! # Occurrences
//!
//! A schedule's occurrences aren't stored. Occurrence k (from 0) falls on
//! [`Cadence::nth`]`(start_date, k)`. Stored per occurrence is only what was
//! decided about it: a skip (`schedule_skip`) or the transaction that
//! fulfilled it (`schedule_match`). Its [`OccurrenceState`] is derived
//! against a date the caller passes as today, so every query is
//! deterministic ([`local_today`] gives the real one):
//!
//! - **skipped**: the user skipped it;
//! - **matched**: a transaction fulfilled it within the tolerance;
//! - **late**: a transaction fulfilled it after its date plus
//!   `tolerance_days`;
//! - **missed**: its date plus `tolerance_days` is before today, nothing
//!   matched or skipped it, and the account's imports cover that date (its
//!   latest imported transaction is on or after it). Until then it is only
//!   not imported yet, and stays **upcoming** (decision record "budgets and
//!   the forecast", point 2);
//! - **upcoming**: anything else.
//!
//! # Matching
//!
//! A transaction fulfils occurrence k when it is on the schedule's account,
//! in its currency, not excluded and not in a transfer; its display merchant
//! normalises ([`normalize_merchant`]) to the schedule's; it has the
//! schedule's sign and is within `tolerance_minor` of its amount; and its
//! date is in occurrence k's window, `[nth(k) - tolerance_days,
//! nth(k + 1) - tolerance_days)`, for k ≥ 0. Only an open occurrence (not
//! matched, not skipped) takes it; each occurrence takes one transaction
//! and each transaction fulfils one occurrence (of any schedule). Rows are
//! taken oldest first, then by ID. The import commit matches its new rows
//! for every active schedule on the account, in its own DB transaction
//! ([`match_batch`]); undoing the import deletes those matches
//! ([`clear_batch`]).
//!
//! # Status
//!
//! An **active** schedule matches and flags. A **paused** one does neither:
//! it lists only occurrences already matched or skipped, and resuming it
//! derives the rest again (occurrences missed while paused show as missed
//! and can be skipped). **Ended** is final: like paused, and it never
//! resumes. Deleting a schedule deletes its matches and skips.
//!
//! # Lock dates
//!
//! Skipping, unskipping, matching or unmatching an occurrence dated on or
//! before its account's `reconciled_through` is refused, and so is deleting
//! a schedule with a match or skip there (end it instead, or unlock). An
//! import's rows are never in a locked period, so its matches never are.

use chrono::NaiveDate;

use crate::category::CategoryKey;
use crate::db::{Database, Reader};
use crate::id::{AccountId, BatchId, ScheduleId, TransactionId};
use crate::money::Money;
use crate::recurring::{self, Cadence, SeriesKey};

#[allow(unused_imports)] // Used by the docs above.
use crate::recurring::normalize_merchant;

/// A schedule's status (`schedule.status`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScheduleStatus {
    Active,
    Paused,
    /// Final.
    Ended,
}

impl ScheduleStatus {
    pub const ALL: [ScheduleStatus; 3] = [
        ScheduleStatus::Active,
        ScheduleStatus::Paused,
        ScheduleStatus::Ended,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            ScheduleStatus::Active => "active",
            ScheduleStatus::Paused => "paused",
            ScheduleStatus::Ended => "ended",
        }
    }

    pub fn from_key(key: &str) -> Option<ScheduleStatus> {
        Self::ALL.into_iter().find(|status| status.as_str() == key)
    }
}

impl std::fmt::Display for ScheduleStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where an occurrence stands. Derived, never stored (see the module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OccurrenceState {
    /// Not due yet, or due but not imported yet.
    Upcoming,
    /// Fulfilled within the tolerance.
    Matched,
    /// Fulfilled after its date plus the tolerance.
    Late,
    /// Due, past the tolerance, covered by imports, and not fulfilled.
    Missed,
    /// The user skipped it.
    Skipped,
}

impl OccurrenceState {
    pub const fn as_str(self) -> &'static str {
        match self {
            OccurrenceState::Upcoming => "upcoming",
            OccurrenceState::Matched => "matched",
            OccurrenceState::Late => "late",
            OccurrenceState::Missed => "missed",
            OccurrenceState::Skipped => "skipped",
        }
    }
}

impl std::fmt::Display for OccurrenceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How far a payment may be from a schedule's amount and date and still
/// fulfil it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tolerance {
    /// Minor units either side of the amount; 0 or more.
    pub amount_minor: i64,
    /// Days either side of the date; at most the cadence's
    /// [`Cadence::max_tolerance_days`].
    pub days: u32,
}

impl Tolerance {
    /// A new schedule's tolerance for `series`: its cadence's
    /// [`Cadence::default_tolerance_days`], and an amount tolerance of the
    /// series' spread or a tenth of its typical amount, whichever is larger.
    pub fn default_for(series: &recurring::RecurringSeries) -> Tolerance {
        let tenth = series.typical.minor().unsigned_abs() / 10;
        let tenth = i64::try_from(tenth).unwrap_or(i64::MAX);
        Tolerance {
            amount_minor: series.amount_spread_minor.max(tenth),
            days: series.cadence.default_tolerance_days(),
        }
    }
}

/// How a match was made (`schedule_match.matched_by`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MatchedBy {
    /// By an import, or when the schedule was confirmed.
    Auto,
    /// By the user.
    User,
}

impl MatchedBy {
    pub const fn as_str(self) -> &'static str {
        match self {
            MatchedBy::Auto => "auto",
            MatchedBy::User => "user",
        }
    }
}

/// A stored schedule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schedule {
    pub id: ScheduleId,
    pub account: AccountId,
    pub merchant_display: String,
    pub category: Option<CategoryKey>,
    pub cadence: Cadence,
    /// Occurrence 0, the anchor.
    pub start_date: NaiveDate,
    /// Signed, in the account's currency.
    pub amount: Money,
    pub tolerance: Tolerance,
    pub status: ScheduleStatus,
    /// The detected series it was confirmed from.
    pub series_key: Option<SeriesKey>,
}

/// A schedule to create ([`create`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewSchedule {
    pub account: AccountId,
    pub merchant_display: String,
    pub category: Option<CategoryKey>,
    pub cadence: Cadence,
    pub start_date: NaiveDate,
    /// Non-zero, in the account's currency.
    pub amount: Money,
    pub tolerance: Tolerance,
    pub series_key: Option<SeriesKey>,
}

impl NewSchedule {
    /// A schedule from a detected series, starting at its anchor (so its
    /// past payments are matched when it is created) with the default
    /// [`Tolerance`].
    pub fn from_series(series: &recurring::RecurringSeries) -> NewSchedule {
        NewSchedule {
            account: series.account,
            merchant_display: series.merchant_display.clone(),
            category: series.category.clone(),
            cadence: series.cadence,
            start_date: series.anchor,
            amount: series.typical,
            tolerance: Tolerance::default_for(series),
            series_key: Some(series.key.clone()),
        }
    }
}

/// The transaction that fulfilled an occurrence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchedPayment {
    pub transaction: TransactionId,
    pub date: NaiveDate,
    pub amount: Money,
    pub matched_by: MatchedBy,
}

/// One occurrence of a schedule, with its derived state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Occurrence {
    pub date: NaiveDate,
    pub state: OccurrenceState,
    /// Set exactly when the state is matched or late.
    pub payment: Option<MatchedPayment>,
}

/// A schedule with its occurrences as of a date ([`view`], [`overview`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduleView {
    pub schedule: Schedule,
    /// Oldest first: for an active schedule every occurrence from the start
    /// through the first upcoming one on or after today; for a paused or
    /// ended one, only those matched or skipped.
    pub occurrences: Vec<Occurrence>,
    /// The first upcoming occurrence on or after today (none when paused or
    /// ended).
    pub next: Option<NaiveDate>,
    /// The account's latest imported transaction's date: imports cover the
    /// account through it.
    pub covered_through: Option<NaiveDate>,
}

/// What [`delete`] removed besides the schedule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deleted {
    pub matches: usize,
    pub skips: usize,
}

/// Why a schedule change failed. Messages never contain transaction text.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("there is no schedule {0}")]
    NotFound(ScheduleId),

    #[error("there is no account {0}")]
    AccountNotFound(AccountId),

    #[error("there is no transaction {0}")]
    TransactionNotFound(TransactionId),

    #[error("there is no category {0}")]
    CategoryNotFound(CategoryKey),

    #[error("a schedule's amount can't be zero")]
    ZeroAmount,

    #[error("the amount is in {amount}, but the account is in {account}")]
    CurrencyMismatch {
        account: crate::money::Currency,
        amount: crate::money::Currency,
    },

    #[error("the merchant name can't be empty")]
    EmptyMerchant,

    #[error("a {cadence} schedule's date tolerance is at most {max} days, not {days}")]
    ToleranceTooWide {
        cadence: Cadence,
        days: u32,
        max: u32,
    },

    #[error("the amount tolerance can't be negative")]
    NegativeTolerance,

    #[error("{date} isn't an occurrence of schedule {schedule}")]
    NotAnOccurrence {
        schedule: ScheduleId,
        date: NaiveDate,
    },

    #[error("the occurrence on {0} is already matched")]
    AlreadyMatched(NaiveDate),

    #[error("the occurrence on {0} is already skipped")]
    AlreadySkipped(NaiveDate),

    #[error("the occurrence on {0} isn't skipped")]
    NotSkipped(NaiveDate),

    #[error("the occurrence on {0} isn't matched")]
    NotMatched(NaiveDate),

    #[error("transaction {0} already fulfils a scheduled payment")]
    TransactionTaken(TransactionId),

    #[error("transaction {transaction} isn't on schedule {schedule}'s account")]
    WrongAccount {
        schedule: ScheduleId,
        transaction: TransactionId,
    },

    #[error("schedule {0} has ended")]
    Ended(ScheduleId),

    #[error("schedule {0} is already {1}")]
    AlreadyInStatus(ScheduleId, ScheduleStatus),

    #[error("{date} is on or before {through}, which is locked; unlock the account first")]
    Locked { date: NaiveDate, through: NaiveDate },

    #[error("recurring series: {0}")]
    Recurring(#[from] recurring::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

// Stubs: the schedules work fills these in.

/// Creates a schedule and, in the same DB transaction, matches the
/// account's existing transactions from its start (oldest first, as an
/// import would; never rows on or before the account's lock date).
pub fn create(_db: &mut Database, _schedule: &NewSchedule) -> Result<ScheduleId, Error> {
    todo!("Block D schedules")
}

/// Confirms the detected series `key` ([`recurring::find`]) as a schedule:
/// [`NewSchedule::from_series`], with `tolerance` in place of the default
/// when given, then [`create`].
pub fn confirm(
    _db: &mut Database,
    _key: &SeriesKey,
    _tolerance: Option<Tolerance>,
) -> Result<ScheduleId, Error> {
    todo!("Block D schedules")
}

pub fn get(_db: &Database, _id: ScheduleId) -> Result<Schedule, Error> {
    todo!("Block D schedules")
}

/// Every schedule, by ID.
pub fn list(_db: &Database) -> Result<Vec<Schedule>, Error> {
    todo!("Block D schedules")
}

/// One schedule with its occurrences as of `today`.
pub fn view(_db: &Database, _id: ScheduleId, _today: NaiveDate) -> Result<ScheduleView, Error> {
    todo!("Block D schedules")
}

/// Every schedule with its occurrences as of `today`, by ID.
pub fn overview(_db: &Database, _today: NaiveDate) -> Result<Vec<ScheduleView>, Error> {
    todo!("Block D schedules")
}

/// [`overview`] on a read-only connection (the MCP server).
pub fn overview_with_reader(
    _reader: &Reader,
    _today: NaiveDate,
) -> Result<Vec<ScheduleView>, Error> {
    todo!("Block D schedules")
}

/// Changes an active schedule's amount and tolerance (the schedule
/// inspector's fields). Existing matches stay.
pub fn update(
    _db: &mut Database,
    _id: ScheduleId,
    _amount: Money,
    _tolerance: Tolerance,
) -> Result<(), Error> {
    todo!("Block D schedules")
}

/// Pauses an active schedule.
pub fn pause(_db: &mut Database, _id: ScheduleId) -> Result<(), Error> {
    todo!("Block D schedules")
}

/// Resumes a paused schedule.
pub fn resume(_db: &mut Database, _id: ScheduleId) -> Result<(), Error> {
    todo!("Block D schedules")
}

/// Ends a schedule, for good.
pub fn end(_db: &mut Database, _id: ScheduleId) -> Result<(), Error> {
    todo!("Block D schedules")
}

/// Deletes a schedule with its matches and skips.
pub fn delete(_db: &mut Database, _id: ScheduleId) -> Result<Deleted, Error> {
    todo!("Block D schedules")
}

/// Skips the occurrence on `date`.
pub fn skip(_db: &mut Database, _id: ScheduleId, _date: NaiveDate) -> Result<(), Error> {
    todo!("Block D schedules")
}

/// Undoes [`skip`].
pub fn unskip(_db: &mut Database, _id: ScheduleId, _date: NaiveDate) -> Result<(), Error> {
    todo!("Block D schedules")
}

/// The user matches the occurrence on `date` to `transaction` ("Match a
/// transaction…"): any transaction on the account not already matched,
/// whatever its amount or date.
pub fn match_occurrence(
    _db: &mut Database,
    _id: ScheduleId,
    _date: NaiveDate,
    _transaction: TransactionId,
) -> Result<(), Error> {
    todo!("Block D schedules")
}

/// Removes the match on the occurrence on `date`, made by the user or
/// automatically.
pub fn unmatch(_db: &mut Database, _id: ScheduleId, _date: NaiveDate) -> Result<(), Error> {
    todo!("Block D schedules")
}

/// The import commit's hook: matches batch `batch`'s rows for every active
/// schedule on their account. Runs in the commit's DB transaction. Returns
/// how many occurrences it matched.
pub(crate) fn match_batch(_conn: &rusqlite::Connection, _batch: BatchId) -> Result<usize, Error> {
    // Stub: matches nothing until the schedules work lands.
    Ok(0)
}

/// The undo hook: deletes the matches that name batch `batch`'s
/// transactions. Returns how many. Only database errors, so callers need
/// no new error variant.
pub(crate) fn clear_batch(
    _conn: &rusqlite::Connection,
    _batch: BatchId,
) -> rusqlite::Result<usize> {
    // Stub: nothing to clear until the schedules work lands.
    Ok(0)
}

/// The manual-delete hook: deletes the match naming `transaction`, if any.
/// Only database errors, like [`clear_batch`].
pub(crate) fn clear(
    _conn: &rusqlite::Connection,
    _transaction: TransactionId,
) -> rusqlite::Result<usize> {
    // Stub: nothing to clear until the schedules work lands.
    Ok(0)
}

/// Today's date in the local time zone, from SQLite's clock (chrono's
/// clock feature isn't enabled). Callers pass it to the queries above.
pub fn local_today() -> Result<NaiveDate, Error> {
    let conn = rusqlite::Connection::open_in_memory()?;
    let today: String = conn.query_row("SELECT date('now', 'localtime')", [], |row| row.get(0))?;
    NaiveDate::parse_from_str(&today, "%Y-%m-%d").map_err(|_| {
        Error::Database(rusqlite::Error::InvalidColumnType(
            0,
            "date".into(),
            rusqlite::types::Type::Text,
        ))
    })
}
