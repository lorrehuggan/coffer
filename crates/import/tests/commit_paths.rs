//! The commit's account-level paths from real files (M1.19): locked
//! periods, undo (counts, balances, flags, edits, later imports) and
//! transfer pairs across files, CSV and OFX alike.

mod common;

use chrono::NaiveDate;
use coffer_core::account::AccountKind;
use coffer_core::id::BatchId;
use coffer_core::import::{
    self, BatchStatus, Committed, Error, FileBalanceStatus, PreviewCounts, RowOutcome,
};
use coffer_core::money::Currency;

use common::*;

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

fn kinds(rows: &[(u64, &str)]) -> Vec<(u64, String)> {
    rows.iter()
        .map(|(n, kind)| (*n, (*kind).to_owned()))
        .collect()
}

const NEWEST_FIRST: &str = "csv/newest-first-tab.tsv";

// Locked periods --------------------------------------------------------------

/// New rows dated on or before the lock are flagged and stay out; rows the
/// account already has are still found; and a batch with nothing in the
/// locked period can be undone.
#[test]
fn rows_in_a_locked_period_stay_out() {
    let mut scratch = Scratch::new();
    let account = scratch.current("Girokonto", Currency::EUR);
    scratch.lock(account, "2026-11-05");

    let preview = scratch.preview(account, "edge-cases.ofx", read_ofx("edge-cases.ofx"));
    let rows: Vec<_> = preview.rows().collect();
    assert_eq!(
        *rows[0].outcome,
        RowOutcome::Flagged(import::Flag::Locked {
            through: date(2026, 11, 5)
        }),
        "1 November"
    );
    assert_eq!(
        *rows[4].outcome,
        RowOutcome::Flagged(import::Flag::Locked {
            through: date(2026, 11, 5)
        }),
        "5 November, the lock date itself"
    );
    assert_eq!(
        outcome_kinds(&preview)
            .into_iter()
            .filter(|(_, kind)| kind == "new")
            .collect::<Vec<_>>(),
        kinds(&[(11, "new"), (13, "new")])
    );

    let summary = scratch.import_batch(account, "edge-cases.ofx", read_ofx("edge-cases.ofx"));
    assert_eq!((summary.transactions, summary.flags), (2, 11));
    let locked = import::batch_flags(&scratch.db, summary.batch)
        .unwrap()
        .into_iter()
        .filter(|flag| flag.kind == import::FlagKind::Locked)
        .count();
    assert_eq!(locked, 2);

    // Nothing of the batch is in the locked period, so it can be undone.
    import::undo(&mut scratch.db, summary.batch).unwrap();

    // Unlocked, the rows come in; locked again over them, the batch can't
    // be undone until the user unlocks.
    scratch
        .writer()
        .execute("UPDATE account SET reconciled_through = NULL", [])
        .unwrap();
    let summary = scratch.import_batch(account, "edge-cases.ofx", read_ofx("edge-cases.ofx"));
    assert_eq!(summary.transactions, 4);
    scratch.lock(account, "2026-11-10");
    for error in [
        import::undo_preview(&scratch.db, summary.batch).unwrap_err(),
        import::undo(&mut scratch.db, summary.batch).unwrap_err(),
    ] {
        assert!(
            matches!(error, Error::Locked { through } if through == date(2026, 11, 10)),
            "{error:?}"
        );
    }
    assert_eq!(scratch.stored(account).len(), 4);

    // Rows already in the account are found, not flagged as locked.
    let again = scratch.preview(account, "edge-cases.ofx", read_ofx("edge-cases.ofx"));
    assert_eq!(again.counts().already_imported, 4);
    assert!(again.is_nothing_new());
}

/// `ARCHITECTURE.md` → Decisions → Reconciliation: "imports … touching
/// locked dates are flagged and need an explicit unlock", and the decision
/// record "import commit, dedupe and undo", point 10: "Locks hold". Rows
/// in the locked period are flagged, but a balance the file gives for a
/// date in it (a running-balance opening, an OFX `LEDGERBAL`) is recorded
/// as new with nothing flagged about it. That adds a check inside the
/// locked period, and leaves a batch the user can't undo without
/// unlocking (`Error::Locked`).
#[test]
#[ignore = "bug: a file balance dated inside the locked period is recorded as new, and the batch then can't be undone"]
fn a_file_balance_in_a_locked_period_is_not_recorded_silently() {
    // A CSV opening (end of 9 November) under a lock through 15 November.
    let mut scratch = Scratch::new();
    let account = scratch.current("Current", Currency::GBP);
    scratch.lock(account, "2026-11-15");
    let preview = scratch.preview(
        account,
        "newest-first-tab.tsv",
        read_with("uk-example-tab", NEWEST_FIRST),
    );
    assert_eq!(preview.balances()[0].date, date(2026, 11, 9));
    assert_ne!(
        preview.balances()[0].status,
        FileBalanceStatus::New,
        "a balance in the locked period is flagged or left out, not recorded"
    );

    // An OFX file wholly inside the lock: every row is flagged, yet the
    // import commits a batch holding only the November ledger balance,
    // which then can't be undone.
    let mut scratch = Scratch::new();
    let account = scratch.current("Current", Currency::GBP);
    scratch.lock(account, "2026-11-30");
    let preview = scratch.preview(account, "bank-sgml.ofx", read_ofx("bank-sgml.ofx"));
    assert!(
        preview.is_nothing_new(),
        "nothing outside the locked period: {:?}",
        preview.balances()
    );
    if let Committed::Batch(summary) =
        scratch.import(account, "bank-sgml.ofx", read_ofx("bank-sgml.ofx"))
    {
        import::undo(&mut scratch.db, summary.batch).unwrap();
    }
}

// Undo ------------------------------------------------------------------------

/// Undo removes the batch's transactions, balances and flags, keeps the
/// batch as `undone`, and importing the file again brings it all back.
#[test]
fn undo_removes_balances_and_flags_and_a_reimport_restores_them() {
    let mut scratch = Scratch::new();
    let current = scratch.current("Current", Currency::GBP);
    let first = scratch.import_batch(current, "bank-sgml.ofx", read_ofx("bank-sgml.ofx"));
    let next = scratch.import_batch(
        current,
        "bank-sgml-next.ofx",
        read_ofx("bank-sgml-next.ofx"),
    );

    let preview = import::undo_preview(&scratch.db, next.batch).unwrap();
    assert_eq!(
        (
            preview.transactions,
            preview.mirrored,
            preview.balances,
            preview.flags,
            preview.edited,
            preview.transfers
        ),
        (3, 0, 1, 1, 0, 0)
    );
    assert!(preview.later_batches.is_empty());
    assert_eq!(preview.batch.source_file, "bank-sgml-next.ofx");

    let undone = import::undo(&mut scratch.db, next.batch).unwrap();
    assert_eq!(undone, preview);
    assert_eq!(scratch.stored(current).len(), 7);
    assert_eq!(scratch.balances(current).len(), 1);
    assert!(
        import::batch_flags(&scratch.db, next.batch)
            .unwrap()
            .is_empty()
    );
    let batches = import::batches(&scratch.db, current).unwrap();
    assert_eq!(
        batches
            .iter()
            .map(|batch| (batch.id, batch.status, batch.transactions))
            .collect::<Vec<_>>(),
        [
            (first.batch, BatchStatus::Committed, 7),
            (next.batch, BatchStatus::Undone, 0),
        ]
    );
    assert!(matches!(
        import::undo(&mut scratch.db, next.batch),
        Err(Error::AlreadyUndone(_))
    ));
    assert!(matches!(
        import::undo_preview(&scratch.db, BatchId::new(99)),
        Err(Error::BatchNotFound(_))
    ));

    // The December balance went with the batch, so it is new again.
    let again = scratch.preview(
        current,
        "bank-sgml-next.ofx",
        read_ofx("bank-sgml-next.ofx"),
    );
    assert_eq!(again.balances()[0].status, FileBalanceStatus::New);
    assert_eq!(
        again.counts(),
        PreviewCounts {
            new: 3,
            already_imported: 2,
            flagged: 1
        }
    );
    let restored = scratch.import_batch(
        current,
        "bank-sgml-next.ofx",
        read_ofx("bank-sgml-next.ofx"),
    );
    assert_eq!(
        (restored.transactions, restored.balances, restored.flags),
        (3, 1, 1)
    );
    assert_eq!(scratch.balances(current).len(), 2);
}

/// Undoing a CSV import takes its opening balance (from the running
/// balances) with it; a later file that relied on the rows is named.
#[test]
fn undoing_a_csv_import_takes_its_opening_and_names_later_imports() {
    let mut scratch = Scratch::new();
    let account = scratch.current("Current", Currency::GBP);
    let first = scratch.import_batch(
        account,
        "newest-first-tab.tsv",
        read_with("uk-example-tab", NEWEST_FIRST),
    );
    let later = scratch.import_batch(
        account,
        "newest-first-tab-later.tsv",
        read_with("uk-example-tab", "csv/newest-first-tab-later.tsv"),
    );

    let preview = import::undo_preview(&scratch.db, first.batch).unwrap();
    assert_eq!((preview.transactions, preview.balances), (7, 1));
    assert_eq!(preview.later_batches, [later.batch]);
    import::undo(&mut scratch.db, first.batch).unwrap();

    // The later file's overlapping rows came from the first batch and are
    // gone with it; its own rows and opening stay.
    assert_eq!(scratch.stored(account).len(), 3);
    assert_eq!(
        scratch
            .balances(account)
            .into_iter()
            .map(|(date, minor, ..)| (date, minor))
            .collect::<Vec<_>>(),
        [("2026-11-19".to_owned(), 220_230)]
    );
    // Importing the later file again brings back the overlap.
    let again = scratch.preview(
        account,
        "newest-first-tab-later.tsv",
        read_with("uk-example-tab", "csv/newest-first-tab-later.tsv"),
    );
    assert_eq!(again.counts().new, 2);
}

/// The user's edits go with the rows, and undo counts them for the
/// confirmation; importing again brings the rows back without them.
#[test]
fn undo_counts_the_users_edits_and_they_go_with_the_rows() {
    let mut scratch = Scratch::new();
    let account = scratch.current("Current", Currency::GBP);
    let read = || read_with("uk-example-current", "csv/signed-amount.csv");
    let summary = scratch.import_batch(account, "signed-amount.csv", read());

    let writer = scratch.writer();
    writer
        .execute(
            "UPDATE \"transaction\" SET user_note = 'FAKE NOTE' WHERE external_id = 'tx_0001'",
            [],
        )
        .unwrap();
    writer
        .execute(
            "UPDATE \"transaction\" SET category_source = 'user', \
             category_id = (SELECT id FROM category WHERE key = 'food.takeaway') \
             WHERE external_id = 'tx_0002'",
            [],
        )
        .unwrap();
    writer
        .execute(
            "UPDATE \"transaction\" SET excluded_reason = 'duplicate', \
             excluded_at = '2026-11-11T00:00:00.000Z' WHERE external_id = 'tx_0005'",
            [],
        )
        .unwrap();
    drop(writer);

    let preview = import::undo_preview(&scratch.db, summary.batch).unwrap();
    assert_eq!(preview.edited, 3);
    assert_eq!(preview.flags, 3);
    import::undo(&mut scratch.db, summary.batch).unwrap();

    scratch.import_batch(account, "signed-amount.csv", read());
    assert_eq!(
        scratch.count(
            "SELECT COUNT(*) FROM \"transaction\" WHERE user_note IS NOT NULL \
             OR category_source = 'user' OR excluded_reason IS NOT NULL"
        ),
        0
    );
    let restored = scratch.by_external_id(account, "tx_0002").unwrap();
    assert_eq!(restored.category.as_deref(), Some("food.eating_out"));
    assert_eq!(restored.category_source.as_deref(), Some("preset_map"));
}

// Transfers -------------------------------------------------------------------

/// A transfer out in a CSV file and in in an OFX file pair up when the
/// second is imported; undoing either side dissolves the pair, and
/// importing it again pairs them again.
#[test]
fn transfers_pair_across_files_and_undo_dissolves_them() {
    let mut scratch = Scratch::new();
    let current = scratch.current("Current", Currency::GBP);
    let savings = scratch.account("Savings", AccountKind::Savings, Currency::GBP);
    let out = scratch.import_batch(
        current,
        "newest-first-tab.tsv",
        read_with("uk-example-tab", NEWEST_FIRST),
    );
    assert_eq!(out.transfer_pairs, 0, "one account so far");

    let savings_in = || read_ofx_statement("multi-statement.ofx", 1);
    let paid_in = scratch.import_batch(savings, "multi-statement.ofx", savings_in());
    assert_eq!(paid_in.transfer_pairs, 1);
    let outgoing = scratch
        .stored(current)
        .into_iter()
        .find(|row| row.amount_minor == -25_000)
        .unwrap();
    let incoming = scratch.stored(savings).pop().unwrap();
    assert!(outgoing.transfer_group.is_some());
    assert_eq!(outgoing.transfer_group, incoming.transfer_group);
    assert_eq!(
        scratch.count("SELECT COUNT(*) FROM transfer_group WHERE status = 'suggested'"),
        1
    );
    // Detection never changes categories.
    assert_eq!(outgoing.category, None);

    let preview = import::undo_preview(&scratch.db, paid_in.batch).unwrap();
    assert_eq!(preview.transfers, 1);
    import::undo(&mut scratch.db, paid_in.batch).unwrap();
    let outgoing = scratch
        .stored(current)
        .into_iter()
        .find(|row| row.amount_minor == -25_000)
        .unwrap();
    assert_eq!(outgoing.transfer_group, None);
    assert_eq!(scratch.count("SELECT COUNT(*) FROM transfer_group"), 0);

    let again = scratch.import_batch(savings, "multi-statement.ofx", savings_in());
    assert_eq!(again.transfer_pairs, 1);

    // Undoing the other side dissolves it too.
    assert_eq!(
        import::undo_preview(&scratch.db, out.batch)
            .unwrap()
            .transfers,
        1
    );
    import::undo(&mut scratch.db, out.batch).unwrap();
    assert_eq!(scratch.stored(savings).pop().unwrap().transfer_group, None);
}

/// A re-import that stores nothing records no batch, whatever it would
/// confirm.
#[test]
fn nothing_new_records_no_batch() {
    let mut scratch = Scratch::new();
    let account = scratch.current("Current", Currency::GBP);
    let first = scratch.import(
        account,
        "newest-first-tab.tsv",
        read_with("uk-example-tab", NEWEST_FIRST),
    );
    assert!(matches!(first, Committed::Batch(_)));
    let second = scratch.import(
        account,
        "newest-first-tab.tsv",
        read_with("uk-example-tab", NEWEST_FIRST),
    );
    assert_eq!(second, Committed::NothingNew);
    assert_eq!(import::batches(&scratch.db, account).unwrap().len(), 1);
}
