//! Tests for possible-duplicate detection and merging against a real
//! database.

use chrono::NaiveDate;
use rusqlite::{ErrorCode, params};
use tempfile::TempDir;

use super::*;
use crate::account::{AccountKind, NewAccount};
use crate::change::{BatchStatus, MerchantValue};
use crate::exclusion;
use crate::import::{self, Committed};
use crate::manual::{self, ManualTransaction};
use crate::money::{Currency, Money};
use crate::note;
use crate::statement::{SourceFormat, SourceLocation, Statement, StatementLine};
use crate::tag::{self, TagName};

struct Fixture {
    db: Database,
    account: AccountId,
    _dir: TempDir,
}

/// One row of a test statement.
struct Line {
    day: u32,
    minor: i64,
    text: &'static str,
    id: Option<&'static str>,
}

fn line(day: u32, minor: i64, text: &'static str) -> Line {
    Line {
        day,
        minor,
        text,
        id: None,
    }
}

fn with_id(day: u32, minor: i64, text: &'static str, id: &'static str) -> Line {
    Line {
        day,
        minor,
        text,
        id: Some(id),
    }
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

    fn other_account(&mut self, name: &str) -> AccountId {
        account::create(
            &mut self.db,
            &NewAccount::new(name, AccountKind::Savings, Currency::GBP),
        )
        .unwrap()
    }

    /// Imports `lines` into `account` as one batch, without detection, and
    /// returns the batch and its rows in line order.
    fn import_into(
        &mut self,
        account: AccountId,
        format: SourceFormat,
        lines: &[Line],
    ) -> (BatchId, Vec<TransactionId>) {
        let mut statement = Statement::new(format, Currency::GBP);
        for (index, row) in lines.iter().enumerate() {
            let mut line = StatementLine::new(day(row.day), gbp(row.minor), row.text);
            line.external_id = row.id.map(str::to_owned);
            statement.push(SourceLocation::Line(index as u64 + 2), Ok(line));
        }
        let preview = import::preview(&self.db, account, "s.csv", statement).unwrap();
        let Committed::Batch(summary) =
            import::commit(&mut self.db, &preview, preview.needs_confirmation()).unwrap()
        else {
            panic!("expected a batch");
        };
        // The commit detects duplicates itself (`CommitSummary::duplicates`);
        // these tests run detection by hand, so they start from none.
        clear_batch(self.db.conn(), summary.batch).unwrap();
        let rows = ids(
            self.db.conn(),
            "SELECT id FROM \"transaction\" WHERE batch_id = ?1 AND origin = 'imported' \
             ORDER BY id",
            [summary.batch.get()],
        )
        .unwrap();
        (summary.batch, rows)
    }

    fn import(&mut self, format: SourceFormat, lines: &[Line]) -> (BatchId, Vec<TransactionId>) {
        self.import_into(self.account, format, lines)
    }

    /// Imports `lines` and runs detection for the batch, as the import
    /// commit will.
    fn import_and_detect(
        &mut self,
        format: SourceFormat,
        lines: &[Line],
    ) -> (Vec<TransactionId>, Vec<NewCandidate>) {
        let (batch, rows) = self.import(format, lines);
        let tx = self.db.conn_mut().transaction().unwrap();
        let found = detect_for_batch(&tx, batch, DuplicateWindow::DEFAULT).unwrap();
        tx.commit().unwrap();
        (rows, found)
    }

    fn manual(&mut self, d: u32, minor: i64, text: &str) -> TransactionId {
        manual::create(
            &mut self.db,
            &ManualTransaction::new(self.account, day(d), gbp(minor), text),
        )
        .unwrap()
    }

    fn scan(&mut self) -> Vec<NewCandidate> {
        scan(&mut self.db, None, DuplicateWindow::DEFAULT).unwrap()
    }

    fn pairs(&self) -> Vec<(TransactionId, TransactionId)> {
        let mut statement = self
            .db
            .conn()
            .prepare("SELECT transaction_a, transaction_b FROM duplicate_candidate ORDER BY id")
            .unwrap();
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
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

    fn unlock(&self) {
        self.db
            .conn()
            .execute(
                "UPDATE account SET reconciled_through = NULL WHERE id = ?1",
                [self.account],
            )
            .unwrap();
    }

    fn exclusion(&self, id: TransactionId) -> Option<ExclusionReason> {
        exclusion::get(&self.db, id)
            .unwrap()
            .map(|exclusion| exclusion.reason)
    }

    fn read(&self, id: TransactionId, field: Field) -> Value {
        change::read(self.db.conn(), id, field).unwrap()
    }

    fn category(&self, key: &str) -> i64 {
        self.db
            .conn()
            .query_row("SELECT id FROM category WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .unwrap()
    }

    fn set_category(&self, id: TransactionId, key: &str, source: &str) {
        self.db
            .conn()
            .execute(
                "UPDATE \"transaction\" SET category_id = ?1, category_source = ?2 WHERE id = ?3",
                params![self.category(key), source, id],
            )
            .unwrap();
    }

    /// Sets the display name of `row`, as an alias would.
    fn name(&self, row: TransactionId, name: &str) {
        self.db
            .conn()
            .execute(
                "UPDATE \"transaction\" SET merchant_display = ?1 WHERE id = ?2",
                params![name, row],
            )
            .unwrap();
    }

    fn merge(&mut self, requests: &[MergeRequest]) -> Merged {
        let preview = merge_preview(&self.db, requests).unwrap();
        merge(&mut self.db, &preview).unwrap()
    }
}

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

#[track_caller]
fn assert_refused<T: std::fmt::Debug>(result: rusqlite::Result<T>) {
    match result {
        Err(err) if err.sqlite_error_code() == Some(ErrorCode::ConstraintViolation) => {}
        other => panic!("expected a constraint violation, got {other:?}"),
    }
}

// Schema ----------------------------------------------------------------------

#[test]
fn reason_and_status_keys_match_the_schema() {
    let schema = include_str!("../../migrations/V10__duplicate_candidate.sql");
    for reason in CandidateReason::ALL {
        assert!(
            schema.contains(&format!("'{}'", reason.as_str())),
            "{reason:?}"
        );
        assert_eq!(CandidateReason::from_key(reason.as_str()), Some(reason));
    }
    for status in CandidateStatus::ALL {
        assert!(
            schema.contains(&format!("'{}'", status.as_str())),
            "{status:?}"
        );
        assert_eq!(CandidateStatus::from_key(status.as_str()), Some(status));
    }
}

#[test]
fn the_schema_keeps_pairs_canonical_on_one_account_and_merged_with_a_batch() {
    let mut f = Fixture::new();
    let a = f.manual(3, -500, "One");
    let b = f.manual(3, -500, "Two");
    let c = f.manual(3, -500, "Four");
    let savings = f.other_account("Savings");
    let elsewhere = manual::create(
        &mut f.db,
        &ManualTransaction::new(savings, day(3), gbp(-500), "Three"),
    )
    .unwrap();
    let conn = f.db.conn();
    let insert = |x: TransactionId, y: TransactionId| {
        conn.execute(
            "INSERT INTO duplicate_candidate (transaction_a, transaction_b, reason) \
             VALUES (?1, ?2, 'manual_entry')",
            params![x, y],
        )
    };
    // Lower ID first, a pair once, both rows on one account.
    assert_refused(insert(b, a));
    assert_refused(insert(a, a));
    assert_refused(insert(a, elsewhere));
    insert(a, b).unwrap();
    assert_refused(insert(a, b));
    let id = conn.last_insert_rowid();

    // A new candidate is pending.
    assert_refused(conn.execute(
        "INSERT INTO duplicate_candidate (transaction_a, transaction_b, reason, status) \
         VALUES (?1, ?2, 'manual_entry', 'dismissed')",
        params![a, c],
    ));
    // Merged exactly with a side kept and a duplicate_merge batch.
    assert_refused(conn.execute(
        "UPDATE duplicate_candidate SET status = 'merged' WHERE id = ?1",
        [id],
    ));
    conn.execute("INSERT INTO change_batch (kind) VALUES ('bulk_edit')", [])
        .unwrap();
    let bulk = conn.last_insert_rowid();
    assert_refused(conn.execute(
        "UPDATE duplicate_candidate SET status = 'merged', kept = 'a', change_batch_id = ?1 \
         WHERE id = ?2",
        params![bulk, id],
    ));
    conn.execute(
        "INSERT INTO change_batch (kind) VALUES ('duplicate_merge')",
        [],
    )
    .unwrap();
    let batch = conn.last_insert_rowid();
    assert_refused(conn.execute(
        "UPDATE duplicate_candidate SET status = 'merged', kept = 'c', change_batch_id = ?1 \
         WHERE id = ?2",
        params![batch, id],
    ));
    assert_refused(conn.execute(
        "UPDATE duplicate_candidate SET status = 'pending', kept = 'a' WHERE id = ?1",
        [id],
    ));
    conn.execute(
        "UPDATE duplicate_candidate SET status = 'merged', kept = 'a', change_batch_id = ?1 \
         WHERE id = ?2",
        params![batch, id],
    )
    .unwrap();
    // A merged candidate goes back to pending, never to dismissed.
    assert_refused(conn.execute(
        "UPDATE duplicate_candidate SET status = 'dismissed', kept = NULL, \
         change_batch_id = NULL WHERE id = ?1",
        [id],
    ));
    // The pair, the reason and the date are read-only.
    assert_refused(conn.execute(
        "UPDATE duplicate_candidate SET reason = 'overlapping_import' WHERE id = ?1",
        [id],
    ));
    assert_refused(conn.execute(
        "UPDATE duplicate_candidate SET created_at = '2026-01-01T00:00:00.000Z' WHERE id = ?1",
        [id],
    ));
    // A referenced transaction can't be deleted.
    assert_refused(conn.execute("DELETE FROM \"transaction\" WHERE id = ?1", [a]));
}

// Detection -------------------------------------------------------------------

#[test]
fn the_import_commit_detects_duplicates_of_its_rows() {
    let mut f = Fixture::new();
    f.import(SourceFormat::Csv, &[line(5, -1250, "TESCO STORES 1234")]);
    let mut statement = Statement::new(SourceFormat::Ofx, Currency::GBP);
    let mut row = StatementLine::new(day(6), gbp(-1250), "Tesco Stores London");
    row.external_id = Some("F1".to_owned());
    statement.push(SourceLocation::Line(2), Ok(row));
    let preview = import::preview(&f.db, f.account, "s.ofx", statement).unwrap();
    let Committed::Batch(summary) =
        import::commit(&mut f.db, &preview, preview.needs_confirmation()).unwrap()
    else {
        panic!("expected a batch");
    };
    assert_eq!(summary.duplicates, 1);
    assert_eq!(f.pairs().len(), 1);
}

#[test]
fn overlapping_exports_with_different_text_pair() {
    let mut f = Fixture::new();
    let (csv, _) = f.import_and_detect(
        SourceFormat::Csv,
        &[
            line(5, -1250, "TESCO STORES 1234"),
            line(6, -400, "Pret A Manger"),
        ],
    );
    let (ofx, found) = f.import_and_detect(
        SourceFormat::Ofx,
        &[
            with_id(6, -1250, "Tesco Stores London", "F1"),
            with_id(7, -999, "Something else", "F2"),
        ],
    );
    assert_eq!(
        found,
        [NewCandidate {
            id: found[0].id,
            transaction_a: csv[0],
            transaction_b: ofx[0],
            reason: CandidateReason::OverlappingImport,
        }]
    );
    let stored = get(&f.db, found[0].id).unwrap();
    assert_eq!(stored.status, CandidateStatus::Pending);
    assert_eq!((stored.kept, stored.change_batch), (None, None));
    assert_eq!(
        pending(&f.db, Some(f.account)).unwrap(),
        std::slice::from_ref(&stored)
    );
    assert_eq!(of_transaction(&f.db, csv[0]).unwrap(), [stored]);
    let savings = f.other_account("Savings");
    assert!(pending(&f.db, Some(savings)).unwrap().is_empty());
}

#[test]
fn the_window_is_three_days_both_ends_included() {
    let mut f = Fixture::new();
    let (first, _) = f.import_and_detect(
        SourceFormat::Csv,
        &[line(10, -300, "CAFE ONE"), line(10, -700, "SHOP ONE")],
    );
    let (second, found) = f.import_and_detect(
        SourceFormat::Ofx,
        &[
            with_id(13, -300, "Cafe one ltd", "F1"),
            with_id(14, -700, "Shop one ltd", "F2"),
        ],
    );
    assert_eq!(found.len(), 1);
    assert_eq!(
        (found[0].transaction_a, found[0].transaction_b),
        (first[0], second[0])
    );
    // Earlier dates count too.
    let (third, found) =
        f.import_and_detect(SourceFormat::Csv, &[line(7, -300, "CAFE ONE LONDON")]);
    assert_eq!(found.len(), 1);
    assert_eq!(
        (found[0].transaction_a, found[0].transaction_b),
        (first[0], third[0])
    );

    // A wider window finds the four-day pair.
    let found = scan(&mut f.db, None, DuplicateWindow::new(4).unwrap()).unwrap();
    assert_eq!(
        found
            .iter()
            .map(|c| (c.transaction_a, c.transaction_b))
            .collect::<Vec<_>>(),
        [(first[1], second[1])]
    );
    assert!(matches!(
        DuplicateWindow::new(32),
        Err(Error::WindowTooLong(32))
    ));
}

#[test]
fn identical_imported_text_is_not_a_near_duplicate() {
    let mut f = Fixture::new();
    f.import_and_detect(SourceFormat::Csv, &[with_id(5, -350, "Coffee  shop", "a1")]);
    let (_, found) =
        f.import_and_detect(SourceFormat::Ofx, &[with_id(6, -350, "COFFEE SHOP", "F1")]);
    assert!(found.is_empty(), "same normalized text: a repeat purchase");
}

#[test]
fn a_manual_row_pairs_with_an_imported_one_whatever_its_text() {
    let mut f = Fixture::new();
    let cash = f.manual(5, -350, "COFFEE SHOP");
    let other = f.manual(5, -900, "Lunch");
    let (rows, found) = f.import_and_detect(
        SourceFormat::Csv,
        &[line(6, -350, "Coffee shop"), line(6, -900, "Something")],
    );
    assert_eq!(found.len(), 2);
    assert_eq!(
        (found[0].transaction_a, found[0].transaction_b),
        (cash, rows[0])
    );
    assert_eq!(found[0].reason, CandidateReason::ManualEntry);
    assert_eq!(
        (found[1].transaction_a, found[1].transaction_b),
        (other, rows[1])
    );
}

#[test]
fn other_accounts_amounts_imports_and_manual_pairs_dont_pair() {
    let mut f = Fixture::new();
    let savings = f.other_account("Savings");
    f.import(SourceFormat::Csv, &[line(5, -500, "SHOP A")]);
    // Another account, another amount, the same import, two manual rows.
    f.import_into(savings, SourceFormat::Csv, &[line(5, -500, "SHOP B")]);
    f.import(
        SourceFormat::Ofx,
        &[
            with_id(5, -501, "SHOP C", "F1"),
            with_id(5, 700, "IN ONE", "F2"),
            with_id(5, 700, "IN TWO", "F3"),
        ],
    );
    f.manual(5, 1200, "Cash one");
    f.manual(5, 1200, "Cash two");
    // Zero amounts never pair.
    f.import(SourceFormat::Csv, &[line(5, 0, "CARD CHECK")]);
    f.import(
        SourceFormat::Ofx,
        &[with_id(5, 0, "Card verification", "F9")],
    );
    assert!(f.scan().is_empty());
}

#[test]
fn excluded_rows_are_skipped() {
    let mut f = Fixture::new();
    let (_, rows) = f.import(SourceFormat::Csv, &[line(5, -500, "SHOP A")]);
    exclusion::exclude(&mut f.db, rows[0], ExclusionReason::Disputed, None).unwrap();
    let (_, found) = f.import_and_detect(SourceFormat::Ofx, &[with_id(5, -500, "Shop a ltd", "F")]);
    assert!(found.is_empty());
}

#[test]
fn a_dismissed_pair_stays_dismissed_and_a_third_row_makes_a_new_pair() {
    let mut f = Fixture::new();
    let (first, _) = f.import_and_detect(SourceFormat::Csv, &[line(5, -500, "SHOP A")]);
    let (second, found) =
        f.import_and_detect(SourceFormat::Ofx, &[with_id(5, -500, "Shop a ltd", "F")]);
    let candidate = found[0].id;
    dismiss(&mut f.db, candidate).unwrap();
    assert!(matches!(
        dismiss(&mut f.db, candidate),
        Err(Error::NotPending {
            status: CandidateStatus::Dismissed,
            ..
        })
    ));
    assert!(pending(&f.db, None).unwrap().is_empty());
    assert!(f.scan().is_empty(), "never raised again");

    let third = f.manual(6, -500, "Shop");
    let found = f.scan();
    assert_eq!(found.len(), 1);
    assert_eq!(
        (found[0].transaction_a, found[0].transaction_b),
        (first[0], third)
    );
    // Closest first by ID: the second row stays out of this run, and is
    // found by the next.
    let found = f.scan();
    assert_eq!(
        (found[0].transaction_a, found[0].transaction_b),
        (second[0], third)
    );
    assert!(f.scan().is_empty());

    // Reopening puts it back.
    reopen(&mut f.db, candidate).unwrap();
    assert_eq!(
        get(&f.db, candidate).unwrap().status,
        CandidateStatus::Pending
    );
    assert!(matches!(
        reopen(&mut f.db, candidate),
        Err(Error::NotDismissed(_))
    ));
}

#[test]
fn closest_pairs_and_agreeing_names_go_first() {
    let mut f = Fixture::new();
    let (_, csv) = f.import(
        SourceFormat::Csv,
        &[line(5, -300, "CAFE ALPHA 1"), line(5, -300, "CAFE BETA 2")],
    );
    let (_, ofx) = f.import(
        SourceFormat::Ofx,
        &[
            with_id(5, -300, "Cafe Beta ref 9", "F1"),
            with_id(5, -300, "Cafe Alpha ref 8", "F2"),
            with_id(8, -300, "Cafe Gamma", "F3"),
        ],
    );
    // Names that agree across the exports, as an alias would give them.
    for (row, name) in [
        (csv[0], "Cafe Alpha"),
        (csv[1], "Cafe Beta"),
        (ofx[0], "CAFE BETA"),
        (ofx[1], "cafe alpha"),
        (ofx[2], "Cafe Gamma"),
    ] {
        f.name(row, name);
    }
    let found = f.scan();
    let pairs: Vec<_> = found
        .iter()
        .map(|c| (c.transaction_a, c.transaction_b))
        .collect();
    assert_eq!(pairs, [(csv[0], ofx[1]), (csv[1], ofx[0])]);
    // The same database always gives the same pairs.
    let mut g = Fixture::new();
    let (_, csv) = g.import(
        SourceFormat::Csv,
        &[line(5, -300, "CAFE ALPHA 1"), line(5, -300, "CAFE BETA 2")],
    );
    let (_, ofx) = g.import(
        SourceFormat::Ofx,
        &[
            with_id(5, -300, "Cafe Beta ref 9", "F1"),
            with_id(5, -300, "Cafe Alpha ref 8", "F2"),
        ],
    );
    // Without agreeing names: first with first by ID.
    for (row, name) in [
        (csv[0], "One"),
        (csv[1], "Two"),
        (ofx[0], "Three"),
        (ofx[1], "Four"),
    ] {
        g.name(row, name);
    }
    let pairs: Vec<_> = g
        .scan()
        .iter()
        .map(|c| (c.transaction_a, c.transaction_b))
        .collect();
    assert_eq!(pairs, [(csv[0], ofx[0]), (csv[1], ofx[1])]);
}

#[test]
fn detect_for_batch_runs_in_the_callers_transaction() {
    let mut f = Fixture::new();
    f.import(SourceFormat::Csv, &[line(5, -500, "SHOP A")]);
    let (batch, _) = f.import(SourceFormat::Ofx, &[with_id(5, -500, "Shop a ltd", "F")]);
    {
        let tx = f.db.conn_mut().transaction().unwrap();
        let found = detect_for_batch(&tx, batch, DuplicateWindow::DEFAULT).unwrap();
        assert_eq!(found.len(), 1);
        tx.rollback().unwrap();
    }
    assert!(f.pairs().is_empty());
    let tx = f.db.conn_mut().transaction().unwrap();
    assert!(matches!(
        detect_for_batch(&tx, BatchId::new(99), DuplicateWindow::DEFAULT),
        Err(Error::BatchNotFound(_))
    ));
    assert_eq!(
        detect_for_batch(&tx, batch, DuplicateWindow::DEFAULT)
            .unwrap()
            .len(),
        1
    );
    // Running it again finds nothing new.
    assert!(
        detect_for_batch(&tx, batch, DuplicateWindow::DEFAULT)
            .unwrap()
            .is_empty()
    );
    tx.commit().unwrap();
    assert_eq!(f.pairs().len(), 1);
}

#[test]
fn transfers_and_split_rows_take_part() {
    let mut f = Fixture::new();
    let savings = f.other_account("Savings");
    let (_, csv) = f.import(SourceFormat::Csv, &[line(5, -5000, "TO SAVINGS")]);
    // The import's transfer detection pairs it with the current account's
    // row as a suggested transfer.
    let (_, inflow) = f.import_into(savings, SourceFormat::Csv, &[line(5, 5000, "FROM CURRENT")]);
    let group: Option<i64> =
        f.db.conn()
            .query_row(
                "SELECT transfer_group_id FROM \"transaction\" WHERE id = ?1",
                [inflow[0]],
                |row| row.get(0),
            )
            .unwrap();
    assert!(group.is_some());
    let (ofx, found) =
        f.import_and_detect(SourceFormat::Ofx, &[with_id(5, -5000, "Transfer out", "F")]);
    assert_eq!(found.len(), 1);
    // The row in the transfer stays by default, though the other has a
    // source ID.
    assert_eq!(default_keep(&f.db, found[0].id).unwrap(), csv[0]);
    // Keeping the other one would exclude a transfer's side.
    let swapped = merge_preview(&f.db, &[MergeRequest::keeping(found[0].id, ofx[0])]);
    assert!(matches!(swapped, Err(Error::InTransfer { transaction, .. }) if transaction == csv[0]));
    let merged = f.merge(&[MergeRequest::new(found[0].id)]);
    assert_eq!(merged.merges[0].duplicate, ofx[0]);
}

// Merging ---------------------------------------------------------------------

#[test]
fn a_merge_keeps_the_best_sourced_row() {
    // A source ID beats none.
    let mut f = Fixture::new();
    f.import(SourceFormat::Csv, &[line(5, -500, "SHOP A")]);
    let (ofx, found) =
        f.import_and_detect(SourceFormat::Ofx, &[with_id(5, -500, "Shop a ltd", "F")]);
    assert_eq!(default_keep(&f.db, found[0].id).unwrap(), ofx[0]);

    // Imported beats manual.
    let mut f = Fixture::new();
    f.manual(5, -500, "Shop");
    let (rows, found) = f.import_and_detect(SourceFormat::Csv, &[line(5, -500, "SHOP A")]);
    assert_eq!(default_keep(&f.db, found[0].id).unwrap(), rows[0]);

    // Then the one imported first.
    let mut f = Fixture::new();
    let (_, first) = f.import(SourceFormat::Csv, &[with_id(5, -500, "SHOP A", "x1")]);
    let (_, found) = f.import_and_detect(SourceFormat::Ofx, &[with_id(5, -500, "Shop a ltd", "F")]);
    assert_eq!(default_keep(&f.db, found[0].id).unwrap(), first[0]);
}

#[test]
fn a_merge_excludes_the_duplicate_in_place_and_swapping_keeps_the_other() {
    let mut f = Fixture::new();
    let (_, csv) = f.import(SourceFormat::Csv, &[line(5, -500, "SHOP A")]);
    let (ofx, found) =
        f.import_and_detect(SourceFormat::Ofx, &[with_id(5, -500, "Shop a ltd", "F")]);
    let candidate = found[0].id;
    let rows_before: i64 =
        f.db.conn()
            .query_row("SELECT COUNT(*) FROM \"transaction\"", [], |row| row.get(0))
            .unwrap();

    let preview = merge_preview(&f.db, &[MergeRequest::keeping(candidate, csv[0])]).unwrap();
    assert_eq!(
        preview.merges,
        [PlannedMerge {
            candidate,
            kept: csv[0],
            duplicate: ofx[0],
            carried: vec![],
        }]
    );
    assert!(!preview.is_locked());
    assert_eq!(preview.changes.len(), 1);
    assert_eq!(preview.changes[0].transaction, ofx[0]);
    // The preview changed nothing.
    assert_eq!(f.exclusion(ofx[0]), None);
    assert_eq!(
        get(&f.db, candidate).unwrap().status,
        CandidateStatus::Pending
    );

    let merged = merge(&mut f.db, &preview).unwrap();
    assert_eq!(f.exclusion(ofx[0]), Some(ExclusionReason::Duplicate));
    assert_eq!(f.exclusion(csv[0]), None);
    let rows_after: i64 =
        f.db.conn()
            .query_row("SELECT COUNT(*) FROM \"transaction\"", [], |row| row.get(0))
            .unwrap();
    assert_eq!(rows_after, rows_before, "nothing is deleted");
    let stored = get(&f.db, candidate).unwrap();
    assert_eq!(stored.status, CandidateStatus::Merged);
    assert_eq!(stored.kept, Some(csv[0]));
    assert_eq!(stored.duplicate(), Some(ofx[0]));
    assert_eq!(stored.change_batch, Some(merged.batch));
    let batch = change::get(&f.db, merged.batch).unwrap();
    assert_eq!(batch.kind, ChangeKind::DuplicateMerge);

    // Merged candidates can't be merged or dismissed again.
    assert!(matches!(
        merge_preview(&f.db, &[MergeRequest::new(candidate)]),
        Err(Error::NotPending { .. })
    ));
    assert!(matches!(
        dismiss(&mut f.db, candidate),
        Err(Error::NotPending { .. })
    ));
    // A merged pair is never raised again.
    assert!(f.scan().is_empty());
}

#[test]
fn a_merge_refuses_a_row_outside_the_pair_repeats_and_nothing() {
    let mut f = Fixture::new();
    f.import(SourceFormat::Csv, &[line(5, -500, "SHOP A")]);
    let (_, found) = f.import_and_detect(SourceFormat::Ofx, &[with_id(5, -500, "Shop a ltd", "F")]);
    let candidate = found[0].id;
    let stranger = f.manual(20, -1, "Other");
    assert!(matches!(
        merge_preview(&f.db, &[MergeRequest::keeping(candidate, stranger)]),
        Err(Error::NotInPair { .. })
    ));
    assert!(matches!(
        merge_preview(
            &f.db,
            &[MergeRequest::new(candidate), MergeRequest::new(candidate)]
        ),
        Err(Error::Repeated(_))
    ));
    assert!(matches!(
        merge_preview(&f.db, &[]),
        Err(Error::NothingToMerge)
    ));
    assert!(matches!(
        merge_preview(&f.db, &[MergeRequest::new(DuplicateCandidateId::new(99))]),
        Err(Error::CandidateNotFound(_))
    ));
}

#[test]
fn a_merge_carries_what_the_user_set_and_undo_takes_it_back() {
    let mut f = Fixture::new();
    let cash = f.manual(5, -1250, "Weekly shop");
    f.set_category(cash, "food.groceries", "user");
    note::set(&mut f.db, cash, "Big shop").unwrap();
    let tag = tag::create(&mut f.db, &TagName::new("Holiday").unwrap(), false).unwrap();
    tag::add_to_transaction(&mut f.db, cash, tag).unwrap();
    f.db.conn()
        .execute(
            "UPDATE \"transaction\" SET reimbursable = 1, merchant_display = 'Tesco', \
             merchant_display_source = 'user' WHERE id = ?1",
            [cash],
        )
        .unwrap();
    let (rows, found) = f.import_and_detect(SourceFormat::Csv, &[line(6, -1250, "TESCO 123")]);
    let bank = rows[0];
    f.set_category(bank, "shopping", "rule");

    let merged = f.merge(&[MergeRequest::new(found[0].id)]);
    assert_eq!(merged.merges[0].kept, bank);
    assert_eq!(
        merged.merges[0].carried,
        [
            Field::Category,
            Field::Merchant,
            Field::UserNote,
            Field::Tags,
            Field::Reimbursable
        ]
    );
    let groceries = f.category("food.groceries");
    assert_eq!(
        f.read(bank, Field::Category),
        Value::Category(CategoryValue {
            category_id: Some(groceries),
            source: Some(CategorySource::User),
            rule_id: None,
        })
    );
    assert_eq!(
        f.read(bank, Field::UserNote),
        Value::UserNote(Some("Big shop".into()))
    );
    assert_eq!(f.read(bank, Field::Tags), Value::Tags(vec![tag]));
    assert_eq!(f.read(bank, Field::Reimbursable), Value::Reimbursable(true));
    assert_eq!(
        f.read(bank, Field::Merchant),
        Value::Merchant(MerchantValue {
            name: Some("Tesco".into()),
            source: Some(NameSource::User),
        })
    );
    // The duplicate keeps its own values.
    assert_eq!(
        f.read(cash, Field::UserNote),
        Value::UserNote(Some("Big shop".into()))
    );
    assert_eq!(f.exclusion(cash), Some(ExclusionReason::Duplicate));

    let summary = change::undo(&mut f.db, merged.batch).unwrap();
    assert_eq!((summary.restored, summary.changed_since), (6, 0));
    assert_eq!(summary.batch.status, BatchStatus::Undone);
    assert_eq!(f.exclusion(cash), None);
    assert_eq!(f.read(bank, Field::UserNote), Value::UserNote(None));
    assert_eq!(f.read(bank, Field::Tags), Value::Tags(vec![]));
    assert_eq!(
        f.read(bank, Field::Reimbursable),
        Value::Reimbursable(false)
    );
    let Value::Category(category) = f.read(bank, Field::Category) else {
        unreachable!()
    };
    assert_eq!(category.source, Some(CategorySource::Rule));
    let stored = get(&f.db, found[0].id).unwrap();
    assert_eq!(stored.status, CandidateStatus::Pending);
    assert_eq!((stored.kept, stored.change_batch), (None, None));
}

#[test]
fn a_merge_carries_a_split_and_never_overwrites_the_kept_rows_own_values() {
    let mut f = Fixture::new();
    let cash = f.manual(5, -1000, "Shop");
    let groceries = f.category("food.groceries");
    split::set(
        &mut f.db,
        cash,
        &[
            split::SplitLine::new(gbp(-600), Some("food.groceries".parse().unwrap())),
            split::SplitLine::new(gbp(-400), None),
        ],
    )
    .unwrap();
    let (rows, found) = f.import_and_detect(SourceFormat::Csv, &[line(5, -1000, "SHOP")]);
    let bank = rows[0];
    f.set_category(bank, "shopping", "preset_map");
    let merged = f.merge(&[MergeRequest::new(found[0].id)]);
    assert_eq!(merged.merges[0].carried, [Field::Split]);
    let Value::Split(lines) = f.read(bank, Field::Split) else {
        unreachable!()
    };
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].category_id, Some(groceries));
    assert_eq!(
        f.read(bank, Field::Category),
        Value::Category(CategoryValue::NONE)
    );
    change::undo(&mut f.db, merged.batch).unwrap();
    assert_eq!(f.read(bank, Field::Split), Value::Split(vec![]));
    let Value::Category(category) = f.read(bank, Field::Category) else {
        unreachable!()
    };
    assert_eq!(category.source, Some(CategorySource::PresetMap));

    // The kept row's own note and user category stay.
    let mut f = Fixture::new();
    let cash = f.manual(5, -1000, "Shop");
    f.set_category(cash, "food.groceries", "user");
    note::set(&mut f.db, cash, "Theirs").unwrap();
    let (rows, found) = f.import_and_detect(SourceFormat::Csv, &[line(5, -1000, "SHOP")]);
    f.set_category(rows[0], "shopping", "user");
    note::set(&mut f.db, rows[0], "Mine").unwrap();
    let merged = f.merge(&[MergeRequest::new(found[0].id)]);
    assert!(merged.merges[0].carried.is_empty());
    assert_eq!(
        f.read(rows[0], Field::UserNote),
        Value::UserNote(Some("Mine".into()))
    );
}

#[test]
fn several_candidates_merge_as_one_batch_and_undo_together() {
    let mut f = Fixture::new();
    let (_, csv) = f.import(
        SourceFormat::Csv,
        &[line(5, -500, "SHOP A"), line(6, -800, "SHOP B")],
    );
    let (ofx, found) = f.import_and_detect(
        SourceFormat::Ofx,
        &[
            with_id(5, -500, "Shop a ltd", "F1"),
            with_id(6, -800, "Shop b ltd", "F2"),
        ],
    );
    assert_eq!(found.len(), 2);
    let merged = f.merge(&[
        MergeRequest::new(found[1].id),
        MergeRequest::keeping(found[0].id, csv[0]),
    ]);
    assert_eq!(merged.merges.len(), 2);
    assert_eq!(merged.merges[0].candidate, found[0].id, "by candidate ID");
    assert_eq!(f.exclusion(ofx[0]), Some(ExclusionReason::Duplicate));
    assert_eq!(f.exclusion(csv[1]), Some(ExclusionReason::Duplicate));
    for candidate in &found {
        assert_eq!(
            get(&f.db, candidate.id).unwrap().change_batch,
            Some(merged.batch)
        );
    }
    change::undo(&mut f.db, merged.batch).unwrap();
    assert!(
        found
            .iter()
            .all(|candidate| get(&f.db, candidate.id).unwrap().status == CandidateStatus::Pending)
    );
    assert_eq!(f.exclusion(ofx[0]), None);
    assert_eq!(f.exclusion(csv[1]), None);
}

#[test]
fn a_row_kept_in_one_merge_and_excluded_in_another_conflicts() {
    let mut f = Fixture::new();
    let (_, first) = f.import(SourceFormat::Csv, &[line(5, -500, "SHOP A")]);
    let (second, found_one) =
        f.import_and_detect(SourceFormat::Ofx, &[with_id(5, -500, "Shop a ltd", "F")]);
    let (third, found_two) =
        f.import_and_detect(SourceFormat::Csv, &[line(6, -500, "SHOP A LONDON")]);
    // (first, third) is the new pair; second is in the earlier one.
    assert_eq!(
        (found_two[0].transaction_a, found_two[0].transaction_b),
        (first[0], third[0])
    );
    let result = merge_preview(
        &f.db,
        &[
            MergeRequest::keeping(found_one[0].id, second[0]),
            MergeRequest::keeping(found_two[0].id, first[0]),
        ],
    );
    assert!(matches!(result, Err(Error::Conflict(id)) if id == first[0]));
    // Keeping one row against both copies is fine.
    let merged = f.merge(&[
        MergeRequest::keeping(found_one[0].id, first[0]),
        MergeRequest::keeping(found_two[0].id, first[0]),
    ]);
    assert_eq!(merged.merges.len(), 2);
}

#[test]
fn a_merge_refuses_excluded_rows_and_rows_that_no_longer_match() {
    let mut f = Fixture::new();
    let cash = f.manual(5, -500, "Shop");
    let (rows, found) = f.import_and_detect(SourceFormat::Csv, &[line(5, -500, "SHOP A")]);
    exclusion::exclude(&mut f.db, rows[0], ExclusionReason::Disputed, None).unwrap();
    assert!(matches!(
        merge_preview(&f.db, &[MergeRequest::new(found[0].id)]),
        Err(Error::Excluded { transaction, .. }) if transaction == rows[0]
    ));
    exclusion::include_again(&mut f.db, rows[0]).unwrap();
    manual::update(
        &mut f.db,
        cash,
        &ManualTransaction::new(f.account, day(5), gbp(-501), "Shop"),
    )
    .unwrap();
    assert!(matches!(
        merge_preview(&f.db, &[MergeRequest::new(found[0].id)]),
        Err(Error::NoLongerAMatch(_))
    ));
}

#[test]
fn a_merge_refuses_a_stale_preview() {
    let mut f = Fixture::new();
    let cash = f.manual(5, -500, "Shop");
    note::set(&mut f.db, cash, "Theirs").unwrap();
    let (rows, found) = f.import_and_detect(SourceFormat::Csv, &[line(5, -500, "SHOP A")]);
    let preview = merge_preview(&f.db, &[MergeRequest::new(found[0].id)]).unwrap();
    assert_eq!(preview.merges[0].carried, [Field::UserNote]);
    note::set(&mut f.db, rows[0], "Mine").unwrap();
    assert!(matches!(merge(&mut f.db, &preview), Err(Error::Stale)));
    assert_eq!(
        get(&f.db, found[0].id).unwrap().status,
        CandidateStatus::Pending
    );
    // Dismissed since: not pending.
    let preview = merge_preview(&f.db, &[MergeRequest::new(found[0].id)]).unwrap();
    dismiss(&mut f.db, found[0].id).unwrap();
    assert!(matches!(
        merge(&mut f.db, &preview),
        Err(Error::NotPending { .. })
    ));
}

#[test]
fn locks_flag_the_merge_and_refuse_it_and_its_undo() {
    let mut f = Fixture::new();
    f.import(SourceFormat::Csv, &[line(5, -500, "SHOP A")]);
    let (ofx, found) =
        f.import_and_detect(SourceFormat::Ofx, &[with_id(5, -500, "Shop a ltd", "F")]);
    let request = [MergeRequest::new(found[0].id)];
    f.lock(5);
    let preview = merge_preview(&f.db, &request).unwrap();
    // The OFX row has a source ID, so it stays; the CSV row is the duplicate.
    let duplicate = preview.merges[0].duplicate;
    assert_ne!(duplicate, ofx[0]);
    assert_eq!(
        preview.locked,
        [LockedRow {
            candidate: found[0].id,
            transaction: duplicate,
            date: day(5),
            through: day(5),
        }]
    );
    assert!(preview.changes.is_empty());
    assert!(matches!(
        merge(&mut f.db, &preview),
        Err(Error::Locked { transaction, .. }) if transaction == duplicate
    ));
    assert_eq!(f.exclusion(duplicate), None);

    // A lock before the row's date doesn't count.
    f.lock(4);
    let merged = f.merge(&request);
    f.lock(5);
    assert!(matches!(
        change::undo(&mut f.db, merged.batch),
        Err(change::Error::Locked { .. })
    ));
    assert_eq!(
        get(&f.db, found[0].id).unwrap().status,
        CandidateStatus::Merged
    );
    f.unlock();
    change::undo(&mut f.db, merged.batch).unwrap();
    assert_eq!(f.exclusion(duplicate), None);
}

#[test]
fn undo_skips_a_duplicate_changed_since_and_the_candidate_is_pending_again() {
    let mut f = Fixture::new();
    f.import(SourceFormat::Csv, &[line(5, -500, "SHOP A")]);
    let (_, found) = f.import_and_detect(SourceFormat::Ofx, &[with_id(5, -500, "Shop a ltd", "F")]);
    let merged = f.merge(&[MergeRequest::new(found[0].id)]);
    let duplicate = merged.merges[0].duplicate;
    exclusion::exclude(
        &mut f.db,
        duplicate,
        ExclusionReason::Other,
        Some("Not mine"),
    )
    .unwrap();
    let summary = change::undo(&mut f.db, merged.batch).unwrap();
    assert_eq!((summary.restored, summary.changed_since), (0, 1));
    assert_eq!(f.exclusion(duplicate), Some(ExclusionReason::Other));
    let stored = get(&f.db, found[0].id).unwrap();
    assert_eq!(stored.status, CandidateStatus::Pending);
    // Its row is excluded, so it can't be merged now; it can be dismissed.
    assert!(matches!(
        merge_preview(&f.db, &[MergeRequest::new(found[0].id)]),
        Err(Error::Excluded { .. })
    ));
    dismiss(&mut f.db, found[0].id).unwrap();
}

// Deleting --------------------------------------------------------------------

#[test]
fn deleting_a_manual_row_clears_its_candidates() {
    let mut f = Fixture::new();
    let cash = f.manual(5, -500, "Shop");
    let (rows, found) = f.import_and_detect(SourceFormat::Csv, &[line(5, -500, "SHOP A")]);
    assert_eq!(found.len(), 1);
    f.merge(&[MergeRequest::keeping(found[0].id, rows[0])]);
    manual::delete(&mut f.db, cash).unwrap();
    assert!(f.pairs().is_empty());
    assert!(matches!(
        get(&f.db, found[0].id),
        Err(Error::CandidateNotFound(_))
    ));
}

#[test]
fn clear_batch_clears_the_candidates_of_an_imports_rows() {
    let mut f = Fixture::new();
    f.import(SourceFormat::Csv, &[line(5, -500, "SHOP A")]);
    let (batch, _) = f.import(
        SourceFormat::Ofx,
        &[
            with_id(5, -500, "Shop a ltd", "F1"),
            with_id(9, -1, "x", "F2"),
        ],
    );
    f.manual(9, -1, "y");
    assert_eq!(f.scan().len(), 2);
    let tx = f.db.conn_mut().transaction().unwrap();
    assert_eq!(clear_batch(&tx, batch).unwrap(), 2);
    tx.commit().unwrap();
    assert!(f.pairs().is_empty());
}
