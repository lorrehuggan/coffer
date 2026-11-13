//! `list_categories` end to end: the pattern every tool's tests copy.
//!
//! Every tool needs at least these three checks:
//!
//! 1. it is registered at exactly the levels at or above its own;
//! 2. its output passes `assert_no_pii` on the fixture database;
//! 3. one call writes exactly one access-log row, with the tool, client,
//!    level, outcome, row count and purpose.

mod support;

use coffer_core::access_log::Outcome;
use serde_json::json;
use support::*;

const TOOL: &str = "list_categories";

#[tokio::test]
async fn is_registered_at_every_level() {
    let fixture = Fixture::new();
    for &level in LEVELS {
        assert!(registered_tools(level).iter().any(|name| name == TOOL));
        let client = fixture.connect("claude-code", level).await;
        assert!(
            client.tool_names().await.iter().any(|name| name == TOOL),
            "{TOOL} isn't listed at {level}"
        );
        client.close().await;
    }
}

#[tokio::test]
async fn returns_the_category_tree() {
    let fixture = Fixture::new();
    let tree = fixture.open().categories().unwrap();
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;

    let result = client.call_ok(TOOL, json!({})).await;
    let groups = result["groups"].as_array().unwrap();
    assert_eq!(groups.len(), tree.groups().len());
    let mut count = 0;
    for (group, expected) in groups.iter().zip(tree.groups()) {
        let category = expected.category();
        assert_eq!(group["key"], category.key().as_str());
        assert_eq!(group["label"], category.label());
        assert_eq!(group["kind"], category.kind().as_str());
        let children = group["children"].as_array().unwrap();
        assert_eq!(children.len(), expected.children().len());
        for (child, expected) in children.iter().zip(expected.children()) {
            assert_eq!(child["key"], expected.key().as_str());
            assert_eq!(child["label"], expected.label());
            assert_eq!(
                child.as_object().unwrap().len(),
                2,
                "a subcategory is a key and a label: {child}"
            );
        }
        count += 1 + children.len();
    }
    assert_eq!(count, tree.len());
    client.close().await;
}

#[tokio::test]
async fn output_has_no_personal_data() {
    let fixture = Fixture::new();
    for &level in LEVELS {
        let client = fixture.connect("claude-code", level).await;
        // `call_ok` checks the whole response with `assert_no_pii`.
        let result = client.call_ok(TOOL, json!({})).await;
        assert_no_pii(&result);
        client.close().await;
    }
}

#[tokio::test]
async fn a_call_writes_one_access_log_row() {
    let fixture = Fixture::new();
    let tree_len = fixture.open().categories().unwrap().len() as u64;
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;
    assert!(fixture.access_log().is_empty());

    client
        .call_ok(TOOL, json!({ "purpose": "monthly review" }))
        .await;

    let log = fixture.access_log();
    assert_eq!(log.len(), 1, "{log:#?}");
    let call = &log[0];
    assert_eq!(call.client, "claude-code");
    assert_eq!(call.tool, TOOL);
    assert_eq!(call.access_level, Some(AccessLevel::Aggregate));
    assert_eq!(call.outcome, Outcome::Ok);
    assert_eq!(call.purpose.as_deref(), Some("monthly review"));
    assert_eq!(call.args_redacted_json, "{}");
    assert_eq!(call.row_count, tree_len);
    assert_eq!(call.withheld_count, 0);
    client.close().await;
}

#[tokio::test]
async fn declares_purpose_and_a_read_only_object_result() {
    let fixture = Fixture::new();
    let client = fixture.connect("claude-code", AccessLevel::Aggregate).await;
    let tool = client
        .tools()
        .await
        .into_iter()
        .find(|tool| tool.name == TOOL)
        .unwrap();
    assert!(tool.input_schema["properties"]["purpose"].is_object());
    assert_eq!(tool.output_schema.unwrap()["type"], "object");
    assert_eq!(tool.annotations.unwrap().read_only_hint, Some(true));
    client.close().await;
}
