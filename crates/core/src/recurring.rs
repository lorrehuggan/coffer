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
//!
//! # Detection
//!
//! [`detect`] is pure and deterministic: rows in, series out, no clock, no
//! floats (amounts are integer minor units throughout). [`find`] runs it
//! over the ledger ([`load_rows`]: one row per transaction, a split's lines
//! summed back) and leaves out dismissed series and series already
//! scheduled.
//!
//! **Groups.** Rows that are excluded, in a suggested or confirmed transfer,
//! zero, or without a merchant are left out. The rest are grouped by
//! account, normalised merchant ([`normalize_merchant`]), [`Direction`] and
//! currency, and each group is sorted by date, then transaction ID. Series
//! never cross groups.
//!
//! **Chains.** A candidate series is a chain of a group's rows on one
//! [`Cadence`], from a start row and an anchor no more than the cadence's
//! [`Cadence::default_tolerance_days`] from the start's date (every such
//! anchor is tried). Occurrence `k` is expected on [`Cadence::nth`]`(anchor,
//! k)`, counted from the anchor, never from the previous payment, so
//! month-ends (31st → 28th, 30th) and payments moved off a weekend stay
//! within the tolerance. For each `k` the chain takes the unused row within
//! the tolerance of that date, after the previous occurrence, within a
//! factor of two of the previous occurrence's amount: the closest in amount,
//! then in date, then the earliest. A row at a new price (outside the 2%
//! band below) is passed over, as a gap, when the next window has a row at
//! the old price: it belongs to something else.
//!
//! **Gaps.** A chain may skip single occurrences, never two in a row (two
//! missed payments end it; [`RecurringSeries::lapsed`] covers a stopped
//! series), and at most one per four occurrences: the chain is cut back to
//! its longest prefix that keeps to that.
//!
//! **Which chain wins.** Each round takes the best valid chain over every
//! cadence, start and anchor, marks its rows used, and repeats until no
//! valid chain is left. Better is: more occurrences, then fewer gaps, then
//! fewer days off the nominal dates in total, then the shorter cadence, then
//! the earlier start, then the anchor nearest the start's date. So a weekly
//! series is read as weekly, once (its fortnightly reading has half the
//! occurrences, and the rows are then used), and a monthly series never as
//! quarterly with extras.
//!
//! **Valid.** A chain is a series when all of these hold:
//!
//! - it has [`Cadence::min_occurrences`] (3; 2 for yearly) for a fixed
//!   amount, one more for a variable amount (4; 3 for yearly), since a
//!   regular date is all a variable amount has to show;
//! - its rows are on average at most a third of the tolerance (and at
//!   least a day) from their nominal dates, so rows scattered across the
//!   whole tolerance don't make a series;
//! - its amounts are fixed or variable, below;
//! - **no other rows around it.** "Rows like it" are the group's other
//!   rows in its amount range: from its smallest to its largest amount
//!   widened by 2% for a fixed amount, within a factor of two of its median
//!   for a variable one. From one cadence period ([`Cadence::typical_days`])
//!   before its first occurrence to one after its last, there is at most
//!   one such row per four occurrences. Irregular purchases at one café
//!   always leave such rows around any chain picked out of them. A limit:
//!   two series of one merchant at the *same* amount on different days look
//!   like noise to each other and aren't reported;
//! - **not likely by chance.** Over the group's whole history (its first
//!   to its last row), take `like` = the rows like it, or, when more, its
//!   amount range's share of the group's other rows (rounded up: a price
//!   only the chain has is still one other rows could have had), and `p =
//!   like × window / history`, the chance that a `window`-day date window
//!   (twice the tolerance, plus one) holds one of them. The search tries
//!   about `(rows − n + 1) × window` starts and anchors, and a chain with
//!   `gaps` gaps could have taken any `n − 1` of its `n − 1 + gaps` later
//!   windows, so it expects `(rows − n + 1) × window × C(n − 1 + gaps,
//!   gaps) × p^(n−1)` chains like this one by chance. The chain is refused
//!   when that is at least 1/32. With no rows like it, it is never chance.
//!   Measured on synthetic noise (random dates and amounts at one shop over
//!   two years, irregular café visits at a few prices), about 1 ledger in
//!   300 still yields a spurious series, always a short weekly one at a
//!   shop with random amounts.
//!
//! **Fixed or variable.** Two amounts are the same price when they differ by
//! at most 2% of the earlier one. The occurrences, oldest first, split into
//! runs of one price (a run ends where an amount leaves the band of the one
//! before it). The amount is **fixed** when every run but the first and the
//! last has at least two occurrences (a price held, not a one-off; the first
//! may be an introductory price, the last a rise just taken) and there are
//! at most `max(1, occurrences / 4)` price changes. Its typical amount is
//! the latest occurrence's, and its spread is over the last run. Otherwise
//! it is **variable** when every amount is within a factor of two of the
//! median of all occurrences (else it is not a series). Its typical amount
//! is the median of the latest 6 occurrences (for an even count, the mean
//! of the two middle amounts, half a minor unit rounded up, away from
//! zero), and its spread is over those 6.
//!
//! **After detection.** A series is dropped when a later series of its
//! group, on the same cadence and within a factor of two of its typical
//! amount, starts after it ended: the payment stopped and started again, so
//! only the current run is reported. Then each series gets its key: its
//! ordinal is its place among its group's series on the same cadence, by
//! typical absolute amount, then first date. A series' `anchor` is the
//! chain's anchor, so [`Cadence::nth`] from it lands within the default
//! tolerance of every occurrence, and `next_expected` is the occurrence
//! after the latest.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use chrono::{Days, Months, NaiveDate, TimeDelta};

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
    /// schedule's amount tolerance starts from it. For a variable amount,
    /// over the recent occurrences its median is taken from (the latest 6).
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

/// Why reading or dismissing series failed. Messages never contain
/// transaction text.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("there is no recurring series {0}")]
    SeriesNotFound(SeriesKey),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// A fixed amount's band: two amounts are "the same price" when they differ
/// by at most 1/50 (2%) of the earlier one.
const FIXED_BAND_DIVISOR: i128 = 50;

/// How many of the latest occurrences a variable amount's median and spread
/// are taken over.
const RECENT: usize = 6;

/// A series may have at most one other row like its own around it per this
/// many occurrences (see the module docs).
const OCCURRENCES_PER_EXTRA: usize = 4;

/// A chain is taken for chance when the search would find at least one in
/// this many chains like it by chance (see the module docs).
const CHANCE_DIVISOR: u128 = 32;

/// The recurring series in `rows`. Pure and deterministic: the same rows
/// (in any order) give the same series in the same order.
///
/// The order: by account, normalised merchant, direction, currency, cadence
/// (shortest first), then key ordinal (typical absolute amount, smallest
/// first). The rules are in the module docs.
pub fn detect(rows: &[LedgerRow]) -> Vec<RecurringSeries> {
    // The account's latest row of any kind: what `lapsed` is judged by.
    let mut latest: BTreeMap<AccountId, NaiveDate> = BTreeMap::new();
    for row in rows {
        latest
            .entry(row.account)
            .and_modify(|date| *date = (*date).max(row.date))
            .or_insert(row.date);
    }

    let mut groups: BTreeMap<GroupKey, Vec<&LedgerRow>> = BTreeMap::new();
    for row in rows {
        if row.excluded || row.transfer {
            continue;
        }
        let Some(direction) = Direction::of(row.amount.minor()) else {
            continue;
        };
        let Some(display) = row.merchant_display.as_deref() else {
            continue;
        };
        let merchant = normalize_merchant(display);
        if merchant.is_empty() {
            continue;
        }
        groups
            .entry(GroupKey {
                account: row.account,
                merchant,
                direction,
                currency: row.amount.currency(),
            })
            .or_default()
            .push(row);
    }

    let mut found = Vec::new();
    for (group, mut members) in groups {
        members.sort_by_key(|row| (row.date, row.transaction));
        let account_latest = latest[&group.account];
        let mut series: Vec<RecurringSeries> = find_chains(&members)
            .into_iter()
            .map(|chain| build(&group, &members, chain, account_latest))
            .collect();
        drop_superseded(&mut series);
        series.sort_by(|a, b| {
            (a.cadence, abs(&a.typical), a.first_date, &a.transactions).cmp(&(
                b.cadence,
                abs(&b.typical),
                b.first_date,
                &b.transactions,
            ))
        });
        let mut ordinal = 0;
        let mut previous = None;
        for one in &mut series {
            if previous != Some(one.cadence) {
                ordinal = 0;
                previous = Some(one.cadence);
            }
            one.key = SeriesKey::derive(
                group.account,
                &group.merchant,
                group.direction,
                group.currency,
                one.cadence,
                ordinal,
            );
            ordinal += 1;
        }
        found.extend(series);
    }
    found
}

/// What groups rows into one merchant's payments.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct GroupKey {
    account: AccountId,
    /// Normalised ([`normalize_merchant`]).
    merchant: String,
    direction: Direction,
    currency: Currency,
}

/// A run of a group's rows on one cadence.
#[derive(Clone, Debug)]
struct Chain {
    cadence: Cadence,
    anchor: NaiveDate,
    /// `(k, index into the group's rows)`: occurrence `k` from the anchor,
    /// `k` increasing. The first is `(0, start)`.
    occurrences: Vec<(u32, usize)>,
    /// Occurrences skipped between the first and the last.
    gaps: u32,
    /// Days between each row and its nominal date, summed.
    deviation: i64,
}

impl Chain {
    /// How chains compete: lower is better. More occurrences, then fewer
    /// gaps, then closer to the nominal dates, then the shorter cadence,
    /// then the earlier start, then the anchor nearest the start's date.
    fn rank(&self, members: &[&LedgerRow]) -> impl Ord {
        let (_, start) = self.occurrences[0];
        let offset = (members[start].date - self.anchor).num_days().abs();
        (
            std::cmp::Reverse(self.occurrences.len()),
            self.gaps,
            self.deviation,
            self.cadence,
            start,
            offset,
            self.anchor,
        )
    }

    fn amounts(&self, members: &[&LedgerRow]) -> Vec<i128> {
        self.occurrences
            .iter()
            .map(|&(_, index)| abs(&members[index].amount))
            .collect()
    }
}

/// The absolute amount, widened so products never overflow.
fn abs(money: &Money) -> i128 {
    i128::from(money.minor()).abs()
}

/// `amount` is the same price as `reference` (within 2%).
fn in_band(amount: i128, reference: i128) -> bool {
    (amount - reference).abs() * FIXED_BAND_DIVISOR <= reference
}

/// Within a factor of two either way: how far one occurrence may be from
/// the previous one, and a variable amount from its median.
fn within_double(amount: i128, reference: i128) -> bool {
    amount * 2 >= reference && amount <= reference * 2
}

fn days(n: i64) -> TimeDelta {
    TimeDelta::days(n)
}

/// The series in one group's rows (sorted by date, then ID): the best valid
/// chain, again and again, until none is left.
fn find_chains(members: &[&LedgerRow]) -> Vec<Chain> {
    let mut used = vec![false; members.len()];
    let mut chains = Vec::new();
    loop {
        let mut best: Option<Chain> = None;
        for cadence in Cadence::ALL {
            let tolerance = i64::from(cadence.default_tolerance_days());
            for start in 0..members.len() {
                if used[start] {
                    continue;
                }
                for offset in -tolerance..=tolerance {
                    let Some(anchor) = members[start].date.checked_add_signed(days(offset)) else {
                        continue;
                    };
                    let chain = extend(members, &used, start, anchor, cadence);
                    if chain.occurrences.len() < cadence.min_occurrences() {
                        continue;
                    }
                    if best
                        .as_ref()
                        .is_some_and(|best| chain.rank(members) >= best.rank(members))
                    {
                        continue;
                    }
                    if is_series(members, &chain) {
                        best = Some(chain);
                    }
                }
            }
        }
        let Some(chain) = best else {
            break;
        };
        for &(_, index) in &chain.occurrences {
            used[index] = true;
        }
        chains.push(chain);
    }
    chains
}

/// The chain from `start` on `cadence` anchored on `anchor`, taken greedily.
fn extend(
    members: &[&LedgerRow],
    used: &[bool],
    start: usize,
    anchor: NaiveDate,
    cadence: Cadence,
) -> Chain {
    let tolerance = i64::from(cadence.default_tolerance_days());
    let last_date = members[members.len() - 1].date;
    let mut occurrences = vec![(0u32, start)];
    let mut misses = 0;
    let mut k = 1u32;
    while let Some(expected) = cadence.nth(anchor, k) {
        if expected - days(tolerance) > last_date {
            break;
        }
        let (_, previous) = occurrences[occurrences.len() - 1];
        let reference = abs(&members[previous].amount);
        let after = members[previous].date;
        let mut pick = pick_in_window(members, used, expected, tolerance, after, reference);
        // A different price here, but the current price again next time:
        // this one belongs to something else (another series, a one-off),
        // and this occurrence is a gap.
        if let Some(index) = pick
            && misses == 0
            && !in_band(abs(&members[index].amount), reference)
        {
            let next = cadence
                .nth(anchor, k + 1)
                .and_then(|next| pick_in_window(members, used, next, tolerance, after, reference));
            if next.is_some_and(|next| in_band(abs(&members[next].amount), reference)) {
                pick = None;
            }
        }
        match pick {
            Some(index) => {
                occurrences.push((k, index));
                misses = 0;
            }
            None => {
                misses += 1;
                if misses > 1 {
                    break;
                }
            }
        }
        k += 1;
    }

    // The longest prefix with at most one gap per four occurrences.
    let mut keep = 1;
    let mut gaps = 0;
    for (n, pair) in occurrences.windows(2).enumerate() {
        gaps += pair[1].0 - pair[0].0 - 1;
        if gaps as usize * 4 <= n + 2 {
            keep = n + 2;
        }
    }
    occurrences.truncate(keep);
    let gaps = occurrences[keep - 1].0 + 1 - keep as u32;
    let deviation = occurrences
        .iter()
        .map(|&(k, index)| {
            cadence.nth(anchor, k).map_or(0, |nominal| {
                (members[index].date - nominal).num_days().abs()
            })
        })
        .sum();
    Chain {
        cadence,
        anchor,
        occurrences,
        gaps,
        deviation,
    }
}

/// The unused row within `tolerance` days of `expected`, after `after`,
/// within a factor of two of `reference`: the closest in amount, then in
/// date, then the earliest.
fn pick_in_window(
    members: &[&LedgerRow],
    used: &[bool],
    expected: NaiveDate,
    tolerance: i64,
    after: NaiveDate,
    reference: i128,
) -> Option<usize> {
    let from = expected - days(tolerance);
    let to = expected + days(tolerance);
    let first = members.partition_point(|row| row.date < from);
    (first..members.len())
        .take_while(|&index| members[index].date <= to)
        .filter(|&index| !used[index] && members[index].date > after)
        .filter(|&index| within_double(abs(&members[index].amount), reference))
        .min_by_key(|&index| {
            let row = members[index];
            (
                (abs(&row.amount) - reference).abs(),
                (row.date - expected).num_days().abs(),
                index,
            )
        })
}

/// A chain long enough for its cadence is a series when its dates sit close
/// to the nominal ones, its amounts fit ([`classify`]), and its merchant
/// has few other rows like it, around it and over its whole history.
fn is_series(members: &[&LedgerRow], chain: &Chain) -> bool {
    let n = chain.occurrences.len();
    // On average at most a third of the tolerance, and at least a day, from
    // the nominal dates.
    let tolerance = i64::from(chain.cadence.default_tolerance_days());
    if chain.deviation * 3 > n as i64 * tolerance.max(3) {
        return false;
    }
    let amounts = chain.amounts(members);
    let Some(fit) = classify(&amounts) else {
        return false;
    };
    let needed = match fit.kind {
        AmountKind::Fixed => chain.cadence.min_occurrences(),
        AmountKind::Variable => chain.cadence.min_occurrences() + 1,
    };
    if n < needed {
        return false;
    }

    // Rows "like the series' own": a fixed amount's prices, widened by the
    // band; within a factor of two of a variable amount's median.
    let (low, high) = match fit.kind {
        AmountKind::Fixed => {
            let low = amounts.iter().copied().min().unwrap_or(0);
            let high = amounts.iter().copied().max().unwrap_or(0);
            (
                low - low / FIXED_BAND_DIVISOR,
                high + high / FIXED_BAND_DIVISOR,
            )
        }
        AmountKind::Variable => {
            let middle = median(&amounts);
            ((middle + 1) / 2, middle * 2)
        }
    };
    let in_chain: BTreeSet<usize> = chain.occurrences.iter().map(|&(_, index)| index).collect();
    let others: Vec<NaiveDate> = members
        .iter()
        .enumerate()
        .filter(|(index, row)| {
            !in_chain.contains(index) && (low..=high).contains(&abs(&row.amount))
        })
        .map(|(_, row)| row.date)
        .collect();

    // Around it: from a period before its first to a period after its last.
    let period = days(i64::from(chain.cadence.typical_days()));
    let (_, first) = chain.occurrences[0];
    let (_, last) = chain.occurrences[n - 1];
    let from = members[first].date - period;
    let to = members[last].date + period;
    let around = others
        .iter()
        .filter(|date| (from..=to).contains(*date))
        .count();
    if around * OCCURRENCES_PER_EXTRA > n {
        return false;
    }

    // Over the merchant's history: how many chains this long its other
    // such rows would make by chance (module docs). How many rows are like
    // the chain's is what was seen, or, when more, what the range's share
    // of the merchant's amounts would hold (a price seen only in the chain
    // is still a price other rows could have).
    let history = (members[members.len() - 1].date - members[0].date).num_days() + 1;
    let window = 2 * i64::from(chain.cadence.default_tolerance_days()) + 1;
    let group_low = members
        .iter()
        .map(|row| abs(&row.amount))
        .min()
        .unwrap_or(0);
    let group_high = members
        .iter()
        .map(|row| abs(&row.amount))
        .max()
        .unwrap_or(0);
    let unexplained = (members.len() - n) as i128;
    let range = group_high - group_low + 1;
    let share = ((unexplained * (high - low + 1) + range - 1) / range).min(unexplained);
    let like = others
        .len()
        .max(usize::try_from(share).unwrap_or(usize::MAX));
    !chance_chains_likely(like, members.len(), window, history, n, chain.gaps)
}

/// Whether a chain is likely to be chance: `n` occurrences with `gaps`
/// gaps, given `like` rows of the merchant like the chain's (outside it)
/// and `rows` rows in all, over `history` days.
///
/// With no rows like it, it is never chance. Otherwise `p = like × window
/// / history` is the chance that one of the chain's `window`-day windows
/// holds such a row. The search starts a chain from about `rows − n + 1`
/// rows, on `window` anchors each, and the chain's later `n − 1`
/// occurrences could have fallen in any `n − 1` of its `n − 1 + gaps`
/// later windows. So the search expects `(rows − n + 1) × window ×
/// C(n − 1 + gaps, gaps) × p^(n−1)` chance chains like it, and the chain
/// is taken for chance when that is at least 1/[`CHANCE_DIVISOR`]. Integer
/// arithmetic: a ratio in 64.64 fixed point.
fn chance_chains_likely(
    like: usize,
    rows: usize,
    window: i64,
    history: i64,
    n: usize,
    gaps: u32,
) -> bool {
    const ONE: u128 = 1 << 64;
    if like == 0 {
        return false;
    }
    let window = u128::from(window.unsigned_abs());
    let hits = u128::from(like as u64) * window;
    let history = u128::from(history.unsigned_abs().max(1));
    if hits >= history {
        return true;
    }
    let starts = (u128::from(rows.saturating_sub(n) as u64) + 1) * window;
    let later = u128::from(n.saturating_sub(1) as u64);
    let gaps = u128::from(gaps);
    let mut expected = starts.saturating_mul(ONE);
    // Each p step shrinks it and each binomial step grows it; interleaved,
    // it stays in range (it saturates rather than wraps if not).
    let steps = later.max(gaps);
    for step in 1..=steps {
        if step <= gaps {
            // C(later + gaps, gaps) = Π (later + i) / i.
            expected = expected.saturating_mul(later + step) / step;
        }
        if step <= later {
            expected = expected.saturating_mul(hits) / history;
        }
        if expected == 0 {
            return false;
        }
    }
    expected.saturating_mul(CHANCE_DIVISOR) >= ONE
}

/// How a chain's amounts behave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AmountFit {
    kind: AmountKind,
    /// Absolute.
    typical: i128,
    spread: i128,
}

/// Fixed or variable (module docs), or `None` when the amounts are too far
/// apart to be one series.
fn classify(amounts: &[i128]) -> Option<AmountFit> {
    let n = amounts.len();
    // Where each run of one price starts: a new run starts where an amount
    // leaves the band of the one before.
    let starts: Vec<usize> = (0..n)
        .filter(|&index| index == 0 || !in_band(amounts[index], amounts[index - 1]))
        .collect();
    let changes = starts.len() - 1;
    // Every run but the first and the last held for two occurrences.
    let inner_runs_held = starts.windows(3).all(|run| run[2] - run[1] >= 2);
    if inner_runs_held && changes <= (n / 4).max(1) {
        let current = &amounts[starts[changes]..];
        return Some(AmountFit {
            kind: AmountKind::Fixed,
            typical: amounts[n - 1],
            spread: spread(current),
        });
    }
    let overall = median(amounts);
    if !amounts.iter().all(|&amount| within_double(amount, overall)) {
        return None;
    }
    let recent = &amounts[n.saturating_sub(RECENT)..];
    Some(AmountFit {
        kind: AmountKind::Variable,
        typical: median(recent),
        spread: spread(recent),
    })
}

fn spread(amounts: &[i128]) -> i128 {
    let high = amounts.iter().copied().max().unwrap_or(0);
    let low = amounts.iter().copied().min().unwrap_or(0);
    high - low
}

/// The median of non-negative amounts; for an even count, the mean of the
/// two middle ones with a half unit rounded up.
fn median(amounts: &[i128]) -> i128 {
    let mut sorted = amounts.to_vec();
    sorted.sort_unstable();
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[middle]
    } else {
        (sorted[middle - 1] + sorted[middle] + 1) / 2
    }
}

/// The series for `chain` (its key is set later, with its ordinal).
fn build(
    group: &GroupKey,
    members: &[&LedgerRow],
    chain: Chain,
    account_latest: NaiveDate,
) -> RecurringSeries {
    let rows: Vec<&LedgerRow> = chain
        .occurrences
        .iter()
        .map(|&(_, index)| members[index])
        .collect();
    let amounts = chain.amounts(members);
    let fit = classify(&amounts).expect("a detected chain's amounts fit");
    let first = rows[0];
    let latest = rows[rows.len() - 1];
    let (last_k, _) = chain.occurrences[chain.occurrences.len() - 1];
    let next_expected = chain
        .cadence
        .nth(chain.anchor, last_k + 1)
        .unwrap_or(NaiveDate::MAX);
    let tolerance = days(i64::from(chain.cadence.default_tolerance_days()));
    let lapsed = next_expected
        .checked_add_signed(tolerance)
        .is_some_and(|late| account_latest > late);
    let typical = i64::try_from(fit.typical).expect("a row's own amount");
    let typical = match group.direction {
        Direction::Out => -typical,
        Direction::In => typical,
    };
    RecurringSeries {
        key: SeriesKey(String::new()),
        account: group.account,
        merchant_display: latest.merchant_display.clone().unwrap_or_default(),
        direction: group.direction,
        cadence: chain.cadence,
        typical: Money::new(typical, group.currency),
        amount_kind: fit.kind,
        amount_spread_minor: i64::try_from(fit.spread).unwrap_or(i64::MAX),
        category: common_category(&rows),
        anchor: chain.anchor,
        first_date: first.date,
        last_date: latest.date,
        next_expected,
        occurrences: rows.len(),
        transactions: rows.iter().map(|row| row.transaction).collect(),
        lapsed,
    }
}

/// The most common category among `rows` (oldest first), the latest's on a
/// tie.
fn common_category(rows: &[&LedgerRow]) -> Option<CategoryKey> {
    let mut seen: BTreeMap<&CategoryKey, (usize, usize)> = BTreeMap::new();
    for (index, row) in rows.iter().enumerate() {
        if let Some(category) = &row.category {
            let entry = seen.entry(category).or_insert((0, 0));
            entry.0 += 1;
            entry.1 = index;
        }
    }
    seen.into_iter()
        .max_by_key(|&(_, count_and_latest)| count_and_latest)
        .map(|(category, _)| category.clone())
}

/// Drops a series that another of its group, on the same cadence and at a
/// compatible amount, took over from: one that starts after it ended (a
/// subscription stopped and taken up again, or a run broken by two missed
/// payments in a row).
fn drop_superseded(series: &mut Vec<RecurringSeries>) {
    let superseded: Vec<bool> = series
        .iter()
        .map(|old| {
            series.iter().any(|new| {
                new.cadence == old.cadence
                    && new.first_date > old.last_date
                    && within_double(abs(&new.typical), abs(&old.typical))
            })
        })
        .collect();
    let mut flags = superseded.into_iter();
    series.retain(|_| !flags.next().unwrap_or(false));
}

/// Every transaction as [`LedgerRow`]s, from `ledger_line`, by transaction
/// ID.
pub(crate) fn load_rows(conn: &rusqlite::Connection) -> Result<Vec<LedgerRow>, Error> {
    let mut statement = conn.prepare(
        "SELECT l.transaction_id                                   AS transaction_id,
                min(l.account_id)                                  AS account_id,
                min(l.date)                                        AS date,
                sum(l.amount_minor)                                AS amount_minor,
                min(l.currency)                                    AS currency,
                min(coalesce(l.merchant_display, l.merchant))      AS merchant_display,
                CASE WHEN count(l.category_id) = count(*)
                      AND count(DISTINCT l.category_id) = 1
                     THEN min(c.key) END                           AS category_key,
                max(l.excluded_reason IS NOT NULL)                 AS excluded,
                max(coalesce(l.transfer_status IN ('suggested', 'confirmed'), 0))
                                                                   AS transfer
         FROM ledger_line l
         LEFT JOIN category c ON c.id = l.category_id
         GROUP BY l.transaction_id
         ORDER BY l.transaction_id",
    )?;
    let rows = statement.query_map([], |row| {
        let date: String = row.get(2)?;
        let date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
            .map_err(|_| crate::account::invalid(row, "date"))?;
        let currency: Currency = row.get(4)?;
        let category: Option<String> = row.get(6)?;
        let category = category
            .map(|key| {
                CategoryKey::new(&key).map_err(|_| crate::account::invalid(row, "category_key"))
            })
            .transpose()?;
        Ok(LedgerRow {
            transaction: row.get(0)?,
            account: row.get(1)?,
            date,
            amount: Money::new(row.get(3)?, currency),
            merchant_display: row.get(5)?,
            category,
            excluded: row.get(7)?,
            transfer: row.get(8)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The series to show the user: [`detect`] over the ledger, without
/// dismissed series and without series already scheduled (one of their
/// transactions matched to a schedule).
pub(crate) fn find_in(conn: &rusqlite::Connection) -> Result<Vec<RecurringSeries>, Error> {
    let series = detect(&load_rows(conn)?);
    let dismissed: BTreeSet<String> = conn
        .prepare("SELECT series_key FROM recurring_dismissal")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    let scheduled: BTreeSet<TransactionId> = conn
        .prepare("SELECT transaction_id FROM schedule_match")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(series
        .into_iter()
        .filter(|one| !dismissed.contains(one.key.as_str()))
        .filter(|one| !one.transactions.iter().any(|id| scheduled.contains(id)))
        .collect())
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
/// Returns false when it was already dismissed. Any key may be dismissed,
/// whether or not a series has it now: keys are opaque.
pub fn dismiss(db: &mut crate::db::Database, key: &SeriesKey) -> Result<bool, Error> {
    let changed = db.conn_mut().execute(
        "INSERT OR IGNORE INTO recurring_dismissal (series_key) VALUES (?1)",
        [key.as_str()],
    )?;
    tracing::debug!(series = %key, changed, "dismissed a recurring series");
    Ok(changed > 0)
}

/// Undoes [`dismiss`]. Returns false when the key wasn't dismissed.
pub fn undismiss(db: &mut crate::db::Database, key: &SeriesKey) -> Result<bool, Error> {
    let changed = db.conn_mut().execute(
        "DELETE FROM recurring_dismissal WHERE series_key = ?1",
        [key.as_str()],
    )?;
    tracing::debug!(series = %key, changed, "undismissed a recurring series");
    Ok(changed > 0)
}

#[cfg(test)]
mod tests;
