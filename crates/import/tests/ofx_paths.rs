//! Every OFX/QFX importer path end to end (M1.19): a fixture in
//! `fixtures/ofx/` → `OfxImporter` → `import::preview` → `commit` → what
//! the database holds, including statement balances (`LEDGERBAL`) and
//! their reconciliation.

mod common;

use chrono::NaiveDate;
use coffer_core::account::{self, AccountKind, EnteredBalance, EnteredSource};
use coffer_core::balance::CheckResult;
use coffer_core::import::{
    self, CheckedBalance, Committed, Confirmations, Error, FileBalance, FileBalanceKind,
    FileBalanceStatus, FlagKind, PreviewCounts, ReconciliationStatus,
};
use coffer_core::money::{Currency, Money};
use coffer_core::statement::{SourceFormat, SourceLocation};
use coffer_import::ofx::OfxImporter;
use coffer_import::{AnyImporter, Detect, Detection, ImportError, SourceFile};

use common::*;

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn kinds(rows: &[(u64, &str)]) -> Vec<(u64, String)> {
    rows.iter()
        .map(|(n, kind)| (*n, (*kind).to_owned()))
        .collect()
}

fn anchor(scratch: &mut Scratch, account: coffer_core::id::AccountId, on: NaiveDate, minor: i64) {
    account::add_balance(
        &mut scratch.db,
        account,
        &EnteredBalance {
            date: on,
            balance: gbp(minor),
            source: EnteredSource::Manual,
            note: None,
        },
    )
    .unwrap();
}

// SGML bank statement with a ledger balance ----------------------------------

#[test]
fn an_sgml_bank_statement_commits_with_its_ledger_balance() {
    let mut scratch = Scratch::new();
    let account = scratch.current("Current", Currency::GBP);
    let preview = scratch.preview(account, "bank-sgml.ofx", read_ofx("bank-sgml.ofx"));

    assert_eq!(
        outcome_kinds(&preview),
        (1..=7).map(|n| (n, "new".to_owned())).collect::<Vec<_>>()
    );
    // `LEDGERBAL` is the statement's balance; `AVAILBAL` isn't used.
    assert_eq!(
        preview.balances(),
        [FileBalance {
            date: date(2026, 11, 30),
            balance: gbp(238_147),
            kind: FileBalanceKind::Stated,
            status: FileBalanceStatus::New,
        }]
    );
    assert_eq!(
        preview.reconciliation().status(),
        ReconciliationStatus::StartingPoint
    );
    assert!(preview.reconciliation().running.is_none());
    assert_eq!(preview.needs_confirmation(), Confirmations::default());

    let summary = scratch.import_batch(account, "bank-sgml.ofx", read_ofx("bank-sgml.ofx"));
    assert_eq!(
        (summary.transactions, summary.balances, summary.flags),
        (7, 1, 0)
    );
    let info = import::batch(&scratch.db, summary.batch).unwrap();
    assert_eq!(info.format, SourceFormat::Ofx);
    assert_eq!(info.preset_id, None);
    assert_eq!(
        scratch.balances(account),
        [(
            "2026-11-30".to_owned(),
            238_147,
            "ofx".to_owned(),
            Some(summary.batch.get())
        )]
    );

    // Windows-1252 text decoded; NAME is the merchant, NAME + MEMO the
    // description; no time when DTPOSTED has none.
    let cafe = scratch.by_external_id(account, "EX-0001").unwrap();
    assert_eq!(cafe.merchant.as_deref(), Some("Corner Café"));
    assert_eq!(cafe.description_raw, "Corner Café CONTACTLESS");
    assert_eq!(cafe.source_type.as_deref(), Some("DEBIT"));
    assert_eq!(cafe.time, None);
    assert_eq!(cafe.date, "2026-11-02");
    assert_eq!(cafe.amount_minor, -1240);
    assert_eq!(
        cafe.fingerprint.as_deref(),
        Some("v1|2026-11-02|-1240|GBP|0|CORNER CAFÉ CONTACTLESS")
    );
    let energy = scratch.by_external_id(account, "EX-0003").unwrap();
    assert_eq!(
        energy.description_raw, "Example Energy & Gas",
        "entities decoded, a MEMO repeating the NAME left out"
    );
    let cash = scratch.by_external_id(account, "EX-0005").unwrap();
    assert_eq!(cash.description_raw, "CASH £50 EXAMPLE STREET");
    // ORIGCURRENCY: the amount is already in pounds, and no original
    // amount is made up from the rate.
    let abroad = scratch.by_external_id(account, "EX-0006").unwrap();
    assert_eq!(abroad.amount_minor, -1724);
    assert_eq!(
        (abroad.original_amount_minor, abroad.original_currency),
        (None, None)
    );
    let cheque = scratch.by_external_id(account, "EX-0007").unwrap();
    assert_eq!(cheque.source_type.as_deref(), Some("CHECK"));
    assert_eq!(cheque.description_raw, "CHEQUE 000101");

    // Again: every row found by its FITID and the balance already recorded.
    let again = scratch.preview(account, "bank-sgml.ofx", read_ofx("bank-sgml.ofx"));
    assert!(
        outcome_kinds(&again)
            .iter()
            .all(|(_, kind)| kind == "already:source_id")
    );
    assert!(matches!(
        again.balances()[0].status,
        FileBalanceStatus::AlreadyRecorded(_)
    ));
    assert!(again.is_nothing_new());
    assert_eq!(
        import::commit(&mut scratch.db, &again, Confirmations::default()).unwrap(),
        Committed::NothingNew
    );
    assert_eq!(scratch.count("SELECT COUNT(*) FROM import_batch"), 1);
    assert_eq!(scratch.balances(account).len(), 1);
}

/// The ledger balance checks the rows against the balance the user
/// entered before them: equal is reconciled, different is a mismatch the
/// caller has to confirm, and it stays visible afterwards.
#[test]
fn a_ledger_balance_reconciles_against_an_entered_anchor() {
    let mut scratch = Scratch::new();
    // 2381.47 at the end of November less November's 1246.37.
    let good = scratch.current("Good", Currency::GBP);
    anchor(&mut scratch, good, date(2026, 11, 1), 113_510);
    let preview = scratch.preview(good, "bank-sgml.ofx", read_ofx("bank-sgml.ofx"));
    assert_eq!(
        preview.reconciliation().status(),
        ReconciliationStatus::Reconciled
    );
    let check = preview
        .reconciliation()
        .checks
        .iter()
        .find(|check| check.balance == CheckedBalance::FromFile(0))
        .unwrap();
    assert_eq!(
        check.after,
        CheckResult::Passed {
            since: date(2026, 11, 1)
        }
    );
    assert_eq!(preview.needs_confirmation(), Confirmations::default());
    scratch.import_batch(good, "bank-sgml.ofx", read_ofx("bank-sgml.ofx"));

    let off = scratch.current("Off by 10p", Currency::GBP);
    anchor(&mut scratch, off, date(2026, 11, 1), 113_500);
    let preview = scratch.preview(off, "bank-sgml.ofx", read_ofx("bank-sgml.ofx"));
    assert_eq!(
        preview.reconciliation().status(),
        ReconciliationStatus::Mismatch
    );
    let check = preview
        .reconciliation()
        .checks
        .iter()
        .find(|check| check.balance == CheckedBalance::FromFile(0))
        .unwrap();
    assert_eq!(check.computed(), Some(gbp(238_137)));
    assert_eq!(
        preview.needs_confirmation(),
        Confirmations {
            flagged_rows: false,
            balance_mismatch: true
        }
    );
    let refused = import::commit(&mut scratch.db, &preview, Confirmations::default());
    assert!(
        matches!(refused, Err(Error::NotConfirmed(_))),
        "{refused:?}"
    );
    assert!(scratch.stored(off).is_empty());

    let summary = scratch.import_batch(off, "bank-sgml.ofx", read_ofx("bank-sgml.ofx"));
    assert_eq!((summary.transactions, summary.balances), (7, 1));
    // The mismatch isn't a row's problem, so nothing is recorded as a flag;
    // it stays visible in the account's balance checks.
    assert_eq!(summary.flags, 0);
    let again = scratch.preview(off, "bank-sgml.ofx", read_ofx("bank-sgml.ofx"));
    assert_eq!(
        again.reconciliation().status(),
        ReconciliationStatus::Mismatch,
        "the stored balance the file states again still fails"
    );
}

/// The next month's file repeats the end of November: rows with known
/// FITIDs are found again (even re-worded), one with a known FITID and
/// another amount is a conflict, and the December balance checks against
/// November's.
#[test]
fn the_next_statement_overlaps_conflicts_and_reconciles() {
    let mut scratch = Scratch::new();
    let account = scratch.current("Current", Currency::GBP);
    let first = scratch.import_batch(account, "bank-sgml.ofx", read_ofx("bank-sgml.ofx"));

    let next = || read_ofx("bank-sgml-next.ofx");
    let preview = scratch.preview(account, "bank-sgml-next.ofx", next());
    assert_eq!(
        outcome_kinds(&preview),
        kinds(&[
            (1, "flag:source_id_conflict"),
            (2, "already:source_id"),
            (3, "already:source_id"),
            (4, "new"),
            (5, "new"),
            (6, "new"),
        ])
    );
    let cash = scratch.by_external_id(account, "EX-0005").unwrap();
    assert_eq!(
        *preview.rows().next().unwrap().outcome,
        import::RowOutcome::Flagged(import::Flag::SourceIdConflict {
            existing: coffer_core::id::TransactionId::new(cash.id)
        })
    );
    assert_eq!(
        preview.balances(),
        [FileBalance {
            date: date(2026, 12, 31),
            balance: gbp(380_698),
            kind: FileBalanceKind::Stated,
            status: FileBalanceStatus::New,
        }]
    );
    assert_eq!(
        preview.reconciliation().status(),
        ReconciliationStatus::Reconciled
    );
    assert_eq!(
        preview.needs_confirmation(),
        Confirmations {
            flagged_rows: true,
            balance_mismatch: false
        }
    );

    let second = scratch.import_batch(account, "bank-sgml-next.ofx", next());
    assert_eq!(
        (second.transactions, second.balances, second.flags),
        (3, 1, 1)
    );
    let flags = import::batch_flags(&scratch.db, second.batch).unwrap();
    assert_eq!(flags.len(), 1);
    assert_eq!(
        (flags[0].location, flags[0].kind),
        (SourceLocation::Entry(1), FlagKind::SourceIdConflict)
    );

    // Imported source fields are never edited: the stored rows keep the
    // first file's amount and text.
    assert_eq!(
        scratch
            .by_external_id(account, "EX-0005")
            .unwrap()
            .amount_minor,
        -5000
    );
    let cheque = scratch.by_external_id(account, "EX-0007").unwrap();
    assert_eq!(cheque.description_raw, "CHEQUE 000101");
    assert_eq!(cheque.batch, Some(first.batch.get()));
    assert_eq!(scratch.stored(account).len(), 10);

    // Undoing the first import names the second, which relied on it.
    let undo = import::undo_preview(&scratch.db, first.batch).unwrap();
    assert_eq!(undo.later_batches, [second.batch]);
}

// XML credit card, times and offsets, PAYEE ------------------------------------

#[test]
fn an_xml_credit_card_statement_keeps_times_as_written() {
    let mut scratch = Scratch::new();
    let pounds = scratch.current("Pounds", Currency::GBP);
    assert!(matches!(
        import::preview(
            &scratch.db,
            pounds,
            "creditcard-xml.ofx",
            read_ofx("creditcard-xml.ofx")
        ),
        Err(Error::CurrencyMismatch { .. })
    ));

    let card = scratch.account("Card", AccountKind::CreditCard, Currency::USD);
    let summary = scratch.import_batch(card, "creditcard-xml.ofx", read_ofx("creditcard-xml.ofx"));
    assert_eq!((summary.transactions, summary.balances), (5, 1));
    // A balance owed is negative, dated by DTASOF's own date (no offset).
    assert_eq!(
        scratch.balances(card),
        [(
            "2026-11-30".to_owned(),
            -123_456,
            "ofx".to_owned(),
            Some(summary.batch.get())
        )]
    );

    let read: Vec<_> = scratch
        .stored(card)
        .into_iter()
        .map(|row| {
            (
                row.external_id.unwrap(),
                row.date,
                row.time,
                row.amount_minor,
                row.merchant,
                row.description_raw,
            )
        })
        .collect();
    let s = |text: &str| text.to_owned();
    assert_eq!(
        read,
        [
            (
                s("CC-2026-0001"),
                s("2026-11-03"),
                Some(s("12:00:00")),
                -2350,
                Some(s("Example Books")),
                s("Example Books"),
            ),
            (
                s("CC-2026-0002"),
                s("2026-11-05"),
                Some(s("15:30:12")),
                -475,
                Some(s("Example Coffee Co")),
                s("Example Coffee Co Store 42"),
            ),
            (
                s("CC-2026-0003"),
                s("2026-11-10"),
                None,
                20_000,
                Some(s("PAYMENT - THANK YOU")),
                s("PAYMENT - THANK YOU"),
            ),
            (
                s("CC-2026-0004"),
                s("2026-11-18"),
                Some(s("00:00:00")),
                -6120,
                Some(s("Example Hardware & Garden")),
                s("Example Hardware & Garden"),
            ),
            (
                s("CC-2026-0005"),
                s("2026-11-25"),
                Some(s("00:00:00")),
                -999,
                Some(s("Example Streaming")),
                s("Example Streaming Service Monthly"),
            ),
        ],
        "local times, offsets not applied; EXTDNAME extends a NAME it starts with"
    );

    // The PAYEE's address never reaches the database.
    for text in ["Fake Street", "Springfield", "555-0100", "00000"] {
        let found = scratch.count(&format!(
            "SELECT COUNT(*) FROM \"transaction\" WHERE description_raw LIKE '%{text}%' \
             OR merchant LIKE '%{text}%' OR IFNULL(note, '') LIKE '%{text}%'"
        ));
        assert_eq!(found, 0, "{text}");
    }
}

// QFX -------------------------------------------------------------------------

#[test]
fn qfx_reads_like_ofx_whatever_its_name() {
    let bytes = fixture_bytes("ofx/intuit.qfx");
    for name in ["intuit.qfx", "INTUIT.QFX", "download.txt"] {
        let file = SourceFile::from_bytes(name, bytes.clone());
        assert_eq!(
            OfxImporter::new().detect(&file),
            Detection::Format,
            "{name}"
        );
    }

    let mut scratch = Scratch::new();
    let account = scratch.current("Checking", Currency::USD);
    let summary = scratch.import_batch(account, "intuit.qfx", read_ofx("intuit.qfx"));
    assert_eq!((summary.transactions, summary.balances), (3, 1));
    let read: Vec<_> = scratch
        .stored(account)
        .into_iter()
        .map(|row| (row.external_id.unwrap(), row.time, row.description_raw))
        .collect();
    assert_eq!(
        read,
        [
            (
                "2026110100001".to_owned(),
                Some("12:00:00".to_owned()),
                "EXAMPLE MARKET #0000 POS PURCHASE".to_owned()
            ),
            (
                "2026110200001".to_owned(),
                Some("12:00:00".to_owned()),
                "EXAMPLE PAYROLL DIRECT DEP".to_owned()
            ),
            (
                "2026110400001".to_owned(),
                Some("12:00:00".to_owned()),
                "CHECK 1001".to_owned()
            ),
        ],
        "placeholder times are kept; closed SGML values read like open ones"
    );
    assert_eq!(scratch.balances(account)[0].1, 284_579);
}

// Edge cases: every flag, rows without a FITID ---------------------------------

#[test]
fn edge_cases_record_every_flag_and_dedupe_rows_without_a_fitid() {
    let mut scratch = Scratch::new();
    let account = scratch.current("Girokonto", Currency::EUR);
    let preview = scratch.preview(account, "edge-cases.ofx", read_ofx("edge-cases.ofx"));
    assert_eq!(
        outcome_kinds(&preview),
        kinds(&[
            (1, "new"),
            (2, "flag:unreadable"),
            (3, "flag:unreadable"),
            (4, "flag:unreadable"),
            (5, "new"),
            (6, "flag:unreadable"),
            (7, "flag:unreadable"),
            (8, "flag:unreadable"),
            (9, "flag:unreadable"),
            (10, "flag:unreadable"),
            (11, "new"),
            (12, "flag:unreadable"),
            (13, "new"),
        ])
    );
    assert!(preview.balances().is_empty(), "no LEDGERBAL");
    assert_eq!(
        preview.reconciliation().status(),
        ReconciliationStatus::NoBalances
    );

    let summary = scratch.import_batch(account, "edge-cases.ofx", read_ofx("edge-cases.ofx"));
    assert_eq!((summary.transactions, summary.flags), (4, 9));
    let flags = import::batch_flags(&scratch.db, summary.batch).unwrap();
    let entries: Vec<_> = flags
        .iter()
        .map(|flag| match flag.location {
            SourceLocation::Entry(n) => n,
            SourceLocation::Line(_) => panic!("OFX rows are entries"),
        })
        .collect();
    assert_eq!(entries, [2, 3, 4, 6, 7, 8, 9, 10, 12]);
    assert!(flags.iter().all(|flag| flag.kind == FlagKind::Unreadable));
    assert_eq!(flags[3].detail, "the external ID is the same as entry 5's");
    for flag in &flags {
        for text in [
            "No date",
            "Thirtieth",
            "Too precise",
            "Second of a pair",
            "Abroad",
            "Two amounts",
            "No amount",
            "Unknown currency",
            "EDGE-",
        ] {
            assert!(!flag.detail.contains(text), "{text}: {}", flag.detail);
        }
    }

    // No FITID: the row is deduped by fingerprint. HHMM is a time; an empty
    // NAME leaves the MEMO; a transaction with no text is an empty
    // description.
    let stored = scratch.stored(account);
    assert_eq!(stored.len(), 4);
    assert_eq!(stored[0].external_id, None);
    assert_eq!(stored[0].time.as_deref(), Some("09:15:00"));
    assert_eq!(stored[0].amount_minor, -350, "a decimal comma");
    assert_eq!(stored[0].merchant, None);
    assert_eq!(
        stored[0].fingerprint.as_deref(),
        Some("v1|2026-11-01|-350|EUR|0|BÄCKEREI BEISPIEL")
    );
    assert_eq!(stored[2].external_id.as_deref(), Some("EDGE-11"));
    assert_eq!(stored[2].time.as_deref(), Some("10:30:00"));
    assert_eq!(stored[2].source_category, None, "X- extensions aren't read");
    assert_eq!(stored[3].description_raw, "");
    assert_eq!(
        stored[3].fingerprint.as_deref(),
        Some("v1|2026-11-17|1|EUR|0|")
    );

    // Again: nothing new, so no batch and no second set of flags, and the
    // flagged rows don't ask for confirmation of nothing.
    let again = scratch.preview(account, "edge-cases.ofx", read_ofx("edge-cases.ofx"));
    assert_eq!(
        again.counts(),
        PreviewCounts {
            new: 0,
            already_imported: 4,
            flagged: 9
        }
    );
    assert_eq!(outcome_kinds(&again)[0].1, "already:fingerprint");
    assert!(again.is_nothing_new());
    assert_eq!(
        import::commit(&mut scratch.db, &again, Confirmations::default()).unwrap(),
        Committed::NothingNew
    );
    assert_eq!(scratch.count("SELECT COUNT(*) FROM import_flag"), 9);
}

// Several statements in one file ------------------------------------------------

#[test]
fn each_statement_of_a_multi_statement_file_goes_to_its_own_account() {
    let file = fixture("ofx/multi-statement.ofx");
    assert!(matches!(
        OfxImporter::new().import(&file),
        Err(ImportError::MultipleStatements { count: 3 })
    ));
    assert!(
        OfxImporter::for_statement(3).import(&file).is_err(),
        "there are three"
    );

    let mut scratch = Scratch::new();
    let current = scratch.current("Current", Currency::GBP);
    let savings = scratch.account("Savings", AccountKind::Savings, Currency::GBP);
    let card = scratch.account("Card", AccountKind::CreditCard, Currency::GBP);
    let expected = [
        (0, current, vec![1, 2], 10_000),
        (1, savings, vec![3], 525_000),
        (2, card, vec![4], -4200),
    ];
    for (index, account, entries, balance) in expected {
        let statement = read_ofx_statement("multi-statement.ofx", index);
        let preview = scratch.preview(account, "multi-statement.ofx", statement.clone());
        // Entries count through the whole file, not per statement.
        let locations: Vec<_> = preview.rows().map(|row| row.location).collect();
        assert_eq!(
            locations,
            entries
                .iter()
                .map(|&n| SourceLocation::Entry(n))
                .collect::<Vec<_>>(),
            "statement {index}"
        );
        let summary = scratch.import_batch(account, "multi-statement.ofx", statement);
        assert_eq!(summary.transactions, entries.len());
        assert_eq!(scratch.balances(account)[0].1, balance);
    }

    // Source IDs are per account: the first statement into another account
    // is new there, and already imported in its own.
    let other = scratch.current("Other", Currency::GBP);
    let preview = scratch.preview(
        other,
        "multi-statement.ofx",
        read_ofx_statement("multi-statement.ofx", 0),
    );
    assert_eq!(preview.counts().new, 2);
    let preview = scratch.preview(
        current,
        "multi-statement.ofx",
        read_ofx_statement("multi-statement.ofx", 0),
    );
    assert!(preview.is_nothing_new());
}

#[test]
fn the_same_file_into_two_accounts_is_two_sets_of_transactions() {
    let mut scratch = Scratch::new();
    let first = scratch.current("First", Currency::GBP);
    let second = scratch.current("Second", Currency::GBP);
    scratch.import_batch(first, "bank-sgml.ofx", read_ofx("bank-sgml.ofx"));
    let preview = scratch.preview(second, "bank-sgml.ofx", read_ofx("bank-sgml.ofx"));
    assert_eq!(preview.counts().new, 7);
    assert_eq!(preview.balances()[0].status, FileBalanceStatus::New);
    scratch.import_batch(second, "bank-sgml.ofx", read_ofx("bank-sgml.ofx"));
    assert_eq!(scratch.stored(first).len(), 7);
    assert_eq!(scratch.stored(second).len(), 7);
}

/// A preview the account has moved on from since is refused, and the
/// refusal writes nothing.
#[test]
fn a_preview_overtaken_by_another_import_is_stale() {
    let mut scratch = Scratch::new();
    let account = scratch.current("Current", Currency::GBP);
    let early = scratch.preview(account, "bank-sgml.ofx", read_ofx("bank-sgml.ofx"));
    scratch.import_batch(account, "bank-sgml.ofx", read_ofx("bank-sgml.ofx"));

    let error = import::commit(&mut scratch.db, &early, Confirmations::default()).unwrap_err();
    assert!(matches!(error, Error::Stale), "{error:?}");
    assert_eq!(scratch.count("SELECT COUNT(*) FROM import_batch"), 1);
    assert_eq!(scratch.stored(account).len(), 7);

    // An import into another account doesn't make it stale.
    let other = scratch.current("Other", Currency::GBP);
    let preview = scratch.preview(
        account,
        "bank-sgml-next.ofx",
        read_ofx("bank-sgml-next.ofx"),
    );
    scratch.import_batch(other, "bank-sgml.ofx", read_ofx("bank-sgml.ofx"));
    let confirm = preview.needs_confirmation();
    assert!(matches!(
        import::commit(&mut scratch.db, &preview, confirm).unwrap(),
        Committed::Batch(_)
    ));
}

/// A `LEDGERBAL` that can't be read fails the whole file, without its
/// text in the error.
#[test]
fn an_unreadable_ledger_balance_fails_the_file() {
    let text = String::from_utf8(fixture_bytes("ofx/intuit.qfx")).unwrap();
    let broken = text.replace("<BALAMT>2845.79", "<BALAMT>28,45.79");
    let error = OfxImporter::new()
        .import(&SourceFile::from_bytes("intuit.qfx", broken))
        .unwrap_err();
    assert!(
        !error.to_string().contains("28,45"),
        "no file text in errors: {error}"
    );
}
