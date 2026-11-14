//! Reading the log for the app's Settings (ROADMAP M3.15; `docs/DESIGN.md`
//! → "Settings"): pages of calls filtered by client and time, what each
//! client did in a period, and a call's redacted arguments as lines.
//!
//! Everything here only reads. The arguments are already redacted by the
//! server (free text is stored as its type and length); [`arguments`] lays
//! them out and cleans the keys, which are client-controlled text, as
//! `coffer ai-access log` does. Nothing here returns the result hash.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use rusqlite::params;
use serde_json::Value;

use super::{Error, LoggedCall, Outcome};
use crate::db::Reader;
use crate::envelope::strip_invisible;

/// `time` as the log stores it: UTC, `YYYY-MM-DDTHH:MM:SS.SSSZ`, so it
/// compares with `at` as text.
pub fn timestamp(time: DateTime<Utc>) -> String {
    crate::clock::stored(time)
}

/// Where a page of calls ends: the last call shown. The next page starts
/// after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    pub at: String,
    pub id: i64,
}

impl Cursor {
    /// The cursor after `call`.
    pub fn after(call: &LoggedCall) -> Self {
        Self {
            at: call.at.clone(),
            id: call.id,
        }
    }
}

/// Which calls to read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    /// Only this client's calls, matched as stored.
    pub client: Option<String>,
    /// Only calls at or after this time ([`timestamp`]).
    pub since: Option<String>,
    /// Only calls older than this one: the next page.
    pub after: Option<Cursor>,
}

/// Up to `limit` calls that `query` selects, newest first (by time, then
/// by row ID, as [`super::recent_for`]).
pub fn page(reader: &Reader, query: &Query, limit: u32) -> Result<Vec<LoggedCall>, Error> {
    let (after_at, after_id) = match &query.after {
        Some(cursor) => (Some(cursor.at.as_str()), Some(cursor.id)),
        None => (None, None),
    };
    let mut statement = reader.conn().prepare(
        "SELECT id, at, client, tool, access_level, outcome, purpose, args_redacted_json, \
         row_count, result_hash, withheld_count, summarised_calls \
         FROM mcp_access_log \
         WHERE (?1 IS NULL OR client = ?1) AND (?2 IS NULL OR at >= ?2) \
           AND (?3 IS NULL OR at < ?3 OR (at = ?3 AND id < ?4)) \
         ORDER BY at DESC, id DESC LIMIT ?5",
    )?;
    let rows = statement.query_map(
        params![query.client, query.since, after_at, after_id, limit],
        |row| {
            Ok(LoggedCall {
                id: row.get(0)?,
                at: row.get(1)?,
                client: row.get(2)?,
                tool: row.get(3)?,
                access_level: row.get(4)?,
                outcome: row.get(5)?,
                purpose: row.get(6)?,
                args_redacted_json: row.get(7)?,
                row_count: row.get::<_, i64>(8)?.unsigned_abs(),
                result_hash: row.get(9)?,
                withheld_count: row.get::<_, i64>(10)?.unsigned_abs(),
                summarised_calls: row.get::<_, Option<i64>>(11)?.map(i64::unsigned_abs),
            })
        },
    )?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// How many calls `query` selects (ignoring [`Query::after`]). A row that
/// summarises refused calls counts as the calls it stands for.
pub fn count(reader: &Reader, query: &Query) -> Result<u64, Error> {
    let count: i64 = reader.conn().query_row(
        "SELECT coalesce(sum(coalesce(summarised_calls, 1)), 0) FROM mcp_access_log \
         WHERE (?1 IS NULL OR client = ?1) AND (?2 IS NULL OR at >= ?2)",
        params![query.client, query.since],
        |row| row.get(0),
    )?;
    Ok(count.unsigned_abs())
}

/// What one client did in a period.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientActivity {
    /// Every call, refused and failed ones included.
    pub calls: u64,
    /// Calls to tools that only read, which returned their result.
    pub reads: u64,
    /// The rows those reads returned.
    pub read_rows: u64,
    /// Calls to tools that write to the Inbox, which returned their result.
    pub writes: u64,
    /// Refused calls by why (every `refused_*` outcome).
    pub refused: BTreeMap<Outcome, u64>,
    /// Calls whose tool failed.
    pub errors: u64,
}

impl ClientActivity {
    /// Every refused call.
    pub fn refused_total(&self) -> u64 {
        self.refused.values().sum()
    }
}

/// What each client did since `since` (every call when `None`), by the
/// name the calls were logged under. `writes` says which tools write: the
/// MCP server's registry knows, core doesn't.
pub fn activity(
    reader: &Reader,
    since: Option<&str>,
    writes: impl Fn(&str) -> bool,
) -> Result<BTreeMap<String, ClientActivity>, Error> {
    let mut statement = reader.conn().prepare(
        "SELECT client, tool, outcome, sum(coalesce(summarised_calls, 1)), sum(row_count) \
         FROM mcp_access_log WHERE ?1 IS NULL OR at >= ?1 \
         GROUP BY client, tool, outcome",
    )?;
    let rows = statement.query_map(params![since], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Outcome>(2)?,
            row.get::<_, i64>(3)?.unsigned_abs(),
            row.get::<_, i64>(4)?.unsigned_abs(),
        ))
    })?;
    let mut clients: BTreeMap<String, ClientActivity> = BTreeMap::new();
    for row in rows {
        let (client, tool, outcome, calls, rows) = row?;
        let entry = clients.entry(client).or_default();
        entry.calls += calls;
        match outcome {
            Outcome::Ok if writes(&tool) => entry.writes += calls,
            Outcome::Ok => {
                entry.reads += calls;
                entry.read_rows += rows;
            }
            Outcome::Error => entry.errors += calls,
            refused => *entry.refused.entry(refused).or_default() += calls,
        }
    }
    Ok(clients)
}

/// What a client left in the Inbox.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Written {
    pub suggestions: u64,
    /// Saved reports ("reviews" in the app).
    pub reports: u64,
}

/// The suggestions and reports each client filed since `since`, by
/// `source_client`, whatever has happened to them since.
pub fn written(reader: &Reader, since: &str) -> Result<BTreeMap<String, Written>, Error> {
    let mut written: BTreeMap<String, Written> = BTreeMap::new();
    for (table, reports) in [("suggestion", false), ("report", true)] {
        let mut statement = reader.conn().prepare(&format!(
            "SELECT source_client, count(*) FROM {table} WHERE created_at >= ?1 \
             GROUP BY source_client"
        ))?;
        let rows = statement.query_map([since], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?.unsigned_abs(),
            ))
        })?;
        for row in rows {
            let (client, count) = row?;
            let entry = written.entry(client).or_default();
            if reports {
                entry.reports += count;
            } else {
                entry.suggestions += count;
            }
        }
    }
    Ok(written)
}

/// One argument of a logged call, as the app shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Argument {
    /// The argument's name, cleaned for display: invisible characters
    /// removed, whitespace runs as one space.
    pub key: String,
    pub value: ArgumentValue,
}

/// An argument's stored value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgumentValue {
    /// Free text, stored as its length only.
    Text { length: u64 },
    /// A value the log keeps (a date, a key, a number, an ID), or a list or
    /// object of them, written out on one line; text inside it as its
    /// length.
    Kept(String),
    /// Keys the tool doesn't declare: only how many there were.
    Undeclared(u64),
}

/// The key the server counts undeclared keys under.
const UNDECLARED: &str = "_undeclared";

/// The call's redacted arguments as lines, in the stored (sorted) order.
/// Arguments that aren't an object (which the server never writes) are one
/// line with an empty key. Values are cleaned like keys.
pub fn arguments(args_redacted_json: &str) -> Vec<Argument> {
    let Ok(value) = serde_json::from_str::<Value>(args_redacted_json) else {
        return vec![Argument {
            key: String::new(),
            value: ArgumentValue::Kept(clean(args_redacted_json)),
        }];
    };
    let Value::Object(map) = value else {
        return vec![Argument {
            key: String::new(),
            value: describe(&value),
        }];
    };
    map.iter()
        .map(|(key, value)| {
            if key == UNDECLARED
                && let Some(count) = value.as_u64()
            {
                return Argument {
                    key: "other keys".to_owned(),
                    value: ArgumentValue::Undeclared(count),
                };
            }
            Argument {
                key: clean(key),
                value: describe(value),
            }
        })
        .collect()
}

/// A redacted value's length, if it is redacted text:
/// `{"length":N,"type":"string"}`.
fn text_length(value: &Value) -> Option<u64> {
    let map = value.as_object()?;
    if map.len() != 2 || map.get("type")?.as_str()? != "string" {
        return None;
    }
    map.get("length")?.as_u64()
}

fn describe(value: &Value) -> ArgumentValue {
    match text_length(value) {
        Some(length) => ArgumentValue::Text { length },
        None => ArgumentValue::Kept(inline(value)),
    }
}

/// `value` on one line: text as its length, lists and objects spelled out.
fn inline(value: &Value) -> String {
    if let Some(length) = text_length(value) {
        return format!("text, {length} characters");
    }
    match value {
        Value::String(text) => clean(text),
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(inline).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(map) => {
            let fields: Vec<String> = map
                .iter()
                .map(|(key, value)| format!("{}: {}", clean(key), inline(value)))
                .collect();
            format!("{{{}}}", fields.join(", "))
        }
        other => other.to_string(),
    }
}

/// Client-controlled text for one line: invisible and control characters
/// removed, whitespace runs as one space.
fn clean(text: &str) -> String {
    strip_invisible(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::super::{NewEntry, append};
    use super::*;
    use crate::db::Database;
    use crate::mcp_client::{AccessLevel, ClientName};

    fn call(client: &str, tool: &str, outcome: Outcome, rows: u64) -> NewEntry {
        NewEntry {
            client: ClientName::new(client).unwrap(),
            tool: tool.to_owned(),
            access_level: Some(AccessLevel::Standard),
            outcome,
            purpose: None,
            args_redacted_json: "{}".to_owned(),
            row_count: rows,
            result_hash: (outcome == Outcome::Ok).then(|| "ab".to_owned()),
            withheld_count: 0,
        }
    }

    #[test]
    fn pages_filter_by_client_and_time() {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
        for n in 0..5 {
            append(
                &mut db,
                &call("claude-code", "list_accounts", Outcome::Ok, n),
            )
            .unwrap();
        }
        append(&mut db, &call("codex", "list_accounts", Outcome::Ok, 1)).unwrap();
        let reader = db.reader().unwrap();

        let everything = Query::default();
        assert_eq!(count(&reader, &everything).unwrap(), 6);
        let first = page(&reader, &everything, 4).unwrap();
        assert_eq!(first.len(), 4);
        assert!(first.windows(2).all(|pair| pair[0].id > pair[1].id));
        let rest = page(
            &reader,
            &Query {
                after: Some(Cursor::after(first.last().unwrap())),
                ..Query::default()
            },
            4,
        )
        .unwrap();
        assert_eq!(rest.len(), 2);
        assert!(rest[0].id < first.last().unwrap().id);

        let codex = Query {
            client: Some("codex".to_owned()),
            ..Query::default()
        };
        assert_eq!(count(&reader, &codex).unwrap(), 1);
        assert_eq!(page(&reader, &codex, 10).unwrap()[0].client, "codex");

        let future = Query {
            since: Some("9999-01-01T00:00:00.000Z".to_owned()),
            ..Query::default()
        };
        assert_eq!(count(&reader, &future).unwrap(), 0);
        assert!(page(&reader, &future, 10).unwrap().is_empty());
    }

    #[test]
    fn activity_counts_reads_writes_and_refusals() {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
        append(
            &mut db,
            &call("claude-code", "search_transactions", Outcome::Ok, 37),
        )
        .unwrap();
        append(
            &mut db,
            &call("claude-code", "search_transactions", Outcome::Ok, 3),
        )
        .unwrap();
        append(
            &mut db,
            &call("claude-code", "propose_categorizations", Outcome::Ok, 9),
        )
        .unwrap();
        append(
            &mut db,
            &call("claude-code", "list_accounts", Outcome::RefusedOff, 0),
        )
        .unwrap();
        append(
            &mut db,
            &call("claude-code", "list_accounts", Outcome::Error, 0),
        )
        .unwrap();
        append(
            &mut db,
            &call("my-agent", "list_accounts", Outcome::RefusedNotEnabled, 0),
        )
        .unwrap();
        let reader = db.reader().unwrap();

        let activity = activity(&reader, None, |tool| tool.starts_with("propose_")).unwrap();
        let claude = &activity["claude-code"];
        assert_eq!(claude.calls, 5);
        assert_eq!((claude.reads, claude.read_rows), (2, 40));
        assert_eq!(claude.writes, 1);
        assert_eq!(claude.errors, 1);
        assert_eq!(claude.refused.get(&Outcome::RefusedOff), Some(&1));
        assert_eq!(activity["my-agent"].refused_total(), 1);

        let tomorrow = "9999-01-01T00:00:00.000Z".to_owned();
        assert!(
            super::activity(&reader, Some(&tomorrow), |_| false)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn written_counts_suggestions_and_reports() {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("coffer.db")).unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO suggestion (kind, payload_json, source_client) \
                     VALUES ('categorization', '[1]', 'claude-code'), \
                            ('categorization', '[2]', 'claude-code'); \
                 INSERT INTO report (period, title, body_markdown, source_client) \
                     VALUES ('2026-10', 'October', '', 'claude-code');",
            )
            .unwrap();
        let reader = db.reader().unwrap();
        let yesterday = "2000-01-01T00:00:00.000Z".to_owned();
        assert_eq!(
            written(&reader, &yesterday).unwrap()["claude-code"],
            Written {
                suggestions: 2,
                reports: 1
            }
        );
        let tomorrow = "9999-01-01T00:00:00.000Z".to_owned();
        assert!(written(&reader, &tomorrow).unwrap().is_empty());
    }

    #[test]
    fn arguments_show_text_as_its_length() {
        let lines = arguments(
            r#"{"_undeclared":2,"category":"eating_out","from":"2026-10-01","limit":50,"query":{"length":24,"type":"string"},"tags":[{"length":3,"type":"string"},"gift"]}"#,
        );
        let line = |key: &str| {
            lines
                .iter()
                .find(|line| line.key == key)
                .unwrap()
                .value
                .clone()
        };
        assert_eq!(line("query"), ArgumentValue::Text { length: 24 });
        assert_eq!(line("from"), ArgumentValue::Kept("2026-10-01".into()));
        assert_eq!(line("category"), ArgumentValue::Kept("eating_out".into()));
        assert_eq!(line("limit"), ArgumentValue::Kept("50".into()));
        assert_eq!(
            line("tags"),
            ArgumentValue::Kept("[text, 3 characters, gift]".into())
        );
        assert_eq!(line("other keys"), ArgumentValue::Undeclared(2));
        // The stored order: keys sorted.
        assert_eq!(lines[0].key, "other keys");

        // Keys are client text: cleaned like the CLI's.
        let lines = arguments("{\"a\\u202e\\nb\":1}");
        assert_eq!(lines[0].key, "a b");
        assert!(arguments("{}").is_empty());
        assert_eq!(
            arguments("not json")[0].value,
            ArgumentValue::Kept("not json".into())
        );
    }
}
