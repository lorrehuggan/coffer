//! Every CSV importer path end to end (M1.19): a fixture in
//! `fixtures/csv/` → its test preset in `fixtures/csv/mappings/` →
//! `import::preview` → `commit` → what the database holds.
//!
//! The importer's own unit tests check each quirk on short inline text;
//! these check the same quirks survive the whole way into the database.
//! Monzo's preset has its own (`preset_uk_monzo.rs`, `commit_uk_monzo.rs`).

mod common;

use chrono::NaiveDate;
use coffer_core::balance::CheckResult;
use coffer_core::import::{
    self, CheckedBalance, Committed, Confirmations, Error, FileBalance, FileBalanceKind,
    FileBalanceStatus, FileOrder, FlagKind, PreviewCounts, ReconciliationStatus, RunningMismatch,
};
use coffer_core::money::{Currency, Money};
use coffer_core::statement::{RowProblem, SourceFormat, SourceLocation};
use coffer_import::csv::TextEncoding;
use coffer_import::preset::{Preset, Presets};
use coffer_import::{AnyImporter, Detect, Detection, Importer, SourceFile, detect};

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

const SIGNED: &str = "csv/signed-amount.csv";
const DEBIT_CREDIT: &str = "csv/debit-credit-windows-1252.csv";
const SEMICOLON: &str = "csv/semicolon-decimal-comma.csv";
const NEWEST_FIRST: &str = "csv/newest-first-tab.tsv";
const NEWEST_FIRST_LATER: &str = "csv/newest-first-tab-later.tsv";
const HEADERLESS: &str = "csv/headerless-preamble.csv";
const REPEATED_IDS: &str = "csv/repeated-ids-us.csv";

// Detection --------------------------------------------------------------------

/// Each test preset recognises its own fixture by its header (`Source`),
/// sees the other CSV fixtures as merely CSV, and none of them competes
/// with the OFX importer on an OFX file.
#[test]
fn each_preset_recognises_only_its_own_fixture() {
    let presets = mappings();
    let own = [
        (SIGNED, "uk-example-current"),
        (DEBIT_CREDIT, "uk-example-savings"),
        (SEMICOLON, "eu-example-giro"),
        (NEWEST_FIRST, "uk-example-tab"),
        (NEWEST_FIRST_LATER, "uk-example-tab"),
        (REPEATED_IDS, "us-example-checking"),
    ];
    for (file, owner) in own {
        let source = fixture(file);
        for preset in &presets {
            let detection = preset.importer().detect(&source);
            if preset.id() == owner {
                assert_eq!(detection, Detection::Source, "{owner} on {file}");
            } else {
                assert!(
                    detection < Detection::Source,
                    "{} on {file}: {detection:?}",
                    preset.id()
                );
            }
        }
        let importers = presets.importers();
        let ranked = detect(&importers, &source);
        assert_eq!(ranked[0].detection, Detection::Source, "{file}");
        assert!(ranked[1..].iter().all(|c| c.detection < Detection::Source));
    }

    // A preset without a header row can't recognise its file: it's only
    // ever chosen by name (`coffer import --preset`).
    let headerless = presets.get("uk-example-headerless").unwrap();
    assert_eq!(
        headerless.importer().detect(&fixture(HEADERLESS)),
        Detection::Format
    );

    let ofx_importer = coffer_import::ofx::OfxImporter::new();
    for file in [
        "ofx/bank-sgml.ofx",
        "ofx/creditcard-xml.ofx",
        "ofx/intuit.qfx",
    ] {
        let source = fixture(file);
        let mut importers = presets.importers();
        importers.push(&ofx_importer);
        let ranked = detect(&importers, &source);
        assert_eq!(ranked[0].importer.format(), SourceFormat::Ofx, "{file}");
        assert!(
            ranked[1..]
                .iter()
                .all(|c| c.detection < ranked[0].detection)
        );
    }
}

/// Two presets naming the same columns tie on a file, and the caller asks.
#[test]
fn presets_with_the_same_columns_tie() {
    let tab = mapping_toml("uk/example-tab.toml");
    let twin = tab
        .replace("uk-example-tab", "uk-twin")
        .replace("Example Bank (tab-separated)", "Twin Bank");
    let presets = Presets::from_files([
        ("uk/example-tab.toml", tab.as_str()),
        ("uk/twin.toml", &twin),
    ])
    .unwrap();
    let importers = presets.importers();
    let ranked = detect(&importers, &fixture(NEWEST_FIRST));
    assert_eq!(ranked.len(), 2);
    assert_eq!(ranked[0].detection, ranked[1].detection);
    assert_eq!(ranked[0].detection, Detection::Source);
}

/// Header columns a preset neither reads nor ignores are listed, so the
/// preview can show them; ignored ones aren't.
#[test]
fn unread_columns_are_listed_for_the_preview() {
    let presets = mappings();
    let tab = presets.get("uk-example-tab").unwrap();
    let parsed = tab.importer().parse(&fixture(NEWEST_FIRST)).unwrap();
    assert!(tab.unread_columns(&parsed).is_empty());

    let reading_less = Preset::from_toml(
        &mapping_toml("uk/example-tab.toml").replace("balance = { column = \"Balance\" }", ""),
    )
    .unwrap();
    assert_eq!(reading_less.unread_columns(&parsed), ["Balance"]);
    let ignoring = Preset::from_toml(&mapping_toml("uk/example-tab.toml").replace(
        "balance = { column = \"Balance\" }",
        "ignore = [\"Balance\"]",
    ))
    .unwrap();
    assert!(ignoring.unread_columns(&parsed).is_empty());
}

// Signed amounts, IDs, every optional column ------------------------------------

#[test]
fn signed_amounts_commit_every_readable_row_and_record_the_rest() {
    let mut scratch = Scratch::new();
    let account = scratch.current("Current", Currency::GBP);
    let statement = read_with("uk-example-current", SIGNED);
    let preview = scratch.preview(account, "signed-amount.csv", statement);

    assert_eq!(
        outcome_kinds(&preview),
        kinds(&[
            (2, "new"),
            (3, "new"),
            (4, "new"),
            (5, "new"),
            (6, "new"),
            (7, "new"),
            (8, "flag:unreadable"),
            (9, "flag:unreadable"),
            (10, "flag:unreadable"),
            (11, "new"),
        ])
    );
    assert_eq!(
        preview.counts(),
        PreviewCounts {
            new: 7,
            already_imported: 0,
            flagged: 3
        }
    );
    assert_eq!(
        preview.needs_confirmation(),
        Confirmations {
            flagged_rows: true,
            balance_mismatch: false
        }
    );
    assert_eq!(
        preview.reconciliation().status(),
        ReconciliationStatus::NoBalances
    );
    assert!(preview.balances().is_empty());

    // Flagged rows need the caller's confirmation; refusing writes nothing.
    let refused = import::commit(&mut scratch.db, &preview, Confirmations::default());
    assert!(
        matches!(refused, Err(Error::NotConfirmed(needed)) if needed.flagged_rows),
        "{refused:?}"
    );
    assert_eq!(scratch.count("SELECT COUNT(*) FROM import_batch"), 0);
    assert_eq!(scratch.count("SELECT COUNT(*) FROM \"transaction\""), 0);

    let confirm = preview.needs_confirmation();
    let Committed::Batch(summary) = import::commit(&mut scratch.db, &preview, confirm).unwrap()
    else {
        panic!("expected a batch");
    };
    assert_eq!(summary.transactions, 7);
    assert_eq!(summary.balances, 0);
    assert_eq!(summary.flags, 3);
    // Eating out (3 rows), Groceries (2), Bills (1); Income isn't mapped.
    assert_eq!(summary.categorized_by_preset, 6);
    assert_eq!(summary.categorized_by_rules, 0);

    let info = import::batch(&scratch.db, summary.batch).unwrap();
    assert_eq!(info.source_file, "signed-amount.csv");
    assert_eq!(info.format, SourceFormat::Csv);
    assert_eq!(info.preset_id.as_deref(), Some("uk-example-current"));
    assert_eq!(info.transactions, 7);

    // Each flagged row is recorded at its line with core's reason, never
    // its text.
    let flags = import::batch_flags(&scratch.db, summary.batch).unwrap();
    let recorded: Vec<_> = flags.iter().map(|f| (f.location, f.kind)).collect();
    assert_eq!(
        recorded,
        [
            (SourceLocation::Line(8), FlagKind::Unreadable),
            (SourceLocation::Line(9), FlagKind::Unreadable),
            (SourceLocation::Line(10), FlagKind::Unreadable),
        ]
    );
    assert_eq!(
        flags[1].detail,
        RowProblem::ColumnCount {
            expected: 12,
            found: 13
        }
        .to_string()
    );
    for flag in &flags {
        for text in ["BAD DATE", "Nowhere", "Odd Shop", "Too Precise", "1.005"] {
            assert!(!flag.detail.contains(text), "{}", flag.detail);
        }
    }

    // A foreign card payment keeps every source field.
    let foreign = scratch.by_external_id(account, "tx_0002").unwrap();
    assert_eq!(foreign.date, "2026-11-01");
    assert_eq!(foreign.time.as_deref(), Some("12:30:00"));
    assert_eq!(foreign.amount_minor, -860);
    assert_eq!(foreign.currency, "GBP");
    assert_eq!(foreign.merchant.as_deref(), Some("Café Lumière"));
    assert_eq!(foreign.description_raw, "CAFE LUMIERE PARIS FRA");
    assert_eq!(foreign.description_normalized, "CAFE LUMIERE PARIS FRA");
    assert_eq!(foreign.source_category.as_deref(), Some("Eating out"));
    assert_eq!(foreign.source_type.as_deref(), Some("Card payment"));
    assert_eq!(foreign.original_amount_minor, Some(-1000));
    assert_eq!(foreign.original_currency.as_deref(), Some("EUR"));
    assert_eq!(foreign.note.as_deref(), Some("lunch"));
    assert_eq!(foreign.origin, "imported");
    assert_eq!(foreign.category.as_deref(), Some("food.eating_out"));
    assert_eq!(foreign.category_source.as_deref(), Some("preset_map"));
    assert_eq!(foreign.batch, Some(summary.batch.get()));
    assert_eq!(
        foreign.fingerprint.as_deref(),
        Some("v1|2026-11-01|-860|GBP|0|CAFE LUMIERE PARIS FRA")
    );

    // A local amount that repeats the amount isn't stored; blanks are NULL.
    let local = scratch.by_external_id(account, "tx_0001").unwrap();
    assert_eq!(local.original_amount_minor, None);
    assert_eq!(local.original_currency, None);
    assert_eq!(local.note, None);
    let no_time = scratch.by_external_id(account, "tx_0010").unwrap();
    assert_eq!(no_time.time, None);
    assert_eq!(no_time.category.as_deref(), Some("bills"));
    let income = scratch.by_external_id(account, "tx_0003").unwrap();
    assert_eq!(income.amount_minor, 200_000, "quoted thousands");
    assert_eq!((income.category, income.category_source), (None, None));
    let refund = scratch.by_external_id(account, "tx_0006").unwrap();
    assert_eq!(refund.amount_minor, 340);

    // Identical same-day rows both import, each with its own ID; their
    // fingerprints count them 0 and 1.
    let first = scratch.by_external_id(account, "tx_0004").unwrap();
    let second = scratch.by_external_id(account, "tx_0005").unwrap();
    assert_eq!(
        first.fingerprint.as_deref(),
        Some("v1|2026-11-03|-120|GBP|0|CORNER SHOP")
    );
    assert_eq!(
        second.fingerprint.as_deref(),
        Some("v1|2026-11-03|-120|GBP|1|CORNER SHOP")
    );

    // Importing the file again: every row is found by its ID, the flagged
    // rows are still flagged, and nothing is stored or recorded again.
    let again = scratch.preview(
        account,
        "signed-amount.csv",
        read_with("uk-example-current", SIGNED),
    );
    assert_eq!(
        again.counts(),
        PreviewCounts {
            new: 0,
            already_imported: 7,
            flagged: 3
        }
    );
    assert!(
        outcome_kinds(&again)
            .iter()
            .all(|(_, kind)| kind == "already:source_id" || kind == "flag:unreadable")
    );
    assert!(again.is_nothing_new());
    assert_eq!(
        import::commit(&mut scratch.db, &again, again.needs_confirmation()).unwrap(),
        Committed::NothingNew
    );
    assert_eq!(scratch.count("SELECT COUNT(*) FROM import_batch"), 1);
    assert_eq!(scratch.count("SELECT COUNT(*) FROM import_flag"), 3);
    assert_eq!(scratch.stored(account).len(), 7);
}

/// Line endings don't move a row: CRLF and old-Mac lone CRs give the same
/// rows at the same line numbers as LF.
#[test]
fn line_endings_dont_change_rows_or_line_numbers() {
    let presets = mappings();
    let importer = presets.get("uk-example-current").unwrap().importer();
    let lf = fixture_bytes(SIGNED);
    let crlf = String::from_utf8(lf.clone()).unwrap().replace('\n', "\r\n");
    let cr = String::from_utf8(lf.clone()).unwrap().replace('\n', "\r");

    let read = |bytes: Vec<u8>| {
        importer
            .import(&SourceFile::from_bytes("signed-amount.csv", bytes))
            .unwrap()
    };
    let expected = read(lf);
    for (name, bytes) in [("CRLF", crlf.into_bytes()), ("CR", cr.into_bytes())] {
        let statement = read(bytes);
        assert_eq!(statement.rows(), expected.rows(), "{name}");
    }
}

// Debit/credit, Windows-1252, a preamble, running balances oldest first -------

#[test]
fn debit_credit_windows_1252_records_the_opening_balance() {
    let mut scratch = Scratch::new();
    let account = scratch.current("Savings", Currency::GBP);
    let preview = scratch.preview(
        account,
        "debit-credit-windows-1252.csv",
        read_with("uk-example-savings", DEBIT_CREDIT),
    );
    assert_eq!(
        outcome_kinds(&preview),
        kinds(&[
            (5, "new"),
            (6, "new"),
            (7, "new"),
            (8, "flag:unreadable"),
            (9, "flag:unreadable"),
        ])
    );

    // The rows check one another, oldest first; the flagged rows end the
    // chain. The file opens with the balance at the end of the day before
    // its first row.
    let running = preview.reconciliation().running.clone().unwrap();
    assert_eq!(running.order, FileOrder::OldestFirst);
    assert_eq!(running.checked, 2);
    assert!(running.mismatches.is_empty());
    assert_eq!(
        preview.balances(),
        [FileBalance {
            date: date(2026, 10, 31),
            balance: gbp(0),
            kind: FileBalanceKind::Opening,
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

    let summary = scratch.import_batch(
        account,
        "debit-credit-windows-1252.csv",
        read_with("uk-example-savings", DEBIT_CREDIT),
    );
    assert_eq!(
        (summary.transactions, summary.balances, summary.flags),
        (3, 1, 2)
    );
    assert_eq!(
        scratch.balances(account),
        [(
            "2026-10-31".to_owned(),
            0,
            "csv".to_owned(),
            Some(summary.batch.get())
        )]
    );

    let stored = scratch.stored(account);
    let read: Vec<_> = stored
        .iter()
        .map(|row| (row.description_raw.as_str(), row.amount_minor))
        .collect();
    assert_eq!(
        read,
        [
            ("Opening transfer", 100_000),
            ("Café Rouge £5 deal", -1240),
            ("Refund Café Rouge", 240),
        ],
        "decoded from Windows-1252, money out negated"
    );
    assert_eq!(stored[1].description_normalized, "CAFÉ ROUGE £5 DEAL");
    assert_eq!(
        stored[1].fingerprint.as_deref(),
        Some("v1|2026-11-02|-1240|GBP|0|CAFÉ ROUGE £5 DEAL")
    );
    assert!(stored.iter().all(|row| row.external_id.is_none()));

    // Again: rows found by fingerprint, the opening already recorded.
    let again = scratch.preview(
        account,
        "debit-credit-windows-1252.csv",
        read_with("uk-example-savings", DEBIT_CREDIT),
    );
    assert_eq!(
        outcome_kinds(&again)[..3],
        kinds(&[
            (5, "already:fingerprint"),
            (6, "already:fingerprint"),
            (7, "already:fingerprint"),
        ])
    );
    assert!(matches!(
        again.balances()[0].status,
        FileBalanceStatus::AlreadyRecorded(_)
    ));
    assert!(again.is_nothing_new());
}

/// Read without its stated encoding, the Windows-1252 file fails as a
/// whole rather than importing mangled text.
#[test]
fn a_windows_1252_file_without_its_encoding_fails_whole() {
    let toml = mapping_toml("uk/example-savings.toml")
        .replace("encoding = \"windows-1252\"\n", "encoding = \"utf-8\"\n");
    let preset = Preset::from_toml(&toml).unwrap();
    assert_eq!(preset.mapping().encoding, TextEncoding::Utf8);
    let error = preset
        .importer()
        .import(&fixture(DEBIT_CREDIT))
        .unwrap_err();
    assert_eq!(error.to_string(), "the file isn't valid UTF-8 text");
}

// Semicolons, decimal commas, a BOM, a row currency, no IDs --------------------

#[test]
fn semicolon_decimal_comma_imports_into_a_euro_account() {
    let mut scratch = Scratch::new();
    let pounds = scratch.current("Pounds", Currency::GBP);
    let euros = scratch.current("Euros", Currency::EUR);
    let read = || read_with("eu-example-giro", SEMICOLON);

    let wrong = import::preview(&scratch.db, pounds, "giro.csv", read());
    assert!(
        matches!(
            wrong,
            Err(Error::CurrencyMismatch {
                account: Currency::GBP,
                statement: Currency::EUR
            })
        ),
        "{wrong:?}"
    );

    let preview = scratch.preview(euros, "giro.csv", read());
    assert_eq!(
        outcome_kinds(&preview),
        kinds(&[(2, "new"), (3, "new"), (4, "new"), (5, "flag:unreadable")])
    );
    scratch.import_batch(euros, "giro.csv", read());
    let stored: Vec<_> = scratch
        .stored(euros)
        .into_iter()
        .map(|row| {
            (
                row.merchant.unwrap(),
                row.description_raw,
                row.amount_minor,
                row.currency,
            )
        })
        .collect();
    assert_eq!(
        stored,
        [
            (
                "Muster GmbH".to_owned(),
                "Muster GmbH Gehalt November".to_owned(),
                250_000,
                "EUR".to_owned()
            ),
            (
                "Bäckerei Beispiel".to_owned(),
                "Bäckerei Beispiel Kartenzahlung".to_owned(),
                -450,
                "EUR".to_owned()
            ),
            (
                "Vermietung Beispiel".to_owned(),
                "Vermietung Beispiel Miete, November".to_owned(),
                -123_456,
                "EUR".to_owned()
            ),
        ]
    );
    let flags = import::batch_flags(
        &scratch.db,
        import::batches(&scratch.db, euros).unwrap()[0].id,
    )
    .unwrap();
    assert_eq!(flags.len(), 1);
    assert_eq!(
        flags[0].detail,
        "the amount is in USD, but the statement is in EUR"
    );

    let again = scratch.preview(euros, "giro.csv", read());
    assert_eq!(
        outcome_kinds(&again),
        kinds(&[
            (2, "already:fingerprint"),
            (3, "already:fingerprint"),
            (4, "already:fingerprint"),
            (5, "flag:unreadable"),
        ])
    );
}

/// A UTF-16 export (either byte order, with its BOM) reads exactly like
/// the UTF-8 original, so its rows get the same fingerprints and a
/// re-export in another encoding is found again.
#[test]
fn a_utf16_export_imports_like_its_utf8_original() {
    let utf8 = fixture_bytes(SEMICOLON);
    let text = String::from_utf8(utf8.clone()).unwrap();
    let text = text.strip_prefix('\u{feff}').unwrap();
    let mut le = vec![0xFF, 0xFE];
    let mut be = vec![0xFE, 0xFF];
    for unit in text.encode_utf16() {
        le.extend(unit.to_le_bytes());
        be.extend(unit.to_be_bytes());
    }

    let presets = mappings();
    let importer = presets.get("eu-example-giro").unwrap().importer();
    let mut scratch = Scratch::new();
    let account = scratch.current("Euros", Currency::EUR);
    let original = importer
        .import(&SourceFile::from_bytes("giro.csv", utf8))
        .unwrap();
    scratch.import_batch(account, "giro.csv", original.clone());

    for (name, bytes) in [("UTF-16LE", le), ("UTF-16BE", be)] {
        let parsed = importer
            .parse(&SourceFile::from_bytes("giro.csv", bytes))
            .unwrap();
        assert_eq!(parsed.encoding().name(), name);
        let statement = importer.normalize(parsed).unwrap();
        assert_eq!(statement.rows(), original.rows(), "{name}");
        let preview = scratch.preview(account, "giro.csv", statement);
        assert!(preview.is_nothing_new(), "{name}");
        assert_eq!(preview.counts().already_imported, 3, "{name}");
    }
}

// Tabs, newest first, no IDs, overlapping exports -----------------------------

#[test]
fn newest_first_running_balances_give_the_opening_of_the_oldest_day() {
    let mut scratch = Scratch::new();
    let account = scratch.current("Current", Currency::GBP);
    let preview = scratch.preview(
        account,
        "newest-first-tab.tsv",
        read_with("uk-example-tab", NEWEST_FIRST),
    );
    assert_eq!(preview.counts().new, 7);
    let running = preview.reconciliation().running.clone().unwrap();
    assert_eq!(running.order, FileOrder::NewestFirst);
    assert_eq!(running.checked, 6);
    assert!(running.mismatches.is_empty());
    assert_eq!(
        preview.balances(),
        [FileBalance {
            date: date(2026, 11, 9),
            balance: gbp(100_000),
            kind: FileBalanceKind::Opening,
            status: FileBalanceStatus::New,
        }]
    );
    assert_eq!(
        preview.reconciliation().status(),
        ReconciliationStatus::Reconciled
    );
    assert_eq!(preview.needs_confirmation(), Confirmations::default());

    let summary = scratch.import_batch(
        account,
        "newest-first-tab.tsv",
        read_with("uk-example-tab", NEWEST_FIRST),
    );
    assert_eq!((summary.transactions, summary.balances), (7, 1));
    assert_eq!(
        scratch.balances(account),
        [(
            "2026-11-09".to_owned(),
            100_000,
            "csv".to_owned(),
            Some(summary.batch.get())
        )]
    );

    // The two identical coffees both import, numbered in file order.
    let coffees: Vec<_> = scratch
        .stored(account)
        .into_iter()
        .filter(|row| row.description_raw == "Fake Coffee Bar")
        .map(|row| row.fingerprint.unwrap())
        .collect();
    assert_eq!(
        coffees,
        [
            "v1|2026-11-12|-280|GBP|0|FAKE COFFEE BAR",
            "v1|2026-11-12|-280|GBP|1|FAKE COFFEE BAR",
        ]
    );

    let again = scratch.preview(
        account,
        "newest-first-tab.tsv",
        read_with("uk-example-tab", NEWEST_FIRST),
    );
    assert!(again.is_nothing_new());
    assert_eq!(again.counts().already_imported, 7);
}

/// The next export repeats the whole of its first day: those rows are
/// found by fingerprint, only the rest is new, and its opening balance
/// checks the rows the first file brought.
#[test]
fn an_overlapping_export_adds_only_its_new_rows_and_reconciles() {
    let mut scratch = Scratch::new();
    let account = scratch.current("Current", Currency::GBP);
    scratch.import_batch(
        account,
        "newest-first-tab.tsv",
        read_with("uk-example-tab", NEWEST_FIRST),
    );

    let later = || read_with("uk-example-tab", NEWEST_FIRST_LATER);
    let preview = scratch.preview(account, "newest-first-tab-later.tsv", later());
    assert_eq!(
        outcome_kinds(&preview),
        kinds(&[
            (2, "new"),
            (3, "new"),
            (4, "new"),
            (5, "already:fingerprint"),
            (6, "already:fingerprint"),
        ])
    );
    assert_eq!(
        preview.balances(),
        [FileBalance {
            date: date(2026, 11, 19),
            balance: gbp(220_230),
            kind: FileBalanceKind::Opening,
            status: FileBalanceStatus::New,
        }]
    );
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
            since: date(2026, 11, 9)
        }
    );

    let summary = scratch.import_batch(account, "newest-first-tab-later.tsv", later());
    assert_eq!((summary.transactions, summary.balances), (3, 1));
    assert_eq!(scratch.stored(account).len(), 10);
    assert_eq!(scratch.balances(account).len(), 2);

    // Both files again: nothing new either way.
    for (name, file) in [
        ("newest-first-tab.tsv", NEWEST_FIRST),
        ("newest-first-tab-later.tsv", NEWEST_FIRST_LATER),
    ] {
        let again = scratch.preview(account, name, read_with("uk-example-tab", file));
        assert!(again.is_nothing_new(), "{name}");
    }
}

// No header, a preamble, a running balance that doesn't follow ----------------

#[test]
fn a_headerless_export_with_a_wrong_running_balance() {
    let mut scratch = Scratch::new();
    let account = scratch.current("Everyday", Currency::GBP);
    let read = || read_with("uk-example-headerless", HEADERLESS);
    let preview = scratch.preview(account, "headerless-preamble.csv", read());

    // Rows start after the two preamble lines; there's no header.
    assert_eq!(
        outcome_kinds(&preview),
        kinds(&[(3, "new"), (4, "new"), (5, "new"), (6, "new")])
    );
    let running = preview.reconciliation().running.clone().unwrap();
    assert_eq!(running.order, FileOrder::OldestFirst);
    assert_eq!(running.checked, 3);
    assert_eq!(
        running.mismatches,
        [RunningMismatch {
            location: SourceLocation::Line(5),
            computed: gbp(47_750),
            stated: gbp(47_000),
        }]
    );
    assert_eq!(
        preview.reconciliation().status(),
        ReconciliationStatus::Mismatch
    );
    assert_eq!(
        preview.needs_confirmation(),
        Confirmations {
            flagged_rows: false,
            balance_mismatch: true
        }
    );
    assert_eq!(
        preview.balances(),
        [FileBalance {
            date: date(2026, 11, 2),
            balance: gbp(50_000),
            kind: FileBalanceKind::Opening,
            status: FileBalanceStatus::New,
        }]
    );

    let refused = import::commit(&mut scratch.db, &preview, Confirmations::default());
    assert!(
        matches!(refused, Err(Error::NotConfirmed(needed)) if needed.balance_mismatch && !needed.flagged_rows),
        "{refused:?}"
    );
    let flagged_only = Confirmations {
        flagged_rows: true,
        balance_mismatch: false,
    };
    assert!(matches!(
        import::commit(&mut scratch.db, &preview, flagged_only),
        Err(Error::NotConfirmed(_))
    ));
    assert_eq!(scratch.count("SELECT COUNT(*) FROM import_batch"), 0);

    // Confirmed: the row with the wrong balance is imported, and its
    // balance is flagged with the batch.
    let summary = scratch.import_batch(account, "headerless-preamble.csv", read());
    assert_eq!(
        (summary.transactions, summary.balances, summary.flags),
        (4, 1, 1)
    );
    let flags = import::batch_flags(&scratch.db, summary.batch).unwrap();
    assert_eq!(flags.len(), 1);
    assert_eq!(flags[0].location, SourceLocation::Line(5));
    assert_eq!(flags[0].kind, FlagKind::RunningBalance);
    assert_eq!(
        flags[0].detail,
        "the running balance is 470.00 GBP, but the rows before it give 477.50 GBP"
    );

    let stored: Vec<_> = scratch
        .stored(account)
        .into_iter()
        .map(|row| {
            (
                row.date,
                row.source_type.unwrap(),
                row.description_raw,
                row.amount_minor,
            )
        })
        .collect();
    assert_eq!(
        stored,
        [
            (
                "2026-11-03".to_owned(),
                "DEB".to_owned(),
                "Example Grocer".to_owned(),
                -2000
            ),
            (
                "2026-11-04".to_owned(),
                "CR".to_owned(),
                "Example Refund".to_owned(),
                500
            ),
            (
                "2026-11-05".to_owned(),
                "DEB".to_owned(),
                "Example Pharmacy".to_owned(),
                -750
            ),
            (
                "2026-11-06".to_owned(),
                "DEB".to_owned(),
                "Example Cinema".to_owned(),
                -1200
            ),
        ]
    );
}

// IDs repeated in the file, a signed debit/credit pair, a line break ----------

#[test]
fn ids_repeated_in_the_file_and_a_row_over_two_lines() {
    let mut scratch = Scratch::new();
    let account = scratch.current("Checking", Currency::USD);
    let read = || read_with("us-example-checking", REPEATED_IDS);
    let preview = scratch.preview(account, "repeated-ids-us.csv", read());

    assert_eq!(
        outcome_kinds(&preview),
        kinds(&[
            (2, "new"),
            (3, "new"),
            // Same ID, date and amount: the file repeats the row.
            (4, "repeated"),
            // A quoted note over two lines, in a preset with one row per line.
            (5, "flag:unreadable"),
            // FAKE-REF-001 again, on another day for another amount.
            (7, "flag:source_id_conflict"),
            (8, "new"),
            (9, "new"),
            (10, "new"),
        ])
    );
    let rows: Vec<_> = preview.rows().collect();
    assert_eq!(
        *rows[2].outcome,
        import::RowOutcome::RepeatedInFile {
            first: SourceLocation::Line(3)
        }
    );
    assert_eq!(
        *rows[3].outcome,
        import::RowOutcome::Flagged(import::Flag::Unreadable(RowProblem::MultiLineRow {
            lines: 2
        }))
    );
    assert_eq!(
        *rows[4].outcome,
        import::RowOutcome::Flagged(import::Flag::SourceIdRepeated {
            first: SourceLocation::Line(2)
        })
    );
    assert_eq!(
        preview.counts(),
        PreviewCounts {
            new: 5,
            already_imported: 1,
            flagged: 2
        }
    );

    let summary = scratch.import_batch(account, "repeated-ids-us.csv", read());
    assert_eq!((summary.transactions, summary.flags), (5, 2));
    let flags = import::batch_flags(&scratch.db, summary.batch).unwrap();
    let recorded: Vec<_> = flags.iter().map(|f| (f.location, f.kind)).collect();
    assert_eq!(
        recorded,
        [
            (SourceLocation::Line(5), FlagKind::Unreadable),
            (SourceLocation::Line(7), FlagKind::SourceIdConflict),
        ]
    );

    let stored: Vec<_> = scratch
        .stored(account)
        .into_iter()
        .map(|row| {
            (
                row.date,
                row.external_id,
                row.amount_minor,
                row.fingerprint.unwrap(),
            )
        })
        .collect();
    assert_eq!(
        stored,
        [
            (
                "2026-11-02".to_owned(),
                Some("FAKE-REF-001".to_owned()),
                -1825,
                "v1|2026-11-02|-1825|USD|0|EXAMPLE DINER".to_owned()
            ),
            (
                "2026-11-03".to_owned(),
                Some("FAKE-REF-002".to_owned()),
                240_000,
                "v1|2026-11-03|240000|USD|0|EXAMPLE PAYROLL".to_owned()
            ),
            (
                "2026-11-06".to_owned(),
                None,
                -150,
                "v1|2026-11-06|-150|USD|0|EXAMPLE VENDING".to_owned()
            ),
            (
                "2026-11-06".to_owned(),
                None,
                -150,
                "v1|2026-11-06|-150|USD|1|EXAMPLE VENDING".to_owned()
            ),
            (
                "2026-11-07".to_owned(),
                Some("FAKE-REF-004".to_owned()),
                2500,
                "v1|2026-11-07|2500|USD|0|EXAMPLE TRANSFER IN".to_owned()
            ),
        ],
        "a signed pair is taken as written; a zero in the other column is fine"
    );

    // With `multiline_fields` the note is read, line break included, and
    // re-importing the file brings in just that row.
    let multiline = Preset::from_toml(&format!(
        "{}multiline_fields = true\n",
        mapping_toml("us/example-checking.toml")
    ))
    .unwrap();
    let statement = multiline.importer().import(&fixture(REPEATED_IDS)).unwrap();
    let preview = scratch.preview(account, "repeated-ids-us.csv", statement.clone());
    assert_eq!(
        outcome_kinds(&preview),
        kinds(&[
            (2, "already:source_id"),
            (3, "already:source_id"),
            (4, "repeated"),
            (5, "new"),
            (7, "flag:source_id_conflict"),
            (8, "already:fingerprint"),
            (9, "already:fingerprint"),
            (10, "already:source_id"),
        ])
    );
    scratch.import_batch(account, "repeated-ids-us.csv", statement);
    let hardware = scratch.by_external_id(account, "FAKE-REF-003").unwrap();
    assert_eq!(hardware.note.as_deref(), Some("Shelf brackets\nand screws"));
    assert_eq!(hardware.amount_minor, -3140);
}
