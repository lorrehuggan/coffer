//! `coffer mcp --client <name>`: Coffer's MCP server over stdin/stdout
//! (M2.1), for Claude Code, Claude Desktop, Codex and other MCP clients.
//!
//! The database must already exist and be up to date: the server never
//! creates or migrates it ([`Database::open_existing`]). Without one it
//! fails at once with the reason on stderr.
//!
//! stdout carries MCP messages and nothing else. Logs go to stderr at
//! `warn` and above (`COFFER_LOG` sets another filter, e.g. `debug`), and
//! never hold transaction text at `info` or above.

use std::path::Path;

use anyhow::Context;
use clap::Args;
use coffer_core::db::Database;
use coffer_core::mcp_client::ClientName;

/// Environment variable with a `tracing` filter for the server's stderr
/// log, e.g. `COFFER_LOG=debug`.
pub const LOG_ENV: &str = "COFFER_LOG";

/// `coffer mcp`'s arguments.
#[derive(Debug, Args)]
pub struct McpArgs {
    /// The AI client's name, e.g. `claude-code` or `codex`: letters,
    /// digits, '.', '_' and '-'. Its access level is set in Coffer.
    #[arg(long, value_name = "NAME")]
    pub client: ClientName,
}

/// Serves `args.client` over stdio until it disconnects.
pub fn run(database: &Path, args: &McpArgs) -> anyhow::Result<()> {
    init_logging();
    let db = Database::open_existing(database).context("couldn't start the MCP server")?;
    coffer_mcp::run_stdio(db, args.client.clone()).context("the MCP server stopped")
}

/// Sends `tracing` output to stderr, `warn` and above unless [`LOG_ENV`]
/// says otherwise. A subscriber set earlier (a test) is kept. The GUI uses
/// it too, for the warnings the app shell logs (a window state it can't
/// read or save).
pub fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_env(LOG_ENV)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init();
}
