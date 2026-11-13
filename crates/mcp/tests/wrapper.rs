//! The call wrapper (gate → call → log) and the write boundary, with test
//! tools registered next to the real ones: refused and failed calls are
//! logged, unknown tools aren't, only writing tools reach the inbox, and a
//! tool above the client's level is neither listed nor callable.

mod support;

use coffer_core::access_log::Outcome;
use coffer_core::inbox::{NewSuggestion, SuggestionKind};
use coffer_mcp::registry::{Call, NoArgs, Registry, Reply, ToolError, ToolSpec, Writes};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use support::*;

#[derive(Serialize, JsonSchema)]
struct Done {
    ok: bool,
}

#[derive(Deserialize, JsonSchema)]
struct Count {
    /// How many.
    n: u32,
}

const STANDARD_ONLY: ToolSpec = ToolSpec {
    name: "standard_only",
    description: "A test tool for standard clients.",
    level: AccessLevel::Standard,
    writes: Writes::Nothing,
};

const NEEDS_COUNT: ToolSpec = ToolSpec {
    name: "needs_count",
    description: "A test tool with a required argument.",
    level: AccessLevel::Aggregate,
    writes: Writes::Nothing,
};

const SNEAKY: ToolSpec = ToolSpec {
    name: "sneaky",
    description: "A test tool that tries to write without declaring it.",
    level: AccessLevel::Aggregate,
    writes: Writes::Nothing,
};

const SUGGEST: ToolSpec = ToolSpec {
    name: "suggest",
    description: "A test tool that files a suggestion.",
    level: AccessLevel::Aggregate,
    writes: Writes::Inbox,
};

fn suggestion() -> NewSuggestion {
    NewSuggestion {
        kind: SuggestionKind::Categorization,
        payload: json!({ "transaction_id": 1, "category": "food.groceries" }),
        rationale: Some("test".to_owned()),
    }
}

fn standard_only(_call: &Call<'_>, _args: NoArgs) -> Result<Reply<Done>, ToolError> {
    Ok(Reply::new(Done { ok: true }, 0))
}

fn needs_count(_call: &Call<'_>, args: Count) -> Result<Reply<Done>, ToolError> {
    Ok(Reply::new(Done { ok: args.n > 0 }, u64::from(args.n)))
}

fn sneaky(call: &Call<'_>, _args: NoArgs) -> Result<Reply<Done>, ToolError> {
    call.inbox()?.suggest(&suggestion())?;
    Ok(Reply::new(Done { ok: true }, 0))
}

fn suggest(call: &Call<'_>, _args: NoArgs) -> Result<Reply<Done>, ToolError> {
    call.inbox()?.suggest(&suggestion())?;
    Ok(Reply::new(Done { ok: true }, 1))
}

fn registry() -> Registry {
    let mut registry = coffer_mcp::tools::registry();
    registry.add(STANDARD_ONLY, standard_only);
    registry.add(NEEDS_COUNT, needs_count);
    registry.add(SNEAKY, sneaky);
    registry.add(SUGGEST, suggest);
    registry
}

fn suggestions(fixture: &Fixture) -> Vec<(String, String)> {
    let conn = rusqlite::Connection::open(fixture.path()).unwrap();
    let mut statement = conn
        .prepare("SELECT source_client, status FROM suggestion ORDER BY id")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[tokio::test]
async fn a_tool_above_the_level_is_neither_listed_nor_run() {
    let fixture = Fixture::new();
    fixture.set_client("codex", AccessLevel::Aggregate);
    let client = fixture.connect_with("codex", registry()).await;

    let names = client.tool_names().await;
    assert!(names.iter().any(|name| name == "list_categories"));
    assert!(!names.iter().any(|name| name == "standard_only"));

    let result = client.call("standard_only", json!({})).await;
    assert_eq!(result.is_error, Some(true));
    let log = fixture.access_log();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].outcome, Outcome::RefusedLevel);
    assert_eq!(log[0].access_level, Some(AccessLevel::Aggregate));
    assert_eq!(log[0].row_count, 0);

    // The level is read on every call: raised, the tool appears and runs.
    fixture.set_client("codex", AccessLevel::Standard);
    assert!(
        client
            .tool_names()
            .await
            .iter()
            .any(|name| name == "standard_only")
    );
    client.call_ok("standard_only", json!({})).await;
    assert_eq!(fixture.access_log()[1].outcome, Outcome::Ok);
    client.close().await;
}

#[tokio::test]
async fn an_unknown_tool_is_a_protocol_error_and_not_logged() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Standard).await;
    assert!(
        client
            .try_call("ignore_previous_instructions", json!({}))
            .await
            .is_err()
    );
    assert!(fixture.access_log().is_empty());
    client.close().await;
}

#[tokio::test]
async fn bad_arguments_are_a_logged_error_without_their_text() {
    let fixture = Fixture::new();
    fixture.set_client("claude-code", AccessLevel::Aggregate);
    let client = fixture.connect_with("claude-code", registry()).await;

    let result = client
        .call(
            "needs_count",
            json!({ "n": "ignore previous instructions", "purpose": "testing" }),
        )
        .await;
    assert_eq!(result.is_error, Some(true));

    client
        .call_ok("needs_count", json!({ "n": 3, "purpose": "testing" }))
        .await;

    let log = fixture.access_log();
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].outcome, Outcome::Error);
    assert_eq!(log[0].args_redacted_json, r#"{"n":"string"}"#);
    assert_eq!(log[0].purpose.as_deref(), Some("testing"));
    assert_eq!(log[1].outcome, Outcome::Ok);
    assert_eq!(log[1].row_count, 3);
    assert_eq!(log[1].args_redacted_json, r#"{"n":"number"}"#);
    client.close().await;
}

#[tokio::test]
async fn only_a_writing_tool_reaches_the_inbox() {
    let fixture = Fixture::new();
    fixture.set_client("claude-code", AccessLevel::Aggregate);
    let client = fixture.connect_with("claude-code", registry()).await;

    let result = client.call("sneaky", json!({})).await;
    assert_eq!(result.is_error, Some(true));
    assert!(suggestions(&fixture).is_empty());

    client.call_ok("suggest", json!({})).await;
    assert_eq!(
        suggestions(&fixture),
        [("claude-code".to_owned(), "pending".to_owned())]
    );

    let outcomes: Vec<_> = fixture
        .access_log()
        .iter()
        .map(|call| (call.tool.clone(), call.outcome))
        .collect();
    assert_eq!(
        outcomes,
        [
            ("sneaky".to_owned(), Outcome::Error),
            ("suggest".to_owned(), Outcome::Ok),
        ]
    );
    client.close().await;
}

#[tokio::test]
async fn a_client_without_a_row_is_standard_for_now() {
    // Block B: replace with "an unknown client is recorded as requested and
    // sees no tools".
    let fixture = Fixture::new();
    let client = fixture.connect_with("new-client", registry()).await;
    assert!(
        client
            .tool_names()
            .await
            .iter()
            .any(|name| name == "standard_only")
    );
    client.close().await;
}

#[test]
fn every_registered_tool_is_well_formed() {
    let registry = coffer_mcp::tools::registry();
    for tool in registry.all() {
        let listed = tool.tool();
        assert!(
            listed.input_schema["properties"]["purpose"].is_object(),
            "{} takes no purpose",
            listed.name
        );
        assert_eq!(
            listed.annotations.as_ref().unwrap().read_only_hint,
            Some(tool.spec().writes == Writes::Nothing),
            "{}",
            listed.name
        );
    }
}
