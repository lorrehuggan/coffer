//! What the access log keeps of a call (ROADMAP M2.12; decision record
//! "Settings, AI access and backups (2026-11-10)", point 4): the arguments
//! with free text redacted, the client's purpose cleaned of invisible
//! characters, and a hash of the result. Never the returned data.

use std::collections::BTreeMap;

use rmcp::model::JsonObject;
use serde_json::Value;

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
pub fn redact_args(args: &JsonObject, _input_schema: &JsonObject) -> String {
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
        let redacted = redact_args(args.as_object().unwrap(), &JsonObject::new());
        assert_eq!(redacted, r#"{"limit":"number","query":"string"}"#);
        assert_eq!(
            purpose(args.as_object().unwrap()).as_deref(),
            Some("monthly review")
        );
    }
}
