//! What the access log keeps of a call (ROADMAP M2.12; decision record
//! "Settings, AI access and backups (2026-11-10)", point 4): the arguments
//! with free text redacted, the client's purpose cleaned of invisible
//! characters, and a hash of the result. Never the returned data.
//!
//! # Redacted arguments
//!
//! [`redact_args`] keeps the structure of the arguments and the values that
//! can't carry free text, and stores everything else as its shape:
//!
//! | Argument value | Stored as |
//! |---|---|
//! | number, boolean, `null` | itself (integer IDs are numbers) |
//! | string that is a value of the argument's `enum` / `const` in the tool's schema (through `$ref`, `anyOf`, `oneOf`, `allOf`) | itself |
//! | string that is a date `YYYY-MM-DD`, a month `YYYY-MM` or an RFC 3339 date-time | itself |
//! | any other string, digits-only strings included | `{"length":N,"type":"string"}`, `N` in characters |
//! | array | an array of its items, each redacted by the item schema |
//! | object | its declared keys, sorted, each value redacted; undeclared keys counted as `"_undeclared": N` |
//!
//! Keys are client-controlled text too, so only keys the schema declares in
//! `properties` are kept; any other key (a map's keys included) is only
//! counted, and its value dropped. The top-level `purpose` is never stored
//! here: [`purpose`] logs it separately.
//!
//! A digits-only string isn't kept: a sort code or an account number is
//! one, and Coffer's IDs are integers. A category key is kept only when the
//! schema lists it in an `enum`.
//!
//! Where the schema says nothing (no schema, an unknown `$ref`, a map), the
//! value rules still apply without it: numbers, booleans, `null` and dates
//! are kept, strings are redacted, and an object's keys are all undeclared.
//!
//! The output is in the canonical form below (compact, every object's keys
//! sorted), so the same arguments always give the same text. For example
//! `{"n":3,"query":{"length":28,"type":"string"}}`.
//!
//! # Result hash
//!
//! [`result_hash`] is the SHA-256 of the result's canonical JSON, as 64
//! lowercase hex digits. The canonical form is serde_json's compact
//! serialisation with every object's keys sorted by their UTF-8 bytes
//! (recursively, whatever order the map holds them in): no whitespace,
//! strings escaped as serde_json escapes them, numbers as serde_json
//! formats them.

use std::collections::{BTreeMap, HashSet};

use coffer_core::envelope::strip_invisible;
use rmcp::model::JsonObject;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

/// The argument every tool takes and the log stores in its own column.
const PURPOSE: &str = "purpose";

/// The key that counts an object's undeclared keys in the redacted
/// arguments. Never kept as a key itself.
const UNDECLARED: &str = "_undeclared";

/// How many `$ref` / combinator hops one schema position may take before
/// the rest is ignored: enough for anything schemars emits, and a stop for
/// a schema that refers to itself.
const MAX_SCHEMA_DEPTH: usize = 32;

/// The `purpose` argument as the log stores it: untrusted text, never
/// interpreted. Invisible characters (controls, zero-width, bidi) are
/// stripped and the rest trimmed; `None` when nothing is left or it isn't
/// a string. Core cuts it to `access_log::MAX_PURPOSE_LEN`.
pub fn purpose(args: &JsonObject) -> Option<String> {
    let raw = args.get(PURPOSE)?.as_str()?;
    let cleaned = strip_invisible(raw);
    let trimmed = cleaned.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// The arguments as the log stores them: never free text. See the module
/// docs for the rules.
pub fn redact_args(args: &JsonObject, input_schema: &JsonObject) -> String {
    let schema = Schema { root: input_schema };
    let root = schema.resolve(input_schema);
    let without_purpose = args.iter().filter(|(key, _)| key.as_str() != PURPOSE);
    let mut out = String::new();
    write_canonical(
        &Value::Object(schema.redact_object(without_purpose, &root)),
        &mut out,
    );
    out
}

/// A hash of the result the client received, so the log can show two
/// calls returned the same data without storing it: SHA-256 of the
/// canonical JSON (module docs), lowercase hex.
pub fn result_hash(result: &Value) -> Option<String> {
    let mut canonical = String::new();
    write_canonical(result, &mut canonical);
    let digest = Sha256::digest(canonical.as_bytes());
    Some(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Appends `value` as canonical JSON.
fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let sorted: BTreeMap<&str, &Value> = map
                .iter()
                .map(|(key, value)| (key.as_str(), value))
                .collect();
            out.push('{');
            for (index, (key, value)) in sorted.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&Value::from(key).to_string());
                out.push(':');
                write_canonical(value, out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

/// The tool's input schema, for resolving `$ref`s against its root.
struct Schema<'a> {
    root: &'a JsonObject,
}

/// Every subschema that applies at one position of the arguments, with
/// `$ref`, `anyOf`, `oneOf` and `allOf` followed. A union: a value is
/// treated as matching any of them.
#[derive(Default)]
struct Position<'a> {
    schemas: Vec<&'a JsonObject>,
}

impl<'a> Schema<'a> {
    /// The subschemas `schema` stands for.
    fn resolve(&self, schema: &'a JsonObject) -> Position<'a> {
        let mut position = Position::default();
        let mut seen = HashSet::new();
        self.collect(schema, 0, &mut seen, &mut position.schemas);
        position
    }

    fn resolve_value(&self, schema: Option<&'a Value>) -> Position<'a> {
        schema
            .and_then(Value::as_object)
            .map(|schema| self.resolve(schema))
            .unwrap_or_default()
    }

    fn collect(
        &self,
        schema: &'a JsonObject,
        depth: usize,
        seen: &mut HashSet<&'a str>,
        out: &mut Vec<&'a JsonObject>,
    ) {
        if depth > MAX_SCHEMA_DEPTH {
            return;
        }
        out.push(schema);
        if let Some(reference) = schema.get("$ref").and_then(Value::as_str)
            && seen.insert(reference)
            && let Some(target) = self.pointer(reference)
        {
            self.collect(target, depth + 1, seen, out);
        }
        for combinator in ["anyOf", "oneOf", "allOf"] {
            let options = schema.get(combinator).and_then(Value::as_array);
            for option in options.into_iter().flatten().filter_map(Value::as_object) {
                self.collect(option, depth + 1, seen, out);
            }
        }
    }

    /// A local reference (`#/$defs/Name`, or any JSON pointer into the
    /// root); `None` for anything else.
    fn pointer(&self, reference: &str) -> Option<&'a JsonObject> {
        let path = reference.strip_prefix('#')?;
        if path.is_empty() {
            return Some(self.root);
        }
        let mut tokens = path
            .strip_prefix('/')?
            .split('/')
            .map(|token| token.replace("~1", "/").replace("~0", "~"));
        let mut node = self.root.get(&tokens.next()?)?;
        for token in tokens {
            node = match node {
                Value::Object(map) => map.get(&token)?,
                Value::Array(items) => items.get(token.parse::<usize>().ok()?)?,
                _ => return None,
            };
        }
        node.as_object()
    }

    fn redact(&self, value: &Value, position: &Position<'a>) -> Value {
        match value {
            Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
            Value::String(text) => {
                if is_date_like(text) || position.allows(value) {
                    value.clone()
                } else {
                    json!({ "type": "string", "length": text.chars().count() })
                }
            }
            Value::Array(items) => Value::Array(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| self.redact(item, &self.items(position, index)))
                    .collect(),
            ),
            Value::Object(map) => Value::Object(self.redact_object(map.iter(), position)),
        }
    }

    /// An object's declared keys, sorted, with their values redacted, and
    /// the count of the others under [`UNDECLARED`].
    fn redact_object<'v>(
        &self,
        entries: impl Iterator<Item = (&'v String, &'v Value)>,
        position: &Position<'a>,
    ) -> Map<String, Value> {
        let mut kept = BTreeMap::new();
        let mut undeclared = 0_u64;
        for (key, value) in entries {
            match position.property(key) {
                Some(schemas) if key != UNDECLARED => {
                    let mut property = Position::default();
                    for schema in schemas {
                        property.schemas.extend(self.resolve(schema).schemas);
                    }
                    kept.insert(key.clone(), self.redact(value, &property));
                }
                _ => undeclared += 1,
            }
        }
        if undeclared > 0 {
            kept.insert(UNDECLARED.to_owned(), Value::from(undeclared));
        }
        kept.into_iter().collect()
    }

    /// The schema for item `index` of an array at `position`: its
    /// `prefixItems` entry, else its `items`.
    fn items(&self, position: &Position<'a>, index: usize) -> Position<'a> {
        let mut items = Position::default();
        for schema in &position.schemas {
            let prefix = schema
                .get("prefixItems")
                .and_then(Value::as_array)
                .and_then(|prefix| prefix.get(index));
            let item = prefix.or_else(|| schema.get("items"));
            items.schemas.extend(self.resolve_value(item).schemas);
        }
        items
    }
}

impl<'a> Position<'a> {
    /// Whether `value` is one of the `enum` or `const` values here.
    fn allows(&self, value: &Value) -> bool {
        self.schemas.iter().any(|schema| {
            schema.get("const") == Some(value)
                || schema
                    .get("enum")
                    .and_then(Value::as_array)
                    .is_some_and(|options| options.contains(value))
        })
    }

    /// The schemas declared for property `key` (a `true` schema declares
    /// it without any), `None` if no schema here declares it.
    fn property(&self, key: &str) -> Option<Vec<&'a JsonObject>> {
        let mut declared = None;
        for schema in &self.schemas {
            let properties = schema.get("properties").and_then(Value::as_object);
            if let Some(property) = properties.and_then(|properties| properties.get(key)) {
                let schemas = declared.get_or_insert_with(Vec::new);
                schemas.extend(property.as_object());
            }
        }
        declared
    }
}

/// A date `YYYY-MM-DD`, a month `YYYY-MM`, or an RFC 3339 date-time
/// (`YYYY-MM-DDTHH:MM:SS`, optional 1–9 fraction digits, then `Z` or
/// `±HH:MM`). Checked field by field, so nothing else fits.
fn is_date_like(text: &str) -> bool {
    let bytes = text.as_bytes();
    match bytes.len() {
        7 => is_month(bytes),
        10 => is_date(bytes),
        _ => is_date_time(bytes),
    }
}

fn is_month(bytes: &[u8]) -> bool {
    bytes.len() == 7
        && number(&bytes[0..4]).is_some()
        && bytes[4] == b'-'
        && number(&bytes[5..7]).is_some_and(|month| (1..=12).contains(&month))
}

fn is_date(bytes: &[u8]) -> bool {
    if bytes.len() != 10 || !is_month(&bytes[0..7]) || bytes[7] != b'-' {
        return false;
    }
    let (Some(year), Some(month), Some(day)) = (
        number(&bytes[0..4]),
        number(&bytes[5..7]),
        number(&bytes[8..10]),
    ) else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (1..=days).contains(&day)
}

fn is_date_time(bytes: &[u8]) -> bool {
    // Date, `T`, `HH:MM:SS`.
    if bytes.len() < 20 || !is_date(&bytes[0..10]) || !matches!(bytes[10], b'T' | b't') {
        return false;
    }
    let time = &bytes[11..19];
    let in_range = |field: &[u8], max| number(field).is_some_and(|n| n <= max);
    if !(in_range(&time[0..2], 23)
        && time[2] == b':'
        && in_range(&time[3..5], 59)
        && time[5] == b':'
        && in_range(&time[6..8], 60))
    {
        return false;
    }
    let mut rest = &bytes[19..];
    if let Some(fraction) = rest.strip_prefix(b".") {
        let digits = fraction.iter().take_while(|b| b.is_ascii_digit()).count();
        if !(1..=9).contains(&digits) {
            return false;
        }
        rest = &fraction[digits..];
    }
    match rest {
        [b'Z' | b'z'] => true,
        [b'+' | b'-', hours @ ..] if hours.len() == 5 => {
            in_range(&hours[0..2], 23) && hours[2] == b':' && in_range(&hours[3..5], 59)
        }
        _ => false,
    }
}

/// The ASCII digits as a number; `None` if any byte isn't a digit.
fn number(bytes: &[u8]) -> Option<u32> {
    bytes.iter().try_fold(0_u32, |acc, &byte| {
        byte.is_ascii_digit()
            .then(|| acc * 10 + u32::from(byte - b'0'))
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn object(value: Value) -> JsonObject {
        match value {
            Value::Object(map) => map,
            other => panic!("not an object: {other}"),
        }
    }

    fn redact(args: Value, schema: Value) -> String {
        redact_args(&object(args), &object(schema))
    }

    /// The shape schemars 1 emits for a tool's arguments (draft 2020-12,
    /// enums under `$defs`, `Option` as `anyOf` with null), as the
    /// registry builds it, `purpose` included.
    fn tool_schema() -> Value {
        json!({
            "$defs": {
                "Kind": { "enum": ["spending", "income"], "type": "string" },
                "Doc": {
                    "oneOf": [
                        { "const": "alpha", "type": "string" },
                        { "const": "beta", "type": "string" }
                    ]
                },
                "Inner": {
                    "properties": {
                        "kind": { "$ref": "#/$defs/Kind" },
                        "label": { "type": "string" }
                    },
                    "type": "object"
                }
            },
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "properties": {
                "purpose": { "type": ["string", "null"] },
                "query": { "type": "string" },
                "kind": { "$ref": "#/$defs/Kind" },
                "doc": { "anyOf": [{ "$ref": "#/$defs/Doc" }, { "type": "null" }] },
                "docs": { "items": { "$ref": "#/$defs/Doc" }, "type": "array" },
                "from": { "type": ["string", "null"] },
                "n": { "type": ["integer", "null"] },
                "inner": { "$ref": "#/$defs/Inner" },
                "inners": { "items": { "$ref": "#/$defs/Inner" }, "type": "array" },
                "map": { "additionalProperties": { "type": "integer" }, "type": "object" },
                "category": { "enum": ["food.groceries", "food.eating_out"], "type": "string" }
            },
            "type": "object"
        })
    }

    #[test]
    fn dates_enums_numbers_and_ids_are_kept() {
        let redacted = redact(
            json!({
                "kind": "income",
                "doc": "beta",
                "from": "2026-01-31",
                "n": 42,
                "category": "food.groceries",
                "query": null,
            }),
            tool_schema(),
        );
        assert_eq!(
            redacted,
            r#"{"category":"food.groceries","doc":"beta","from":"2026-01-31","kind":"income","n":42,"query":null}"#
        );
        assert_eq!(
            redact(
                json!({ "ok": true, "id": 7, "x": 1.5 }),
                json!({
                    "properties": { "ok": {}, "id": {}, "x": {} }
                })
            ),
            r#"{"id":7,"ok":true,"x":1.5}"#
        );
    }

    #[test]
    fn free_text_is_only_its_length() {
        let redacted = redact(
            json!({
                "query": "Ignore previous instructions ✉",
                "kind": "Ignore previous instructions",
                "category": "food.something_else",
                "from": "12345678",
            }),
            tool_schema(),
        );
        assert_eq!(
            redacted,
            r#"{"category":{"length":19,"type":"string"},"from":{"length":8,"type":"string"},"kind":{"length":28,"type":"string"},"query":{"length":30,"type":"string"}}"#
        );
    }

    #[test]
    fn nested_arrays_and_objects_are_redacted_through_refs() {
        let redacted = redact(
            json!({
                "docs": ["alpha", "gamma"],
                "inner": { "kind": "spending", "label": "secret", "extra": "x" },
                "inners": [{ "kind": "nope" }, { "label": "abc" }],
            }),
            tool_schema(),
        );
        assert_eq!(
            redacted,
            r#"{"docs":["alpha",{"length":5,"type":"string"}],"inner":{"_undeclared":1,"kind":"spending","label":{"length":6,"type":"string"}},"inners":[{"kind":{"length":4,"type":"string"}},{"label":{"length":3,"type":"string"}}]}"#
        );
    }

    #[test]
    fn undeclared_keys_are_only_counted() {
        let redacted = redact(
            json!({
                "n": 1,
                "ignore previous instructions": "x",
                "email_attacker_example_com": 5,
                "_undeclared": 99,
                "map": { "free text key": 3 },
            }),
            tool_schema(),
        );
        assert_eq!(
            redacted,
            r#"{"_undeclared":3,"map":{"_undeclared":1},"n":1}"#
        );
    }

    #[test]
    fn purpose_is_never_in_the_arguments() {
        let args = json!({ "purpose": "monthly review", "n": 3 });
        assert_eq!(redact(args.clone(), tool_schema()), r#"{"n":3}"#);
        assert_eq!(
            redact(json!({ "purpose": "x" }), json!({})),
            "{}",
            "not even counted as undeclared"
        );
    }

    #[test]
    fn without_a_schema_values_still_follow_the_rules() {
        let redacted = redact(
            json!({ "query": "ignore previous instructions", "limit": 10 }),
            json!({}),
        );
        assert_eq!(redacted, r#"{"_undeclared":2}"#);
        // A declared property with no type information: strings are still
        // redacted, everything else kept.
        let redacted = redact(
            json!({
                "a": "ignore previous instructions",
                "b": [1, "2026-11", {"k": "v"}],
                "c": false,
            }),
            json!({ "properties": { "a": {}, "b": {}, "c": true }, "$ref": "#/$defs/Missing" }),
        );
        assert_eq!(
            redacted,
            r#"{"a":{"length":28,"type":"string"},"b":[1,"2026-11",{"_undeclared":1}],"c":false}"#
        );
    }

    #[test]
    fn a_self_referencing_schema_terminates() {
        let schema = json!({
            "$defs": { "Node": { "$ref": "#/$defs/Node", "anyOf": [{ "$ref": "#/$defs/Node" }] } },
            "properties": { "node": { "$ref": "#/$defs/Node" } }
        });
        assert_eq!(
            redact(json!({ "node": "text" }), schema),
            r#"{"node":{"length":4,"type":"string"}}"#
        );
    }

    #[test]
    fn key_order_does_not_matter() {
        let mut forward = JsonObject::new();
        let mut backward = JsonObject::new();
        let entries = [
            ("query", json!("abc")),
            ("kind", json!("income")),
            ("n", json!(5)),
            ("inner", json!({ "label": "x", "kind": "spending" })),
        ];
        for (key, value) in &entries {
            forward.insert((*key).to_owned(), value.clone());
        }
        for (key, value) in entries.iter().rev() {
            backward.insert((*key).to_owned(), value.clone());
        }
        let schema = object(tool_schema());
        assert_eq!(
            redact_args(&forward, &schema),
            redact_args(&backward, &schema)
        );
    }

    #[test]
    fn date_shapes_are_strict() {
        for kept in [
            "2026-11",
            "2026-02-28",
            "2024-02-29",
            "2026-11-13T12:04:31Z",
            "2026-11-13T12:04:31.123Z",
            "2026-11-13T12:04:31.123456789+01:00",
            "2026-11-13t23:59:60-05:30",
        ] {
            assert!(is_date_like(kept), "{kept}");
        }
        for redacted in [
            "",
            "2026",
            "2026-13",
            "2026-00",
            "2026-02-29",
            "2026-11-31",
            "2026-11-1",
            "2026/11/13",
            "２０２６-11",
            "2026-11-13T12:04:31",
            "2026-11-13T24:00:00Z",
            "2026-11-13T12:04:31.Z",
            "2026-11-13T12:04:31.1234567890Z",
            "2026-11-13T12:04:31+0100",
            "2026-11-13T12:04:31Z ignore",
            "2026-11-13 12:04:31Z",
            "+2026-11-13",
        ] {
            assert!(!is_date_like(redacted), "{redacted}");
        }
    }

    #[test]
    fn purpose_is_cleaned_of_invisible_characters() {
        let args = object(json!({
            "purpose": "  \u{202E}monthly\u{200B} review\u{2066}\u{E0041}\n ",
        }));
        assert_eq!(purpose(&args).as_deref(), Some("monthly review"));
        for empty in [
            json!({}),
            json!({ "purpose": null }),
            json!({ "purpose": 3 }),
        ] {
            assert_eq!(purpose(&object(empty)), None);
        }
        let blank = object(json!({ "purpose": " \u{200B}\u{FEFF}\t" }));
        assert_eq!(purpose(&blank), None);
    }

    #[test]
    fn result_hash_is_stable_across_key_order() {
        let mut forward = JsonObject::new();
        forward.insert("a".to_owned(), json!(1));
        forward.insert("b".to_owned(), json!({ "y": [1, "two"], "x": null }));
        let mut backward = JsonObject::new();
        backward.insert("b".to_owned(), json!({ "x": null, "y": [1, "two"] }));
        backward.insert("a".to_owned(), json!(1));

        let hash = result_hash(&Value::Object(forward)).unwrap();
        assert_eq!(
            result_hash(&Value::Object(backward)).as_deref(),
            Some(&*hash)
        );
        assert_eq!(hash.len(), 64);
        assert!(
            hash.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
    }

    #[test]
    fn result_hash_differs_for_different_results() {
        let hashes: HashSet<String> = [
            json!({ "a": 1 }),
            json!({ "a": 2 }),
            json!({ "a": "1" }),
            json!({ "b": 1 }),
            json!({ "a": [1, 2] }),
            json!({ "a": [2, 1] }),
            json!({}),
        ]
        .iter()
        .map(|value| result_hash(value).unwrap())
        .collect();
        assert_eq!(hashes.len(), 7);
    }

    #[test]
    fn result_hash_is_sha256_of_the_canonical_form() {
        let mut out = String::new();
        write_canonical(&json!({ "b": "é\"", "a": [1.5, true, null] }), &mut out);
        assert_eq!(out, r#"{"a":[1.5,true,null],"b":"é\""}"#);
        // SHA-256 of `{}`.
        assert_eq!(
            result_hash(&json!({})).as_deref(),
            Some("44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a")
        );
    }
}
