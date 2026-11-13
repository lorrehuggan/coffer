//! Access control (ROADMAP M2.11; decision record 2026-11-10, points 1–3):
//! every outcome is logged with its level, the kill switch and the client's
//! row are read on every call, the client hears
//! `notifications/tools/list_changed` when its level changes, and the
//! server never enables a client, changes a level or writes a setting.

mod support;

use std::time::Duration;

use coffer_core::access_log::Outcome;
use coffer_core::db::Database;
use coffer_core::mcp_client::{self, ClientAccess, ClientName};
use coffer_core::setting;
use coffer_mcp::registry::{Call, NoArgs, Registry, Reply, ToolError, ToolSpec, Writes};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::json;
use support::*;

#[derive(Serialize, JsonSchema)]
struct Done {
    ok: bool,
}

const STANDARD_ONLY: ToolSpec = ToolSpec {
    name: "standard_only",
    description: "A test tool for standard clients.",
    level: AccessLevel::Standard,
    writes: Writes::Nothing,
};

const FAILS: ToolSpec = ToolSpec {
    name: "fails",
    description: "A test tool that always fails.",
    level: AccessLevel::Aggregate,
    writes: Writes::Nothing,
};

fn standard_only(_call: &Call<'_>, _args: NoArgs) -> Result<Reply<Done>, ToolError> {
    Ok(Reply::new(Done { ok: true }, 1))
}

fn fails(_call: &Call<'_>, _args: NoArgs) -> Result<Reply<Done>, ToolError> {
    Err(ToolError::Internal("this test tool always fails"))
}

fn registry() -> Registry {
    let mut registry = coffer_mcp::tools::registry();
    registry.add(STANDARD_ONLY, standard_only);
    registry.add(FAILS, fails);
    registry
}

/// The tools [`registry`] lists for `level`.
fn listed_at(level: AccessLevel) -> Vec<String> {
    registry()
        .tools_for(level)
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect()
}

fn name(client: &str) -> ClientName {
    ClientName::new(client).unwrap()
}

/// What the user does in the app or the CLI, on its own connection.
fn user(fixture: &Fixture, change: impl FnOnce(&mut Database)) {
    change(&mut fixture.open());
}

fn switch(fixture: &Fixture, on: bool) {
    user(fixture, |db| setting::set_mcp_enabled(db, on).unwrap());
}

/// `(client, tool, outcome, level)` of every access-log row, oldest first.
fn outcomes(fixture: &Fixture) -> Vec<(String, String, Outcome, Option<AccessLevel>)> {
    fixture
        .access_log()
        .into_iter()
        .map(|call| (call.client, call.tool, call.outcome, call.access_level))
        .collect()
}

fn row(
    client: &str,
    tool: &str,
    outcome: Outcome,
    level: Option<AccessLevel>,
) -> (String, String, Outcome, Option<AccessLevel>) {
    (client.to_owned(), tool.to_owned(), outcome, level)
}

fn names(tools: &[String]) -> Vec<&str> {
    tools.iter().map(String::as_str).collect()
}

#[tokio::test]
async fn every_outcome_is_logged_with_its_level() {
    use AccessLevel::Aggregate;

    let fixture = Fixture::new();
    fixture.set_client("codex", Aggregate);
    let codex = fixture.connect_with("codex", registry()).await;

    codex.call_ok("list_categories", json!({})).await;
    assert_eq!(
        codex.call("standard_only", json!({})).await.is_error,
        Some(true)
    );
    assert_eq!(codex.call("fails", json!({})).await.is_error, Some(true));
    switch(&fixture, false);
    let refused = codex.call("list_categories", json!({})).await;
    assert_eq!(refused.is_error, Some(true));
    assert!(
        serde_json::to_string(&refused)
            .unwrap()
            .contains("switched off")
    );
    switch(&fixture, true);
    user(&fixture, |db| {
        mcp_client::revoke(db, &name("codex")).unwrap()
    });
    assert_eq!(
        codex.call("list_categories", json!({})).await.is_error,
        Some(true)
    );
    user(&fixture, |db| {
        mcp_client::ignore(db, &name("codex")).unwrap()
    });
    assert_eq!(
        codex.call("list_categories", json!({})).await.is_error,
        Some(true)
    );
    codex.close().await;

    // Unknown on its first call, then requested.
    let newbie = fixture.connect_with("newbie", registry()).await;
    for _ in 0..2 {
        assert_eq!(
            newbie.call("list_categories", json!({})).await.is_error,
            Some(true)
        );
    }
    newbie.close().await;

    assert_eq!(
        outcomes(&fixture),
        [
            row("codex", "list_categories", Outcome::Ok, Some(Aggregate)),
            row(
                "codex",
                "standard_only",
                Outcome::RefusedLevel,
                Some(Aggregate)
            ),
            row("codex", "fails", Outcome::Error, Some(Aggregate)),
            // The kill switch refuses an enabled client: its level is kept.
            row(
                "codex",
                "list_categories",
                Outcome::RefusedOff,
                Some(Aggregate)
            ),
            // Revoked, then ignored.
            row("codex", "list_categories", Outcome::RefusedNotEnabled, None),
            row("codex", "list_categories", Outcome::RefusedNotEnabled, None),
            // Unknown, then requested.
            row(
                "newbie",
                "list_categories",
                Outcome::RefusedNotEnabled,
                None
            ),
            row(
                "newbie",
                "list_categories",
                Outcome::RefusedNotEnabled,
                None
            ),
        ]
    );
    for call in fixture.access_log() {
        if call.outcome != Outcome::Ok {
            assert_eq!(call.row_count, 0, "{call:?}");
            assert_eq!(call.result_hash, None, "{call:?}");
        }
    }
}

#[tokio::test]
async fn a_client_that_isnt_enabled_sees_no_tools() {
    let fixture = Fixture::new();
    let client = fixture.connect_with("newbie", registry()).await;
    let reader = fixture.open().reader().unwrap();

    // Listing records the name, once.
    assert!(client.tool_names().await.is_empty());
    assert_eq!(
        mcp_client::access(&reader, &name("newbie")).unwrap(),
        ClientAccess::Requested
    );
    assert!(client.tool_names().await.is_empty());
    assert_eq!(mcp_client::list(&reader).unwrap().len(), 1);

    user(&fixture, |db| {
        mcp_client::ignore(db, &name("newbie")).unwrap()
    });
    assert!(client.tool_names().await.is_empty());
    assert_eq!(
        mcp_client::access(&reader, &name("newbie")).unwrap(),
        ClientAccess::Ignored
    );

    // Enabled in the app: the next list has the aggregate tools.
    user(&fixture, |db| {
        mcp_client::enable(db, &name("newbie")).unwrap();
    });
    assert_eq!(client.tool_names().await, listed_at(AccessLevel::Aggregate));

    user(&fixture, |db| {
        mcp_client::revoke(db, &name("newbie")).unwrap()
    });
    assert!(client.tool_names().await.is_empty());
    client.close().await;
}

#[tokio::test]
async fn a_lower_level_is_announced_and_applies_to_the_next_call() {
    let fixture = Fixture::new();
    fixture.set_client("codex", AccessLevel::Standard);
    let client = fixture.connect_notified("codex", registry(), None).await;
    assert!(
        client
            .tool_names()
            .await
            .contains(&"standard_only".to_owned())
    );

    user(&fixture, |db| {
        mcp_client::set_level(db, &name("codex"), AccessLevel::Aggregate).unwrap();
    });
    let result = client.call("standard_only", json!({})).await;
    assert_eq!(result.is_error, Some(true));
    client.wait_for_list_changes(1).await;
    assert_eq!(client.tool_names().await, listed_at(AccessLevel::Aggregate));

    // One change, one notification: the next calls at the same level
    // don't repeat it.
    client.call("list_categories", json!({})).await;
    client.call("list_categories", json!({})).await;
    client.tool_names().await;
    assert_eq!(client.heard(), 1);

    let log = outcomes(&fixture);
    assert_eq!(
        log[0],
        row(
            "codex",
            "standard_only",
            Outcome::RefusedLevel,
            Some(AccessLevel::Aggregate)
        )
    );
    client.close().await;
}

#[tokio::test]
async fn the_kill_switch_refuses_a_connected_client_until_it_is_back_on() {
    let fixture = Fixture::new();
    fixture.set_client("codex", AccessLevel::Aggregate);
    let client = fixture.connect_notified("codex", registry(), None).await;
    let aggregate = listed_at(AccessLevel::Aggregate);
    assert_eq!(client.tool_names().await, aggregate);

    switch(&fixture, false);
    let result = client.call("list_categories", json!({})).await;
    assert_eq!(result.is_error, Some(true));
    client.wait_for_list_changes(1).await;
    assert!(client.tool_names().await.is_empty());

    switch(&fixture, true);
    let result = client.call("list_categories", json!({})).await;
    assert_ne!(result.is_error, Some(true));
    client.wait_for_list_changes(2).await;
    assert_eq!(client.tool_names().await, aggregate);

    assert_eq!(
        outcomes(&fixture),
        [
            row(
                "codex",
                "list_categories",
                Outcome::RefusedOff,
                Some(AccessLevel::Aggregate)
            ),
            row(
                "codex",
                "list_categories",
                Outcome::Ok,
                Some(AccessLevel::Aggregate)
            ),
        ]
    );
    client.close().await;
}

#[tokio::test]
async fn a_raised_level_takes_effect_without_a_restart() {
    let fixture = Fixture::new();
    fixture.set_client("codex", AccessLevel::Aggregate);
    let client = fixture.connect_notified("codex", registry(), None).await;
    assert!(
        !client
            .tool_names()
            .await
            .contains(&"standard_only".to_owned())
    );
    assert_eq!(
        client.call("standard_only", json!({})).await.is_error,
        Some(true)
    );
    assert_eq!(client.heard(), 0);

    user(&fixture, |db| {
        mcp_client::set_level(db, &name("codex"), AccessLevel::Standard).unwrap();
    });
    let result = client.call("standard_only", json!({})).await;
    assert_ne!(result.is_error, Some(true));
    client.wait_for_list_changes(1).await;
    let tools = client.tool_names().await;
    assert!(names(&tools).contains(&"standard_only"));

    assert_eq!(
        outcomes(&fixture),
        [
            row(
                "codex",
                "standard_only",
                Outcome::RefusedLevel,
                Some(AccessLevel::Aggregate)
            ),
            row(
                "codex",
                "standard_only",
                Outcome::Ok,
                Some(AccessLevel::Standard)
            ),
        ]
    );
    client.close().await;
}

#[tokio::test]
async fn an_idle_client_hears_about_a_revoke() {
    let fixture = Fixture::new();
    fixture.set_client("codex", AccessLevel::Pseudonymous);
    let client = fixture
        .connect_notified("codex", registry(), Some(Duration::from_millis(20)))
        .await;
    assert_eq!(
        client.tool_names().await,
        listed_at(AccessLevel::Pseudonymous)
    );

    // No call: the server's own check finds the change.
    user(&fixture, |db| {
        mcp_client::revoke(db, &name("codex")).unwrap()
    });
    client.wait_for_list_changes(1).await;
    assert!(client.tool_names().await.is_empty());

    // And the kill switch, once enabled again.
    user(&fixture, |db| {
        mcp_client::enable(db, &name("codex")).unwrap();
    });
    client.wait_for_list_changes(2).await;
    assert_eq!(client.tool_names().await, listed_at(AccessLevel::Aggregate));
    switch(&fixture, false);
    client.wait_for_list_changes(3).await;
    assert!(client.tool_names().await.is_empty());

    // Checking writes nothing.
    assert!(fixture.access_log().is_empty());
    client.close().await;
}

/// `(name, status, enabled, access_level)` of a client.
type ClientRow = (String, String, bool, String);

/// `mcp_client` rows and settings, as the server must leave them.
fn snapshot(fixture: &Fixture) -> (Vec<ClientRow>, Vec<(String, String)>) {
    let conn = rusqlite::Connection::open(fixture.path()).unwrap();
    let clients = conn
        .prepare("SELECT name, status, enabled, access_level FROM mcp_client ORDER BY name")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let settings = conn
        .prepare("SELECT key, value FROM setting ORDER BY key")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    (clients, settings)
}

#[tokio::test]
async fn the_server_never_changes_a_client_or_a_setting() {
    let fixture = Fixture::new();
    fixture.set_client("enabled", AccessLevel::Aggregate);
    fixture.set_client("revoked", AccessLevel::Standard);
    fixture.set_client("ignored", AccessLevel::Standard);
    user(&fixture, |db| {
        mcp_client::revoke(db, &name("revoked")).unwrap();
        mcp_client::ignore(db, &name("ignored")).unwrap();
        mcp_client::record_requested(db, &name("requested")).unwrap();
    });

    let calls = async |fixture: &Fixture| {
        for client in ["enabled", "revoked", "ignored", "requested", "unknown"] {
            let connected = fixture.connect_with(client, registry()).await;
            connected.tool_names().await;
            for tool in ["list_categories", "standard_only", "fails"] {
                connected.call(tool, json!({})).await;
            }
            connected.close().await;
        }
    };

    let (mut clients, settings) = snapshot(&fixture);
    calls(&fixture).await;
    // The one write: the unknown name, recorded as requested.
    clients.push((
        "unknown".to_owned(),
        "requested".to_owned(),
        false,
        "aggregate".to_owned(),
    ));
    clients.sort();
    assert_eq!(snapshot(&fixture), (clients, settings));

    // With the kill switch off, nothing changes either.
    switch(&fixture, false);
    let before = snapshot(&fixture);
    calls(&fixture).await;
    assert_eq!(snapshot(&fixture), before);
    assert!(
        fixture
            .access_log()
            .iter()
            .rev()
            .take(15)
            .all(|call| call.outcome == Outcome::RefusedOff)
    );
}
