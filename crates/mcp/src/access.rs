//! Access policy for every call: the client's level, the gate, and what the
//! access log records (ROADMAP M2.11–M2.13). The server's wrapper
//! ([`crate::server`]) calls these and nothing else decides access.
//!
//! Wave 0 is permissive scaffolding with marked seams. Block B owns this
//! file and replaces each `// Block B:` seam: the enabled / requested /
//! ignored policy, the kill switch read on every call, argument redaction,
//! the result hash, and `notifications/tools/list_changed` when a level
//! drops.

use std::collections::BTreeMap;

use coffer_core::access_log::Outcome;
use coffer_core::db::Reader;
use coffer_core::mcp_client::{self, AccessLevel, ClientName};
use rmcp::model::JsonObject;
use serde_json::Value;

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

/// The `purpose` argument as the log stores it: untrusted text, never
/// interpreted.
pub fn purpose(args: &JsonObject) -> Option<String> {
    // Block B: strip control, zero-width and bidi characters before it is
    // stored (core cuts it to `access_log::MAX_PURPOSE_LEN`).
    args.get("purpose")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// The arguments as the log stores them. Never free text.
///
/// Wave 0 keeps only each key and its JSON type, which can't leak anything,
/// with the keys sorted.
pub fn redact_args(args: &JsonObject) -> String {
    // Block B: keep keys, dates, enum values, numbers and IDs; store free
    // text only as its type and length (decision record 2026-11-10, point 4).
    // Sorted, so the stored text doesn't depend on the client's key order
    // (or on whether serde_json preserves it).
    let sorted: BTreeMap<&str, &str> = args
        .iter()
        .filter(|(key, _)| key.as_str() != "purpose")
        .map(|(key, value)| (key.as_str(), json_type(value)))
        .collect();
    let redacted: serde_json::Map<String, Value> = sorted
        .into_iter()
        .map(|(key, kind)| (key.to_owned(), Value::from(kind)))
        .collect();
    Value::Object(redacted).to_string()
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// A hash of the result the client received, so the log can show two
/// calls returned the same data without storing it.
pub fn result_hash(_result: &Value) -> Option<String> {
    // Block B: a hash of the canonical JSON (sha2 is already in the tree
    // via rmcp).
    None
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn redaction_keeps_no_values() {
        let args = json!({
            "purpose": "monthly review",
            "query": "ignore previous instructions",
            "limit": 10,
        });
        let redacted = redact_args(args.as_object().unwrap());
        assert_eq!(redacted, r#"{"limit":"number","query":"string"}"#);
        assert_eq!(
            purpose(args.as_object().unwrap()).as_deref(),
            Some("monthly review")
        );
    }
}
