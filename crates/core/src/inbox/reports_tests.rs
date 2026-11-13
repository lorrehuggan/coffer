//! Deleting saved reports: by ID, by client and date, all or nothing, and
//! room under the cap again.

use super::super::testing::Fixture;
use super::super::{Error, MAX_REPORTS_PER_SOURCE, NewReport, insert_report};
use super::*;

fn client(name: &str) -> ClientName {
    ClientName::new(name).unwrap()
}

/// Saves a report from `source` titled `title`, saved at `created_at`
/// when given.
fn save(f: &mut Fixture, source: &str, title: &str, created_at: Option<&str>) -> i64 {
    let id = insert_report(
        &mut f.db,
        &client(source),
        &NewReport {
            period: "2026-10".to_owned(),
            title: title.to_owned(),
            body_markdown: "Spending was down.".to_owned(),
        },
    )
    .unwrap();
    if let Some(at) = created_at {
        f.conn()
            .execute(
                "UPDATE report SET created_at = ?1 WHERE id = ?2",
                params![at, id],
            )
            .unwrap();
    }
    id
}

fn ids(reports: &[ReportHeading]) -> Vec<i64> {
    reports.iter().map(|r| r.id).collect()
}

fn stored(f: &Fixture) -> Vec<i64> {
    f.conn()
        .prepare("SELECT id FROM report ORDER BY id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn deletes_one_report() {
    let mut f = Fixture::new();
    let october = save(&mut f, "claude-code", "October", None);
    let november = save(&mut f, "claude-code", "November", None);
    let selection = ReportSelection::Ids(vec![october]);

    let preview = reports_to_delete(&f.db, &selection).unwrap();
    assert_eq!(ids(&preview), [october]);
    assert_eq!(preview[0].title, "October");
    assert_eq!(preview[0].period, "2026-10");
    assert_eq!(preview[0].source_client, "claude-code");
    assert_eq!(stored(&f), [october, november], "the preview wrote");

    let deleted = delete_reports(&mut f.db, &selection).unwrap();
    assert_eq!(deleted, preview);
    assert_eq!(stored(&f), [november]);
}

#[test]
fn deletes_several_in_the_order_named() {
    let mut f = Fixture::new();
    let a = save(&mut f, "claude-code", "A", None);
    let b = save(&mut f, "codex", "B", None);
    let c = save(&mut f, "claude-code", "C", None);

    let deleted = delete_reports(&mut f.db, &ReportSelection::Ids(vec![c, a])).unwrap();
    assert_eq!(ids(&deleted), [c, a]);
    assert_eq!(stored(&f), [b]);
}

#[test]
fn an_unknown_or_repeated_id_deletes_nothing() {
    let mut f = Fixture::new();
    let a = save(&mut f, "claude-code", "A", None);
    let b = save(&mut f, "claude-code", "B", None);

    for selection in [
        ReportSelection::Ids(vec![a, 999, b]),
        ReportSelection::Ids(vec![999]),
    ] {
        let error = reports_to_delete(&f.db, &selection).unwrap_err();
        assert!(matches!(error, Error::ReportNotFound(999)), "{error}");
        let error = delete_reports(&mut f.db, &selection).unwrap_err();
        assert!(matches!(error, Error::ReportNotFound(999)), "{error}");
        assert_eq!(error.to_string(), "there is no report 999");
        assert_eq!(stored(&f), [a, b]);
    }

    let error = delete_reports(&mut f.db, &ReportSelection::Ids(vec![a, b, a])).unwrap_err();
    assert!(
        matches!(error, Error::ReportNamedTwice(id) if id == a),
        "{error}"
    );
    assert_eq!(error.to_string(), format!("report {a} is named twice"));
    assert_eq!(stored(&f), [a, b]);
}

#[test]
fn a_failure_part_way_deletes_nothing() {
    let mut f = Fixture::new();
    let a = save(&mut f, "claude-code", "A", None);
    let b = save(&mut f, "claude-code", "B", None);
    // Every row checks out, but the second delete fails inside the
    // transaction: the first must come back.
    f.conn()
        .execute_batch(&format!(
            "CREATE TEMP TRIGGER keep_b BEFORE DELETE ON report WHEN OLD.id = {b} \
             BEGIN SELECT RAISE(ABORT, 'kept for the test'); END;"
        ))
        .unwrap();

    let error = delete_reports(&mut f.db, &ReportSelection::Ids(vec![a, b])).unwrap_err();
    assert!(matches!(error, Error::Database(_)), "{error}");
    assert_eq!(stored(&f), [a, b]);
}

#[test]
fn deletes_a_clients_reports_or_those_before_a_date() {
    let mut f = Fixture::new();
    let old = save(&mut f, "codex", "Old", Some("2026-09-30T23:59:59.999Z"));
    let midnight = save(
        &mut f,
        "codex",
        "Midnight",
        Some("2026-10-01T00:00:00.000Z"),
    );
    let other = save(
        &mut f,
        "claude-code",
        "Other",
        Some("2026-01-01T00:00:00.000Z"),
    );
    let new = save(&mut f, "codex", "New", None);
    let before = |day: &str| ReportSelection::Source {
        source: client("codex"),
        before: Some(day.parse().unwrap()),
    };

    // Before the day began: the report at midnight is on the day.
    assert_eq!(
        ids(&reports_to_delete(&f.db, &before("2026-10-01")).unwrap()),
        [old]
    );
    assert!(
        reports_to_delete(&f.db, &before("2026-09-01"))
            .unwrap()
            .is_empty()
    );
    // A client with no reports selects none: not an error.
    let none = ReportSelection::Source {
        source: client("nobody"),
        before: None,
    };
    assert!(delete_reports(&mut f.db, &none).unwrap().is_empty());

    let deleted = delete_reports(&mut f.db, &before("2026-10-01")).unwrap();
    assert_eq!(ids(&deleted), [old]);
    assert_eq!(stored(&f), [midnight, other, new]);

    // Every report from the client, oldest first; the other client's stay.
    let all = ReportSelection::Source {
        source: client("codex"),
        before: None,
    };
    let deleted = delete_reports(&mut f.db, &all).unwrap();
    assert_eq!(ids(&deleted), [midnight, new]);
    assert_eq!(stored(&f), [other]);
}

#[test]
fn deleting_makes_room_under_the_cap_and_ids_are_not_reused() {
    let mut f = Fixture::new();
    let source = client("claude-code");
    f.conn()
        .execute(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < ?1) \
             INSERT INTO report (period, title, body_markdown, source_client) \
             SELECT '2026-10', 'Seeded', '', ?2 FROM n",
            params![MAX_REPORTS_PER_SOURCE as i64, source.as_str()],
        )
        .unwrap();
    assert_eq!(
        report_counts(&f.db).unwrap(),
        [("claude-code".to_owned(), MAX_REPORTS_PER_SOURCE)]
    );
    let report = NewReport {
        period: "2026-11".to_owned(),
        title: "November".to_owned(),
        body_markdown: String::new(),
    };
    assert!(matches!(
        insert_report(&mut f.db, &source, &report),
        Err(Error::TooManyReports { .. })
    ));

    let newest = *stored(&f).last().unwrap();
    delete_reports(&mut f.db, &ReportSelection::Ids(vec![newest])).unwrap();
    let id = insert_report(&mut f.db, &source, &report).unwrap();
    assert!(id > newest, "report {newest}'s ID was given to {id}");
    assert_eq!(
        report_counts(&f.db).unwrap(),
        [("claude-code".to_owned(), MAX_REPORTS_PER_SOURCE)]
    );
}
