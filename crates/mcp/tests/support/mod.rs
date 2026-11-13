//! The MCP test harness (ROADMAP M2.9): a fixture database built through
//! the real import path, the server run in-process with an rmcp client over
//! an in-memory duplex, and the checks every tool's tests use.
//!
//! - [`Fixture::new`] imports the synthetic Monzo export and an OFX
//!   statement into a fresh database, then seeds the personal data no tool
//!   may return: a user note, a private tag, a share label, and a manual
//!   row whose bank text names a person, a sort code, an account number and
//!   an IBAN ([`PII`]).
//! - [`Fixture::connect`] starts a server for a client at a level and
//!   returns a [`TestClient`] to call tools through.
//! - [`assert_no_pii`] fails on any [`PII`] string in a result.
//! - [`registered_tools`] lists the tools registered for a level;
//!   [`TestClient::tool_names`] lists what the client sees over the wire.
//! - [`Fixture::access_log`] reads back the access log.
//!
//! Every database lives under one scratch data directory, and
//! `COFFER_DATA_DIR` points there for the whole test process, so nothing
//! can reach the real data dir.
//!
//! Each test binary uses a different subset, hence `dead_code`.
#![allow(dead_code)]

#[path = "../../../import/tests/common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use chrono::NaiveDate;
use coffer_core::access_log::{self, LoggedCall};
use coffer_core::account::{self, AccountKind, NewAccount};
use coffer_core::db::Database;
use coffer_core::id::{AccountId, TransactionId};
use coffer_core::import;
use coffer_core::link::{self, ShareLabel, Sharing};
use coffer_core::manual::{self, ManualTransaction};
use coffer_core::money::{Currency, Money};
use coffer_core::paths::{DATA_DIR_ENV, DATABASE_FILE};
use coffer_core::statement::Statement;
use coffer_core::tag::{self, TagName};
use coffer_core::{mcp_client::ClientName, note};
use coffer_import::AnyImporter;
use coffer_import::preset::Presets;
use coffer_mcp::Server;
use coffer_mcp::registry::Registry;
use rmcp::model::{CallToolRequestParams, CallToolResult, Tool};
use rmcp::service::{RunningService, ServiceError};
use rmcp::{RoleClient, ServiceExt};
use serde_json::Value;
use tempfile::TempDir;

pub use coffer_core::mcp_client::AccessLevel;

/// Every access level, lowest first.
pub const LEVELS: &[AccessLevel] = AccessLevel::ALL;

/// The account holder named in the seeded bank text.
pub const HOLDER_NAME: &str = "Robin Fixture-Holder";
/// The sort code in the seeded bank text, and its digits.
pub const SORT_CODE: &str = "77-41-29";
pub const SORT_CODE_DIGITS: &str = "774129";
/// The account number in the seeded bank text.
pub const ACCOUNT_NUMBER: &str = "58316402";
/// The IBAN in the seeded bank text (spaces are ignored when matching).
pub const IBAN: &str = "GB94BARC10201530093459";
/// The manual row's bank text: a transfer naming the payee's details.
pub const PII_DESCRIPTION: &str =
    "TRANSFER TO ROBIN FIXTURE-HOLDER 77-41-29 58316402 IBAN GB94 BARC 1020 1530 0934 59";
/// The user's own note on an imported row.
pub const USER_NOTE: &str = "Birthday present for Sam Notefixture";
/// A private tag on an imported row.
pub const PRIVATE_TAG: &str = "therapy-fixture";
/// The share label on the manual row: who owes half.
pub const SHARE_LABEL: &str = "Housemate Sharefixture";

/// Strings no tool output may contain, compared ignoring case (and, for
/// the IBAN, spaces). Imported notes and merchant addresses from
/// `fixtures/presets/uk/monzo.csv` are here too: the preset doesn't store
/// addresses, and notes are never exposed.
pub const PII: &[&str] = &[
    HOLDER_NAME,
    SORT_CODE,
    SORT_CODE_DIGITS,
    ACCOUNT_NUMBER,
    IBAN,
    USER_NOTE,
    PRIVATE_TAG,
    SHARE_LABEL,
    // Imported notes (Monzo "Notes and #tags").
    "Team dinner",
    "the usual",
    // Merchant addresses (Monzo "Address").
    "1 Fake Street",
    "3 Fake Lane",
    "9 Rue Imaginaire",
];

/// Fails if any string in `value` (keys included) contains a [`PII`]
/// string, ignoring case, spaces and, for the sort code, separators.
pub fn assert_no_pii(value: &Value) {
    let mut found = Vec::new();
    visit_strings(value, &mut |text| {
        let folded = fold(text);
        let digits: String = text.chars().filter(char::is_ascii_digit).collect();
        for needle in PII {
            if folded.contains(&fold(needle)) {
                found.push(format!("{needle:?} in {text:?}"));
            }
        }
        for needle in [SORT_CODE_DIGITS, ACCOUNT_NUMBER] {
            if digits.contains(needle) && !folded.contains(&fold(needle)) {
                found.push(format!("{needle:?} (digits) in {text:?}"));
            }
        }
    });
    assert!(found.is_empty(), "personal data in tool output: {found:#?}");
}

/// Lowercase, without whitespace.
fn fold(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

fn visit_strings(value: &Value, visit: &mut impl FnMut(&str)) {
    match value {
        Value::String(text) => visit(text),
        Value::Array(items) => items.iter().for_each(|item| visit_strings(item, visit)),
        Value::Object(map) => {
            for (key, item) in map {
                visit(key);
                visit_strings(item, visit);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// The names of the tools registered for `level`, straight from the
/// registry.
pub fn registered_tools(level: AccessLevel) -> Vec<String> {
    coffer_mcp::tools::registry()
        .tools_for(level)
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect()
}

/// The scratch data directory every fixture database lives in. Creating it
/// points `COFFER_DATA_DIR` at it for the rest of the process.
static DATA_DIR: LazyLock<TempDir> = LazyLock::new(|| {
    let dir = TempDir::new().unwrap();
    // SAFETY: set once, the first time any test builds a fixture, before
    // that test starts a server. No test reads the environment on another
    // thread while it changes: every test goes through here first, and
    // `LazyLock` makes the others wait.
    unsafe { std::env::set_var(DATA_DIR_ENV, dir.path()) };
    dir
});

/// Accounts and rows of the fixture database.
#[derive(Debug, Clone, Copy)]
pub struct Seeded {
    /// The Monzo current account (`fixtures/presets/uk/monzo.csv`).
    pub monzo: AccountId,
    /// The bank account (`fixtures/ofx/bank-sgml.ofx`), with the manual row.
    pub bank: AccountId,
    /// The imported row with the user note and the private tag.
    pub noted: TransactionId,
    /// The manual row whose bank text holds account details, shared with
    /// [`SHARE_LABEL`].
    pub manual: TransactionId,
}

/// A fixture database in its own directory under the scratch data dir.
pub struct Fixture {
    _dir: TempDir,
    path: PathBuf,
    pub seeded: Seeded,
}

impl Fixture {
    /// Imports the fixtures and seeds the personal data.
    pub fn new() -> Fixture {
        let dir = tempfile::tempdir_in(DATA_DIR.path()).unwrap();
        let path = dir.path().join(DATABASE_FILE);
        let mut db = Database::open(&path).unwrap();

        let monzo = account::create(
            &mut db,
            &NewAccount::new("Everyday", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        import_file(&mut db, monzo, "monzo.csv", monzo_statement());
        let bank = account::create(
            &mut db,
            &NewAccount::new("Bills", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        import_file(
            &mut db,
            bank,
            "bank-sgml.ofx",
            common::read_ofx("bank-sgml.ofx"),
        );

        let noted = first_imported(&path, monzo);
        note::set(&mut db, noted, USER_NOTE).unwrap();
        let private = tag::create(&mut db, &TagName::new(PRIVATE_TAG).unwrap(), true).unwrap();
        tag::add_to_transaction(&mut db, noted, private).unwrap();

        let manual = manual::create(
            &mut db,
            &ManualTransaction::new(
                bank,
                NaiveDate::from_ymd_opt(2026, 11, 20).unwrap(),
                Money::new(-4500, Currency::GBP),
                PII_DESCRIPTION,
            ),
        )
        .unwrap();
        link::set_sharing(
            &mut db,
            manual,
            &Sharing::Shared {
                my_share: Money::new(-2250, Currency::GBP),
                label: ShareLabel::new(SHARE_LABEL).unwrap(),
            },
        )
        .unwrap();

        Fixture {
            _dir: dir,
            path,
            seeded: Seeded {
                monzo,
                bank,
                noted,
                manual,
            },
        }
    }

    /// The database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Opens the database as `coffer mcp` does: existing, never migrated.
    pub fn open(&self) -> Database {
        Database::open_existing(&self.path).unwrap()
    }

    /// Sets `client`'s row in `mcp_client`: enabled at `level`.
    pub fn set_client(&self, client: &str, level: AccessLevel) {
        // Block B: use core's client API once it exists.
        let conn = rusqlite::Connection::open(&self.path).unwrap();
        conn.execute(
            "INSERT INTO mcp_client (name, enabled, access_level, status) \
             VALUES (?1, 1, ?2, 'known') \
             ON CONFLICT (name) DO UPDATE SET access_level = excluded.access_level",
            rusqlite::params![client, level.as_str()],
        )
        .unwrap();
    }

    /// Starts a server for `client`, enabled at `level`, and connects to it.
    pub async fn connect(&self, client: &str, level: AccessLevel) -> TestClient {
        self.set_client(client, level);
        self.connect_with(client, coffer_mcp::tools::registry())
            .await
    }

    /// Starts a server for `client` with the tools in `registry`, without
    /// touching `mcp_client`, and connects to it.
    pub async fn connect_with(&self, client: &str, registry: Registry) -> TestClient {
        let server =
            Server::with_registry(self.open(), ClientName::new(client).unwrap(), registry).unwrap();
        let (server_io, client_io) = tokio::io::duplex(64 * 1024);
        let server = tokio::spawn(async move {
            let running = server.serve(server_io).await.unwrap();
            let _ = running.waiting().await;
        });
        let service = ().serve(client_io).await.unwrap();
        TestClient { service, server }
    }

    /// Every access-log row, oldest first.
    pub fn access_log(&self) -> Vec<LoggedCall> {
        let reader = self.open().reader().unwrap();
        let mut calls = access_log::recent(&reader, u32::MAX).unwrap();
        calls.reverse();
        calls
    }
}

fn monzo_statement() -> Statement {
    let presets = Presets::bundled().unwrap();
    presets
        .get("uk-monzo")
        .expect("bundled")
        .importer()
        .import(&common::fixture("presets/uk/monzo.csv"))
        .unwrap()
}

fn import_file(db: &mut Database, account: AccountId, file: &str, statement: Statement) {
    let preview = import::preview(db, account, file, statement).unwrap();
    let confirm = preview.needs_confirmation();
    import::commit(db, &preview, confirm).unwrap();
}

/// The account's first imported row.
fn first_imported(path: &Path, account: AccountId) -> TransactionId {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.query_row(
        "SELECT id FROM \"transaction\" WHERE account_id = ?1 AND origin = 'imported' \
         ORDER BY id LIMIT 1",
        [account.get()],
        |row| row.get(0),
    )
    .map(TransactionId::new)
    .unwrap()
}

/// An MCP client connected to an in-process server.
pub struct TestClient {
    service: RunningService<RoleClient, ()>,
    server: tokio::task::JoinHandle<()>,
}

impl TestClient {
    /// Calls `tool` with `args` (a JSON object).
    pub async fn try_call(&self, tool: &str, args: Value) -> Result<CallToolResult, ServiceError> {
        let Value::Object(args) = args else {
            panic!("arguments are a JSON object");
        };
        self.service
            .call_tool(CallToolRequestParams::new(tool.to_owned()).with_arguments(args))
            .await
    }

    /// Calls `tool`, expecting a result (which may be a tool error).
    pub async fn call(&self, tool: &str, args: Value) -> CallToolResult {
        self.try_call(tool, args).await.unwrap()
    }

    /// Calls `tool`, expecting success, and returns its structured result.
    /// The whole response, text included, must be free of [`PII`].
    pub async fn call_ok(&self, tool: &str, args: Value) -> Value {
        let result = self.call(tool, args).await;
        assert_ne!(result.is_error, Some(true), "{tool} failed: {result:?}");
        assert_no_pii(&serde_json::to_value(&result).unwrap());
        result
            .structured_content
            .unwrap_or_else(|| panic!("{tool} returned no structured content"))
    }

    /// The tools this client sees.
    pub async fn tools(&self) -> Vec<Tool> {
        self.service.list_all_tools().await.unwrap()
    }

    /// The names of the tools this client sees.
    pub async fn tool_names(&self) -> Vec<String> {
        self.tools()
            .await
            .into_iter()
            .map(|tool| tool.name.into_owned())
            .collect()
    }

    /// Disconnects and waits for the server to stop.
    pub async fn close(self) {
        self.service.cancel().await.unwrap();
        self.server.await.unwrap();
    }
}
