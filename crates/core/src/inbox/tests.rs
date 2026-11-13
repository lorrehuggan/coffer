//! Filing: payload shapes and references, canonical form, de-duplication,
//! caps, reports, reading back and rejecting.

use serde_json::json;

use super::testing::{Fixture, day};
use super::*;
use crate::id::TransactionId;
use crate::money::Currency;

fn invalid(result: Result<Filed, Error>) -> Invalid {
    match result {
        Err(Error::Invalid(invalid)) => invalid,
        other => panic!("expected an invalid payload, got {other:?}"),
    }
}

#[test]
fn files_each_kind_pending_from_its_source() {
    let mut f = Fixture::new();
    let coffee = f.row(day(1), -350, "Coffee", None);
    let out = f.row(day(2), -5000, "To savings", None);
    let inn = f.row_on(f.savings, day(2), 5000, "From current", None);
    let shoes = f.row(day(3), -6000, "Shoes", None);
    let back = f.row(day(9), 6000, "Shoes", None);

    let payloads = [
        (
            SuggestionKind::Categorization,
            json!({"items": [{"transaction_id": coffee.get(), "category": "food.eating_out"}]}),
        ),
        (
            SuggestionKind::Rule,
            json!({"match_field": "merchant", "match_kind": "is", "pattern": "Coffee",
                   "actions": [{"kind": "set_category", "value": "food.eating_out"}]}),
        ),
        (
            SuggestionKind::TransferPair,
            json!({"outflow_id": out.get(), "inflow_id": inn.get()}),
        ),
        (
            SuggestionKind::MerchantAlias,
            json!({"pattern": "COFFEE", "display_name": "Corner Café"}),
        ),
        (
            SuggestionKind::RefundLink,
            json!({"refund_id": back.get(), "purchase_id": shoes.get(), "link_kind": "refund"}),
        ),
    ];
    for (kind, payload) in payloads {
        let filed = f.file_from("claude-code", kind, payload).unwrap();
        assert!(!filed.duplicate);
        let stored = get(&f.db, filed.id).unwrap();
        assert_eq!(stored.kind, kind);
        assert_eq!(stored.status, SuggestionStatus::Pending);
        assert_eq!(stored.source_client, "claude-code");
        assert_eq!(stored.rationale.as_deref(), Some("Looks right"));
        assert_eq!(stored.payload.kind(), kind);
        assert_eq!(stored.decided_at, None);
    }
    assert_eq!(f.count("suggestion"), 5);
    let pending = list(
        &f.db,
        &Filter {
            status: Some(SuggestionStatus::Pending),
            kind: Some(SuggestionKind::Rule),
        },
    )
    .unwrap();
    assert_eq!(pending.len(), 1);
}

#[test]
fn stores_the_canonical_form_and_files_identical_ones_once() {
    let mut f = Fixture::new();
    let a = f.row(day(1), -100, "A", None);
    let b = f.row(day(1), -200, "B", None);
    let first = f
        .file_from(
            "claude-code",
            SuggestionKind::Categorization,
            json!({"items": [
                {"category": "food.groceries", "transaction_id": b.get()},
                {"transaction_id": a.get(), "category": "food.groceries"},
            ]}),
        )
        .unwrap();
    // The same items in another order, from another client.
    let second = f
        .file_from(
            "codex",
            SuggestionKind::Categorization,
            json!({"items": [
                {"transaction_id": a.get(), "category": "food.groceries"},
                {"transaction_id": b.get(), "category": "food.groceries"},
            ]}),
        )
        .unwrap();
    assert!(second.duplicate);
    assert_eq!(second.id, first.id);
    assert_eq!(f.count("suggestion"), 1);
    let json: String = f
        .conn()
        .query_row(
            "SELECT payload_json FROM suggestion WHERE id = ?1",
            [first.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        json,
        format!(
            "{{\"items\":[{{\"transaction_id\":{},\"category\":\"food.groceries\"}},\
             {{\"transaction_id\":{},\"category\":\"food.groceries\"}}]}}",
            a.get(),
            b.get()
        )
    );

    // Once it is decided, the same proposal is a new suggestion.
    reject(&mut f.db, first.id, None).unwrap();
    let third = f
        .file_from(
            "codex",
            SuggestionKind::Categorization,
            json!({"items": [
                {"transaction_id": a.get(), "category": "food.groceries"},
                {"transaction_id": b.get(), "category": "food.groceries"},
            ]}),
        )
        .unwrap();
    assert!(!third.duplicate);
}

#[test]
fn a_rule_is_stored_canonically() {
    let mut f = Fixture::new();
    let id = f.file(
        SuggestionKind::Rule,
        json!({"actions": [
                  {"kind": "add_tag", "value": ["coffee"]},
                  {"kind": "set_category", "value": "food.eating_out"}],
               "pattern": "Coffee", "match_kind": "starts_with", "match_field": "merchant",
               "amount_max": 0}),
    );
    let json: String = f
        .conn()
        .query_row(
            "SELECT payload_json FROM suggestion WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        json,
        "{\"match_field\":\"merchant\",\"match_kind\":\"starts_with\",\"pattern\":\"Coffee\",\
         \"amount_max\":0,\"actions\":[{\"kind\":\"set_category\",\"value\":\"food.eating_out\"},\
         {\"kind\":\"add_tag\",\"value\":[\"coffee\"]}]}"
    );
}

#[test]
fn refuses_payloads_of_the_wrong_shape() {
    let mut f = Fixture::new();
    let a = f.row(day(1), -100, "A", None).get();
    let cases = [
        (SuggestionKind::Categorization, json!({"items": []})),
        (SuggestionKind::Categorization, json!([1, 2])),
        (
            SuggestionKind::Categorization,
            json!({"items": [{"transaction_id": a, "category": "food.groceries", "x": 1}]}),
        ),
        (
            SuggestionKind::Categorization,
            json!({"items": [{"transaction_id": 1.0, "category": "food.groceries"}]}),
        ),
        (
            SuggestionKind::Categorization,
            json!({"items": [{"transaction_id": a, "split_id": 1, "category": "food.groceries"}]}),
        ),
        (
            SuggestionKind::Categorization,
            json!({"items": [{"category": "food.groceries"}]}),
        ),
        (
            SuggestionKind::Categorization,
            json!({"items": [{"transaction_id": a, "category": "Not A Key"}]}),
        ),
        (
            SuggestionKind::Categorization,
            json!({"items": [
                {"transaction_id": a, "category": "food.groceries"},
                {"transaction_id": a, "category": "food.takeaway"}]}),
        ),
        (
            SuggestionKind::Rule,
            json!({"match_field": "merchant", "match_kind": "regex", "pattern": "(",
                   "actions": [{"kind": "set_category", "value": "food.groceries"}]}),
        ),
        (
            SuggestionKind::Rule,
            json!({"match_field": "merchant", "match_kind": "is", "pattern": "A", "actions": []}),
        ),
        (
            SuggestionKind::Rule,
            json!({"match_field": "merchant", "match_kind": "is", "pattern": "A",
                   "actions": [{"kind": "exclude", "value": "duplicate"}]}),
        ),
        (
            SuggestionKind::Rule,
            json!({"match_field": "merchant", "match_kind": "is", "pattern": "A",
                   "actions": [{"kind": "set_note", "value": "hi"}]}),
        ),
        (
            SuggestionKind::Rule,
            json!({"match_field": "merchant", "match_kind": "is", "pattern": "A",
                   "actions": [{"kind": "my_share", "value": {"basis_points": 5000}}]}),
        ),
        (
            SuggestionKind::Rule,
            json!({"match_field": "merchant", "match_kind": "is", "pattern": "A",
                   "amount_min": 10, "amount_max": 0,
                   "actions": [{"kind": "set_category", "value": "food.groceries"}]}),
        ),
        (
            SuggestionKind::TransferPair,
            json!({"outflow_id": a, "inflow_id": a}),
        ),
        (
            SuggestionKind::TransferPair,
            json!({"outflow_id": a, "inflow_id": a + 1, "note": "x"}),
        ),
        (
            SuggestionKind::MerchantAlias,
            json!({"pattern": " ", "display_name": "Tesco"}),
        ),
        (
            SuggestionKind::MerchantAlias,
            json!({"pattern": "TESCO", "display_name": "x".repeat(101)}),
        ),
        (
            SuggestionKind::MerchantAlias,
            json!({"pattern": "TESCO", "display_name": "Tes\u{7}co"}),
        ),
        (
            SuggestionKind::RefundLink,
            json!({"refund_id": a, "purchase_id": a + 1, "link_kind": "settlement"}),
        ),
        (
            SuggestionKind::RefundLink,
            json!({"refund_id": a, "purchase_id": a + 1, "link_kind": "refund",
                   "amount_minor": 0}),
        ),
    ];
    for (kind, payload) in cases {
        let result = f.file_from("claude-code", kind, payload.clone());
        assert!(
            matches!(result, Err(Error::Invalid(_))),
            "{kind}: {payload} gave {result:?}"
        );
    }
    let many: Vec<_> = (0..=payload::MAX_ITEMS as i64)
        .map(|i| json!({"transaction_id": i + 1, "category": "food.groceries"}))
        .collect();
    assert_eq!(
        invalid(f.file_from(
            "claude-code",
            SuggestionKind::Categorization,
            json!({ "items": many })
        )),
        Invalid::ItemCount {
            max: payload::MAX_ITEMS
        }
    );
    assert_eq!(f.count("suggestion"), 0);
}

#[test]
fn refuses_references_that_dont_hold() {
    let mut f = Fixture::new();
    let euros = f.account("Euro", Currency::EUR);
    let spend = f.row(day(1), -500, "Shop", None);
    let other = f.row(day(1), 500, "Shop", None);
    let savings_in = f.row_on(f.savings, day(1), 500, "In", None);
    let savings_more = f.row_on(f.savings, day(1), 700, "In", None);
    let euro_in = f.row_on(euros, day(1), 500, "In", None);
    let split = f.row(day(2), -1000, "Split", None);
    let lines = f.split(split, &[-400, -600]);

    let cases = [
        (
            SuggestionKind::Categorization,
            json!({"items": [{"transaction_id": 9999, "category": "food.groceries"}]}),
            Invalid::TransactionNotFound(TransactionId::new(9999)),
        ),
        (
            SuggestionKind::Categorization,
            json!({"items": [{"transaction_id": spend.get(), "category": "food.nope"}]}),
            Invalid::UnknownCategory(crate::category::CategoryKey::new("food.nope").unwrap()),
        ),
        (
            SuggestionKind::Categorization,
            json!({"items": [{"transaction_id": split.get(), "category": "food.groceries"}]}),
            Invalid::Split(split),
        ),
        (
            SuggestionKind::Categorization,
            json!({"items": [{"split_id": 9999, "category": "food.groceries"}]}),
            Invalid::SplitLineNotFound(crate::id::SplitId::new(9999)),
        ),
        (
            SuggestionKind::Rule,
            json!({"match_field": "merchant", "match_kind": "is", "pattern": "A",
                   "account_id": 9999,
                   "actions": [{"kind": "set_category", "value": "food.groceries"}]}),
            Invalid::AccountNotFound(crate::id::AccountId::new(9999)),
        ),
        (
            SuggestionKind::Rule,
            json!({"match_field": "merchant", "match_kind": "is", "pattern": "A",
                   "actions": [{"kind": "set_category", "value": "food.nope"}]}),
            Invalid::UnknownCategory(crate::category::CategoryKey::new("food.nope").unwrap()),
        ),
        (
            SuggestionKind::TransferPair,
            json!({"outflow_id": spend.get(), "inflow_id": other.get()}),
            Invalid::NotATransfer(spend, other, "they are on the same account"),
        ),
        (
            SuggestionKind::TransferPair,
            json!({"outflow_id": spend.get(), "inflow_id": euro_in.get()}),
            Invalid::NotATransfer(spend, euro_in, "they are in different currencies"),
        ),
        (
            SuggestionKind::TransferPair,
            json!({"outflow_id": savings_in.get(), "inflow_id": spend.get()}),
            Invalid::NotATransfer(savings_in, spend, "the outflow isn't money out"),
        ),
        (
            SuggestionKind::TransferPair,
            json!({"outflow_id": spend.get(), "inflow_id": savings_more.get()}),
            Invalid::NotATransfer(spend, savings_more, "the inflow isn't the same amount in"),
        ),
        (
            SuggestionKind::RefundLink,
            json!({"refund_id": spend.get(), "purchase_id": other.get(), "link_kind": "refund"}),
            Invalid::NotMoneyIn(spend),
        ),
        (
            SuggestionKind::RefundLink,
            json!({"refund_id": other.get(), "purchase_id": savings_in.get(),
                   "link_kind": "refund"}),
            Invalid::NotMoneyOut(savings_in),
        ),
        (
            SuggestionKind::RefundLink,
            json!({"refund_id": euro_in.get(), "purchase_id": spend.get(),
                   "link_kind": "refund"}),
            Invalid::CurrencyMismatch(euro_in, spend),
        ),
        (
            SuggestionKind::RefundLink,
            json!({"refund_id": other.get(), "purchase_id": spend.get(),
                   "link_kind": "refund", "amount_minor": 501}),
            Invalid::LinkAmount,
        ),
    ];
    for (kind, payload, expected) in cases {
        assert_eq!(
            invalid(f.file_from("claude-code", kind, payload.clone())),
            expected,
            "{payload}"
        );
    }
    // A split line can be categorised.
    f.file(
        SuggestionKind::Categorization,
        json!({"items": [{"split_id": lines[0].get(), "category": "food.groceries"}]}),
    );
    assert_eq!(f.count("suggestion"), 1);
}

#[test]
fn a_transfer_pair_may_be_detections_own_suggestion() {
    let mut f = Fixture::new();
    let out = f.row(day(1), -500, "Out", None);
    let inn = f.row_on(f.savings, day(1), 500, "In", None);
    let third = f.row(day(1), -500, "Out again", None);
    f.conn()
        .execute(
            "INSERT INTO transfer_group (status) VALUES ('suggested')",
            [],
        )
        .unwrap();
    let group = f.conn().last_insert_rowid();
    f.conn()
        .execute(
            "UPDATE \"transaction\" SET transfer_group_id = ?1 WHERE id IN (?2, ?3)",
            rusqlite::params![group, out, inn],
        )
        .unwrap();
    f.file(
        SuggestionKind::TransferPair,
        json!({"outflow_id": out.get(), "inflow_id": inn.get()}),
    );
    // A row of another group isn't free.
    f.conn()
        .execute(
            "UPDATE transfer_group SET status = 'confirmed' WHERE id = ?1",
            [group],
        )
        .unwrap();
    assert_eq!(
        invalid(f.file_from(
            "claude-code",
            SuggestionKind::TransferPair,
            json!({"outflow_id": third.get(), "inflow_id": inn.get()})
        )),
        Invalid::NotATransfer(third, inn, "one of them is already in a transfer")
    );
}

#[test]
fn caps_the_rationale_and_the_pending_count() {
    let mut f = Fixture::new();
    let client = ClientName::new("claude-code").unwrap();
    let a = f.row(day(1), -100, "A", None);
    let long = NewSuggestion {
        kind: SuggestionKind::Categorization,
        payload: json!({"items": [{"transaction_id": a.get(), "category": "food.groceries"}]}),
        rationale: Some("x".repeat(MAX_RATIONALE_CHARS + 1)),
    };
    assert!(matches!(
        file_suggestion(&mut f.db, &client, &long),
        Err(Error::RationaleTooLong)
    ));

    for i in 0..MAX_PENDING_PER_SOURCE {
        f.file(
            SuggestionKind::MerchantAlias,
            json!({"pattern": format!("SHOP {i}"), "display_name": "Shop"}),
        );
    }
    let over = f.file_from(
        "claude-code",
        SuggestionKind::MerchantAlias,
        json!({"pattern": "ONE MORE", "display_name": "Shop"}),
    );
    assert!(matches!(over, Err(Error::TooManyPending)), "{over:?}");
    // An identical one is still answered with the existing ID.
    let again = f
        .file_from(
            "claude-code",
            SuggestionKind::MerchantAlias,
            json!({"pattern": "SHOP 0", "display_name": "Shop"}),
        )
        .unwrap();
    assert!(again.duplicate);
    // Another client has its own allowance.
    f.file_from(
        "codex",
        SuggestionKind::MerchantAlias,
        json!({"pattern": "ONE MORE", "display_name": "Shop"}),
    )
    .unwrap();
    assert_eq!(f.count("suggestion"), MAX_PENDING_PER_SOURCE as i64 + 1);
}

#[test]
fn saves_reports_within_their_caps() {
    let mut f = Fixture::new();
    let client = ClientName::new("claude-code").unwrap();
    let report = |period: &str, title: &str, body: String| NewReport {
        period: period.to_owned(),
        title: title.to_owned(),
        body_markdown: body,
    };
    insert_report(
        &mut f.db,
        &client,
        &report("2026-10", "October", "Spending was down.".to_owned()),
    )
    .unwrap();
    assert!(matches!(
        insert_report(&mut f.db, &client, &report("2026-10", " ", String::new())),
        Err(Error::BlankReportField { field: "title" })
    ));
    assert!(matches!(
        insert_report(
            &mut f.db,
            &client,
            &report("2026-10", "Long", "x".repeat(MAX_REPORT_BODY_CHARS + 1))
        ),
        Err(Error::ReportTooLong { field: "body", .. })
    ));
    assert_eq!(f.count("report"), 1);
}

#[test]
fn caps_the_saved_reports_per_source() {
    let mut f = Fixture::new();
    let client = ClientName::new("claude-code").unwrap();
    let other = ClientName::new("codex").unwrap();
    let report = NewReport {
        period: "2026-10".to_owned(),
        title: "October".to_owned(),
        body_markdown: "Spending was down.".to_owned(),
    };
    // All but the last through SQL, to keep the test fast.
    f.conn()
        .execute(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < ?1) \
             INSERT INTO report (period, title, body_markdown, source_client) \
             SELECT '2026-10', 'Seeded', '', ?2 FROM n",
            rusqlite::params![MAX_REPORTS_PER_SOURCE as i64 - 1, client.as_str()],
        )
        .unwrap();
    insert_report(&mut f.db, &client, &report).unwrap();
    assert_eq!(f.count("report"), MAX_REPORTS_PER_SOURCE as i64);

    let error = insert_report(&mut f.db, &client, &report).unwrap_err();
    assert!(
        matches!(error, Error::TooManyReports { max } if max == MAX_REPORTS_PER_SOURCE),
        "{error}"
    );
    assert_eq!(
        error.to_string(),
        "this client has saved 500 reports, the most one client may keep; no more can be saved \
         until the user deletes some in Coffer (`coffer inbox delete-report`)"
    );
    assert_eq!(f.count("report"), MAX_REPORTS_PER_SOURCE as i64);

    // Another source has its own allowance.
    insert_report(&mut f.db, &other, &report).unwrap();
    assert_eq!(f.count("report"), MAX_REPORTS_PER_SOURCE as i64 + 1);
}

#[test]
fn rejecting_records_the_reason_once() {
    let mut f = Fixture::new();
    let a = f.row(day(1), -100, "A", None);
    let id = f.file(
        SuggestionKind::Categorization,
        json!({"items": [{"transaction_id": a.get(), "category": "food.groceries"}]}),
    );
    assert!(matches!(
        reject(&mut f.db, id, Some(" ")),
        Err(ApproveError::Reason)
    ));
    reject(&mut f.db, id, Some("Not groceries")).unwrap();
    let stored = get(&f.db, id).unwrap();
    assert_eq!(stored.status, SuggestionStatus::Rejected);
    assert_eq!(stored.decision_reason.as_deref(), Some("Not groceries"));
    assert!(stored.decided_at.is_some());
    assert_eq!(f.category_of(a), None);
    assert!(matches!(
        reject(&mut f.db, id, None),
        Err(ApproveError::NotPending {
            status: SuggestionStatus::Rejected,
            ..
        })
    ));
    assert!(matches!(
        reject(&mut f.db, SuggestionId::new(9999), None),
        Err(ApproveError::Inbox(Error::NotFound(_)))
    ));
}

#[test]
fn the_schema_keeps_decisions() {
    let mut f = Fixture::new();
    let a = f.row(day(1), -100, "A", None);
    let id = f.file(
        SuggestionKind::Categorization,
        json!({"items": [{"transaction_id": a.get(), "category": "food.groceries"}]}),
    );
    let conn = f.conn();
    // The proposal is read-only.
    assert!(
        conn.execute("UPDATE suggestion SET rationale = 'x' WHERE id = ?1", [id])
            .is_err()
    );
    // A decision needs its time.
    assert!(
        conn.execute(
            "UPDATE suggestion SET status = 'approved' WHERE id = ?1",
            [id]
        )
        .is_err()
    );
    // Only a rejection has a reason.
    assert!(
        conn.execute(
            "UPDATE suggestion SET status = 'approved', decision_reason = 'x', \
             decided_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
            [id],
        )
        .is_err()
    );
    // A suggestion is filed pending.
    assert!(
        conn.execute(
            "INSERT INTO suggestion (kind, payload_json, source_client, status) \
             VALUES ('rule', '{}', 'x', 'approved')",
            [],
        )
        .is_err()
    );
    reject(&mut f.db, id, None).unwrap();
    // Decided stays decided.
    assert!(
        f.conn()
            .execute(
                "UPDATE suggestion SET status = 'pending', decided_at = NULL WHERE id = ?1",
                [id]
            )
            .is_err()
    );
}
