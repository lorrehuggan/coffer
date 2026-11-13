//! `coffer schedules`: find recurring payments, confirm them as schedules,
//! and manage the schedules (ROADMAP M2.6, M2.10).
//!
//! - `detect`: the recurring series Coffer found and the user hasn't
//!   confirmed or dismissed, with the key `confirm` and `dismiss` take.
//! - `list`, `show <id>`: the schedules, and one schedule's occurrences.
//! - `confirm <key>`: a detected series becomes a schedule.
//! - `skip`/`unskip`, `match`/`unmatch`: decide about one occurrence.
//! - `pause`, `resume`, `end`, `delete`: a schedule's status, or remove it.
//! - `dismiss`/`undismiss`: mark a series "Not recurring", or undo that.
//!
//! Merchant names are the user's own display names: this is their own
//! terminal. Nothing here is logged.

use std::io::Write;

use anyhow::{Context, bail, ensure};
use chrono::NaiveDate;
use clap::{Args, Subcommand};
use coffer_core::db::Database;
use coffer_core::id::{ScheduleId, TransactionId};
use coffer_core::money::{DecimalMark, Money};
use coffer_core::recurring::{self, SeriesKey};
use coffer_core::schedule::{self, OccurrenceState, Schedule, Tolerance};

use super::import::plural;
use super::write_table;

#[derive(Debug, Args)]
pub struct SchedulesArgs {
    #[command(subcommand)]
    pub command: SchedulesCommand,
}

#[derive(Debug, Subcommand)]
pub enum SchedulesCommand {
    /// List the recurring payments Coffer found that aren't schedules yet,
    /// with the key to confirm or dismiss each by.
    Detect,
    /// List the schedules, with their next date and missed or late
    /// payments.
    List,
    /// Show one schedule's occurrences: paid, late, missed, skipped or
    /// upcoming.
    Show(ScheduleArg),
    /// Confirm a detected series as a schedule. Its past payments are
    /// matched at once.
    Confirm(ConfirmArgs),
    /// Skip one occurrence of a schedule: it isn't expected.
    Skip(OccurrenceArgs),
    /// Undo a skip.
    Unskip(OccurrenceArgs),
    /// Match a transaction to one occurrence of a schedule, whatever its
    /// amount or date.
    Match(MatchArgs),
    /// Remove the transaction matched to one occurrence of a schedule.
    Unmatch(OccurrenceArgs),
    /// Pause a schedule: nothing is matched or flagged until it resumes.
    Pause(ScheduleArg),
    /// Resume a paused schedule.
    Resume(ScheduleArg),
    /// End a schedule for good. Its matches and skips stay.
    End(ScheduleArg),
    /// Delete a schedule with its matches and skips. The transactions
    /// stay.
    Delete(DeleteArgs),
    /// Mark a detected series "Not recurring": `detect` no longer lists it.
    Dismiss(SeriesArg),
    /// List a dismissed series again.
    Undismiss(SeriesArg),
}

#[derive(Debug, Args)]
pub struct ScheduleArg {
    /// The schedule's ID, as `coffer schedules list` shows it.
    #[arg(value_name = "ID")]
    pub id: i64,
}

#[derive(Debug, Args)]
pub struct SeriesArg {
    /// The series' key, as `coffer schedules detect` shows it.
    #[arg(value_name = "KEY")]
    pub key: SeriesKey,
}

#[derive(Debug, Args)]
pub struct ConfirmArgs {
    /// The series' key, as `coffer schedules detect` shows it.
    #[arg(value_name = "KEY")]
    pub key: SeriesKey,

    /// How far a payment's amount may be from the schedule's either way,
    /// e.g. 1.50. Defaults to the series' spread or a tenth of its amount,
    /// whichever is larger.
    #[arg(long, value_name = "AMOUNT", allow_hyphen_values = true)]
    pub tolerance: Option<String>,

    /// How many days either side of its date a payment may fall. Defaults
    /// to the cadence's (3 for monthly).
    #[arg(long, value_name = "DAYS")]
    pub tolerance_days: Option<u32>,
}

#[derive(Debug, Args)]
pub struct OccurrenceArgs {
    /// The schedule's ID, as `coffer schedules list` shows it.
    #[arg(value_name = "ID")]
    pub id: i64,

    /// The occurrence's date, YYYY-MM-DD, as `coffer schedules show`
    /// shows it.
    #[arg(value_name = "DATE", value_parser = parse_date)]
    pub date: NaiveDate,
}

#[derive(Debug, Args)]
pub struct MatchArgs {
    /// The schedule's ID, as `coffer schedules list` shows it.
    #[arg(value_name = "ID")]
    pub id: i64,

    /// The occurrence's date, YYYY-MM-DD, as `coffer schedules show`
    /// shows it.
    #[arg(value_name = "DATE", value_parser = parse_date)]
    pub date: NaiveDate,

    /// The transaction that paid it (its ID).
    #[arg(value_name = "TRANSACTION")]
    pub transaction: i64,
}

#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// The schedule's ID, as `coffer schedules list` shows it.
    #[arg(value_name = "ID")]
    pub id: i64,

    /// Delete it. Without this, only shows what deleting would remove.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

fn parse_date(text: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d")
        .map_err(|_| format!("{text:?} isn't a date; use YYYY-MM-DD, e.g. 2026-11-03"))
}

/// `coffer schedules <command>`.
pub fn run(db: &mut Database, args: &SchedulesArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    match &args.command {
        SchedulesCommand::Detect => detect(db, out),
        SchedulesCommand::List => list(db, out),
        SchedulesCommand::Show(args) => show(db, ScheduleId::new(args.id), out),
        SchedulesCommand::Confirm(args) => confirm(db, args, out),
        SchedulesCommand::Skip(args) => {
            let id = ScheduleId::new(args.id);
            schedule::skip(db, id, args.date)?;
            writeln!(
                out,
                "Skipped the {} occurrence of schedule {id}.",
                args.date
            )?;
            Ok(())
        }
        SchedulesCommand::Unskip(args) => {
            let id = ScheduleId::new(args.id);
            schedule::unskip(db, id, args.date)?;
            writeln!(
                out,
                "The {} occurrence of schedule {id} is expected again.",
                args.date
            )?;
            Ok(())
        }
        SchedulesCommand::Match(args) => {
            let id = ScheduleId::new(args.id);
            let transaction = TransactionId::new(args.transaction);
            schedule::match_occurrence(db, id, args.date, transaction)?;
            writeln!(
                out,
                "Matched transaction {transaction} to the {} occurrence of schedule {id}.",
                args.date
            )?;
            Ok(())
        }
        SchedulesCommand::Unmatch(args) => {
            let id = ScheduleId::new(args.id);
            schedule::unmatch(db, id, args.date)?;
            writeln!(
                out,
                "Removed the match on the {} occurrence of schedule {id}; the transaction stays.",
                args.date
            )?;
            Ok(())
        }
        SchedulesCommand::Pause(args) => {
            let id = ScheduleId::new(args.id);
            schedule::pause(db, id)?;
            writeln!(
                out,
                "Paused schedule {id}: nothing is matched or flagged until you resume it."
            )?;
            Ok(())
        }
        SchedulesCommand::Resume(args) => {
            let id = ScheduleId::new(args.id);
            schedule::resume(db, id)?;
            writeln!(out, "Resumed schedule {id}.")?;
            Ok(())
        }
        SchedulesCommand::End(args) => {
            let id = ScheduleId::new(args.id);
            schedule::end(db, id)?;
            writeln!(out, "Ended schedule {id}.")?;
            Ok(())
        }
        SchedulesCommand::Delete(args) => delete(db, args, out),
        SchedulesCommand::Dismiss(args) => {
            if recurring::dismiss(db, &args.key)? {
                writeln!(
                    out,
                    "Marked series {} as not recurring; `coffer schedules detect` won't list it.",
                    args.key
                )?;
            } else {
                writeln!(out, "Series {} is already marked not recurring.", args.key)?;
            }
            Ok(())
        }
        SchedulesCommand::Undismiss(args) => {
            if recurring::undismiss(db, &args.key)? {
                writeln!(
                    out,
                    "Series {} is no longer marked not recurring.",
                    args.key
                )?;
            } else {
                writeln!(out, "Series {} isn't marked not recurring.", args.key)?;
            }
            Ok(())
        }
    }
}

fn today() -> anyhow::Result<NaiveDate> {
    schedule::local_today().context("couldn't read today's date")
}

/// `-10.99 GBP`.
fn money(amount: Money) -> String {
    format!("{} {}", amount.to_decimal_string(), amount.currency())
}

/// A schedule's amount tolerance as money in its currency.
fn tolerance_money(schedule: &Schedule) -> Money {
    Money::new(schedule.tolerance.amount_minor, schedule.amount.currency())
}

/// `-` for nothing.
fn or_dash<T: ToString>(value: Option<T>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| value.to_string())
}

fn detect(db: &Database, out: &mut dyn Write) -> anyhow::Result<()> {
    let series = recurring::find(db)?;
    if series.is_empty() {
        writeln!(
            out,
            "No recurring payments found that aren't schedules already."
        )?;
        return Ok(());
    }
    let rows: Vec<Vec<String>> = series
        .iter()
        .map(|series| {
            vec![
                series.key.to_string(),
                series.account.to_string(),
                series.merchant_display.clone(),
                series.cadence.to_string(),
                money(series.typical),
                series.amount_kind.to_string(),
                series.occurrences.to_string(),
                series.last_date.to_string(),
                series.next_expected.to_string(),
                if series.lapsed { "lapsed" } else { "" }.to_owned(),
            ]
        })
        .collect();
    write_table(
        out,
        &[
            "KEY", "ACCOUNT", "MERCHANT", "CADENCE", "AMOUNT", "KIND", "COUNT", "LAST", "NEXT", "",
        ],
        &rows,
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "Confirm one as a schedule with `coffer schedules confirm <key>`, or hide it with \
         `coffer schedules dismiss <key>`."
    )?;
    Ok(())
}

fn list(db: &Database, out: &mut dyn Write) -> anyhow::Result<()> {
    let views = schedule::overview(db, today()?)?;
    if views.is_empty() {
        writeln!(
            out,
            "No schedules yet. `coffer schedules detect` lists recurring payments to confirm."
        )?;
        return Ok(());
    }
    let rows: Vec<Vec<String>> = views
        .iter()
        .map(|view| {
            let schedule = &view.schedule;
            let count = |state: OccurrenceState| {
                view.occurrences
                    .iter()
                    .filter(|occurrence| occurrence.state == state)
                    .count()
                    .to_string()
            };
            vec![
                schedule.id.to_string(),
                schedule.merchant_display.clone(),
                schedule.cadence.to_string(),
                money(schedule.amount),
                format!("±{}", tolerance_money(schedule).to_decimal_string()),
                format!("±{}", schedule.tolerance.days),
                schedule.status.to_string(),
                or_dash(view.next),
                count(OccurrenceState::Missed),
                count(OccurrenceState::Late),
            ]
        })
        .collect();
    write_table(
        out,
        &[
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
        ],
        &rows,
    )?;
    Ok(())
}

fn show(db: &Database, id: ScheduleId, out: &mut dyn Write) -> anyhow::Result<()> {
    let view = schedule::view(db, id, today()?)?;
    describe(out, &view.schedule)?;
    if let Some(through) = view.covered_through {
        writeln!(out, "Imported through {through}.")?;
    }
    if view.occurrences.is_empty() {
        writeln!(out, "No occurrences to show.")?;
        return Ok(());
    }
    let rows: Vec<Vec<String>> = view
        .occurrences
        .iter()
        .map(|occurrence| {
            let payment = occurrence.payment.as_ref();
            vec![
                occurrence.date.to_string(),
                occurrence.state.to_string(),
                or_dash(payment.map(|payment| payment.date)),
                or_dash(payment.map(|payment| money(payment.amount))),
                or_dash(payment.map(|payment| payment.transaction)),
            ]
        })
        .collect();
    writeln!(out)?;
    write_table(
        out,
        &["DATE", "STATE", "PAID ON", "PAID", "TRANSACTION"],
        &rows,
    )?;
    Ok(())
}

/// `Schedule 3: Streamfix Media, monthly, …`, on its own line.
fn describe(out: &mut dyn Write, schedule: &Schedule) -> std::io::Result<()> {
    writeln!(out, "Schedule {}: {}", schedule.id, summary(schedule))
}

/// `Streamfix Media, monthly, -10.99 GBP ±1.10, ±3 days, active (account
/// 1)`.
fn summary(schedule: &Schedule) -> String {
    format!(
        "{}, {}, {} ±{}, ±{} days, {} (account {})",
        schedule.merchant_display,
        schedule.cadence,
        money(schedule.amount),
        tolerance_money(schedule).to_decimal_string(),
        schedule.tolerance.days,
        schedule.status,
        schedule.account
    )
}

fn confirm(db: &mut Database, args: &ConfirmArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    let tolerance = if args.tolerance.is_some() || args.tolerance_days.is_some() {
        let all = recurring::find(db)?;
        let Some(series) = all.iter().find(|series| series.key == args.key) else {
            bail!(
                "there is no recurring series {}; `coffer schedules detect` lists them",
                args.key
            );
        };
        let default = Tolerance::default_for(series);
        let amount_minor = match &args.tolerance {
            None => default.amount_minor,
            Some(text) => {
                let currency = series.typical.currency();
                let amount = Money::parse_decimal(text, currency, DecimalMark::Point)
                    .with_context(|| {
                        format!("{text:?} isn't an amount in {currency}, e.g. 1.50")
                    })?;
                ensure!(
                    !amount.is_negative(),
                    "the amount tolerance can't be negative"
                );
                amount.minor()
            }
        };
        Some(Tolerance {
            amount_minor,
            days: args.tolerance_days.unwrap_or(default.days),
        })
    } else {
        None
    };

    let id = schedule::confirm(db, &args.key, tolerance)?;
    let created = schedule::get(db, id)?;
    writeln!(out, "Confirmed schedule {id}: {}", summary(&created))?;
    writeln!(out, "`coffer schedules show {id}` lists its occurrences.")?;
    Ok(())
}

fn delete(db: &mut Database, args: &DeleteArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    let id = ScheduleId::new(args.id);
    let view = schedule::view(db, id, today()?)?;
    let count = |states: &[OccurrenceState]| {
        view.occurrences
            .iter()
            .filter(|occurrence| states.contains(&occurrence.state))
            .count()
    };
    let matches = count(&[OccurrenceState::Matched, OccurrenceState::Late]);
    let skips = count(&[OccurrenceState::Skipped]);
    describe(out, &view.schedule)?;
    writeln!(
        out,
        "Deleting it removes the schedule, {} and {}. The transactions stay.",
        plural(matches, "matched payment"),
        plural(skips, "skip")
    )?;

    if !args.yes {
        writeln!(out)?;
        writeln!(
            out,
            "Nothing was deleted. Run the command again with --yes to delete it, or end it \
             with `coffer schedules end {id}` to keep its history."
        )?;
        return Ok(());
    }

    let deleted = schedule::delete(db, id)?;
    writeln!(out)?;
    writeln!(
        out,
        "Deleted schedule {id}, {} and {}.",
        plural(deleted.matches, "matched payment"),
        plural(deleted.skips, "skip")
    )?;
    Ok(())
}
