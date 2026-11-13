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
//! "Imports cover" counts imported and mirrored rows (`batch_id IS NOT
//! NULL`), excluded ones too; manual rows say nothing about what the bank's
//! exports reach ([`ScheduleView::covered_through`]). A view of an active
//! schedule lists every occurrence from the start through the first
//! upcoming one on or after today, so a skip or match stored for a later
//! occurrence (a payment made early, a skip planned ahead) shows once that
//! occurrence is reached. At most [`MAX_LISTED_OCCURRENCES`] are listed,
//! so a `today` far in the future can't loop without end.
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
//! (`match_batch`); undoing the import deletes those matches
//! (`clear_batch`).
//!
//! - **The amount** compared is the transaction's whole `amount_minor`,
//!   not the user's share or a split line.
//! - **Two schedules, one row**: schedules match in ID order and a matched
//!   row is never matched again, so the lower schedule ID wins.
//! - **When matching runs**: on each import, for that batch's rows; and
//!   when a schedule is created ([`create`], [`confirm`]), over every row of
//!   the account after its lock date. [`update`] and [`resume`] match
//!   nothing: from then on imports do, and earlier rows are matched by hand.
//!   So a row the user unmatched is never matched again automatically.
//! - **The user's match** ([`match_occurrence`]) takes any transaction on
//!   the account that fulfils no occurrence yet: excluded, in a transfer,
//!   of any amount, merchant or date.
//!
//! # Status
//!
//! An **active** schedule matches and flags. A **paused** one does neither:
//! it lists only occurrences already matched or skipped, and resuming it
//! derives the rest again (occurrences missed while paused show as missed
//! and can be skipped). **Ended** is final: like paused, and it never
//! resumes. Deleting a schedule deletes its matches and skips.
//!
//! A paused schedule's occurrences can still be skipped, matched and
//! changed back, and its amount and tolerance changed ([`update`]). An
//! ended one takes no new decision (skip, match, update: [`Error::Ended`]),
//! but its skips and matches can be undone ([`unskip`], [`unmatch`]) to
//! correct its history, and it can be deleted.
//!
//! # Lock dates
//!
//! Skipping, unskipping, matching or unmatching an occurrence dated on or
//! before its account's `reconciled_through` is refused, and so is deleting
//! a schedule with a match or skip there (end it instead, or unlock).
//!
//! The lock is on the occurrence's date. Automatic matching only takes rows
//! dated after the lock (an import's rows always are), but a row paid late
//! within its window can fulfil an occurrence on or before the lock date:
//! that match is then locked like any other. The user may match an open
//! occurrence after the lock to a row on or before it (a payment made
//! early): the row itself doesn't change.

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use chrono::{Days, NaiveDate};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::account::{self, date_key};
use crate::category::CategoryKey;
use crate::db::{Database, Reader};
use crate::id::{AccountId, BatchId, ScheduleId, TransactionId};
use crate::money::{Currency, Money};
use crate::recurring::{self, Cadence, SeriesKey, normalize_merchant};

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

    #[error("account {0} holds valuations, not transactions, so it can't have schedules")]
    HoldsValuations(AccountId),

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

/// Creates a schedule and, in the same DB transaction, matches the
/// account's existing transactions from its start (oldest first, as an
/// import would; never rows on or before the account's lock date).
pub fn create(db: &mut Database, schedule: &NewSchedule) -> Result<ScheduleId, Error> {
    let tx = db.conn_mut().transaction()?;
    let (id, matched) = create_in(&tx, schedule)?;
    tx.commit()?;
    tracing::info!(
        schedule = %id,
        account = %schedule.account,
        matches = matched,
        "created a schedule"
    );
    Ok(id)
}

/// Confirms the detected series `key` ([`recurring::find`]) as a schedule:
/// [`NewSchedule::from_series`], with `tolerance` in place of the default
/// when given, then [`create`].
pub fn confirm(
    db: &mut Database,
    key: &SeriesKey,
    tolerance: Option<Tolerance>,
) -> Result<ScheduleId, Error> {
    let tx = db.conn_mut().transaction()?;
    let series = recurring::find_in(&tx)?
        .into_iter()
        .find(|series| series.key == *key)
        .ok_or_else(|| recurring::Error::SeriesNotFound(key.clone()))?;
    let mut schedule = NewSchedule::from_series(&series);
    if let Some(tolerance) = tolerance {
        schedule.tolerance = tolerance;
    }
    let (id, matched) = create_in(&tx, &schedule)?;
    tx.commit()?;
    tracing::info!(
        schedule = %id,
        account = %schedule.account,
        matches = matched,
        "confirmed a recurring series as a schedule"
    );
    Ok(id)
}

pub fn get(db: &Database, id: ScheduleId) -> Result<Schedule, Error> {
    load(db.conn(), id)
}

/// Every schedule, by ID.
pub fn list(db: &Database) -> Result<Vec<Schedule>, Error> {
    list_in(db.conn())
}

/// One schedule with its occurrences as of `today`.
pub fn view(db: &Database, id: ScheduleId, today: NaiveDate) -> Result<ScheduleView, Error> {
    let conn = db.conn();
    let schedule = load(conn, id)?;
    let covered = covered_through(conn, schedule.account)?;
    view_of(conn, schedule, covered, today)
}

/// Every schedule with its occurrences as of `today`, by ID.
pub fn overview(db: &Database, today: NaiveDate) -> Result<Vec<ScheduleView>, Error> {
    overview_in(db.conn(), today)
}

/// [`overview`] on a read-only connection (the MCP server).
pub fn overview_with_reader(reader: &Reader, today: NaiveDate) -> Result<Vec<ScheduleView>, Error> {
    overview_in(reader.conn(), today)
}

/// Changes an active schedule's amount and tolerance (the schedule
/// inspector's fields). Existing matches stay.
///
/// A paused schedule can be changed too (before resuming it); an ended one
/// can't. Nothing is matched again: transactions that fit the new amount
/// match from the next import on, and earlier ones are matched by hand.
pub fn update(
    db: &mut Database,
    id: ScheduleId,
    amount: Money,
    tolerance: Tolerance,
) -> Result<(), Error> {
    let tx = db.conn_mut().transaction()?;
    let schedule = load(&tx, id)?;
    if schedule.status == ScheduleStatus::Ended {
        return Err(Error::Ended(id));
    }
    let account =
        account::find(&tx, schedule.account)?.ok_or(Error::AccountNotFound(schedule.account))?;
    validate_amount(account.currency, amount)?;
    validate_tolerance(schedule.cadence, tolerance)?;
    tx.execute(
        "UPDATE schedule SET amount_minor = ?1, tolerance_minor = ?2, tolerance_days = ?3 \
         WHERE id = ?4",
        params![amount.minor(), tolerance.amount_minor, tolerance.days, id],
    )?;
    tx.commit()?;
    tracing::info!(schedule = %id, "updated a schedule");
    Ok(())
}

/// Pauses an active schedule.
pub fn pause(db: &mut Database, id: ScheduleId) -> Result<(), Error> {
    set_status(db, id, ScheduleStatus::Paused)
}

/// Resumes a paused schedule.
///
/// Transactions imported while it was paused aren't matched: its
/// occurrences from then show as missed, to match by hand or skip.
pub fn resume(db: &mut Database, id: ScheduleId) -> Result<(), Error> {
    set_status(db, id, ScheduleStatus::Active)
}

/// Ends a schedule, for good.
pub fn end(db: &mut Database, id: ScheduleId) -> Result<(), Error> {
    set_status(db, id, ScheduleStatus::Ended)
}

/// Deletes a schedule with its matches and skips.
pub fn delete(db: &mut Database, id: ScheduleId) -> Result<Deleted, Error> {
    let tx = db.conn_mut().transaction()?;
    let schedule = load(&tx, id)?;
    if let Some(through) = lock_date(&tx, schedule.account)? {
        let earliest: Option<String> = tx.query_row(
            "SELECT min(occurrence_date) FROM ( \
                 SELECT occurrence_date FROM schedule_match WHERE schedule_id = ?1 \
                 UNION ALL \
                 SELECT occurrence_date FROM schedule_skip WHERE schedule_id = ?1)",
            [id],
            |row| row.get(0),
        )?;
        if let Some(earliest) = earliest.as_deref().and_then(parse_date)
            && earliest <= through
        {
            return Err(Error::Locked {
                date: earliest,
                through,
            });
        }
    }
    let matches = tx.execute("DELETE FROM schedule_match WHERE schedule_id = ?1", [id])?;
    let skips = tx.execute("DELETE FROM schedule_skip WHERE schedule_id = ?1", [id])?;
    tx.execute("DELETE FROM schedule WHERE id = ?1", [id])?;
    tx.commit()?;
    tracing::info!(schedule = %id, matches, skips, "deleted a schedule");
    Ok(Deleted { matches, skips })
}

/// Skips the occurrence on `date`.
pub fn skip(db: &mut Database, id: ScheduleId, date: NaiveDate) -> Result<(), Error> {
    let tx = db.conn_mut().transaction()?;
    let schedule = load(&tx, id)?;
    if schedule.status == ScheduleStatus::Ended {
        return Err(Error::Ended(id));
    }
    check_occurrence(&schedule, date)?;
    check_lock(&tx, schedule.account, date)?;
    match decision(&tx, id, date)? {
        Some(Decision::Skipped) => return Err(Error::AlreadySkipped(date)),
        Some(Decision::Matched) => return Err(Error::AlreadyMatched(date)),
        None => {}
    }
    tx.execute(
        "INSERT INTO schedule_skip (schedule_id, occurrence_date) VALUES (?1, ?2)",
        params![id, date_key(date)],
    )?;
    tx.commit()?;
    tracing::info!(schedule = %id, "skipped an occurrence");
    Ok(())
}

/// Undoes [`skip`].
pub fn unskip(db: &mut Database, id: ScheduleId, date: NaiveDate) -> Result<(), Error> {
    let tx = db.conn_mut().transaction()?;
    let schedule = load(&tx, id)?;
    check_occurrence(&schedule, date)?;
    check_lock(&tx, schedule.account, date)?;
    let removed = tx.execute(
        "DELETE FROM schedule_skip WHERE schedule_id = ?1 AND occurrence_date = ?2",
        params![id, date_key(date)],
    )?;
    if removed == 0 {
        return Err(Error::NotSkipped(date));
    }
    tx.commit()?;
    tracing::info!(schedule = %id, "unskipped an occurrence");
    Ok(())
}

/// The user matches the occurrence on `date` to `transaction` ("Match a
/// transaction…"): any transaction on the account not already matched,
/// whatever its amount or date.
pub fn match_occurrence(
    db: &mut Database,
    id: ScheduleId,
    date: NaiveDate,
    transaction: TransactionId,
) -> Result<(), Error> {
    let tx = db.conn_mut().transaction()?;
    let schedule = load(&tx, id)?;
    if schedule.status == ScheduleStatus::Ended {
        return Err(Error::Ended(id));
    }
    check_occurrence(&schedule, date)?;
    check_lock(&tx, schedule.account, date)?;
    let on_account: AccountId = tx
        .query_row(
            "SELECT account_id FROM \"transaction\" WHERE id = ?1",
            [transaction],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(Error::TransactionNotFound(transaction))?;
    if on_account != schedule.account {
        return Err(Error::WrongAccount {
            schedule: id,
            transaction,
        });
    }
    let taken = tx
        .query_row(
            "SELECT 1 FROM schedule_match WHERE transaction_id = ?1",
            [transaction],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if taken {
        return Err(Error::TransactionTaken(transaction));
    }
    match decision(&tx, id, date)? {
        Some(Decision::Skipped) => return Err(Error::AlreadySkipped(date)),
        Some(Decision::Matched) => return Err(Error::AlreadyMatched(date)),
        None => {}
    }
    insert_match(&tx, id, date, transaction, MatchedBy::User)?;
    tx.commit()?;
    tracing::info!(schedule = %id, transaction = %transaction, "matched an occurrence");
    Ok(())
}

/// Removes the match on the occurrence on `date`, made by the user or
/// automatically.
pub fn unmatch(db: &mut Database, id: ScheduleId, date: NaiveDate) -> Result<(), Error> {
    let tx = db.conn_mut().transaction()?;
    let schedule = load(&tx, id)?;
    check_occurrence(&schedule, date)?;
    check_lock(&tx, schedule.account, date)?;
    let removed = tx.execute(
        "DELETE FROM schedule_match WHERE schedule_id = ?1 AND occurrence_date = ?2",
        params![id, date_key(date)],
    )?;
    if removed == 0 {
        return Err(Error::NotMatched(date));
    }
    tx.commit()?;
    tracing::info!(schedule = %id, "unmatched an occurrence");
    Ok(())
}

/// The import commit's hook: matches batch `batch`'s rows for every active
/// schedule on their account. Runs in the commit's DB transaction. Returns
/// how many occurrences it matched.
pub(crate) fn match_batch(conn: &Connection, batch: BatchId) -> Result<usize, Error> {
    let accounts: Vec<AccountId> = {
        let mut statement = conn.prepare(
            "SELECT DISTINCT account_id FROM \"transaction\" WHERE batch_id = ?1 \
             ORDER BY account_id",
        )?;
        statement
            .query_map([batch], |row| row.get(0))?
            .collect::<Result<_, _>>()?
    };
    let mut matched = 0;
    for account in accounts {
        let lock = lock_date(conn, account)?;
        let schedules = {
            let mut statement = conn.prepare(&format!(
                "SELECT {COLUMNS} FROM {FROM} \
                 WHERE s.account_id = ?1 AND s.status = 'active' ORDER BY s.id"
            ))?;
            statement
                .query_map([account], from_row)?
                .collect::<Result<Vec<_>, _>>()?
        };
        for schedule in &schedules {
            let rows = candidates(conn, account, Some(batch), lock)?;
            matched += match_rows(conn, schedule, &rows)?;
        }
    }
    tracing::debug!(batch = %batch, matches = matched, "matched scheduled payments");
    Ok(matched)
}

/// The undo hook: deletes the matches that name batch `batch`'s
/// transactions. Returns how many. Only database errors, so callers need
/// no new error variant.
pub(crate) fn clear_batch(conn: &Connection, batch: BatchId) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM schedule_match WHERE transaction_id IN \
         (SELECT id FROM \"transaction\" WHERE batch_id = ?1)",
        [batch],
    )
}

/// The manual-delete hook: deletes the match naming `transaction`, if any.
/// Only database errors, like [`clear_batch`].
pub(crate) fn clear(conn: &Connection, transaction: TransactionId) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM schedule_match WHERE transaction_id = ?1",
        [transaction],
    )
}

// Storage -------------------------------------------------------------------

const COLUMNS: &str = "s.id, s.account_id, s.merchant_display, c.key AS category_key, \
                       s.cadence, s.start_date, s.amount_minor, s.currency, \
                       s.tolerance_minor, s.tolerance_days, s.status, s.series_key";

const FROM: &str = "schedule s LEFT JOIN category c ON c.id = s.category_id";

fn from_row(row: &Row<'_>) -> rusqlite::Result<Schedule> {
    let cadence: String = row.get("cadence")?;
    let cadence = Cadence::from_key(&cadence).ok_or_else(|| account::invalid(row, "cadence"))?;
    let status: String = row.get("status")?;
    let status =
        ScheduleStatus::from_key(&status).ok_or_else(|| account::invalid(row, "status"))?;
    let currency: Currency = row.get("currency")?;
    let days: i64 = row.get("tolerance_days")?;
    let days = u32::try_from(days).map_err(|_| account::invalid(row, "tolerance_days"))?;
    let series_key: Option<String> = row.get("series_key")?;
    let series_key = series_key
        .map(|key| {
            key.parse::<SeriesKey>()
                .map_err(|_| account::invalid(row, "series_key"))
        })
        .transpose()?;
    Ok(Schedule {
        id: row.get("id")?,
        account: row.get("account_id")?,
        merchant_display: row.get("merchant_display")?,
        category: row.get("category_key")?,
        cadence,
        start_date: account::date(row, "start_date")?,
        amount: Money::new(row.get("amount_minor")?, currency),
        tolerance: Tolerance {
            amount_minor: row.get("tolerance_minor")?,
            days,
        },
        status,
        series_key,
    })
}

fn load(conn: &Connection, id: ScheduleId) -> Result<Schedule, Error> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM {FROM} WHERE s.id = ?1"),
        [id],
        from_row,
    )
    .optional()?
    .ok_or(Error::NotFound(id))
}

fn list_in(conn: &Connection) -> Result<Vec<Schedule>, Error> {
    let mut statement = conn.prepare(&format!("SELECT {COLUMNS} FROM {FROM} ORDER BY s.id"))?;
    let schedules = statement
        .query_map([], from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(schedules)
}

/// Validates `schedule`, stores it and matches the account's transactions
/// from its start. Returns its ID and how many occurrences it matched.
fn create_in(conn: &Connection, schedule: &NewSchedule) -> Result<(ScheduleId, usize), Error> {
    let account =
        account::find(conn, schedule.account)?.ok_or(Error::AccountNotFound(schedule.account))?;
    if !account.kind.holds_transactions() {
        return Err(Error::HoldsValuations(schedule.account));
    }
    validate_amount(account.currency, schedule.amount)?;
    let merchant = schedule.merchant_display.trim();
    if merchant.is_empty() {
        return Err(Error::EmptyMerchant);
    }
    validate_tolerance(schedule.cadence, schedule.tolerance)?;
    let category: Option<i64> = match &schedule.category {
        Some(key) => Some(
            conn.query_row("SELECT id FROM category WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?
            .ok_or_else(|| Error::CategoryNotFound(key.clone()))?,
        ),
        None => None,
    };
    conn.execute(
        "INSERT INTO schedule (account_id, merchant_display, category_id, cadence, start_date, \
         amount_minor, currency, tolerance_minor, tolerance_days, series_key) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            schedule.account,
            merchant,
            category,
            schedule.cadence.as_str(),
            date_key(schedule.start_date),
            schedule.amount.minor(),
            schedule.amount.currency(),
            schedule.tolerance.amount_minor,
            schedule.tolerance.days,
            schedule.series_key.as_ref().map(SeriesKey::as_str),
        ],
    )?;
    let id = ScheduleId::new(conn.last_insert_rowid());
    let stored = load(conn, id)?;
    let rows = candidates(conn, account.id, None, account.reconciled_through)?;
    let matched = match_rows(conn, &stored, &rows)?;
    Ok((id, matched))
}

fn validate_amount(currency: Currency, amount: Money) -> Result<(), Error> {
    if amount.is_zero() {
        return Err(Error::ZeroAmount);
    }
    if amount.currency() != currency {
        return Err(Error::CurrencyMismatch {
            account: currency,
            amount: amount.currency(),
        });
    }
    Ok(())
}

fn validate_tolerance(cadence: Cadence, tolerance: Tolerance) -> Result<(), Error> {
    let max = cadence.max_tolerance_days();
    if tolerance.days > max {
        return Err(Error::ToleranceTooWide {
            cadence,
            days: tolerance.days,
            max,
        });
    }
    if tolerance.amount_minor < 0 {
        return Err(Error::NegativeTolerance);
    }
    Ok(())
}

fn set_status(db: &mut Database, id: ScheduleId, to: ScheduleStatus) -> Result<(), Error> {
    let tx = db.conn_mut().transaction()?;
    let schedule = load(&tx, id)?;
    // With two live statuses, every change between different ones is
    // allowed (pause from active, resume from paused, end from either), and
    // nothing leaves ended.
    if schedule.status == to {
        return Err(Error::AlreadyInStatus(id, to));
    }
    if schedule.status == ScheduleStatus::Ended {
        return Err(Error::Ended(id));
    }
    tx.execute(
        "UPDATE schedule SET status = ?1 WHERE id = ?2",
        params![to.as_str(), id],
    )?;
    tx.commit()?;
    tracing::info!(schedule = %id, status = to.as_str(), "changed a schedule's status");
    Ok(())
}

/// The account's lock date (`reconciled_through`).
fn lock_date(conn: &Connection, account: AccountId) -> Result<Option<NaiveDate>, Error> {
    Ok(account::find(conn, account)?
        .ok_or(Error::AccountNotFound(account))?
        .reconciled_through)
}

/// Refuses an occurrence dated on or before the account's lock date.
fn check_lock(conn: &Connection, account: AccountId, date: NaiveDate) -> Result<(), Error> {
    match lock_date(conn, account)? {
        Some(through) if date <= through => Err(Error::Locked { date, through }),
        _ => Ok(()),
    }
}

/// What is stored about one occurrence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Decision {
    Matched,
    Skipped,
}

fn decision(conn: &Connection, id: ScheduleId, date: NaiveDate) -> Result<Option<Decision>, Error> {
    let key = date_key(date);
    let exists = |table: &str| -> rusqlite::Result<bool> {
        Ok(conn
            .query_row(
                &format!("SELECT 1 FROM {table} WHERE schedule_id = ?1 AND occurrence_date = ?2"),
                params![id, key],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    };
    Ok(if exists("schedule_match")? {
        Some(Decision::Matched)
    } else if exists("schedule_skip")? {
        Some(Decision::Skipped)
    } else {
        None
    })
}

fn insert_match(
    conn: &Connection,
    id: ScheduleId,
    date: NaiveDate,
    transaction: TransactionId,
    by: MatchedBy,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO schedule_match (schedule_id, occurrence_date, transaction_id, matched_by) \
         VALUES (?1, ?2, ?3, ?4)",
        params![id, date_key(date), transaction, by.as_str()],
    )?;
    Ok(())
}

fn parse_date(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()
}

// Occurrence arithmetic -----------------------------------------------------

/// The largest `k` with `nth(k) - shift_days <= date`; `None` when even
/// occurrence 0 shifted is after `date`. `nth` is increasing in `k`, so
/// this starts from an estimate and steps.
fn last_index_at_or_before(
    cadence: Cadence,
    start: NaiveDate,
    shift_days: u32,
    date: NaiveDate,
) -> Option<u32> {
    // A shifted occurrence, or `None` when it is past chrono's last date.
    let point = |k: u32| {
        cadence.nth(start, k).map(|nth| {
            nth.checked_sub_days(Days::new(u64::from(shift_days)))
                .unwrap_or(NaiveDate::MIN)
        })
    };
    let at_or_before = |k: u32| point(k).is_some_and(|point| point <= date);
    if !at_or_before(0) {
        return None;
    }
    let elapsed = (date - start).num_days().max(0);
    let estimate = elapsed / i64::from(cadence.typical_days());
    let mut k = u32::try_from(estimate).unwrap_or(u32::MAX);
    while k > 0 && !at_or_before(k) {
        k -= 1;
    }
    while k < u32::MAX && at_or_before(k + 1) {
        k += 1;
    }
    Some(k)
}

/// The occurrence whose matching window holds `date`: occurrence k's window
/// is `[nth(k) - tolerance_days, nth(k + 1) - tolerance_days)`.
fn window_occurrence(schedule: &Schedule, date: NaiveDate) -> Option<NaiveDate> {
    let k = last_index_at_or_before(
        schedule.cadence,
        schedule.start_date,
        schedule.tolerance.days,
        date,
    )?;
    schedule.cadence.nth(schedule.start_date, k)
}

/// Refuses a date that isn't one of the schedule's occurrences.
fn check_occurrence(schedule: &Schedule, date: NaiveDate) -> Result<(), Error> {
    let found = last_index_at_or_before(schedule.cadence, schedule.start_date, 0, date)
        .and_then(|k| schedule.cadence.nth(schedule.start_date, k));
    if found == Some(date) {
        Ok(())
    } else {
        Err(Error::NotAnOccurrence {
            schedule: schedule.id,
            date,
        })
    }
}

// Matching ------------------------------------------------------------------

/// A transaction that may fulfil an occurrence: not excluded, not in a
/// suggested or confirmed transfer, not already matched.
struct Candidate {
    id: TransactionId,
    date: NaiveDate,
    amount_minor: i64,
    currency: Currency,
    /// `coalesce(merchant_display, merchant)`, normalised.
    merchant: Option<String>,
}

/// The account's candidates, oldest first then by ID: of batch `batch`
/// only, when given, and after `lock` (the account's lock date), when set.
fn candidates(
    conn: &Connection,
    account: AccountId,
    batch: Option<BatchId>,
    lock: Option<NaiveDate>,
) -> rusqlite::Result<Vec<Candidate>> {
    let mut statement = conn.prepare(
        "SELECT t.id, t.date, t.amount_minor, t.currency, \
                coalesce(t.merchant_display, t.merchant) AS merchant \
         FROM \"transaction\" t \
         WHERE t.account_id = ?1 \
           AND (?2 IS NULL OR t.batch_id = ?2) \
           AND (?3 IS NULL OR t.date > ?3) \
           AND t.excluded_reason IS NULL \
           AND NOT EXISTS (SELECT 1 FROM transfer_group g \
                           WHERE g.id = t.transfer_group_id AND g.status <> 'rejected') \
           AND NOT EXISTS (SELECT 1 FROM schedule_match m WHERE m.transaction_id = t.id) \
         ORDER BY t.date, t.id",
    )?;
    statement
        .query_map(params![account, batch, lock.map(date_key)], |row| {
            let merchant: Option<String> = row.get("merchant")?;
            Ok(Candidate {
                id: row.get("id")?,
                date: account::date(row, "date")?,
                amount_minor: row.get("amount_minor")?,
                currency: row.get("currency")?,
                merchant: merchant.as_deref().map(normalize_merchant),
            })
        })?
        .collect()
}

/// Matches `rows` (oldest first) to `schedule`'s open occurrences: each row
/// to the occurrence whose window holds it, when that one is open. Returns
/// how many it matched.
fn match_rows(conn: &Connection, schedule: &Schedule, rows: &[Candidate]) -> Result<usize, Error> {
    let merchant = normalize_merchant(&schedule.merchant_display);
    let amount = i128::from(schedule.amount.minor());
    let tolerance = i128::from(schedule.tolerance.amount_minor);
    let mut decided: HashSet<NaiveDate> = {
        let mut statement = conn.prepare(
            "SELECT occurrence_date FROM schedule_match WHERE schedule_id = ?1 \
             UNION SELECT occurrence_date FROM schedule_skip WHERE schedule_id = ?1",
        )?;
        statement
            .query_map([schedule.id], |row| account::date(row, "occurrence_date"))?
            .collect::<Result<_, _>>()?
    };
    let mut matched = 0;
    for row in rows {
        let fits = row.currency == schedule.amount.currency()
            && row.merchant.as_deref() == Some(merchant.as_str())
            && row.amount_minor.signum() == schedule.amount.minor().signum()
            && (i128::from(row.amount_minor) - amount).abs() <= tolerance;
        if !fits {
            continue;
        }
        let Some(occurrence) = window_occurrence(schedule, row.date) else {
            continue;
        };
        if !decided.insert(occurrence) {
            continue;
        }
        insert_match(conn, schedule.id, occurrence, row.id, MatchedBy::Auto)?;
        matched += 1;
    }
    Ok(matched)
}

// Deriving occurrences ------------------------------------------------------

/// The most occurrences a view lists. A weekly schedule reaches it after
/// about 190 years, so it only stops a `today` far in the future (or a
/// schedule started far in the past) from listing without end.
pub const MAX_LISTED_OCCURRENCES: usize = 10_000;

/// The account's latest imported (or mirrored) transaction's date. Manual
/// rows don't count: they say nothing about what the bank's exports cover.
fn covered_through(conn: &Connection, account: AccountId) -> Result<Option<NaiveDate>, Error> {
    let latest: Option<String> = conn.query_row(
        "SELECT max(date) FROM \"transaction\" WHERE account_id = ?1 AND batch_id IS NOT NULL",
        [account],
        |row| row.get(0),
    )?;
    Ok(latest.as_deref().and_then(parse_date))
}

fn overview_in(conn: &Connection, today: NaiveDate) -> Result<Vec<ScheduleView>, Error> {
    let mut covered: HashMap<AccountId, Option<NaiveDate>> = HashMap::new();
    let mut views = Vec::new();
    for schedule in list_in(conn)? {
        let through = match covered.get(&schedule.account) {
            Some(through) => *through,
            None => {
                let through = covered_through(conn, schedule.account)?;
                covered.insert(schedule.account, through);
                through
            }
        };
        views.push(view_of(conn, schedule, through, today)?);
    }
    Ok(views)
}

fn view_of(
    conn: &Connection,
    schedule: Schedule,
    covered_through: Option<NaiveDate>,
    today: NaiveDate,
) -> Result<ScheduleView, Error> {
    let matches: BTreeMap<NaiveDate, MatchedPayment> = {
        let mut statement = conn.prepare(
            "SELECT m.occurrence_date, m.transaction_id, m.matched_by, \
                    t.date, t.amount_minor, t.currency \
             FROM schedule_match m JOIN \"transaction\" t ON t.id = m.transaction_id \
             WHERE m.schedule_id = ?1",
        )?;
        statement
            .query_map([schedule.id], |row| {
                let by: String = row.get("matched_by")?;
                let matched_by = match by.as_str() {
                    "auto" => MatchedBy::Auto,
                    "user" => MatchedBy::User,
                    _ => return Err(account::invalid(row, "matched_by")),
                };
                let currency: Currency = row.get("currency")?;
                Ok((
                    account::date(row, "occurrence_date")?,
                    MatchedPayment {
                        transaction: row.get("transaction_id")?,
                        date: account::date(row, "date")?,
                        amount: Money::new(row.get("amount_minor")?, currency),
                        matched_by,
                    },
                ))
            })?
            .collect::<Result<_, _>>()?
    };
    let skips: BTreeSet<NaiveDate> = {
        let mut statement =
            conn.prepare("SELECT occurrence_date FROM schedule_skip WHERE schedule_id = ?1")?;
        statement
            .query_map([schedule.id], |row| account::date(row, "occurrence_date"))?
            .collect::<Result<_, _>>()?
    };
    let tolerance = Days::new(u64::from(schedule.tolerance.days));
    let derive = |date: NaiveDate| -> Occurrence {
        // Past chrono's last date nothing is due.
        let due_by = date.checked_add_days(tolerance).unwrap_or(NaiveDate::MAX);
        if skips.contains(&date) {
            return Occurrence {
                date,
                state: OccurrenceState::Skipped,
                payment: None,
            };
        }
        if let Some(payment) = matches.get(&date) {
            let state = if payment.date > due_by {
                OccurrenceState::Late
            } else {
                OccurrenceState::Matched
            };
            return Occurrence {
                date,
                state,
                payment: Some(payment.clone()),
            };
        }
        let missed = due_by < today && covered_through.is_some_and(|through| through >= due_by);
        Occurrence {
            date,
            state: if missed {
                OccurrenceState::Missed
            } else {
                OccurrenceState::Upcoming
            },
            payment: None,
        }
    };

    let mut occurrences = Vec::new();
    let mut next = None;
    if schedule.status == ScheduleStatus::Active {
        let mut k = 0;
        while occurrences.len() < MAX_LISTED_OCCURRENCES {
            let Some(date) = schedule.cadence.nth(schedule.start_date, k) else {
                break;
            };
            let occurrence = derive(date);
            let is_next = date >= today && occurrence.state == OccurrenceState::Upcoming;
            occurrences.push(occurrence);
            if is_next {
                next = Some(date);
                break;
            }
            k += 1;
        }
    } else {
        let decided: BTreeSet<NaiveDate> = matches.keys().chain(skips.iter()).copied().collect();
        occurrences.extend(
            decided
                .into_iter()
                .take(MAX_LISTED_OCCURRENCES)
                .map(&derive),
        );
    }
    Ok(ScheduleView {
        schedule,
        occurrences,
        next,
        covered_through,
    })
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
