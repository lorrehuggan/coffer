//! Tests for preview, commit and undo against a real database.

use chrono::NaiveDate;
use rusqlite::params;
use tempfile::TempDir;

use super::*;
use crate::account::{AccountKind, EnteredBalance, EnteredSource, NewAccount};
use crate::balance::CheckResult;
use crate::statement::{DatedBalance, Field};

/// `(id, account_id, date, amount_minor, description_raw, external_id,
/// fingerprint)`.
type StoredRow = (i64, i64, String, i64, String, Option<String>, String);

struct Fixture {
    db: Database,
    _dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("coffer.db")).unwrap();
        Fixture { db, _dir: dir }
    }

    fn account(&mut self, name: &str) -> AccountId {
        account::create(
            &mut self.db,
            &NewAccount::new(name, AccountKind::Current, Currency::GBP),
        )
        .unwrap()
    }

    fn anchor(&mut self, account: AccountId, d: u32, minor: i64) {
        account::add_balance(
            &mut self.db,
            account,
            &EnteredBalance {
                date: day(d),
                balance: gbp(minor),
                source: EnteredSource::Manual,
                note: None,
            },
        )
        .unwrap();
    }

    fn preview(&self, account: AccountId, statement: Statement) -> ImportPreview {
        preview(&self.db, account, "statement.csv", statement).unwrap()
    }

    /// Previews and commits, confirming whatever the preview asks for.
    fn import(&mut self, account: AccountId, statement: Statement) -> Committed {
        let preview = self.preview(account, statement);
        let confirm = preview.needs_confirmation();
        commit(&mut self.db, &preview, confirm).unwrap()
    }

    fn import_batch(&mut self, account: AccountId, statement: Statement) -> CommitSummary {
        match self.import(account, statement) {
            Committed::Batch(summary) => summary,
            Committed::NothingNew => panic!("expected a new batch"),
        }
    }

    fn count(&self, sql: &str) -> i64 {
        self.db.conn().query_row(sql, [], |row| row.get(0)).unwrap()
    }

    fn sql(&self, sql: &str) {
        self.db.conn().execute_batch(sql).unwrap();
    }

    /// Every transaction's source fields and keys, by ID.
    fn transactions(&self) -> Vec<StoredRow> {
        let mut statement = self
            .db
            .conn()
            .prepare(
                "SELECT id, account_id, date, amount_minor, description_raw, external_id, \
                 fingerprint FROM \"transaction\" ORDER BY id",
            )
            .unwrap();
        statement
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }
}

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn line(d: u32, minor: i64, description: &str) -> StatementLine {
    StatementLine::new(day(d), gbp(minor), description)
}

fn with_id(mut line: StatementLine, id: &str) -> StatementLine {
    line.external_id = Some(id.to_owned());
    line
}

fn with_balance(mut line: StatementLine, minor: i64) -> StatementLine {
    line.balance_after = Some(gbp(minor));
    line
}

/// A CSV statement, a header on line 1 and one row per line after it.
fn csv(rows: Vec<Result<StatementLine, RowProblem>>) -> Statement {
    let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
    for (index, row) in rows.into_iter().enumerate() {
        statement.push(SourceLocation::Line(index as u64 + 2), row);
    }
    statement
}

fn lines(lines: Vec<StatementLine>) -> Statement {
    csv(lines.into_iter().map(Ok).collect())
}

/// An OFX statement with a closing balance.
fn ofx(lines: Vec<StatementLine>, closing: Option<(u32, i64)>) -> Statement {
    let mut statement = Statement::new(SourceFormat::Ofx, Currency::GBP);
    for (index, line) in lines.into_iter().enumerate() {
        statement.push(SourceLocation::Entry(index as u64 + 1), Ok(line));
    }
    if let Some((d, minor)) = closing {
        statement
            .push_balance(DatedBalance {
                date: day(d),
                balance: gbp(minor),
            })
            .unwrap();
    }
    statement
}

fn outcomes(preview: &ImportPreview) -> Vec<RowOutcome> {
    preview.rows().map(|row| row.outcome.clone()).collect()
}

fn is_new(outcome: &RowOutcome) -> bool {
    matches!(outcome, RowOutcome::New { .. })
}

fn already(outcome: &RowOutcome) -> Option<(TransactionId, MatchedBy)> {
    match outcome {
        RowOutcome::AlreadyImported {
            transaction,
            matched_by,
        } => Some((*transaction, *matched_by)),
        _ => None,
    }
}

// Dedupe ----------------------------------------------------------------------

#[test]
fn importing_the_same_file_again_adds_nothing() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    let file = || {
        lines(vec![
            line(1, -1240, "Tesco Stores"),
            line(2, 250_000, "Salary"),
            line(3, -500, "Coffee"),
        ])
    };

    let first = fixture.preview(account, file());
    assert_eq!(
        first.counts(),
        PreviewCounts {
            new: 3,
            already_imported: 0,
            flagged: 0
        }
    );
    assert_eq!(first.needs_confirmation(), Confirmations::default());
    let summary = fixture.import_batch(account, file());
    assert_eq!(summary.transactions, 3);
    let stored = fixture.transactions();

    let again = fixture.preview(account, file());
    assert_eq!(
        again.counts(),
        PreviewCounts {
            new: 0,
            already_imported: 3,
            flagged: 0
        }
    );
    let matched: Vec<_> = outcomes(&again).iter().map(already).collect();
    assert_eq!(
        matched,
        stored
            .iter()
            .map(|row| Some((TransactionId::new(row.0), MatchedBy::Fingerprint)))
            .collect::<Vec<_>>()
    );
    assert!(again.is_nothing_new());
    assert_eq!(
        commit(&mut fixture.db, &again, Confirmations::default()).unwrap(),
        Committed::NothingNew
    );
    assert_eq!(fixture.transactions(), stored);
    assert_eq!(fixture.count("SELECT COUNT(*) FROM import_batch"), 1);
}

#[test]
fn identical_rows_in_one_file_both_import() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    let file = || {
        lines(vec![
            line(2, -500, "Coffee"),
            line(2, -500, "Coffee"),
            line(2, -500, "COFFEE "),
        ])
    };

    let preview = fixture.preview(account, file());
    let fingerprints: Vec<_> = outcomes(&preview)
        .into_iter()
        .map(|outcome| match outcome {
            RowOutcome::New { fingerprint } => fingerprint,
            other => panic!("expected new, got {other:?}"),
        })
        .collect();
    assert_eq!(
        fingerprints,
        [
            "v1|2026-11-02|-500|GBP|0|COFFEE",
            "v1|2026-11-02|-500|GBP|1|COFFEE",
            "v1|2026-11-02|-500|GBP|2|COFFEE",
        ]
    );
    fixture.import_batch(account, file());
    assert_eq!(fixture.count("SELECT COUNT(*) FROM \"transaction\""), 3);

    // Each matches its own stored row again.
    let again = fixture.preview(account, file());
    let matched: Vec<_> = outcomes(&again)
        .iter()
        .map(|outcome| already(outcome).unwrap().0)
        .collect();
    let ids: Vec<_> = fixture
        .transactions()
        .iter()
        .map(|row| TransactionId::new(row.0))
        .collect();
    assert_eq!(matched, ids);
}

#[test]
fn overlapping_files_add_only_the_rows_not_yet_imported() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    // 1–10 November, with two identical coffees on the 7th.
    fixture.import_batch(
        account,
        lines(vec![
            line(1, -100, "A"),
            line(5, -200, "B"),
            line(7, -500, "Coffee"),
            line(7, -500, "Coffee"),
            line(10, -300, "C"),
        ]),
    );

    // 5–15 November, newest first, with a third coffee on the 7th.
    let overlap = lines(vec![
        line(15, -400, "D"),
        line(10, -300, "C"),
        line(7, -500, "Coffee"),
        line(7, -500, "Coffee"),
        line(7, -500, "Coffee"),
        line(5, -200, "B"),
    ]);
    let preview = fixture.preview(account, overlap.clone());
    let new: Vec<_> = outcomes(&preview).iter().map(is_new).collect();
    assert_eq!(new, [true, false, false, false, true, false]);

    fixture.import_batch(account, overlap.clone());
    assert_eq!(fixture.count("SELECT COUNT(*) FROM \"transaction\""), 7);
    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM \"transaction\" WHERE date = '2026-11-07'"),
        3
    );
    assert!(fixture.preview(account, overlap).is_nothing_new());
}

#[test]
fn source_ids_come_first_and_never_match_by_fingerprint() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    fixture.import_batch(
        account,
        lines(vec![
            with_id(line(1, -500, "Coffee"), "tx_1"),
            with_id(line(2, -900, "Lunch"), "tx_2"),
            line(3, -100, "No ID"),
        ]),
    );
    let ids: Vec<_> = fixture
        .transactions()
        .iter()
        .map(|row| TransactionId::new(row.0))
        .collect();

    let preview = fixture.preview(
        account,
        lines(vec![
            // The same ID, date and amount: a bank re-wording the
            // description doesn't make it new.
            with_id(line(1, -500, "COFFEE SHOP LTD"), "tx_1"),
            // The same ID for a different amount: flagged, not skipped.
            with_id(line(2, -950, "Lunch"), "tx_2"),
            // A new ID on a row identical to tx_1: a second coffee.
            with_id(line(1, -500, "Coffee"), "tx_3"),
            // An ID on the row that had none: not matched by fingerprint.
            with_id(line(3, -100, "No ID"), "tx_4"),
            // The file repeats tx_3 exactly, then reuses it differently.
            with_id(line(1, -500, "Coffee"), "tx_3"),
            with_id(line(4, -1, "Other"), "tx_3"),
        ]),
    );
    assert_eq!(
        outcomes(&preview),
        [
            RowOutcome::AlreadyImported {
                transaction: ids[0],
                matched_by: MatchedBy::SourceId,
            },
            RowOutcome::Flagged(Flag::SourceIdConflict { existing: ids[1] }),
            RowOutcome::New {
                fingerprint: "v1|2026-11-01|-500|GBP|0|COFFEE".into()
            },
            RowOutcome::New {
                fingerprint: "v1|2026-11-03|-100|GBP|0|NO ID".into()
            },
            RowOutcome::RepeatedInFile {
                first: SourceLocation::Line(4)
            },
            RowOutcome::Flagged(Flag::SourceIdRepeated {
                first: SourceLocation::Line(4)
            }),
        ]
    );
    assert!(preview.needs_confirmation().flagged_rows);

    let summary = match commit(&mut fixture.db, &preview, preview.needs_confirmation()).unwrap() {
        Committed::Batch(summary) => summary,
        other => panic!("{other:?}"),
    };
    assert_eq!((summary.transactions, summary.flags), (2, 2));
    let flags = batch_flags(&fixture.db, summary.batch).unwrap();
    assert_eq!(
        flags
            .iter()
            .map(|flag| (flag.location, flag.kind))
            .collect::<Vec<_>>(),
        [
            (SourceLocation::Line(3), FlagKind::SourceIdConflict),
            (SourceLocation::Line(7), FlagKind::SourceIdConflict),
        ]
    );
    assert_eq!(
        flags[1].detail,
        "line 4 has the same ID, on another date or for another amount"
    );
}

#[test]
fn source_ids_are_per_account() {
    let mut fixture = Fixture::new();
    let current = fixture.account("Current");
    let savings = fixture.account("Savings");
    let file = || lines(vec![with_id(line(1, -500, "Coffee"), "tx_1")]);

    fixture.import_batch(current, file());
    let preview = fixture.preview(savings, file());
    assert!(is_new(&outcomes(&preview)[0]));
    fixture.import_batch(savings, file());
    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM \"transaction\" WHERE external_id = 'tx_1'"),
        2
    );
    assert!(fixture.preview(current, file()).is_nothing_new());
    assert!(fixture.preview(savings, file()).is_nothing_new());
}

#[test]
fn excluded_rows_still_count_as_imported() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    let file = || lines(vec![line(1, -500, "Coffee")]);
    fixture.import_batch(account, file());
    fixture.sql(
        "UPDATE \"transaction\" SET excluded_reason = 'bank_error', \
         excluded_at = '2026-11-11T09:00:00.000Z'",
    );
    assert!(fixture.preview(account, file()).is_nothing_new());
}

// Flagged rows and the commit -------------------------------------------------

#[test]
fn unreadable_rows_need_confirmation_and_are_recorded() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    let statement = csv(vec![
        Ok(line(1, -500, "Coffee")),
        Err(RowProblem::Missing(Field::Amount)),
        Ok(line(2, -900, "Lunch")),
    ]);
    let preview = fixture.preview(account, statement);
    assert_eq!(
        preview.counts(),
        PreviewCounts {
            new: 2,
            already_imported: 0,
            flagged: 1
        }
    );
    let row = preview.rows().nth(1).unwrap();
    assert_eq!(row.location, SourceLocation::Line(3));
    assert!(row.line.is_none());
    assert_eq!(
        row.outcome,
        &RowOutcome::Flagged(Flag::Unreadable(RowProblem::Missing(Field::Amount)))
    );

    let needed = Confirmations {
        flagged_rows: true,
        balance_mismatch: false,
    };
    assert_eq!(preview.needs_confirmation(), needed);
    let refused = commit(&mut fixture.db, &preview, Confirmations::default());
    assert!(matches!(refused, Err(Error::NotConfirmed(n)) if n == needed));
    assert_eq!(fixture.count("SELECT COUNT(*) FROM import_batch"), 0);

    let Committed::Batch(summary) = commit(&mut fixture.db, &preview, needed).unwrap() else {
        panic!("expected a batch");
    };
    assert_eq!((summary.transactions, summary.flags), (2, 1));
    assert_eq!(
        batch_flags(&fixture.db, summary.batch).unwrap(),
        [RecordedFlag {
            location: SourceLocation::Line(3),
            kind: FlagKind::Unreadable,
            detail: "the amount is missing".into(),
        }]
    );
}

#[test]
fn the_file_and_account_must_fit() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    let house = account::create(
        &mut fixture.db,
        &NewAccount::new("House", AccountKind::Asset, Currency::GBP),
    )
    .unwrap();
    let file = || lines(vec![line(1, -500, "Coffee")]);

    let euros = Statement::new(SourceFormat::Csv, Currency::EUR);
    assert!(matches!(
        preview(&fixture.db, account, "a.csv", euros),
        Err(Error::CurrencyMismatch {
            account: Currency::GBP,
            statement: Currency::EUR
        })
    ));
    assert!(matches!(
        preview(&fixture.db, house, "a.csv", file()),
        Err(Error::HoldsValuations("asset"))
    ));
    assert!(matches!(
        preview(&fixture.db, AccountId::new(99), "a.csv", file()),
        Err(Error::AccountNotFound(_))
    ));
    assert!(matches!(
        preview(&fixture.db, account, "dir/", file()),
        Err(Error::NoFileName)
    ));

    let preview = preview(&fixture.db, account, "/home/someone/Nov.csv", file()).unwrap();
    assert_eq!(preview.source_file(), "Nov.csv");
    let Committed::Batch(summary) =
        commit(&mut fixture.db, &preview, Confirmations::default()).unwrap()
    else {
        panic!("expected a batch");
    };
    let info = batch(&fixture.db, summary.batch).unwrap();
    assert_eq!(info.source_file, "Nov.csv");
    assert_eq!(info.format, SourceFormat::Csv);
    assert_eq!(info.status, BatchStatus::Committed);
    assert_eq!(info.transactions, 1);
}

#[test]
fn imported_rows_keep_every_source_field() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    let mut full = with_id(line(2, -860, "  Café  du Coin "), "tx_9");
    full.time = chrono::NaiveTime::from_hms_opt(9, 30, 15);
    full.merchant = Some("Café du Coin".into());
    full.source_category = Some("eating_out".into());
    full.source_type = Some("card_payment".into());
    full.original_amount = Some(Money::new(-1000, Currency::EUR));
    full.note = Some("team lunch".into());
    let statement = lines(vec![full]);
    let preview = preview(
        &fixture.db,
        account,
        "nov.csv",
        statement.with_preset("uk-x"),
    )
    .unwrap();
    commit(&mut fixture.db, &preview, Confirmations::default()).unwrap();

    let stored = fixture
        .db
        .conn()
        .query_row(
            "SELECT t.date, t.time, t.amount_minor, t.currency, t.merchant, t.description_raw, \
             t.description_normalized, t.external_id, t.fingerprint, t.source_category, \
             t.source_type, t.original_amount_minor, t.original_currency, t.note, t.origin, \
             t.created_at = b.imported_at, b.preset_id \
             FROM \"transaction\" t JOIN import_batch b ON b.id = t.batch_id",
            [],
            |row| {
                Ok((
                    (
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                    ),
                    (
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, String>(10)?,
                        row.get::<_, i64>(11)?,
                        row.get::<_, String>(12)?,
                        row.get::<_, String>(13)?,
                        row.get::<_, String>(14)?,
                        row.get::<_, bool>(15)?,
                        row.get::<_, String>(16)?,
                    ),
                ))
            },
        )
        .unwrap();
    assert_eq!(
        stored,
        (
            (
                "2026-11-02".into(),
                "09:30:15".into(),
                -860,
                "GBP".into(),
                "Café du Coin".into(),
                "  Café  du Coin ".into(),
                "CAFÉ DU COIN".into(),
                "tx_9".into(),
            ),
            (
                "v1|2026-11-02|-860|GBP|0|CAFÉ DU COIN".into(),
                "eating_out".into(),
                "card_payment".into(),
                -1000,
                "EUR".into(),
                "team lunch".into(),
                "imported".into(),
                true,
                "uk-x".into(),
            )
        )
    );
}

#[test]
fn a_stale_preview_is_refused() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    let file = || lines(vec![line(1, -500, "Coffee")]);
    let first = fixture.preview(account, file());
    let second = fixture.preview(account, file());

    commit(&mut fixture.db, &first, Confirmations::default()).unwrap();
    assert!(matches!(
        commit(&mut fixture.db, &second, Confirmations::default()),
        Err(Error::Stale)
    ));
    assert_eq!(fixture.count("SELECT COUNT(*) FROM \"transaction\""), 1);

    // A lock set after the preview changes it too.
    let third = fixture.preview(account, lines(vec![line(2, -1, "Tea")]));
    fixture.sql("UPDATE account SET reconciled_through = '2026-11-05'");
    assert!(matches!(
        commit(&mut fixture.db, &third, Confirmations::default()),
        Err(Error::Stale)
    ));
}

#[test]
fn a_failed_commit_leaves_nothing() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    let statement = csv(vec![
        Ok(with_balance(line(1, -500, "Coffee"), 9_500)),
        Err(RowProblem::Missing(Field::Date)),
        Ok(line(2, -900, "Lunch")),
        Ok(line(3, -100, "boom")),
        Ok(line(4, -200, "After")),
    ]);
    let preview = fixture.preview(account, statement);
    assert_eq!(preview.balances().len(), 1, "the opening balance");

    // The fourth insert fails, after a batch, two rows and a flag.
    fixture.sql(
        "CREATE TEMP TRIGGER boom BEFORE INSERT ON main.\"transaction\" \
         WHEN NEW.description_raw = 'boom' BEGIN SELECT RAISE(ABORT, 'boom'); END;",
    );
    let failed = commit(&mut fixture.db, &preview, preview.needs_confirmation());
    assert!(matches!(failed, Err(Error::Database(_))), "{failed:?}");
    for table in [
        "import_batch",
        "\"transaction\"",
        "import_flag",
        "balance_snapshot",
    ] {
        assert_eq!(
            fixture.count(&format!("SELECT COUNT(*) FROM {table}")),
            0,
            "{table}"
        );
    }

    fixture.sql("DROP TRIGGER temp.boom");
    let Committed::Batch(summary) =
        commit(&mut fixture.db, &preview, preview.needs_confirmation()).unwrap()
    else {
        panic!("expected a batch");
    };
    assert_eq!(
        (summary.transactions, summary.flags, summary.balances),
        (4, 1, 1)
    );
}

#[test]
fn rows_in_a_locked_period_stay_out() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    fixture.sql("UPDATE account SET reconciled_through = '2026-11-05'");
    let file = || {
        lines(vec![
            line(5, -100, "On the lock date"),
            line(6, -200, "After it"),
        ])
    };
    let preview = fixture.preview(account, file());
    assert_eq!(
        outcomes(&preview)[0],
        RowOutcome::Flagged(Flag::Locked { through: day(5) })
    );
    assert!(is_new(&outcomes(&preview)[1]));
    let summary = fixture.import_batch(account, file());
    assert_eq!((summary.transactions, summary.flags), (1, 1));
    assert_eq!(
        batch_flags(&fixture.db, summary.batch).unwrap()[0].kind,
        FlagKind::Locked
    );

    // Unlocked, the row comes in; the other is already there.
    fixture.sql("UPDATE account SET reconciled_through = NULL");
    let preview = fixture.preview(account, file());
    assert_eq!(preview.counts().new, 1);
    assert_eq!(preview.counts().already_imported, 1);
}

// Reconciliation --------------------------------------------------------------

#[test]
fn a_file_without_balances_is_not_reconciled() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    let preview = fixture.preview(account, lines(vec![line(1, -500, "Coffee")]));
    assert_eq!(preview.reconciliation().running, None);
    assert_eq!(preview.reconciliation().checks, []);
    assert_eq!(
        preview.reconciliation().status(),
        ReconciliationStatus::NoBalances
    );
    assert_eq!(preview.balances(), []);
}

#[test]
fn running_balances_reconcile_and_record_the_opening() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    // Newest first, as many banks export.
    let file = || {
        lines(vec![
            with_balance(line(3, 1_000, "Refund"), 10_250),
            with_balance(line(2, -250, "Tea"), 9_250),
            with_balance(line(1, -500, "Coffee"), 9_500),
        ])
    };
    let preview = fixture.preview(account, file());
    let reconciliation = preview.reconciliation();
    let running = reconciliation.running.as_ref().unwrap();
    assert_eq!(running.order, FileOrder::NewestFirst);
    assert_eq!((running.checked, running.mismatches.len()), (2, 0));
    assert_eq!(
        preview.balances(),
        [FileBalance {
            date: NaiveDate::from_ymd_opt(2026, 10, 31).unwrap(),
            balance: gbp(10_000),
            kind: FileBalanceKind::Opening,
            status: FileBalanceStatus::New,
        }]
    );
    assert_eq!(
        reconciliation.checks,
        [BalanceCheck {
            balance: CheckedBalance::FromFile(0),
            date: NaiveDate::from_ymd_opt(2026, 10, 31).unwrap(),
            stated: gbp(10_000),
            before: None,
            after: CheckResult::StartingPoint,
        }]
    );
    assert_eq!(reconciliation.status(), ReconciliationStatus::Reconciled);

    let summary = fixture.import_batch(account, file());
    assert_eq!(summary.balances, 1);
    assert_eq!(
        fixture.count(
            "SELECT COUNT(*) FROM balance_snapshot \
             WHERE source = 'csv' AND date = '2026-10-31' AND balance_minor = 10000"
        ),
        1
    );

    // Again: the balance is already recorded, so nothing is new.
    let again = fixture.preview(account, file());
    assert!(matches!(
        again.balances()[0].status,
        FileBalanceStatus::AlreadyRecorded(_)
    ));
    assert!(again.is_nothing_new());
    assert_eq!(
        again.reconciliation().status(),
        ReconciliationStatus::Reconciled
    );

    // The next month's file opens where this one ended, and is checked.
    let next = fixture.preview(
        account,
        lines(vec![with_balance(line(4, -50, "Bus"), 10_200)]),
    );
    assert_eq!(
        next.reconciliation().checks[0].after,
        CheckResult::Passed {
            since: NaiveDate::from_ymd_opt(2026, 10, 31).unwrap()
        }
    );
    assert_eq!(
        next.reconciliation().status(),
        ReconciliationStatus::Reconciled
    );
}

#[test]
fn a_running_balance_mismatch_is_flagged_not_committed_silently() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    let preview = fixture.preview(
        account,
        lines(vec![
            with_balance(line(1, -500, "Coffee"), 9_500),
            with_balance(line(2, -250, "Tea"), 9_000),
        ]),
    );
    let running = preview.reconciliation().running.clone().unwrap();
    assert_eq!(
        running.mismatches,
        [RunningMismatch {
            location: SourceLocation::Line(3),
            computed: gbp(9_250),
            stated: gbp(9_000),
        }]
    );
    assert_eq!(
        preview.reconciliation().status(),
        ReconciliationStatus::Mismatch
    );
    let needed = preview.needs_confirmation();
    assert_eq!(
        needed,
        Confirmations {
            flagged_rows: false,
            balance_mismatch: true
        }
    );
    assert!(matches!(
        commit(&mut fixture.db, &preview, Confirmations::default()),
        Err(Error::NotConfirmed(_))
    ));

    let Committed::Batch(summary) = commit(&mut fixture.db, &preview, needed).unwrap() else {
        panic!("expected a batch");
    };
    assert_eq!((summary.transactions, summary.flags), (2, 1));
    assert_eq!(
        batch_flags(&fixture.db, summary.batch).unwrap(),
        [RecordedFlag {
            location: SourceLocation::Line(3),
            kind: FlagKind::RunningBalance,
            detail: "the running balance is 90.00 GBP, but the rows before it give 92.50 GBP"
                .into(),
        }]
    );
}

#[test]
fn a_closing_balance_is_checked_against_the_account() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    fixture.anchor(account, 1, 10_000);
    let rows = || vec![line(2, -500, "Coffee"), line(5, 1_000, "Refund")];

    let good = fixture.preview(account, ofx(rows(), Some((5, 10_500))));
    assert_eq!(
        good.reconciliation().checks,
        [BalanceCheck {
            balance: CheckedBalance::FromFile(0),
            date: day(5),
            stated: gbp(10_500),
            before: None,
            after: CheckResult::Passed { since: day(1) },
        }]
    );
    assert_eq!(
        good.reconciliation().status(),
        ReconciliationStatus::Reconciled
    );

    let bad = fixture.preview(account, ofx(rows(), Some((5, 10_458))));
    let check = bad.reconciliation().checks[0];
    assert_eq!(
        check.after,
        CheckResult::Failed {
            since: day(1),
            computed_minor: 10_500,
        }
    );
    assert_eq!(check.computed(), Some(gbp(10_500)));
    assert_eq!(
        bad.reconciliation().status(),
        ReconciliationStatus::Mismatch
    );
    assert!(bad.needs_confirmation().balance_mismatch);

    // Committed with confirmation, the balance is stored as an OFX one
    // and goes on failing until something explains the gap.
    let summary = match commit(&mut fixture.db, &bad, bad.needs_confirmation()).unwrap() {
        Committed::Batch(summary) => summary,
        other => panic!("{other:?}"),
    };
    assert_eq!(summary.balances, 1);
    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM balance_snapshot WHERE source = 'ofx'"),
        1
    );
    // A row excluded as a duplicate leaves the computed balance.
    fixture.sql(
        "UPDATE \"transaction\" SET excluded_reason = 'duplicate', \
         excluded_at = '2026-11-11T09:00:00.000Z' WHERE amount_minor = 1000",
    );
    let after = fixture.preview(account, ofx(vec![], Some((6, 9_500))));
    assert_eq!(
        after.reconciliation().checks.last().unwrap().after,
        CheckResult::Passed { since: day(1) },
        "10_000 - 500, without the excluded refund"
    );
}

#[test]
fn the_first_balance_of_an_account_is_its_starting_point() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    let preview = fixture.preview(
        account,
        ofx(vec![line(2, -500, "Coffee")], Some((5, 10_000))),
    );
    assert_eq!(
        preview.reconciliation().checks[0].after,
        CheckResult::StartingPoint
    );
    assert_eq!(
        preview.reconciliation().status(),
        ReconciliationStatus::StartingPoint
    );
    assert_eq!(preview.needs_confirmation(), Confirmations::default());
}

#[test]
fn an_entered_anchor_checks_files_without_balances() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    fixture.anchor(account, 1, 10_000);
    fixture.anchor(account, 10, 9_000);
    let stored = |id| CheckedBalance::Stored(BalanceId::new(id));

    // The rows that explain the drop between the two anchors.
    let fills = fixture.preview(
        account,
        lines(vec![line(3, -600, "Rent"), line(8, -400, "Gas")]),
    );
    assert_eq!(
        fills.reconciliation().checks,
        [BalanceCheck {
            balance: stored(2),
            date: day(10),
            stated: gbp(9_000),
            before: Some(CheckResult::Failed {
                since: day(1),
                computed_minor: 10_000,
            }),
            after: CheckResult::Passed { since: day(1) },
        }]
    );
    assert_eq!(
        fills.reconciliation().status(),
        ReconciliationStatus::Reconciled
    );

    // Rows that don't: the check still fails, with a different figure.
    let short = fixture.preview(account, lines(vec![line(3, -600, "Rent")]));
    assert_eq!(
        short.reconciliation().checks[0].after,
        CheckResult::Failed {
            since: day(1),
            computed_minor: 9_400,
        }
    );
    assert_eq!(
        short.reconciliation().status(),
        ReconciliationStatus::Mismatch
    );

    // Rows after the last anchor have nothing to check against.
    let later = fixture.preview(account, lines(vec![line(12, -1, "Later")]));
    assert_eq!(
        later.reconciliation().status(),
        ReconciliationStatus::NoBalances
    );
}

// Undo ------------------------------------------------------------------------

#[test]
fn undo_restores_the_previous_state() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    fixture.import_batch(
        account,
        lines(vec![
            with_balance(line(1, -500, "Coffee"), 9_500),
            line(2, -250, "Tea"),
        ]),
    );
    let before = fixture.transactions();
    let snapshots = fixture.count("SELECT COUNT(*) FROM balance_snapshot");

    let second = || {
        csv(vec![
            Ok(line(2, -250, "Tea")),
            Ok(with_balance(line(3, -100, "Bus"), 9_150)),
            Err(RowProblem::Missing(Field::Date)),
            Ok(line(4, -50, "Bun")),
        ])
    };
    let summary = fixture.import_batch(account, second());
    assert_eq!(
        (summary.transactions, summary.balances, summary.flags),
        (2, 1, 1)
    );

    let preview = undo_preview(&fixture.db, summary.batch).unwrap();
    assert_eq!(
        (
            preview.transactions,
            preview.mirrored,
            preview.balances,
            preview.flags,
            preview.edited,
            preview.transfers,
        ),
        (2, 0, 1, 1, 0, 0)
    );
    assert_eq!(preview.later_batches, []);

    let undone = undo(&mut fixture.db, summary.batch).unwrap();
    assert_eq!(undone, preview);
    assert_eq!(fixture.transactions(), before);
    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM balance_snapshot"),
        snapshots
    );
    assert_eq!(fixture.count("SELECT COUNT(*) FROM import_flag"), 0);
    let info = batch(&fixture.db, summary.batch).unwrap();
    assert_eq!((info.status, info.transactions), (BatchStatus::Undone, 0));
    assert_eq!(batches(&fixture.db, account).unwrap().len(), 2);

    assert!(matches!(
        undo(&mut fixture.db, summary.batch),
        Err(Error::AlreadyUndone(_))
    ));
    assert!(matches!(
        undo_preview(&fixture.db, BatchId::new(99)),
        Err(Error::BatchNotFound(_))
    ));

    // The file imports again, as new rows with new IDs.
    let again = fixture.preview(account, second());
    assert_eq!(again.counts().new, 2);
    fixture.import_batch(account, second());
    let ids: Vec<_> = fixture.transactions().iter().map(|row| row.0).collect();
    assert_eq!(ids.len(), 4);
    assert!(
        ids[2] > before.last().unwrap().0 + 2,
        "IDs are never reused"
    );
}

#[test]
fn undo_takes_user_edits_with_it_and_dissolves_transfers() {
    let mut fixture = Fixture::new();
    let current = fixture.account("Current");
    let savings = fixture.account("Savings");
    let summary = fixture.import_batch(
        current,
        lines(vec![
            line(1, -500, "Coffee"),
            line(2, -10_000, "To savings"),
            line(3, -250, "Tea"),
        ]),
    );
    let other = fixture.import_batch(savings, lines(vec![line(2, 10_000, "From current")]));
    fixture.sql(
        "UPDATE \"transaction\" SET user_note = 'mine' WHERE description_raw = 'Coffee';
         UPDATE \"transaction\" SET excluded_reason = 'disputed', \
             excluded_at = '2026-11-11T09:00:00.000Z' WHERE description_raw = 'Tea';
         INSERT INTO transfer_group (status) VALUES ('confirmed');
         UPDATE \"transaction\" SET transfer_group_id = last_insert_rowid() \
             WHERE description_raw IN ('To savings', 'From current');",
    );

    let preview = undo_preview(&fixture.db, summary.batch).unwrap();
    assert_eq!((preview.edited, preview.transfers), (2, 1));
    undo(&mut fixture.db, summary.batch).unwrap();

    assert_eq!(fixture.count("SELECT COUNT(*) FROM transfer_group"), 0);
    assert_eq!(
        fixture.count(
            "SELECT COUNT(*) FROM \"transaction\" \
             WHERE description_raw = 'From current' AND transfer_group_id IS NULL"
        ),
        1,
        "the other side stays, unpaired"
    );
    assert_eq!(
        batch(&fixture.db, other.batch).unwrap().status,
        BatchStatus::Committed
    );
}

#[test]
fn undo_names_later_imports_that_relied_on_the_batch() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    let first = fixture.import_batch(account, lines(vec![line(1, -100, "A"), line(5, -200, "B")]));
    let second = fixture.import_batch(account, lines(vec![line(5, -200, "B"), line(9, -300, "C")]));
    let other_account = fixture.account("Other");
    fixture.import_batch(other_account, lines(vec![line(1, -1, "X")]));

    let preview = undo_preview(&fixture.db, first.batch).unwrap();
    assert_eq!(preview.later_batches, [second.batch]);
    undo(&mut fixture.db, first.batch).unwrap();
    // B came from the first file only; the second file brings it back.
    let again = fixture.preview(account, lines(vec![line(5, -200, "B"), line(9, -300, "C")]));
    assert_eq!(again.counts().new, 1);
}

#[test]
fn undo_removes_mirrored_pot_rows() {
    let mut fixture = Fixture::new();
    let current = fixture.account("Current");
    let mut pot = NewAccount::new("Holiday", AccountKind::Pot, Currency::GBP);
    pot.parent = Some(current);
    let pot = account::create(&mut fixture.db, &pot).unwrap();
    let summary = fixture.import_batch(current, lines(vec![line(1, -5_000, "To pot")]));
    fixture
        .db
        .conn()
        .execute(
            "INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, currency, \
             description_raw, description_normalized, origin) \
             VALUES (?1, ?2, '2026-11-01', 5000, 'GBP', 'To pot', 'TO POT', 'mirrored')",
            params![pot, summary.batch],
        )
        .unwrap();

    let undone = undo(&mut fixture.db, summary.batch).unwrap();
    assert_eq!((undone.transactions, undone.mirrored), (1, 1));
    assert_eq!(fixture.count("SELECT COUNT(*) FROM \"transaction\""), 0);
}

#[test]
fn undo_leaves_a_locked_period_alone() {
    let mut fixture = Fixture::new();
    let account = fixture.account("Current");
    let summary = fixture.import_batch(account, lines(vec![line(3, -500, "Coffee")]));
    fixture.sql("UPDATE account SET reconciled_through = '2026-11-03'");
    assert!(matches!(
        undo_preview(&fixture.db, summary.batch),
        Err(Error::Locked { through }) if through == day(3)
    ));
    assert!(matches!(
        undo(&mut fixture.db, summary.batch),
        Err(Error::Locked { .. })
    ));
    assert_eq!(fixture.count("SELECT COUNT(*) FROM \"transaction\""), 1);

    fixture.sql("UPDATE account SET reconciled_through = '2026-11-02'");
    undo(&mut fixture.db, summary.batch).unwrap();
}
