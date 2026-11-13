//! Access policy for every call: the client's level, the gate, and what the
//! access log records (ROADMAP M2.11–M2.13). The server's wrapper
//! ([`crate::server`]) calls these and nothing else decides access.
//!
//! This file decides access: the enabled / requested / ignored policy and
//! the kill switch, read on every call. [`redact`] decides what the log
//! keeps of a call: the redacted arguments, the purpose and the result
//! hash.

mod redact;

pub use redact::{purpose, redact_args, result_hash};

use coffer_core::access_log::Outcome;
use coffer_core::db::Reader;
use coffer_core::mcp_client::{self, AccessLevel, ClientName};

use crate::registry::ToolSpec;

/// The client's level now, `None` when it may use no tool at all. Read on
/// every `tools/list` and every call, so a change applies at once.
pub fn client_level(
    reader: &Reader,
    client: &ClientName,
) -> Result<Option<AccessLevel>, mcp_client::Error> {
    // Block B: the real policy. A missing row is recorded as `requested`
    // and gets no tools; a row that isn't enabled gets none either.
    Ok(Some(
        mcp_client::stored_level(reader, client)?.unwrap_or(AccessLevel::Standard),
    ))
}

/// The gate's answer for one call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Run the tool for a client at this level.
    Allow(AccessLevel),
    /// Refused, with the outcome to log (never [`Outcome::Ok`] or
    /// [`Outcome::Error`]).
    Refuse(Outcome),
}

/// Whether `client`, at `level`, may call `tool` now.
pub fn gate(
    _reader: &Reader,
    _client: &ClientName,
    level: Option<AccessLevel>,
    tool: &ToolSpec,
) -> Result<Gate, mcp_client::Error> {
    // Block B: the kill switch (`refused_off`, read from `setting` on every
    // call) comes first, then the client's state (`refused_not_enabled`).
    let Some(level) = level else {
        return Ok(Gate::Refuse(Outcome::RefusedNotEnabled));
    };
    // A tool above the client's level isn't registered for it.
    if tool.level > level {
        return Ok(Gate::Refuse(Outcome::RefusedLevel));
    }
    Ok(Gate::Allow(level))
}

/// What a refused call tells the client.
pub fn refusal_message(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::RefusedOff => "AI access to Coffer is switched off.",
        Outcome::RefusedNotEnabled => {
            "This client isn't enabled in Coffer. The user can enable it in Coffer's settings."
        }
        Outcome::RefusedLevel => "This tool isn't available at this client's access level.",
        Outcome::Ok | Outcome::Error => "Refused.",
    }
}
