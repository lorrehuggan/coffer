//! `recurring_payments` and schedules (ROADMAP M2.6, M2.10).
//!
//! Block D. `recurring_payments` returns, in one call, the recurring series
//! Coffer has detected but the user hasn't confirmed or dismissed
//! ([`recurring::find_with_reader`]), and the user's confirmed schedules with
//! their recent occurrences ([`schedule::overview_with_reader`]).
//!
//! What it never returns: a series' key (a hash of the merchant, which a
//! guess could be checked against, so it stays in the CLI), transaction
//! IDs, account names or numbers, notes, tags, share labels or bank text.
//! Merchants go through [`Call::merchant`], so they are tokens at
//! `pseudonymous`.

use coffer_core::recurring::{self, RecurringSeries};
use coffer_core::schedule::{self, Occurrence, OccurrenceState, ScheduleStatus, ScheduleView};
use schemars::JsonSchema;
use serde::Serialize;

use crate::output::{Amount, MerchantLabel};
use crate::registry::{AccessLevel, Call, NoArgs, Registry, Reply, ToolError, ToolSpec, Writes};

/// Adds this group's tools.
pub fn register(registry: &mut Registry) {
    registry.add(RECURRING_PAYMENTS, recurring_payments);
}

pub const RECURRING_PAYMENTS: ToolSpec = ToolSpec {
    name: "recurring_payments",
    description: "Recurring payments and income: series Coffer detected in the transactions \
                  that the user hasn't confirmed yet (`detected`), and the schedules the user \
                  confirmed (`schedules`), each with its recent occurrences and whether they \
                  were paid, paid late, missed or skipped. Amounts are integer minor units; \
                  negative is money out.",
    level: AccessLevel::Pseudonymous,
    writes: Writes::Nothing,
};

/// How many past occurrences a schedule lists, before its next one.
pub const RECENT_OCCURRENCES: usize = 12;

/// `recurring_payments`' result.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RecurringPayments {
    /// The date the occurrence states are judged against (today, in the
    /// user's time zone), `YYYY-MM-DD`.
    pub as_of: String,
    /// Detected series the user hasn't confirmed as schedules or marked
    /// "Not recurring".
    pub detected: Vec<DetectedSeries>,
    /// The user's confirmed schedules, by ID.
    pub schedules: Vec<ScheduleSummary>,
}

/// A recurring series found in the transactions.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DetectedSeries {
    /// The merchant, as of its latest payment.
    pub merchant: MerchantLabel,
    /// The account it is on: an ID, as other tools return it.
    pub account_id: i64,
    pub direction: Direction,
    pub cadence: Cadence,
    /// The typical amount, signed: the latest payment's for a fixed amount,
    /// the median of the recent ones for a variable one.
    pub typical: Amount,
    pub amount_kind: AmountKind,
    /// The first payment's date, `YYYY-MM-DD`.
    pub first_date: String,
    /// The latest payment's date, `YYYY-MM-DD`.
    pub last_date: String,
    /// When the next payment is due, `YYYY-MM-DD`.
    pub next_expected: String,
    /// How many payments the series has.
    pub occurrences: u64,
    /// True when the account's transactions go past the next due date (and
    /// its tolerance) without a payment: the series looks stopped, e.g. a
    /// cancelled subscription.
    pub lapsed: bool,
    /// The most common category key among the payments, e.g.
    /// `bills.subscriptions`; null when none is categorised.
    pub category: Option<String>,
}

/// A schedule the user confirmed.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ScheduleSummary {
    /// The schedule's ID.
    pub id: i64,
    pub merchant: MerchantLabel,
    /// The account it is on: an ID, as other tools return it.
    pub account_id: i64,
    pub direction: Direction,
    pub cadence: Cadence,
    /// The expected amount, signed.
    pub amount: Amount,
    /// How far a payment's amount may be from `amount` either way and still
    /// count (never negative).
    pub tolerance_amount: Amount,
    /// How many days either side of its date a payment may fall and still
    /// count as on time.
    pub tolerance_days: u32,
    pub status: Status,
    /// The schedule's category key, or null.
    pub category: Option<String>,
    /// The next expected occurrence on or after `as_of`, `YYYY-MM-DD`; null
    /// when the schedule is paused or ended.
    pub next: Option<String>,
    /// The date of the account's latest imported transaction, `YYYY-MM-DD`:
    /// occurrences after it can't be judged missed yet. Null when the
    /// account has none.
    pub covered_through: Option<String>,
    /// The most recent occurrences (at most 12) and then the next upcoming
    /// one, oldest first. A paused or ended schedule lists only occurrences
    /// that were paid or skipped.
    pub occurrences: Vec<ScheduleOccurrence>,
}

/// One occurrence of a schedule.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ScheduleOccurrence {
    /// The date it is due, `YYYY-MM-DD`.
    pub date: String,
    pub state: State,
    /// What was paid, when `state` is `matched` or `late`; otherwise null.
    pub paid: Option<Amount>,
    /// When it was paid, `YYYY-MM-DD`, when `state` is `matched` or `late`;
    /// otherwise null.
    pub paid_on: Option<String>,
}

/// Which way the money goes.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Money out: payments.
    Out,
    /// Money in: income, refunds.
    In,
}

impl From<recurring::Direction> for Direction {
    fn from(direction: recurring::Direction) -> Direction {
        match direction {
            recurring::Direction::Out => Direction::Out,
            recurring::Direction::In => Direction::In,
        }
    }
}

/// How often it repeats.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cadence {
    /// Every 7 days.
    Weekly,
    /// Every 14 days.
    Fortnightly,
    /// Every 28 days.
    FourWeekly,
    /// The same day every month.
    Monthly,
    /// Every 3 months.
    Quarterly,
    /// Every 12 months.
    Yearly,
}

impl From<recurring::Cadence> for Cadence {
    fn from(cadence: recurring::Cadence) -> Cadence {
        match cadence {
            recurring::Cadence::Weekly => Cadence::Weekly,
            recurring::Cadence::Fortnightly => Cadence::Fortnightly,
            recurring::Cadence::FourWeekly => Cadence::FourWeekly,
            recurring::Cadence::Monthly => Cadence::Monthly,
            recurring::Cadence::Quarterly => Cadence::Quarterly,
            recurring::Cadence::Yearly => Cadence::Yearly,
        }
    }
}

/// Whether the amount is the same every time.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AmountKind {
    /// The same every time, apart from a price change: a subscription,
    /// rent.
    Fixed,
    /// It varies: a utility bill, a card repayment.
    Variable,
}

impl From<recurring::AmountKind> for AmountKind {
    fn from(kind: recurring::AmountKind) -> AmountKind {
        match kind {
            recurring::AmountKind::Fixed => AmountKind::Fixed,
            recurring::AmountKind::Variable => AmountKind::Variable,
        }
    }
}

/// A schedule's status.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Payments are matched and missed ones flagged.
    Active,
    /// Paused by the user: nothing is matched or flagged.
    Paused,
    /// Ended by the user, for good.
    Ended,
}

impl From<ScheduleStatus> for Status {
    fn from(status: ScheduleStatus) -> Status {
        match status {
            ScheduleStatus::Active => Status::Active,
            ScheduleStatus::Paused => Status::Paused,
            ScheduleStatus::Ended => Status::Ended,
        }
    }
}

/// Where an occurrence stands.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Not due yet, or due but the account's transactions aren't imported
    /// that far yet.
    Upcoming,
    /// Paid within the tolerance.
    Matched,
    /// Paid, but after its date plus the tolerance.
    Late,
    /// Due, past the tolerance, imported that far, and not paid.
    Missed,
    /// The user skipped it.
    Skipped,
}

impl From<OccurrenceState> for State {
    fn from(state: OccurrenceState) -> State {
        match state {
            OccurrenceState::Upcoming => State::Upcoming,
            OccurrenceState::Matched => State::Matched,
            OccurrenceState::Late => State::Late,
            OccurrenceState::Missed => State::Missed,
            OccurrenceState::Skipped => State::Skipped,
        }
    }
}

fn recurring_payments(
    call: &Call<'_>,
    _args: NoArgs,
) -> Result<Reply<RecurringPayments>, ToolError> {
    let today = schedule::local_today().map_err(ToolError::core)?;
    let series = recurring::find_with_reader(call.reader()).map_err(ToolError::core)?;
    let views = schedule::overview_with_reader(call.reader(), today).map_err(ToolError::core)?;

    let detected: Vec<DetectedSeries> =
        series.iter().map(|series| detected(call, series)).collect();
    let schedules: Vec<ScheduleSummary> = views.iter().map(|view| summary(call, view)).collect();
    let rows = (detected.len() + schedules.len()) as u64;
    Ok(Reply::new(
        RecurringPayments {
            as_of: today.to_string(),
            detected,
            schedules,
        },
        rows,
    ))
}

fn detected(call: &Call<'_>, series: &RecurringSeries) -> DetectedSeries {
    DetectedSeries {
        merchant: call.merchant(&series.merchant_display),
        account_id: series.account.get(),
        direction: series.direction.into(),
        cadence: series.cadence.into(),
        typical: call.amount(series.typical),
        amount_kind: series.amount_kind.into(),
        first_date: series.first_date.to_string(),
        last_date: series.last_date.to_string(),
        next_expected: series.next_expected.to_string(),
        occurrences: series.occurrences as u64,
        lapsed: series.lapsed,
        category: series.category.as_ref().map(|key| key.as_str().to_owned()),
    }
}

fn summary(call: &Call<'_>, view: &ScheduleView) -> ScheduleSummary {
    let schedule = &view.schedule;
    let amount = schedule.amount;
    let direction = recurring::Direction::of(amount.minor()).unwrap_or(recurring::Direction::Out);
    let tolerance = coffer_core::money::Money::new(
        schedule.tolerance.amount_minor.saturating_abs(),
        amount.currency(),
    );
    ScheduleSummary {
        id: schedule.id.get(),
        merchant: call.merchant(&schedule.merchant_display),
        account_id: schedule.account.get(),
        direction: direction.into(),
        cadence: schedule.cadence.into(),
        amount: call.amount(amount),
        tolerance_amount: call.amount(tolerance),
        tolerance_days: schedule.tolerance.days,
        status: schedule.status.into(),
        category: schedule
            .category
            .as_ref()
            .map(|key| key.as_str().to_owned()),
        next: view.next.map(|date| date.to_string()),
        covered_through: view.covered_through.map(|date| date.to_string()),
        occurrences: recent(view)
            .iter()
            .map(|occurrence| occurrence_out(call, occurrence))
            .collect(),
    }
}

/// The last [`RECENT_OCCURRENCES`] occurrences before the next upcoming
/// one, then that one.
fn recent(view: &ScheduleView) -> &[Occurrence] {
    let occurrences = &view.occurrences;
    let cut = view
        .next
        .and_then(|next| {
            occurrences
                .iter()
                .position(|occurrence| occurrence.date == next)
        })
        .unwrap_or(occurrences.len());
    let start = cut.saturating_sub(RECENT_OCCURRENCES);
    let end = (cut + 1).min(occurrences.len());
    &occurrences[start..end]
}

fn occurrence_out(call: &Call<'_>, occurrence: &Occurrence) -> ScheduleOccurrence {
    let payment = occurrence.payment.as_ref().filter(|_| {
        matches!(
            occurrence.state,
            OccurrenceState::Matched | OccurrenceState::Late
        )
    });
    ScheduleOccurrence {
        date: occurrence.date.to_string(),
        state: occurrence.state.into(),
        paid: payment.map(|payment| call.amount(payment.amount)),
        paid_on: payment.map(|payment| payment.date.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use coffer_core::id::{AccountId, ScheduleId};
    use coffer_core::money::{Currency, Money};
    use coffer_core::recurring::Cadence as CoreCadence;
    use coffer_core::schedule::{Schedule, Tolerance};

    use super::*;

    fn day(m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, m, d).unwrap()
    }

    fn view(months: u32, next: Option<u32>) -> ScheduleView {
        ScheduleView {
            schedule: Schedule {
                id: ScheduleId::new(1),
                account: AccountId::new(1),
                merchant_display: "Streamfix Media".into(),
                category: None,
                cadence: CoreCadence::Monthly,
                start_date: day(1, 3),
                amount: Money::new(-1099, Currency::GBP),
                tolerance: Tolerance {
                    amount_minor: 110,
                    days: 3,
                },
                status: ScheduleStatus::Active,
                series_key: None,
            },
            occurrences: (1..=months)
                .map(|m| Occurrence {
                    date: day(m, 3),
                    state: if Some(m) == next {
                        OccurrenceState::Upcoming
                    } else {
                        OccurrenceState::Missed
                    },
                    payment: None,
                })
                .collect(),
            next: next.map(|m| day(m, 3)),
            covered_through: None,
        }
    }

    #[test]
    fn keeps_twelve_past_occurrences_and_the_next() {
        let all = view(12, Some(12));
        assert_eq!(recent(&all).len(), 12);

        let mut long = view(12, Some(12));
        // 2025's occurrences in front: 23 in all, the next last.
        let earlier: Vec<Occurrence> = (1..=11)
            .map(|m| Occurrence {
                date: NaiveDate::from_ymd_opt(2025, m, 3).unwrap(),
                state: OccurrenceState::Skipped,
                payment: None,
            })
            .collect();
        long.occurrences.splice(0..0, earlier);
        let kept = recent(&long);
        assert_eq!(kept.len(), RECENT_OCCURRENCES + 1);
        assert_eq!(kept.last().unwrap().date, day(12, 3));
        assert_eq!(kept[0].date, NaiveDate::from_ymd_opt(2025, 11, 3).unwrap());
    }

    #[test]
    fn without_a_next_keeps_the_last_twelve() {
        let paused = view(12, None);
        let kept = recent(&paused);
        assert_eq!(kept.len(), 12);
        let mut long = view(12, None);
        long.occurrences.insert(
            0,
            Occurrence {
                date: NaiveDate::from_ymd_opt(2025, 12, 3).unwrap(),
                state: OccurrenceState::Skipped,
                payment: None,
            },
        );
        let kept = recent(&long);
        assert_eq!(kept.len(), RECENT_OCCURRENCES);
        assert_eq!(kept[0].date, day(1, 3));
    }
}
