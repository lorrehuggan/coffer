//! Coffer's MCP server (`coffer mcp --client <name>`): rmcp over stdio, on
//! top of the core API (ROADMAP M2).
//!
//! - **Reads** run on a read-only connection ([`coffer_core::db::Reader`]).
//! - **Writes** go through [`store::Store`], the only writer: suggestions and
//!   reports (the inbox) for tools that declare they write, and the access
//!   log for the server itself (`AGENTS.md` → Invariants).
//! - **Tools** are registered per access level ([`registry`]); one module
//!   per group in [`tools`]. A client sees and can call only the tools at or
//!   below its level.
//! - **Every call** goes through one wrapper, gate → call → log
//!   ([`server`]), so every call leaves exactly one access-log row.
//! - **Prompts**: `monthly_review(month)` ([`prompts`]), built from the
//!   tools at the client's level.
//! - **Output** uses the shared types in [`output`]: integer money, display
//!   merchants or tokens, and bank text only as sanitised untrusted text.
//!
//! `README.md` in this crate says how to add a tool.

pub mod access;
pub mod output;
pub mod prompts;
pub mod registry;
pub mod server;
pub mod store;
pub mod tools;

use coffer_core::db::{self, Database};
use coffer_core::mcp_client::ClientName;

pub use server::Server;

/// Why the server couldn't start or stopped with an error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Database(#[from] db::Error),

    #[error("couldn't start the async runtime: {0}")]
    Runtime(#[source] std::io::Error),

    #[error("the MCP connection failed: {0}")]
    Protocol(String),
}

/// Serves `client` over stdin/stdout until the client disconnects. Blocks
/// the calling thread on a single-threaded runtime.
///
/// Nothing but MCP messages is written to stdout; logs go wherever the
/// binary's `tracing` subscriber sends them (stderr).
pub fn run_stdio(db: Database, client: ClientName) -> Result<(), Error> {
    let server = Server::new(db, client)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::Runtime)?;
    runtime.block_on(async move {
        use rmcp::ServiceExt;

        let service = server
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|error| Error::Protocol(error.to_string()))?;
        service
            .waiting()
            .await
            .map_err(|error| Error::Protocol(error.to_string()))?;
        Ok(())
    })
}
