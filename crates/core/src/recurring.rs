//! Recurring-payment and subscription detection (ROADMAP M2.6).
//!
//! Block D: detects recurring series in the ledger, deterministically, for
//! the `recurring_payments` tool and for confirming a series as a schedule
//! ([`crate::schedule`]).
//!
//! # The shared types
//!
//! - [`Cadence`]: how often a series repeats, and the date arithmetic every
//!   schedule uses ([`Cadence::nth`]).
//! - [`LedgerRow`]: one transaction as detection sees it, read from
//!   `ledger_line` (a split transaction's lines summed back to one row).
//! - [`RecurringSeries`]: what [`detect`] finds.
//! - [`SeriesKey`]: a series' stable key, which the CLI's `schedules
//!   confirm` and "Not recurring" (`recurring_dismissal`) use. It is a hash
//!   that names nothing: never a merchant, amount or account in clear.
//! - [`normalize_merchant`]: how a display merchant is compared, by detection
//!   and by schedule matching alike.
//!
//! M4.8 (the forecast) and M4.9 (yearly schedules seeding budgets) build on
//! these types.

use std::fmt;
use std::str::FromStr;

use chrono::{Days, Months, NaiveDate};

use crate::category::CategoryKey;
use crate::id::{AccountId, TransactionId};
use crate::money::{Currency, Money};

/// How often a series repeats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Cadence {
    /// Every 7 days.
    Weekly,
    /// Every 14 days.
    Fortnightly,
    /// Every 28 days (some salaries, some subscriptions).
    FourWeekly,
    /// The same day every month; a day the month doesn't have falls on its
    /// last day.
    Monthly,
    /// Every 3 months, like [`Cadence::Monthly`].
    Quarterly,
    /// Every 12 months, like [`Cadence::Monthly`] (29 February falls on 28
    /// February in other years).
    Yearly,
}

impl Cadence {
    /// Shortest first.
    pub const ALL: [Cadence; 6] = [
        Cadence::Weekly,
        Cadence::Fortnightly,
        Cadence::FourWeekly,
        Cadence::Monthly,
        Cadence::Quarterly,
        Cadence::Yearly,
    ];

    /// The key stored in `schedule.cadence`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Cadence::Weekly => "weekly",
            Cadence::Fortnightly => "fortnightly",
            Cadence::FourWeekly => "four_weekly",
            Cadence::Monthly => "monthly",
            Cadence::Quarterly => "quarterly",
            Cadence::Yearly => "yearly",
        }
    }

    pub fn from_key(key: &str) -> Option<Cadence> {
        Self::ALL
            .into_iter()
            .find(|cadence| cadence.as_str() == key)
    }

    /// The typical gap between occurrences, in days (for monthly and longer,
    /// an average: 30, 91, 365). For ordering and estimates only; dates
    /// come from [`Cadence::nth`].
    pub const fn typical_days(self) -> u32 {
        match self {
            Cadence::Weekly => 7,
            Cadence::Fortnightly => 14,
            Cadence::FourWeekly => 28,
            Cadence::Monthly => 30,
            Cadence::Quarterly => 91,
            Cadence::Yearly => 365,
        }
    }

    /// The date tolerance a new schedule starts with, in days either side.
    pub const fn default_tolerance_days(self) -> u32 {
        match self {
            Cadence::Weekly => 1,
            Cadence::Fortnightly | Cadence::FourWeekly => 2,
            Cadence::Monthly => 3,
            Cadence::Quarterly => 5,
            Cadence::Yearly => 7,
        }
    }

    /// The largest date tolerance a schedule may have: under half the
    /// shortest gap, so an occurrence's window never reaches the next one
    /// (13 for monthly: February's gap is 28 days).
    pub const fn max_tolerance_days(self) -> u32 {
        match self {
            Cadence::Weekly => 3,
            Cadence::Fortnightly => 6,
            Cadence::FourWeekly | Cadence::Monthly => 13,
            Cadence::Quarterly => 44,
            Cadence::Yearly => 181,
        }
    }

    /// How many occurrences detection needs before it calls a series
    /// recurring: 3, or 2 for yearly.
    pub const fn min_occurrences(self) -> usize {
        match self {
            Cadence::Yearly => 2,
            _ => 3,
        }
    }

    /// Occurrence `k` of a series anchored on `anchor` (occurrence 0).
    ///
    /// Always counted from the anchor, never from the previous occurrence,
    /// so a monthly series from 31 January falls on 28 (or 29) February and
    /// on 31 March again. `None` past chrono's last date.
    pub fn nth(self, anchor: NaiveDate, k: u32) -> Option<NaiveDate> {
        match self {
            Cadence::Weekly | Cadence::Fortnightly | Cadence::FourWeekly => {
                let days = u64::from(self.typical_days()) * u64::from(k);
                anchor.checked_add_days(Days::new(days))
            }
            Cadence::Monthly => anchor.checked_add_months(Months::new(k)),
            Cadence::Quarterly => anchor.checked_add_months(Months::new(k.checked_mul(3)?)),
            Cadence::Yearly => anchor.checked_add_months(Months::new(k.checked_mul(12)?)),
        }
    }
}

impl fmt::Display for Cadence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which way a series' money goes. Part of what groups a series: a
/// merchant's payments and its refunds are different series.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Direction {
    /// Money out: a negative amount.
    Out,
    /// Money in: a positive amount.
    In,
}

impl Direction {
    /// The direction of a non-zero amount; `None` for zero.
    pub fn of(minor: i64) -> Option<Direction> {
        match minor.signum() {
            -1 => Some(Direction::Out),
            1 => Some(Direction::In),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Direction::Out => "out",
            Direction::In => "in",
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether a series charges the same amount each time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AmountKind {
    /// The same amount every time, apart from a price change: a
    /// subscription, rent.
    Fixed,
    /// It varies from one occurrence to the next: a utility bill, a card
    /// repayment.
    Variable,
}

impl AmountKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            AmountKind::Fixed => "fixed",
            AmountKind::Variable => "variable",
        }
    }
}

impl fmt::Display for AmountKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A detected series' stable key: 12 lowercase hex digits.
///
/// Derived ([`SeriesKey::derive`]) from the account, the normalised
/// merchant, the direction, the currency, the cadence and the series'
/// ordinal among the series sharing all of those (by typical amount,
/// smallest first; 0 in the usual case of one). So it survives new
/// occurrences and price rises, and changes only when a second series of
/// the same merchant and cadence appears below it. A hash (FNV-1a, not a
/// secret): it carries no merchant name, but a guessed merchant can be
/// checked against it, so MCP output never includes it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SeriesKey(String);

impl SeriesKey {
    /// The key's length in hex digits.
    pub const LEN: usize = 12;

    /// The key for a series. `merchant` is normalised
    /// ([`normalize_merchant`]) first, so callers may pass the display name.
    pub fn derive(
        account: AccountId,
        merchant: &str,
        direction: Direction,
        currency: Currency,
        cadence: Cadence,
        ordinal: usize,
    ) -> SeriesKey {
        let text = format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            account.get(),
            normalize_merchant(merchant),
            direction.as_str(),
            currency.code(),
            cadence.as_str(),
            ordinal
        );
        // FNV-1a, 64-bit: deterministic across builds and platforms.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in text.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        SeriesKey(format!("{:012x}", hash >> 16))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SeriesKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A string that isn't a series key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("a series key is {} lowercase hex digits", SeriesKey::LEN)]
pub struct SeriesKeyError;

impl FromStr for SeriesKey {
    type Err = SeriesKeyError;

    /// Accepts the key in either case.
    fn from_str(text: &str) -> Result<SeriesKey, SeriesKeyError> {
        let key = text.trim().to_ascii_lowercase();
        if key.len() == SeriesKey::LEN && key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            Ok(SeriesKey(key))
        } else {
            Err(SeriesKeyError)
        }
    }
}

/// How a display merchant is compared: trimmed, whitespace runs collapsed
/// to one space, lowercased. Detection groups by it and schedule matching
/// compares by it, so "NETFLIX.COM" and "Netflix.com " are one merchant.
pub fn normalize_merchant(display: &str) -> String {
    display
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// One transaction, as detection reads it from `ledger_line`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerRow {
    pub transaction: TransactionId,
    pub account: AccountId,
    pub date: NaiveDate,
    /// The transaction's whole amount: its `ledger_line` rows summed (split
    /// lines and link parts), signed.
    pub amount: Money,
    /// `merchant_display`, or the bank's merchant when there is none. Rows
    /// with neither aren't grouped.
    pub merchant_display: Option<String>,
    /// The category, when the transaction (or every line of a split) has
    /// one.
    pub category: Option<CategoryKey>,
    /// Excluded with any reason. Detection leaves it out.
    pub excluded: bool,
    /// In a suggested or confirmed transfer pair. Detection leaves it out.
    pub transfer: bool,
}

/// A recurring series [`detect`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecurringSeries {
    pub key: SeriesKey,
    pub account: AccountId,
    /// The display merchant of the latest occurrence.
    pub merchant_display: String,
    pub direction: Direction,
    pub cadence: Cadence,
    /// The typical amount, signed, in the account's currency: the latest
    /// occurrence's for a fixed amount, the median of the recent ones for a
    /// variable one. Integer minor units.
    pub typical: Money,
    pub amount_kind: AmountKind,
    /// Largest minus smallest absolute amount over the occurrences since the
    /// last price change (0 for a fixed amount that never changed). A new
    /// schedule's amount tolerance starts from it.
    pub amount_spread_minor: i64,
    /// The most common category among the occurrences (the latest's on a
    /// tie), if any.
    pub category: Option<CategoryKey>,
    /// Occurrence 0's nominal date: [`Cadence::nth`] from it lands within
    /// the default tolerance of every occurrence, despite weekend shifts and
    /// short months.
    pub anchor: NaiveDate,
    /// The first and the latest occurrence's actual dates.
    pub first_date: NaiveDate,
    pub last_date: NaiveDate,
    /// The nominal date after the latest occurrence.
    pub next_expected: NaiveDate,
    /// How many transactions are in the series.
    pub occurrences: usize,
    /// The series' transactions, oldest first.
    pub transactions: Vec<TransactionId>,
    /// The account's latest row is past `next_expected` plus the default
    /// tolerance with no payment: the series looks stopped (a cancelled
    /// subscription). Judged from the rows alone, never the clock.
    pub lapsed: bool,
}

// Detection (M2.6) -------------------------------------------------------------
//
// Stubs: the detection work fills these in.

/// Why reading or dismissing series failed. Messages never contain
/// transaction text.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("there is no recurring series {0}")]
    SeriesNotFound(SeriesKey),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// The recurring series in `rows`. Pure and deterministic: the same rows
/// (in any order) give the same series in the same order.
pub fn detect(_rows: &[LedgerRow]) -> Vec<RecurringSeries> {
    todo!("Block D detection")
}

/// Every transaction as [`LedgerRow`]s, from `ledger_line`.
#[allow(dead_code)] // Stub until detection lands.
pub(crate) fn load_rows(_conn: &rusqlite::Connection) -> Result<Vec<LedgerRow>, Error> {
    todo!("Block D detection")
}

/// The series to show the user: [`detect`] over the ledger, without
/// dismissed series and without series already scheduled (one of their
/// transactions matched to a schedule).
pub(crate) fn find_in(_conn: &rusqlite::Connection) -> Result<Vec<RecurringSeries>, Error> {
    todo!("Block D detection")
}

/// [`find_in`] on the database.
pub fn find(db: &crate::db::Database) -> Result<Vec<RecurringSeries>, Error> {
    find_in(db.conn())
}

/// [`find_in`] on a read-only connection (the MCP server).
pub fn find_with_reader(reader: &crate::db::Reader) -> Result<Vec<RecurringSeries>, Error> {
    find_in(reader.conn())
}

/// Marks a series "Not recurring": [`find`] leaves it out from now on.
/// Returns false when it was already dismissed.
pub fn dismiss(_db: &mut crate::db::Database, _key: &SeriesKey) -> Result<bool, Error> {
    todo!("Block D detection")
}

/// Undoes [`dismiss`]. Returns false when the key wasn't dismissed.
pub fn undismiss(_db: &mut crate::db::Database, _key: &SeriesKey) -> Result<bool, Error> {
    todo!("Block D detection")
}
