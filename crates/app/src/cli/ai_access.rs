//! `coffer ai-access`: which AI clients may use Coffer's MCP server and at
//! which level, the kill switch, and the access log (ROADMAP M2.11–M2.12;
//! the app's Settings take this over in M3.15).
//!
//! - `list` (the default): every client that asked for access or was
//!   enabled, and whether AI access is on.
//! - `enable <name>`, `level <name> <level>`, `revoke <name>`,
//!   `ignore <name>`: a client's access. Enabling starts at `aggregate`.
//! - `off`, `on`: the kill switch, for every client.
//! - `log`: the latest calls, newest first.
//!
//! A running server reads all of this on every call, so a change applies
//! to a connected client's next call without restarting it.
//!
//! Every subcommand first moves the calls servers refused while they
//! couldn't open the database (during a restore, or with migrations
//! pending) from the pending file into the log
//! ([`access_log::pending`]), so `log` and the clients' last-seen times
//! include them.
//!
//! The log shows what the server recorded: the arguments with free text
//! already redacted, the outcome and the counts. A row summarising calls
//! the pending file had no room for shows how many, and has no arguments. The client's `purpose` is
//! untrusted text from the AI client: it is shown on one line, without
//! invisible or control characters, and cut to [`PURPOSE_CHARS`].

use std::io::Write;
use std::path::Path;

use clap::builder::{PossibleValuesParser, TypedValueParser};
use clap::{Args, Subcommand};
use coffer_core::access_log::{self, LoggedCall};
use coffer_core::db::Database;
use coffer_core::envelope::strip_invisible;
use coffer_core::mcp_client::{self, AccessLevel, ClientInfo, ClientName};
use coffer_core::setting;

use super::{open_existing, write_table};

/// How many characters of a call's `purpose` the log shows.
pub const PURPOSE_CHARS: usize = 60;

/// How many characters of a call's redacted arguments the log shows.
pub const ARGS_CHARS: usize = 120;

/// How many calls `coffer ai-access log` shows unless `--limit` says.
pub const DEFAULT_LOG_LIMIT: u32 = 50;

#[derive(Debug, Args)]
pub struct AiAccessArgs {
    /// What to do; lists the clients when left out.
    #[command(subcommand)]
    pub command: Option<AiAccessCommand>,
}

#[derive(Debug, Subcommand)]
pub enum AiAccessCommand {
    /// List the AI clients that asked for access or were enabled (the
    /// default).
    List,
    /// Enable a client. One that wasn't enabled starts at `aggregate`.
    Enable(ClientArgs),
    /// Set an enabled client's access level.
    Level(LevelArgs),
    /// Revoke a client's access. It stays in the list and can be enabled
    /// again.
    Revoke(ClientArgs),
    /// Hide a client's request without enabling it (revokes it if it was
    /// enabled).
    Ignore(ClientArgs),
    /// Switch AI access off for every client (the kill switch).
    Off,
    /// Switch AI access back on for the enabled clients.
    On,
    /// Show the AI access log, newest first.
    Log(LogArgs),
}

#[derive(Debug, Args)]
pub struct ClientArgs {
    /// The client's name, as `coffer mcp --client` was given it.
    #[arg(value_name = "NAME")]
    pub name: ClientName,
}

#[derive(Debug, Args)]
pub struct LevelArgs {
    /// The client's name, as `coffer mcp --client` was given it.
    #[arg(value_name = "NAME")]
    pub name: ClientName,

    /// `aggregate`: totals only. `pseudonymous`: transaction rows with
    /// merchants as tokens. `standard`: everything the tools offer.
    #[arg(value_name = "LEVEL", value_parser = level_parser())]
    pub level: AccessLevel,
}

#[derive(Debug, Args)]
pub struct LogArgs {
    /// Only this client's calls.
    #[arg(long, value_name = "NAME")]
    pub client: Option<ClientName>,

    /// How many calls to show.
    #[arg(long, value_name = "N", default_value_t = DEFAULT_LOG_LIMIT,
          value_parser = clap::value_parser!(u32).range(1..))]
    pub limit: u32,
}

fn level_parser() -> impl TypedValueParser<Value = AccessLevel> {
    PossibleValuesParser::new(AccessLevel::ALL.iter().map(|level| level.as_str())).map(|key| {
        key.parse()
            .expect("the parser only accepts the levels' keys")
    })
}

/// Runs `coffer ai-access` against the database at `database`, which must
/// exist.
pub fn run(database: &Path, args: AiAccessArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    let mut db = open_existing(database)?;
    // Refused calls waiting outside the database. A failure leaves them
    // there for next time; the command still runs.
    match access_log::pending::flush(&mut db) {
        Ok(flushed) if flushed.unreadable > 0 => writeln!(
            out,
            "Skipped {} unreadable entries waiting for the AI access log.",
            flushed.unreadable
        )?,
        Ok(_) => {}
        Err(error) => writeln!(
            out,
            "Couldn't move the calls refused while the database was unavailable into the \
             AI access log ({error}); they're kept and moved next time."
        )?,
    }
    match args.command.unwrap_or(AiAccessCommand::List) {
        AiAccessCommand::List => list(&db, out),
        AiAccessCommand::Enable(args) => enable(&mut db, &args.name, out),
        AiAccessCommand::Level(args) => {
            mcp_client::set_level(&mut db, &args.name, args.level)?;
            writeln!(
                out,
                "{} is now at {}. A connected client gets the change on its next call.",
                args.name, args.level
            )?;
            Ok(())
        }
        AiAccessCommand::Revoke(args) => {
            mcp_client::revoke(&mut db, &args.name)?;
            writeln!(
                out,
                "Revoked {}: its next call is refused. `coffer ai-access enable {}` \
                 enables it again, at aggregate.",
                args.name, args.name
            )?;
            Ok(())
        }
        AiAccessCommand::Ignore(args) => {
            mcp_client::ignore(&mut db, &args.name)?;
            writeln!(
                out,
                "Ignored {}: it isn't enabled, and its calls are refused.",
                args.name
            )?;
            Ok(())
        }
        AiAccessCommand::Off => {
            setting::set_mcp_enabled(&mut db, false)?;
            writeln!(
                out,
                "AI access is off: every client's next call is refused. \
                 `coffer ai-access on` switches it back on."
            )?;
            Ok(())
        }
        AiAccessCommand::On => {
            setting::set_mcp_enabled(&mut db, true)?;
            writeln!(out, "AI access is on for the enabled clients.")?;
            Ok(())
        }
        AiAccessCommand::Log(args) => log(&db, &args, out),
    }
}

/// `coffer ai-access list`.
fn list(db: &Database, out: &mut dyn Write) -> anyhow::Result<()> {
    let reader = db.reader()?;
    if setting::mcp_enabled(&reader)? {
        writeln!(out, "AI access is on.")?;
    } else {
        writeln!(
            out,
            "AI access is off: every client is refused. `coffer ai-access on` switches it \
             back on."
        )?;
    }
    let clients = mcp_client::list(&reader)?;
    if clients.is_empty() {
        writeln!(
            out,
            "No AI clients yet. A client appears here after its first \
             `coffer mcp --client <name>`; `coffer ai-access enable <name>` enables one."
        )?;
        return Ok(());
    }
    let rows: Vec<Vec<String>> = clients.iter().map(client_row).collect();
    write_table(
        out,
        &["NAME", "STATUS", "ENABLED", "LEVEL", "LAST SEEN (UTC)"],
        &rows,
    )?;
    Ok(())
}

fn client_row(client: &ClientInfo) -> Vec<String> {
    vec![
        client.name.to_string(),
        client.status.as_str().to_owned(),
        if client.enabled { "yes" } else { "no" }.to_owned(),
        // The stored level only applies while the client is enabled.
        if client.enabled {
            client.level.as_str().to_owned()
        } else {
            "-".to_owned()
        },
        client
            .last_seen_at
            .as_deref()
            .map_or_else(|| "-".to_owned(), timestamp),
    ]
}

/// `coffer ai-access enable <name>`.
fn enable(db: &mut Database, name: &ClientName, out: &mut dyn Write) -> anyhow::Result<()> {
    let info = mcp_client::enable(db, name)?;
    writeln!(
        out,
        "Enabled {name} at {}. `coffer ai-access level {name} <level>` changes its level.",
        info.level
    )?;
    if !setting::mcp_enabled(&db.reader()?)? {
        writeln!(
            out,
            "AI access is off, so its calls are refused until `coffer ai-access on`."
        )?;
    }
    Ok(())
}

/// `coffer ai-access log`.
fn log(db: &Database, args: &LogArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    let reader = db.reader()?;
    let calls = access_log::recent_for(
        &reader,
        args.client.as_ref().map(ClientName::as_str),
        args.limit,
    )?;
    if calls.is_empty() {
        match &args.client {
            Some(client) => writeln!(out, "No calls from {client} in the AI access log.")?,
            None => writeln!(out, "No calls in the AI access log yet.")?,
        }
        return Ok(());
    }
    let rows: Vec<Vec<String>> = calls.iter().map(call_row).collect();
    write_table(
        out,
        &[
            "TIME (UTC)",
            "CLIENT",
            "TOOL",
            "LEVEL",
            "OUTCOME",
            "ROWS",
            "WITHHELD",
            "ARGS",
            "PURPOSE",
        ],
        &rows,
    )?;
    Ok(())
}

fn call_row(call: &LoggedCall) -> Vec<String> {
    vec![
        timestamp(&call.at),
        call.client.clone(),
        call.tool.clone(),
        call.access_level
            .map_or_else(|| "-".to_owned(), |level| level.as_str().to_owned()),
        match call.summarised_calls {
            Some(calls) => format!("{} ({calls} calls)", call.outcome),
            None => call.outcome.as_str().to_owned(),
        },
        call.row_count.to_string(),
        call.withheld_count.to_string(),
        // Already redacted by the server; keys can still come from the
        // client, so it gets the same clean-up as the purpose.
        one_line(&call.args_redacted_json, ARGS_CHARS),
        call.purpose.as_deref().map_or_else(
            || "-".to_owned(),
            |purpose| one_line(purpose, PURPOSE_CHARS),
        ),
    ]
}

/// `YYYY-MM-DDTHH:MM:SS.SSSZ` as `YYYY-MM-DD HH:MM:SS`.
fn timestamp(at: &str) -> String {
    match at.get(..19) {
        Some(seconds) => seconds.replacen('T', " ", 1),
        None => at.to_owned(),
    }
}

/// Untrusted text for one table cell: invisible and control characters
/// removed, whitespace runs (tabs and newlines too) as one space, cut to
/// `max` characters with an ellipsis.
fn one_line(text: &str, max: usize) -> String {
    let clean = strip_invisible(text);
    let words: Vec<&str> = clean.split_whitespace().collect();
    let line = words.join(" ");
    if line.chars().count() <= max {
        return line;
    }
    let mut cut: String = line.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}
