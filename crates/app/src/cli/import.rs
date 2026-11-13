//! `coffer import <file>`: read a bank export, preview the import into an
//! account, and commit it.
//!
//! Choosing the importer: `--preset` names a bundled CSV preset; otherwise
//! detection runs over the OFX importer and every bundled preset, and only
//! a confident, unique match is used: OFX by its format, a preset by its
//! own header row. Anything else (no match, a tie, a CSV that merely
//! parses) is an error that lists the presets to choose from, never a
//! guess (`docs/ARCHITECTURE.md` → "The `Importer` trait").
//!
//! An OFX file can hold several statements (one per account); then
//! `--statement N` picks one, and without it the command lists them.

use std::io::Write;
use std::num::NonZeroUsize;
use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::Args;
use coffer_core::account::{self, Account, AccountKind};
use coffer_core::balance::CheckResult;
use coffer_core::db::Database;
use coffer_core::import::{
    self, BalanceCheck, CheckedBalance, Committed, Confirmations, FileBalanceKind,
    FileBalanceStatus, FileOrder, ImportPreview, ReconciliationStatus, RowOutcome,
};
use coffer_core::money::Currency;
use coffer_core::statement::Statement;
use coffer_import::ofx::{OfxImporter, OfxStatement, OfxStatementKind};
use coffer_import::preset::{Preset, Presets};
use coffer_import::{AnyImporter, Detect, Detection, Importer, SourceFile};

use super::accounts;

#[derive(Debug, Args)]
pub struct ImportArgs {
    /// The bank export to import: a CSV file from a bank with a bundled
    /// preset, or an OFX/QFX file.
    pub file: PathBuf,

    /// The account to import into: its ID or exact name. Needed unless
    /// exactly one open account in the file's currency can take it (pots
    /// don't count: their transfers come in with their parent's file).
    #[arg(long, value_name = "ID|NAME")]
    pub account: Option<String>,

    /// The bundled preset to read a CSV file with, e.g. uk-monzo. Detected
    /// from the file when left out.
    #[arg(long, value_name = "ID")]
    pub preset: Option<String>,

    /// For an OFX file that holds several statements (one per account):
    /// which one to import, counting from 1.
    #[arg(long, value_name = "N")]
    pub statement: Option<NonZeroUsize>,

    /// Show what the import would do, and write nothing.
    #[arg(long)]
    pub dry_run: bool,

    /// Import even with flagged rows or a balance mismatch. Flagged rows
    /// aren't imported but are recorded with the import; a mismatch stays
    /// flagged.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

/// How the file is read.
#[derive(Clone, Copy)]
enum Reader<'p> {
    Ofx,
    Preset(&'p Preset),
}

impl Reader<'_> {
    fn detect(&self, file: &SourceFile) -> Detection {
        match self {
            Reader::Ofx => OfxImporter::new().detect(file),
            Reader::Preset(preset) => preset.importer().detect(file),
        }
    }

    /// Whether `detection` is enough to use this reader without being told
    /// to: a preset needs its own header row, since any CSV file "parses";
    /// OFX has no per-bank variants, so its format is enough.
    fn is_confident(&self, detection: Detection) -> bool {
        match self {
            Reader::Ofx => detection >= Detection::Format,
            Reader::Preset(_) => detection >= Detection::Source,
        }
    }
}

/// `coffer import`.
pub fn run(
    db: &mut Database,
    presets: &Presets,
    args: &ImportArgs,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let file = SourceFile::read(&args.file)
        .with_context(|| format!("couldn't read {}", args.file.display()))?;
    let (statement, format) = read_statement(&file, presets, args)?;
    let target = choose_account(db, args.account.as_deref(), statement.currency())?;
    let preview = import::preview(db, target.id, file.name(), statement)?;

    write_preview(out, &preview, &format)?;

    let needed = preview.needs_confirmation();
    if args.dry_run {
        writeln!(out)?;
        writeln!(out, "Dry run: nothing was written.")?;
        if !preview.is_nothing_new() && needs_any(needed) {
            writeln!(
                out,
                "Importing it needs --yes to accept {}.",
                accepting(needed)
            )?;
        }
        return Ok(());
    }
    if preview.is_nothing_new() {
        writeln!(out)?;
        writeln!(
            out,
            "Nothing new to import: the account already has every row and balance in this file."
        )?;
        return Ok(());
    }
    if needs_any(needed) && !args.yes {
        bail!(
            "not imported: the import has {} (listed above). Run the command again with --yes \
             to import the new rows anyway; {}",
            accepting(needed),
            yes_explained(needed)
        );
    }

    let confirmed = if args.yes {
        needed
    } else {
        Confirmations::default()
    };
    match import::commit(db, &preview, confirmed) {
        Ok(Committed::Batch(summary)) => {
            writeln!(out)?;
            writeln!(
                out,
                "Imported {} as batch {}: {}, {}, {}.",
                preview.source_file(),
                summary.batch,
                plural(summary.transactions, "transaction"),
                plural(summary.balances, "balance"),
                plural(summary.flags, "flag")
            )?;
            writeln!(
                out,
                "Categorised: {} by the preset, {} by rules. Transfer pairs suggested: {}.",
                summary.categorized_by_preset, summary.categorized_by_rules, summary.transfer_pairs
            )?;
            if summary.splits_skipped > 0 {
                writeln!(
                    out,
                    "Rule splits not made: {} (in a transfer, too small or linked); \
                     they keep their category.",
                    summary.splits_skipped
                )?;
            }
            if summary.duplicates > 0 || summary.link_suggestions > 0 {
                writeln!(
                    out,
                    "To review: {}, {}.",
                    plural(summary.duplicates, "possible duplicate"),
                    plural(summary.link_suggestions, "suggested money-back link")
                )?;
            }
            if summary.mirrored > 0 {
                writeln!(
                    out,
                    "Pots: {} mirrored onto pots as confirmed transfers, {} created.",
                    plural(summary.mirrored, "transfer"),
                    plural(summary.pots_created, "pot")
                )?;
            }
            writeln!(out, "To undo it: coffer undo-import {}", summary.batch)?;
            Ok(())
        }
        Ok(Committed::NothingNew) => {
            writeln!(out)?;
            writeln!(out, "Nothing new to import.")?;
            Ok(())
        }
        Err(import::Error::Stale) => bail!(
            "not imported: the account changed while the import was being prepared \
             (another import, or the app). Run the command again to preview it afresh."
        ),
        Err(error) => Err(error.into()),
    }
}

fn needs_any(needed: Confirmations) -> bool {
    needed.flagged_rows || needed.balance_mismatch
}

fn accepting(needed: Confirmations) -> &'static str {
    match (needed.flagged_rows, needed.balance_mismatch) {
        (true, true) => "flagged rows and a balance mismatch",
        (true, false) => "flagged rows",
        _ => "a balance mismatch",
    }
}

fn yes_explained(needed: Confirmations) -> &'static str {
    match (needed.flagged_rows, needed.balance_mismatch) {
        (true, true) => {
            "the flagged rows stay out and are recorded with the import, and the mismatch \
             stays flagged on the account"
        }
        (true, false) => "the flagged rows stay out and are recorded with the import",
        _ => "the mismatch stays flagged on the account",
    }
}

/// Picks the importer and reads the file into a statement. Also returns
/// how it was read, for the summary.
fn read_statement(
    file: &SourceFile,
    presets: &Presets,
    args: &ImportArgs,
) -> anyhow::Result<(Statement, String)> {
    let reader = match &args.preset {
        Some(id) => match presets.get(id) {
            Some(preset) => Reader::Preset(preset),
            None => bail!(
                "there is no preset {id:?}; the bundled presets are:\n{}",
                preset_lines(presets)
            ),
        },
        None => detect(file, presets)?,
    };

    match reader {
        Reader::Preset(preset) => {
            if args.statement.is_some() {
                bail!("--statement is for OFX files that hold several statements");
            }
            let statement = preset.importer().import(file).with_context(|| {
                format!(
                    "couldn't read {} with the {} preset",
                    file.name(),
                    preset.id()
                )
            })?;
            Ok((
                statement,
                format!("CSV, preset {} ({})", preset.id(), preset.name()),
            ))
        }
        Reader::Ofx => {
            let read_error = || format!("couldn't read {} as OFX", file.name());
            let parsed = OfxImporter::new().parse(file).with_context(read_error)?;
            let statements = parsed.statements();
            let count = statements.len();
            let (index, format) = match (args.statement, count) {
                (Some(n), _) if n.get() <= count => {
                    (n.get() - 1, format!("OFX, statement {n} of {count}"))
                }
                (Some(n), _) => bail!(
                    "{} holds {}; there is no statement {n}:\n{}",
                    file.name(),
                    plural(count, "statement"),
                    statement_lines(statements)
                ),
                (None, 0 | 1) => (0, "OFX".to_owned()),
                (None, _) => bail!(
                    "{} holds {count} statements, one per account; choose one with --statement:\n{}",
                    file.name(),
                    statement_lines(statements)
                ),
            };
            let importer = if count == 0 {
                OfxImporter::new()
            } else {
                OfxImporter::for_statement(index)
            };
            let statement = importer.normalize(parsed).with_context(read_error)?;
            Ok((statement, format))
        }
    }
}

/// The one importer that confidently recognises the file.
fn detect<'p>(file: &SourceFile, presets: &'p Presets) -> anyhow::Result<Reader<'p>> {
    let mut found: Vec<(Reader<'p>, Detection)> = std::iter::once(Reader::Ofx)
        .chain(presets.iter().map(Reader::Preset))
        .map(|reader| (reader, reader.detect(file)))
        .filter(|(reader, detection)| reader.is_confident(*detection))
        .collect();
    // Best first; equals keep their order, as in `coffer_import::detect`.
    found.sort_by_key(|(_, detection)| std::cmp::Reverse(*detection));

    match found.as_slice() {
        [] => bail!(
            "couldn't tell what kind of file {} is: it isn't OFX or QFX, and it doesn't have \
             the columns of any bundled preset. If it's a CSV export from one of these banks, \
             choose its preset with --preset:\n{}\nOther CSV layouts can't be imported yet.",
            file.name(),
            preset_lines(presets)
        ),
        [(reader, _)] => Ok(*reader),
        [(reader, best), (_, next), ..] if best > next => Ok(*reader),
        [(_, best), ..] => {
            let tied: Vec<String> = found
                .iter()
                .filter(|(_, detection)| detection == best)
                .map(|(reader, _)| match reader {
                    Reader::Ofx => "  OFX / QFX".to_owned(),
                    Reader::Preset(preset) => format!("  {}  {}", preset.id(), preset.name()),
                })
                .collect();
            bail!(
                "{} fits more than one importer equally well; choose its preset with --preset:\n{}",
                file.name(),
                tied.join("\n")
            )
        }
    }
}

fn preset_lines(presets: &Presets) -> String {
    if presets.is_empty() {
        return "  (none)".to_owned();
    }
    presets
        .iter()
        .map(|preset| format!("  {}  {}", preset.id(), preset.name()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The file's statements, numbered for `--statement`. Only the end of an
/// account number is shown.
fn statement_lines(statements: &[OfxStatement]) -> String {
    statements
        .iter()
        .enumerate()
        .map(|(index, statement)| {
            let kind = match statement.kind() {
                OfxStatementKind::Bank => "bank account",
                OfxStatementKind::CreditCard => "credit card",
                OfxStatementKind::Investment => "investment account (can't be imported)",
            };
            let currency = statement
                .currency()
                .map_or_else(|| "unknown currency".to_owned(), |c| c.to_string());
            let suffix = statement
                .account_suffix()
                .map(|suffix| format!(", ending {suffix}"))
                .unwrap_or_default();
            format!(
                "  {}  {kind}{suffix}, {currency}, {}",
                index + 1,
                plural(statement.transaction_count(), "transaction")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The account named by `--account`, or the only one that can take the
/// file. A pot is never chosen without `--account`: its transfers come in
/// with its parent's export and are mirrored onto it, so once a Monzo
/// import has created pots, the next import still finds the one account.
fn choose_account(
    db: &Database,
    wanted: Option<&str>,
    currency: Currency,
) -> anyhow::Result<Account> {
    let all = account::list(db)?;
    if all.is_empty() {
        bail!(
            "there are no accounts yet; add the one to import into with \
             `coffer accounts add --name <name> --kind <kind> --currency {currency}`"
        );
    }
    if let Some(wanted) = wanted {
        return accounts::find(&all, wanted).cloned();
    }
    let fits: Vec<&Account> = all
        .iter()
        .filter(|account| {
            account.kind.holds_transactions()
                && account.kind != AccountKind::Pot
                && account.currency == currency
                && account.closed_on.is_none()
        })
        .collect();
    match fits.as_slice() {
        [only] => Ok((*only).clone()),
        [] => bail!(
            "no open account in {currency} can take this file; add one with `coffer accounts add`, \
             or name the account with --account. The accounts are:\n{}",
            accounts::list_lines(all.iter())
        ),
        several => bail!(
            "choose the account to import into with --account <ID or name>:\n{}",
            accounts::list_lines(several.iter().copied())
        ),
    }
}

/// The summary: what the file is, where it goes, and what the import does
/// with each row and balance. Shows positions and core's reasons only,
/// never a row's text.
fn write_preview(
    out: &mut dyn Write,
    preview: &ImportPreview,
    format: &str,
) -> std::io::Result<()> {
    let counts = preview.counts();
    writeln!(out, "File:      {}", preview.source_file())?;
    writeln!(out, "Format:    {format}")?;
    writeln!(out, "Account:   {}", accounts::describe(preview.account()))?;
    writeln!(
        out,
        "Rows:      {} new, {} already imported, {} flagged",
        counts.new, counts.already_imported, counts.flagged
    )?;

    let balances = preview.balances();
    if balances.is_empty() {
        writeln!(out, "Balances:  none in the file")?;
    } else {
        writeln!(out, "Balances:")?;
        for balance in balances {
            let kind = match balance.kind {
                FileBalanceKind::Stated => "stated by the file",
                FileBalanceKind::Opening => "opening, from the running balances",
            };
            let status = match balance.status {
                FileBalanceStatus::New => "new".to_owned(),
                FileBalanceStatus::AlreadyRecorded(id) => {
                    format!("already recorded (balance {id})")
                }
                FileBalanceStatus::Repeated => "repeated in the file".to_owned(),
                FileBalanceStatus::Locked => {
                    "in the locked period, left out (unlock the account to record it)".to_owned()
                }
            };
            writeln!(
                out,
                "  {}  {}  {kind}, {status}",
                balance.date, balance.balance
            )?;
        }
    }

    let reconciliation = preview.reconciliation();
    let status = match reconciliation.status() {
        ReconciliationStatus::NoBalances => {
            "nothing to check: the file has no balances, and none of the account's balances \
             cover its dates"
        }
        ReconciliationStatus::StartingPoint => {
            "starting point: the file's balance is the account's first, so there's nothing to \
             check it against yet"
        }
        ReconciliationStatus::Reconciled => "reconciled: every balance check passed",
        ReconciliationStatus::Mismatch => "MISMATCH: a balance doesn't agree with the transactions",
    };
    writeln!(out, "Reconciliation: {status}")?;
    if let Some(running) = &reconciliation.running {
        let order = match running.order {
            FileOrder::OldestFirst => "oldest first",
            FileOrder::NewestFirst => "newest first",
        };
        writeln!(
            out,
            "  running balances ({order}): {} checked, {} wrong",
            running.checked,
            running.mismatches.len()
        )?;
        for mismatch in &running.mismatches {
            writeln!(
                out,
                "    {}: states {}, but the rows before it give {} (the row is imported, its balance is flagged)",
                mismatch.location, mismatch.stated, mismatch.computed
            )?;
        }
    }
    for check in &reconciliation.checks {
        writeln!(out, "  {}", describe_check(check))?;
    }

    let pots = preview.sub_accounts();
    if !pots.is_empty() {
        writeln!(
            out,
            "Pots:      {} mirrored onto {} as confirmed transfers",
            plural(pots.mirrored_rows, "transfer"),
            plural(pots.new_pots.len() + pots.existing_pots.len(), "pot")
        )?;
        // A new pot's name is what the account will be called, so it is
        // shown; its key (the bank's ID) is not.
        for pot in &pots.new_pots {
            writeln!(out, "  creates pot \"{}\"", pot.name)?;
        }
        for &pot in &pots.closed_pots {
            writeln!(
                out,
                "  pot {pot} is closed, but still gets its transfers from this file"
            )?;
        }
    }

    let flagged: Vec<_> = preview
        .rows()
        .filter_map(|row| match row.outcome {
            RowOutcome::Flagged(flag) => Some((row.location, flag)),
            _ => None,
        })
        .collect();
    if !flagged.is_empty() {
        writeln!(
            out,
            "Flagged rows (not imported; recorded with the import):"
        )?;
        for (location, flag) in flagged {
            writeln!(out, "  {location}: {flag}")?;
        }
    }
    Ok(())
}

fn describe_check(check: &BalanceCheck) -> String {
    let which = match check.balance {
        CheckedBalance::FromFile(_) => "from this file".to_owned(),
        CheckedBalance::Stored(id) => format!("stored balance {id}"),
        CheckedBalance::Opening => "the account's opening balance".to_owned(),
    };
    let mut text = format!(
        "{}  {} ({which}): {}",
        check.date,
        check.stated,
        describe_result(&check.after, check)
    );
    if let Some(before) = &check.before
        && before != &check.after
    {
        text.push_str(&format!("; before this import it {}", result_was(before)));
    }
    text
}

fn describe_result(result: &CheckResult, check: &BalanceCheck) -> String {
    match result {
        CheckResult::StartingPoint => "the starting point".to_owned(),
        CheckResult::Passed { since } => format!("passes, counting from {since}"),
        CheckResult::Failed { since, .. } => {
            let computed = check.computed().map_or_else(
                || "a different amount".to_owned(),
                |money| money.to_string(),
            );
            if *since < check.date {
                format!(
                    "FAILS: the transactions give {computed}; the gap is after {since}, up to {}",
                    check.date
                )
            } else {
                format!(
                    "FAILS: the transactions give {computed}, from another balance on the same day"
                )
            }
        }
    }
}

fn result_was(result: &CheckResult) -> &'static str {
    match result {
        CheckResult::StartingPoint => "was the starting point",
        CheckResult::Passed { .. } => "passed",
        CheckResult::Failed { .. } => "failed",
    }
}

/// `3 transactions`, `1 balance`.
pub(super) fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}
