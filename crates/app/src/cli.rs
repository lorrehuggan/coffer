//! The `coffer` command line (M1.18): argument parsing and the headless
//! commands.
//!
//! `coffer` with no subcommand opens the GUI (`src/main.rs`); every
//! subcommand here runs without a window, so none of this touches GPUI.
//! Each command takes the database (or its path) and a writer for its
//! output, so the tests run them against a scratch directory and read what
//! they print.
//!
//! - `coffer import <file>`: detect the file's importer, preview the import
//!   into an account, and commit it ([`import`]).
//! - `coffer accounts [list]`, `coffer accounts add`: the accounts an import
//!   goes into ([`accounts`]).
//! - `coffer undo-import <batch>`: remove an import again ([`undo`]).
//! - `coffer backup [--list]`, `coffer restore <file>`: local backups
//!   ([`backup`]).
//! - `coffer export --format csv|json`: the ledger as a file ([`export`]).
//! - `coffer ai-access list|enable|level|revoke|ignore|off|on|log`: which AI
//!   clients may use the MCP server, at which level ([`ai_access`]).
//! - `coffer budgets [list|set|remove]`: monthly budgets per category
//!   ([`budgets`]).
//! - `coffer mcp --client <name>`: the MCP server over stdio ([`mcp`]). It
//!   ignores the output writer: stdout carries MCP messages only.
//! - `coffer schedules detect|list|show|confirm|…`: recurring payments and
//!   schedules ([`schedules`]).
//! - `coffer inbox [list|show|approve|reject|reports|report|delete-report]`:
//!   review AI clients' suggestions and decide them, and read and delete
//!   their saved reports ([`inbox`]).
//! - `coffer categorize <ID>... <CATEGORY>`: set transactions' category as
//!   the user, as one change batch ([`categorize`]).
//!
//! Output is what the user asked for and nothing else. Errors are returned
//! (as `anyhow` errors: this is the binary's layer) for `main` to print on
//! stderr. Nothing is logged; transaction text never reaches a log.

pub mod accounts;
pub mod ai_access;
pub mod backup;
pub mod budgets;
pub mod categorize;
pub mod export;
pub mod import;
pub mod inbox;
pub mod mcp;
pub mod schedules;
pub mod undo;

#[cfg(test)]
mod ai_access_tests;
#[cfg(test)]
mod backup_tests;
#[cfg(test)]
mod budgets_tests;
#[cfg(test)]
mod categorize_tests;
#[cfg(test)]
mod export_tests;
#[cfg(test)]
mod inbox_tests;
#[cfg(test)]
mod schedules_tests;
#[cfg(test)]
mod tests;

use std::io::Write;
use std::path::Path;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use coffer_core::db::Database;
use coffer_import::preset::Presets;

pub use accounts::{AccountsArgs, AccountsCommand, AddAccountArgs};
pub use ai_access::{AiAccessArgs, AiAccessCommand};
pub use backup::{BackupArgs, RestoreArgs};
pub use categorize::CategorizeArgs;
pub use export::{ExportArgs, ExportFormat};
pub use import::ImportArgs;
pub use inbox::{
    ApproveArgs, DeleteReportArgs, InboxArgs, InboxCommand, ListArgs, RejectArgs, ReportArgs,
    ShowArgs,
};
pub use mcp::McpArgs;
pub use schedules::{SchedulesArgs, SchedulesCommand};
pub use undo::UndoImportArgs;

/// Coffer: a local-first personal finance dashboard.
///
/// With no command, opens the app.
#[derive(Debug, Parser)]
#[command(name = "coffer", version, args_conflicts_with_subcommands = true)]
pub struct Cli {
    /// Open the component showcase (a development tool) instead of the app.
    #[arg(long)]
    pub showcase: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

/// The headless commands. None of them opens a window.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Import a bank export (CSV with a bundled preset, OFX or QFX) into an
    /// account.
    Import(ImportArgs),
    /// List the accounts, or add one.
    Accounts(AccountsArgs),
    /// Undo an import: remove its transactions, balances and flags.
    UndoImport(UndoImportArgs),
    /// Back up the database now (kept until you delete it), or list the
    /// backups.
    Backup(BackupArgs),
    /// Restore a backup over the database, after backing up the current one.
    Restore(RestoreArgs),
    /// Export transactions (split lines as rows) as CSV or JSON. Notes and
    /// private tags only when asked for.
    Export(ExportArgs),
    /// Run the MCP server over stdin/stdout for an AI client (Claude Code,
    /// Codex, …). Needs an existing database.
    Mcp(McpArgs),
    /// Which AI clients may use the MCP server and at which level, the
    /// kill switch, and the AI access log.
    AiAccess(AiAccessArgs),
    /// List, set or remove budgets: what you mean to spend in a category
    /// each month.
    Budgets(budgets::BudgetsArgs),
    /// Find recurring payments, confirm them as schedules, and manage
    /// schedules.
    Schedules(SchedulesArgs),
    /// Review the suggestions AI clients filed, and approve or reject them.
    /// Lists the pending ones when given no command.
    Inbox(InboxArgs),
    /// Set the category of transactions, as your own choice, recorded as
    /// one change batch: `coffer categorize 12 13 food.groceries`.
    Categorize(CategorizeArgs),
}

/// The bundled presets and the ones the user saved from the import screen
/// (`<data dir>/presets`). A saved preset that can't be used is left out
/// with a warning in the log; its name and path are the user's, not
/// transaction data.
fn presets(database: &Path) -> anyhow::Result<Presets> {
    let dir = coffer_core::paths::presets_dir(database);
    let (presets, errors) =
        coffer_import::preset::load_user_presets(&dir).context("the bundled presets don't load")?;
    for error in errors {
        tracing::warn!(%error, "a saved preset can't be used");
    }
    Ok(presets)
}

/// Runs `command` against the database at `database` (the data dir's
/// `coffer.db` in the binary), writing its output to `out`.
///
/// `accounts add` and `import` create the database if there is none;
/// `restore` creates it from the backup; `backup --list` reads only the
/// backups directory. The other commands need the database to exist
/// already; `mcp` also needs it up to date and never migrates it.
pub fn run(command: Command, database: &Path, out: &mut dyn Write) -> anyhow::Result<()> {
    match command {
        Command::Import(args) => {
            let presets = presets(database)?;
            let mut db = open_or_create(database)?;
            import::run(&mut db, &presets, &args, out)
        }
        Command::Accounts(AccountsArgs {
            command: Some(AccountsCommand::Add(args)),
        }) => {
            let presets = presets(database)?;
            let mut db = open_or_create(database)?;
            accounts::add(&mut db, &presets, &args, out)
        }
        Command::Accounts(AccountsArgs {
            command: None | Some(AccountsCommand::List),
        }) => {
            let db = open_existing(database)?;
            accounts::list(&db, out)
        }
        Command::UndoImport(args) => {
            let mut db = open_existing(database)?;
            undo::run(&mut db, &args, out)
        }
        Command::Backup(args) => backup::run(database, &args, out),
        Command::Restore(args) => backup::restore(database, &args, out),
        Command::Export(args) => {
            let db = open_existing(database)?;
            export::run(&db, &args, out)
        }
        Command::Mcp(args) => mcp::run(database, &args),
        Command::AiAccess(args) => ai_access::run(database, args, out),
        Command::Budgets(args) => budgets::run(database, args, out),
        Command::Schedules(args) => {
            let mut db = open_existing(database)?;
            schedules::run(&mut db, &args, out)
        }
        Command::Inbox(args) => {
            let mut db = open_existing(database)?;
            inbox::run(&mut db, args, out)
        }
        Command::Categorize(args) => {
            let mut db = open_existing(database)?;
            categorize::run(&mut db, &args, out)
        }
    }
}

/// Opens the database, creating it (and the data dir) if needed.
fn open_or_create(path: &Path) -> anyhow::Result<Database> {
    Database::open(path).with_context(|| format!("couldn't open the database {}", path.display()))
}

/// Opens the database only if it's there, so a read-only command never
/// leaves an empty database behind. Pending migrations are applied: the CLI,
/// like the app, keeps the schema current.
fn open_existing(path: &Path) -> anyhow::Result<Database> {
    if !path.try_exists().unwrap_or(true) {
        bail!(
            "there is no Coffer database at {} yet; add an account with \
             `coffer accounts add` to create it (or set COFFER_DATA_DIR to use another data directory)",
            path.display()
        );
    }
    open_or_create(path)
}

/// Writes rows under a header, each column padded to its widest cell.
fn write_table(out: &mut dyn Write, header: &[&str], rows: &[Vec<String>]) -> std::io::Result<()> {
    let mut widths: Vec<usize> = header.iter().map(|cell| cell.chars().count()).collect();
    for row in rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let header: Vec<String> = header.iter().map(|cell| (*cell).to_owned()).collect();
    for row in std::iter::once(&header).chain(rows) {
        let mut line = String::new();
        for (index, (cell, width)) in row.iter().zip(&widths).enumerate() {
            if index > 0 {
                line.push_str("  ");
            }
            line.push_str(cell);
            if index + 1 < row.len() {
                line.extend(std::iter::repeat_n(' ', width - cell.chars().count()));
            }
        }
        writeln!(out, "{}", line.trim_end())?;
    }
    Ok(())
}
