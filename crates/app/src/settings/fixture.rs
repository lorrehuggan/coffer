//! Sample Settings data, as the Paper artboards draw it (`AQ2-0`, `DC6-0`,
//! `F63-0`; `DESIGN.md` → "Settings": Claude Code at standard access, the
//! November Monzo import, 1,312 transactions), for snapshots and tests.
//! Times are local and equal to UTC; schema versions are this build's, so
//! "same as this version" reads as drawn.

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{NaiveDate, NaiveDateTime, TimeZone as _, Utc};
use coffer_core::access_log::activity::{ClientActivity, Written};
use coffer_core::access_log::{LoggedCall, Outcome};
use coffer_core::backup::detail::{Contents, Detail, ImportSince, Since};
use coffer_core::backup::{self, Backup, Reason};
use coffer_core::db::Database;
use coffer_core::export::Counts;
use coffer_core::id::AccountId;
use coffer_core::mcp_client::{AccessLevel, ClientName};

use super::data::{
    AiData, BackupRow, BackupsData, ClientRow, Clock, ExportChoices, LogRow, RequestRow,
};

/// The data file's path in the panel's foot (Paper `BRB-0`).
pub const DATA_FILE: &str = "~/.local/share/coffer/coffer.db";

/// Tuesday 10 November 2026, 10:42:07.
pub fn clock() -> Clock {
    Clock {
        now: Utc.with_ymd_and_hms(2026, 11, 10, 10, 42, 7).unwrap(),
        today: NaiveDate::from_ymd_opt(2026, 11, 10).unwrap(),
    }
}

fn at(month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, month, day)
        .unwrap()
        .and_hms_opt(hour, minute, 0)
        .unwrap()
}

fn name(name: &str) -> ClientName {
    ClientName::new(name).unwrap()
}

/// Settings · AI clients (`AQ2-0`).
pub fn ai() -> AiData {
    AiData {
        on: true,
        clients: vec![
            ClientRow {
                name: name("claude-code"),
                enabled: true,
                level: AccessLevel::Standard,
                last_seen: Some(at(11, 10, 10, 42)),
                week: ClientActivity {
                    calls: 214,
                    reads: 199,
                    read_rows: 1_906,
                    writes: 13,
                    refused: BTreeMap::from([(Outcome::RefusedOff, 2)]),
                    errors: 0,
                },
                written: Written {
                    suggestions: 12,
                    reports: 1,
                },
            },
            ClientRow {
                name: name("codex"),
                enabled: true,
                level: AccessLevel::Pseudonymous,
                last_seen: Some(at(11, 3, 21, 40)),
                week: ClientActivity {
                    calls: 12,
                    reads: 12,
                    read_rows: 73,
                    ..ClientActivity::default()
                },
                written: Written::default(),
            },
            ClientRow {
                name: name("claude-desktop"),
                enabled: false,
                level: AccessLevel::Aggregate,
                last_seen: Some(at(10, 28, 8, 15)),
                week: ClientActivity::default(),
                written: Written::default(),
            },
        ],
        requests: vec![RequestRow {
            name: name("my-agent"),
            last_tried: Some(at(11, 10, 10, 41)),
            refused: 3,
        }],
        calls_week: 226,
        log_clients: vec!["claude-code".into(), "codex".into(), "my-agent".into()],
    }
}

/// A call in [`log`]: id, time, client, tool, outcome, purpose, rows and
/// redacted arguments.
type LogFixture = (
    i64,
    NaiveDateTime,
    &'static str,
    &'static str,
    Outcome,
    Option<&'static str>,
    u64,
    &'static str,
);

/// Settings · access log (`DC6-0`): today's six calls and Monday's four.
pub fn log() -> Vec<LogRow> {
    let rows: [LogFixture; 10] = [
        (
            10,
            at(11, 10, 10, 42),
            "claude-code",
            "search_transactions",
            Outcome::Ok,
            Some("Find takeaway spending since October"),
            37,
            r#"{"category":"eating_out","from":"2026-10-01","limit":50,"query":{"length":24,"type":"string"}}"#,
        ),
        (
            9,
            at(11, 10, 10, 42),
            "claude-code",
            "spending_by_category",
            Outcome::Ok,
            Some("Compare October and November by category"),
            14,
            r#"{"from":"2026-10-01","to":"2026-11-30"}"#,
        ),
        (
            8,
            at(11, 10, 10, 41),
            "my-agent",
            "list_accounts",
            Outcome::RefusedNotEnabled,
            None,
            0,
            "{}",
        ),
        (
            7,
            at(11, 10, 10, 38),
            "claude-code",
            "propose_categorizations",
            Outcome::Ok,
            Some("Suggest categories for the uncategorised rows"),
            9,
            "{}",
        ),
        (
            6,
            at(11, 10, 10, 36),
            "claude-code",
            "uncategorized_transactions",
            Outcome::Ok,
            Some("Find what still needs a category"),
            9,
            "{}",
        ),
        (
            5,
            at(11, 10, 9, 12),
            "claude-code",
            "monthly_summary",
            Outcome::Ok,
            None,
            2,
            "{}",
        ),
        (
            4,
            at(11, 9, 21, 12),
            "codex",
            "search_transactions",
            Outcome::Ok,
            Some("Grocery spending this month"),
            42,
            "{}",
        ),
        (
            3,
            at(11, 9, 21, 11),
            "codex",
            "list_categories",
            Outcome::Ok,
            Some("Check the category names"),
            31,
            "{}",
        ),
        (
            2,
            at(11, 9, 9, 14),
            "claude-code",
            "save_report",
            Outcome::Ok,
            Some("Save the October review"),
            1,
            "{}",
        ),
        (
            1,
            at(11, 9, 9, 5),
            "claude-code",
            "net_worth_history",
            Outcome::Ok,
            Some("October review: net worth over the year"),
            12,
            "{}",
        ),
    ];
    rows.into_iter()
        .map(
            |(id, local, client, tool, outcome, purpose, rows, args)| LogRow {
                call: LoggedCall {
                    id,
                    at: format!("{}Z", local.format("%Y-%m-%dT%H:%M:%S%.3f")),
                    client: client.into(),
                    tool: tool.into(),
                    access_level: (outcome == Outcome::Ok).then_some(AccessLevel::Standard),
                    outcome,
                    purpose: purpose.map(str::to_owned),
                    args_redacted_json: args.into(),
                    row_count: rows,
                    result_hash: None,
                    withheld_count: 0,
                    summarised_calls: None,
                },
                local,
                writes: super::store::writes(tool),
            },
        )
        .collect()
}

/// Settings · backups (`F63-0`): ten recent and three monthly.
pub fn backups() -> BackupsData {
    let schema = Database::supported_schema_version();
    let import = |file: &str| Detail::Import {
        file: Some(file.into()),
    };
    let rows: [(NaiveDateTime, Reason, Detail, u64, u64); 13] = [
        (
            at(11, 10, 14, 2),
            Reason::Import,
            import("monzo-november.csv"),
            1_016,
            42,
        ),
        (at(11, 9, 9, 0), Reason::Manual, Detail::Manual, 1_016, 42),
        (
            at(11, 5, 21, 17),
            Reason::Import,
            import("savings-2026-10.ofx"),
            1_014,
            42,
        ),
        (
            at(11, 2, 19, 30),
            Reason::Import,
            import("monzo-october.csv"),
            742,
            31,
        ),
        (
            at(11, 1, 8, 58),
            Reason::Migration,
            Detail::Migration {
                from: Some(schema - 1),
                to: Some(schema),
            },
            742,
            31,
        ),
        (
            at(10, 31, 19, 40),
            Reason::Import,
            import("amex-october.csv"),
            721,
            30,
        ),
        (
            at(10, 30, 12, 5),
            Reason::Import,
            import("joint-october.csv"),
            668,
            28,
        ),
        (
            at(10, 12, 10, 22),
            Reason::Restore,
            Detail::Restore,
            655,
            28,
        ),
        (
            at(10, 5, 21, 2),
            Reason::Import,
            import("savings-2026-09.ofx"),
            655,
            28,
        ),
        (
            at(10, 4, 18, 1),
            Reason::Import,
            import("monzo-september.csv"),
            652,
            27,
        ),
        (
            at(9, 30, 22, 10),
            Reason::Import,
            import("joint-september.csv"),
            610,
            26,
        ),
        (
            at(8, 31, 20, 44),
            Reason::Import,
            import("monzo-august.csv"),
            402,
            19,
        ),
        (
            at(8, 1, 10, 15),
            Reason::Import,
            import("monzo-july.csv"),
            0,
            1,
        ),
    ];
    let rows = rows
        .into_iter()
        .enumerate()
        .map(|(index, (local, reason, detail, transactions, tenths))| {
            let taken_at = local.and_utc();
            BackupRow {
                backup: Backup {
                    path: PathBuf::from(format!(
                        "~/.local/share/coffer/backups/{}",
                        backup::file_name(reason, taken_at)
                    )),
                    reason,
                    taken_at,
                    size: tenths * 1024 * 1024 / 10,
                },
                local,
                contents: Some(Contents {
                    schema_version: if index < 4 { schema } else { schema - 1 },
                    transactions,
                    accounts: 6,
                    rules: 14,
                }),
                detail,
                monthly: index >= 10,
            }
        })
        .collect();
    BackupsData {
        rows,
        supported: schema,
    }
}

/// What restoring the 10 Nov backup would undo (`FY2-0`).
pub fn since() -> Since {
    Since {
        imports: vec![ImportSince {
            file: "monzo-november.csv".into(),
            rows: 296,
        }],
        edits: 4,
        suggestions: 12,
    }
}

/// The export dialog's choices (`HAG-0`).
pub fn export_choices() -> ExportChoices {
    ExportChoices {
        accounts: vec![
            (AccountId::new(1), "Monzo".into()),
            (AccountId::new(2), "Joint".into()),
            (AccountId::new(3), "Amex".into()),
        ],
        categories: vec![
            ("food".into(), "Food".into(), true),
            ("food.groceries".into(), "Groceries".into(), false),
            ("food.eating_out".into(), "Eating out".into(), false),
        ],
        tags: vec![("Holiday".into(), false)],
        span: Some((
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 11, 10).unwrap(),
        )),
        today: Some(clock().today),
    }
}

/// The export dialog's counts (`HDD-0`).
pub fn export_counts() -> Counts {
    Counts {
        transactions: 1_312,
        rows: 1_327,
    }
}
