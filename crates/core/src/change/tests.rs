//! Tests for change batches against a real database.

use chrono::NaiveDate;
use rusqlite::params;
use tempfile::TempDir;

use super::*;
use crate::account::{self, AccountKind, NewAccount, date_key};
use crate::id::AccountId;
use crate::import::{self, Committed};
use crate::manual::{self, ManualTransaction};
use crate::statement::{SourceFormat, SourceLocation, Statement, StatementLine};
use crate::tag::{self, TagName};

struct Fixture {
    db: Database,
    account: AccountId,
    _dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
        let account = account::create(
            &mut db,
            &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        Fixture {
            db,
            account,
            _dir: dir,
        }
    }

    fn manual(&mut self, d: u32, minor: i64) -> TransactionId {
        manual::create(
            &mut self.db,
            &ManualTransaction::new(self.account, day(d), gbp(minor), "Corner shop"),
        )
        .unwrap()
    }

    fn category(&self, key: &str) -> i64 {
        self.db
            .conn()
            .query_row("SELECT id FROM category WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .unwrap()
    }

    fn lock(&self, d: u32) {
        self.db
            .conn()
            .execute(
                "UPDATE account SET reconciled_through = ?1 WHERE id = ?2",
                params![date_key(day(d)), self.account],
            )
            .unwrap();
    }

    fn read(&self, id: TransactionId, field: Field) -> Value {
        read(self.db.conn(), id, field).unwrap()
    }

    /// Applies `changes` as one batch of `kind`.
    fn apply(
        &mut self,
        kind: ChangeKind,
        changes: Vec<(TransactionId, Value)>,
    ) -> Option<ChangeBatchId> {
        let tx = self.db.conn_mut().transaction().unwrap();
        let mut recorder = Recorder::new(kind);
        for (id, value) in changes {
            recorder.set(&tx, id, value).unwrap();
        }
        let batch = recorder.finish(&tx).unwrap();
        tx.commit().unwrap();
        batch
    }
}

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn category(id: i64, source: CategorySource) -> Value {
    Value::Category(CategoryValue {
        category_id: Some(id),
        source: Some(source),
        rule_id: None,
    })
}

fn note(text: &str) -> Value {
    Value::UserNote(Some(text.to_owned()))
}

#[test]
fn every_kind_and_field_key_matches_the_schema() {
    let schema = include_str!("../../migrations/V8__change_batch.sql");
    for kind in ChangeKind::ALL {
        assert!(schema.contains(&format!("'{}'", kind.as_str())), "{kind:?}");
        assert_eq!(ChangeKind::from_key(kind.as_str()), Some(kind));
    }
    for field in Field::ALL {
        assert!(
            schema.contains(&format!("'{}'", field.as_str())),
            "{field:?}"
        );
        assert_eq!(Field::from_key(field.as_str()), Some(field));
    }
}

#[test]
fn values_round_trip_through_json() {
    let values = [
        category(3, CategorySource::Rule),
        Value::Category(CategoryValue::NONE),
        Value::Merchant(MerchantValue {
            name: Some("Café".into()),
            source: Some(NameSource::User),
        }),
        Value::UserNote(None),
        Value::Tags(vec![TagId::new(1), TagId::new(4)]),
        Value::Exclusion(Some(ExclusionValue {
            reason: ExclusionReason::BankError,
            note: None,
            at: Some("2026-11-12T10:00:00.000Z".into()),
        })),
        Value::Split(vec![SplitLineValue {
            amount_minor: -100,
            category_id: Some(2),
            my_share_minor: Some(-50),
            share_label: Some("Sam".into()),
            user_note: None,
        }]),
        Value::Share(ShareValue::WHOLE),
        Value::Reimbursable(true),
    ];
    for value in values {
        let json = value.to_json();
        assert_eq!(
            Value::from_json(value.field(), &json),
            Some(value),
            "{json}"
        );
    }
    assert_eq!(
        Value::from_json(Field::Share, r#"{"my_share_minor":1.5}"#),
        None
    );
    assert_eq!(
        Value::from_json(
            Field::Share,
            r#"{"my_share_minor":null,"share_label":null,"x":1}"#
        ),
        None
    );
}

#[test]
fn a_batch_records_old_and_new_values_and_undo_restores_them() {
    let mut f = Fixture::new();
    let a = f.manual(3, -1200);
    let b = f.manual(4, -800);
    let groceries = f.category("food.groceries");
    let batch = f
        .apply(
            ChangeKind::BulkEdit,
            vec![
                (a, category(groceries, CategorySource::User)),
                (a, note("Weekly shop")),
                (b, category(groceries, CategorySource::User)),
                (b, Value::Reimbursable(true)),
            ],
        )
        .unwrap();

    let info = get(&f.db, batch).unwrap();
    assert_eq!(info.kind, ChangeKind::BulkEdit);
    assert_eq!(info.status, BatchStatus::Applied);
    assert_eq!((info.items, info.transactions), (4, 2));
    let stored = items(&f.db, batch).unwrap();
    assert_eq!(stored[0].transaction, a);
    assert_eq!(stored[0].old, Value::Category(CategoryValue::NONE));
    assert_eq!(stored[1].new, note("Weekly shop"));
    assert_eq!(f.read(b, Field::Reimbursable), Value::Reimbursable(true));

    let preview = undo_preview(&f.db, batch).unwrap();
    assert_eq!((preview.restored, preview.changed_since), (4, 0));
    // The preview changed nothing.
    assert_eq!(f.read(a, Field::UserNote), note("Weekly shop"));

    let undone = undo(&mut f.db, batch).unwrap();
    assert_eq!((undone.restored, undone.transactions), (4, 2));
    assert_eq!(undone.batch.status, BatchStatus::Undone);
    assert!(undone.batch.undone_at.is_some());
    assert_eq!(
        f.read(a, Field::Category),
        Value::Category(CategoryValue::NONE)
    );
    assert_eq!(f.read(a, Field::UserNote), Value::UserNote(None));
    assert_eq!(f.read(b, Field::Reimbursable), Value::Reimbursable(false));

    assert!(matches!(
        undo(&mut f.db, batch),
        Err(Error::AlreadyUndone(id)) if id == batch
    ));
}

#[test]
fn nothing_changed_records_no_batch() {
    let mut f = Fixture::new();
    let a = f.manual(3, -1200);
    assert_eq!(
        f.apply(ChangeKind::BulkEdit, vec![(a, Value::UserNote(None))]),
        None
    );
    // Setting a field and setting it back is no change either.
    assert_eq!(
        f.apply(
            ChangeKind::BulkEdit,
            vec![(a, note("x")), (a, Value::UserNote(None))]
        ),
        None
    );
    assert!(list(&f.db).unwrap().is_empty());
}

#[test]
fn setting_a_field_twice_records_first_old_and_last_new() {
    let mut f = Fixture::new();
    let a = f.manual(3, -1200);
    let batch = f
        .apply(
            ChangeKind::BulkEdit,
            vec![(a, note("one")), (a, note("two"))],
        )
        .unwrap();
    let stored = items(&f.db, batch).unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].old, Value::UserNote(None));
    assert_eq!(stored[0].new, note("two"));
}

#[test]
fn undo_skips_fields_changed_since() {
    let mut f = Fixture::new();
    let a = f.manual(3, -1200);
    let b = f.manual(4, -800);
    let batch = f
        .apply(
            ChangeKind::BulkEdit,
            vec![(a, note("batch")), (b, note("batch"))],
        )
        .unwrap();
    // The user edits a's note afterwards.
    f.apply(ChangeKind::BulkEdit, vec![(a, note("mine"))]);

    let undone = undo(&mut f.db, batch).unwrap();
    assert_eq!((undone.restored, undone.changed_since), (1, 1));
    assert_eq!(f.read(a, Field::UserNote), note("mine"));
    assert_eq!(f.read(b, Field::UserNote), Value::UserNote(None));
}

#[test]
fn a_deleted_rule_doesnt_count_as_a_change_since() {
    let mut f = Fixture::new();
    let a = f.manual(3, -1200);
    let groceries = f.category("food.groceries");
    f.db.conn()
        .execute_batch(
            "INSERT INTO rule (id, priority, match_field, match_kind, pattern) \
             VALUES (7, 0, 'description', 'contains', 'shop'); \
             INSERT INTO rule_action (rule_id, kind, value_json) \
             VALUES (7, 'set_category', '\"food.groceries\"');",
        )
        .unwrap();
    let batch = f
        .apply(
            ChangeKind::RuleApply,
            vec![(
                a,
                Value::Category(CategoryValue {
                    category_id: Some(groceries),
                    source: Some(CategorySource::Rule),
                    rule_id: Some(RuleId::new(7)),
                }),
            )],
        )
        .unwrap();
    crate::rule::delete(&mut f.db, RuleId::new(7)).unwrap();

    let undone = undo(&mut f.db, batch).unwrap();
    assert_eq!(undone.restored, 1);
    assert_eq!(
        f.read(a, Field::Category),
        Value::Category(CategoryValue::NONE)
    );
}

#[test]
fn tags_are_restored_as_a_set() {
    let mut f = Fixture::new();
    let a = f.manual(3, -1200);
    let work = tag::create(&mut f.db, &TagName::new("work").unwrap(), false).unwrap();
    let trip = tag::create(&mut f.db, &TagName::new("trip").unwrap(), false).unwrap();
    tag::add_to_transaction(&mut f.db, a, work).unwrap();

    let batch = f
        .apply(
            ChangeKind::RuleApply,
            vec![(a, Value::Tags(vec![trip, work]))],
        )
        .unwrap();
    let mut both = vec![work, trip];
    both.sort();
    assert_eq!(f.read(a, Field::Tags), Value::Tags(both));

    undo(&mut f.db, batch).unwrap();
    assert_eq!(f.read(a, Field::Tags), Value::Tags(vec![work]));
}

#[test]
fn a_split_is_undone_before_the_category_returns() {
    let mut f = Fixture::new();
    let a = f.manual(3, -1000);
    let groceries = f.category("food.groceries");
    let eating_out = f.category("food.eating_out");
    f.apply(
        ChangeKind::BulkEdit,
        vec![(a, category(groceries, CategorySource::User))],
    );
    let lines = vec![
        SplitLineValue {
            amount_minor: -600,
            category_id: Some(groceries),
            my_share_minor: None,
            share_label: None,
            user_note: None,
        },
        SplitLineValue {
            amount_minor: -400,
            category_id: Some(eating_out),
            my_share_minor: None,
            share_label: None,
            user_note: None,
        },
    ];
    let batch = f
        .apply(
            ChangeKind::RuleApply,
            vec![
                (a, Value::Category(CategoryValue::NONE)),
                (a, Value::Share(ShareValue::WHOLE)),
                (a, Value::Split(lines.clone())),
            ],
        )
        .unwrap();
    assert_eq!(f.read(a, Field::Split), Value::Split(lines));

    let undone = undo(&mut f.db, batch).unwrap();
    assert_eq!(undone.restored, 2);
    assert_eq!(f.read(a, Field::Split), Value::Split(Vec::new()));
    assert_eq!(
        f.read(a, Field::Category),
        category(groceries, CategorySource::User)
    );
}

#[test]
fn a_split_needs_the_category_cleared_first() {
    let mut f = Fixture::new();
    let a = f.manual(3, -1000);
    let groceries = f.category("food.groceries");
    f.apply(
        ChangeKind::BulkEdit,
        vec![(a, category(groceries, CategorySource::User))],
    );
    let line = |minor| SplitLineValue {
        amount_minor: minor,
        category_id: None,
        my_share_minor: None,
        share_label: None,
        user_note: None,
    };
    let tx = f.db.conn_mut().transaction().unwrap();
    let mut recorder = Recorder::new(ChangeKind::RuleApply);
    let error = recorder
        .set(&tx, a, Value::Split(vec![line(-600), line(-400)]))
        .unwrap_err();
    assert!(matches!(
        error,
        Error::Unwritable {
            field: Field::Split,
            ..
        }
    ));
    recorder
        .set(&tx, a, Value::Category(CategoryValue::NONE))
        .unwrap();
    // Lines that don't sum to the amount are refused too.
    assert!(matches!(
        recorder.set(&tx, a, Value::Split(vec![line(-600), line(-300)])),
        Err(Error::Unwritable { .. })
    ));
    assert!(
        recorder
            .set(&tx, a, Value::Split(vec![line(-600), line(-400)]))
            .unwrap()
    );
}

#[test]
fn an_exclusion_in_a_locked_period_is_refused_whatever_the_reason() {
    let mut f = Fixture::new();
    let a = f.manual(3, -1200);
    let b = f.manual(9, -800);
    f.lock(5);

    for reason in [ExclusionReason::Duplicate, ExclusionReason::Disputed] {
        let tx = f.db.conn_mut().transaction().unwrap();
        let mut recorder = Recorder::new(ChangeKind::RuleApply);
        assert_eq!(
            locked(&tx, a, Field::Exclusion).unwrap(),
            Some(day(5)),
            "{reason:?}"
        );
        assert!(matches!(
            recorder.set(&tx, a, Value::Exclusion(Some(ExclusionValue::now(reason)))),
            Err(Error::Locked { transaction, through, .. }) if transaction == a && through == day(5)
        ));
    }
    // Other fields aren't guarded, and rows after the lock aren't either.
    assert_eq!(locked(f.db.conn(), a, Field::UserNote).unwrap(), None);
    f.apply(ChangeKind::RuleApply, vec![(a, note("fine"))]);
    let batch = f
        .apply(
            ChangeKind::DuplicateMerge,
            vec![(
                b,
                Value::Exclusion(Some(ExclusionValue::now(ExclusionReason::Duplicate))),
            )],
        )
        .unwrap();

    // Undoing it once the row is locked needs an unlock.
    f.lock(10);
    assert!(matches!(
        undo(&mut f.db, batch),
        Err(Error::Locked { transaction, .. }) if transaction == b
    ));
    assert!(undo_preview(&f.db, batch).is_err());
    f.db.conn()
        .execute("UPDATE account SET reconciled_through = NULL", [])
        .unwrap();
    undo(&mut f.db, batch).unwrap();
    assert_eq!(f.read(b, Field::Exclusion), Value::Exclusion(None));
}

#[test]
fn excluding_again_for_the_same_reason_is_no_change() {
    let mut f = Fixture::new();
    let a = f.manual(3, -1200);
    let exclude = || Value::Exclusion(Some(ExclusionValue::now(ExclusionReason::Disputed)));
    f.apply(ChangeKind::RuleApply, vec![(a, exclude())])
        .unwrap();
    assert_eq!(f.apply(ChangeKind::RuleApply, vec![(a, exclude())]), None);
}

#[test]
fn a_derived_merchant_name_is_recomputed_on_undo() {
    let mut f = Fixture::new();
    let a = f.manual(3, -1200);
    let derived = f.read(a, Field::Merchant);
    let batch = f
        .apply(
            ChangeKind::RuleApply,
            vec![(
                a,
                Value::Merchant(MerchantValue {
                    name: Some("Shop".into()),
                    source: Some(NameSource::Rule),
                }),
            )],
        )
        .unwrap();
    undo(&mut f.db, batch).unwrap();
    assert_eq!(f.read(a, Field::Merchant), derived);
}

#[test]
fn a_dry_run_writes_nothing() {
    let mut f = Fixture::new();
    let a = f.manual(3, -1200);
    let count = dry_run(&f.db, |conn| -> Result<usize, Error> {
        let mut recorder = Recorder::new(ChangeKind::BulkEdit);
        recorder.set(conn, a, note("x"))?;
        recorder.finish(conn)?;
        Ok(recorder_count(conn))
    })
    .unwrap();
    assert_eq!(count, 1);
    assert_eq!(f.read(a, Field::UserNote), Value::UserNote(None));
    assert!(list(&f.db).unwrap().is_empty());
    let _ = &mut f;
}

fn recorder_count(conn: &Connection) -> usize {
    conn.query_row("SELECT COUNT(*) FROM change_batch", [], |row| {
        row.get::<_, i64>(0)
    })
    .unwrap() as usize
}

#[test]
fn deleting_a_row_deletes_its_items() {
    let mut f = Fixture::new();
    let a = f.manual(3, -1200);
    let b = f.manual(4, -800);
    let batch = f
        .apply(ChangeKind::BulkEdit, vec![(a, note("x")), (b, note("y"))])
        .unwrap();
    manual::delete(&mut f.db, a).unwrap();
    assert_eq!(get(&f.db, batch).unwrap().items, 1);

    // Undoing an import clears its rows' items too.
    let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
    statement.push(
        SourceLocation::Line(2),
        Ok(StatementLine::new(day(6), gbp(-500), "Card payment")),
    );
    let preview = import::preview(&f.db, f.account, "s.csv", statement).unwrap();
    let Committed::Batch(summary) =
        import::commit(&mut f.db, &preview, preview.needs_confirmation()).unwrap()
    else {
        panic!("expected a batch");
    };
    let imported: TransactionId =
        f.db.conn()
            .query_row(
                "SELECT id FROM \"transaction\" WHERE batch_id = ?1",
                [summary.batch],
                |row| row.get(0),
            )
            .unwrap();
    let second = f
        .apply(ChangeKind::BulkEdit, vec![(imported, note("z"))])
        .unwrap();
    import::undo(&mut f.db, summary.batch).unwrap();
    assert_eq!(get(&f.db, second).unwrap().items, 0);
}

#[test]
fn items_and_batches_are_read_only() {
    let mut f = Fixture::new();
    let a = f.manual(3, -1200);
    let batch = f.apply(ChangeKind::BulkEdit, vec![(a, note("x"))]).unwrap();
    let conn = f.db.conn();
    assert!(
        conn.execute("UPDATE change_item SET new_value = '\"y\"'", [])
            .is_err()
    );
    assert!(
        conn.execute("UPDATE change_batch SET kind = 'rule_apply'", [])
            .is_err()
    );
    undo(&mut f.db, batch).unwrap();
    assert!(
        f.db.conn()
            .execute(
                "UPDATE change_batch SET status = 'applied', undone_at = NULL",
                []
            )
            .is_err()
    );
}
