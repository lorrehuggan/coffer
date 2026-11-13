//! The harness's own checks: the fixture database holds the personal data
//! `assert_no_pii` looks for, and the helper catches each piece of it.

mod support;

use std::panic::{AssertUnwindSafe, catch_unwind};

use serde_json::{Value, json};
use support::*;

fn caught(value: &Value) -> bool {
    catch_unwind(AssertUnwindSafe(|| assert_no_pii(value))).is_err()
}

#[test]
fn the_fixture_holds_the_personal_data() {
    let fixture = Fixture::new();
    let conn = rusqlite::Connection::open(fixture.path()).unwrap();
    let text: String = conn
        .query_row(
            "SELECT group_concat(coalesce(t.description_raw, '') || ' ' || \
             coalesce(t.note, '') || ' ' || coalesce(t.user_note, '') || ' ' || \
             coalesce(t.share_label, ''), ' ') FROM \"transaction\" t",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let private_tags: String = conn
        .query_row(
            "SELECT group_concat(name) FROM tag WHERE private = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    for needle in [
        HOLDER_NAME,
        SORT_CODE,
        ACCOUNT_NUMBER,
        USER_NOTE,
        SHARE_LABEL,
    ] {
        assert!(
            text.to_lowercase().contains(&needle.to_lowercase()),
            "{needle} isn't in the fixture"
        );
    }
    assert_eq!(private_tags, PRIVATE_TAG);
    // The database as a whole fails the check.
    assert!(caught(&Value::String(text)));
}

#[test]
fn assert_no_pii_catches_each_kind() {
    for needle in PII {
        assert!(caught(&json!({ "field": needle })), "{needle}");
        assert!(
            caught(
                &json!({ "nested": [{ "x": format!("before {} after", needle.to_uppercase()) }] })
            ),
            "{needle}"
        );
    }
    // Keys, spaced IBANs and sort codes without separators.
    assert!(caught(&json!({ USER_NOTE: 1 })));
    assert!(caught(&json!("IBAN GB94 BARC 1020 1530 0934 59")));
    assert!(caught(&json!("sort code 774129")));
    assert!(caught(&json!("sort code 77 41 29")));

    assert!(!caught(&json!({
        "groups": [{ "key": "food", "label": "Food", "amount": { "minor": 774129 } }]
    })));
}

#[tokio::test]
async fn registered_tools_matches_the_wire() {
    let fixture = Fixture::new();
    for &level in LEVELS {
        let client = fixture.connect("claude-code", level).await;
        assert_eq!(client.tool_names().await, registered_tools(level));
        client.close().await;
    }
}
