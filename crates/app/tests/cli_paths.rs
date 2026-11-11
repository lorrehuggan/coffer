//! `coffer import`, `accounts` and `undo-import` on every fixture shape
//! (M1.19), against scratch data dirs. The CLI's own tests
//! (`src/cli/tests.rs`) cover argument parsing and the Monzo preset; these
//! add the generic CSV shapes (through the test presets in
//! `fixtures/csv/mappings/`), QFX, credit cards, running balances, ties
//! between presets, undo with balances, flags and transfers, and a sweep
//! that no output carries a fixture's transaction text.

use std::path::{Path, PathBuf};

use clap::Parser;
use coffer_app::cli::{self, Cli, Command};
use coffer_core::db::Database;
use coffer_import::ofx::OfxImporter;
use coffer_import::preset::Presets;
use coffer_import::{AnyImporter, SourceFile};
use tempfile::TempDir;

fn fixture(path: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(path)
        .to_string_lossy()
        .into_owned()
}

/// The test presets in `fixtures/csv/mappings/<region>/<name>.toml`.
fn mappings() -> Presets {
    let root = PathBuf::from(fixture("csv/mappings"));
    let mut files: Vec<(String, String)> = Vec::new();
    for region in std::fs::read_dir(&root).unwrap() {
        let region = region.unwrap();
        for file in std::fs::read_dir(region.path()).unwrap() {
            let file = file.unwrap().path();
            files.push((
                format!(
                    "{}/{}",
                    region.file_name().to_string_lossy(),
                    file.file_name().unwrap().to_string_lossy()
                ),
                std::fs::read_to_string(&file).unwrap(),
            ));
        }
    }
    Presets::from_files(files.iter().map(|(p, t)| (p.as_str(), t.as_str()))).unwrap()
}

/// A scratch data dir; the database appears when a command creates it.
struct Scratch {
    _dir: TempDir,
    db: PathBuf,
}

impl Scratch {
    fn new() -> Scratch {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("coffer.db");
        Scratch { _dir: dir, db }
    }

    fn parse(args: &[&str]) -> Command {
        Cli::try_parse_from(std::iter::once("coffer").chain(args.iter().copied()))
            .unwrap_or_else(|error| panic!("{args:?} doesn't parse: {error}"))
            .command
            .expect("a subcommand")
    }

    /// `coffer <args>` with the bundled presets: output, or the error as
    /// `main` prints it.
    fn run(&self, args: &[&str]) -> Result<String, String> {
        let mut out = Vec::new();
        let result = cli::run(Self::parse(args), &self.db, &mut out);
        let out = String::from_utf8(out).unwrap();
        result.map(|()| out).map_err(|error| format!("{error:#}"))
    }

    /// `coffer import <args>` with `presets` in place of the bundled ones.
    fn import_with(&self, presets: &Presets, args: &[&str]) -> Result<String, String> {
        let Command::Import(import) = Self::parse(&[&["import"], args].concat()) else {
            unreachable!()
        };
        let mut db = Database::open(&self.db).unwrap();
        let mut out = Vec::new();
        let result = cli::import::run(&mut db, presets, &import, &mut out);
        let out = String::from_utf8(out).unwrap();
        result.map(|()| out).map_err(|error| format!("{error:#}"))
    }

    fn ok(&self, args: &[&str]) -> String {
        self.run(args)
            .unwrap_or_else(|error| panic!("{args:?} failed: {error}"))
    }

    fn err(&self, args: &[&str]) -> String {
        self.run(args)
            .err()
            .unwrap_or_else(|| panic!("{args:?} succeeded"))
    }

    fn ok_with(&self, presets: &Presets, args: &[&str]) -> String {
        self.import_with(presets, args)
            .unwrap_or_else(|error| panic!("{args:?} failed: {error}"))
    }

    fn err_with(&self, presets: &Presets, args: &[&str]) -> String {
        self.import_with(presets, args)
            .err()
            .unwrap_or_else(|| panic!("{args:?} succeeded"))
    }

    fn add(&self, name: &str, kind: &str, currency: &str) {
        self.ok(&[
            "accounts",
            "add",
            "--name",
            name,
            "--kind",
            kind,
            "--currency",
            currency,
        ]);
    }
}

fn assert_has(out: &str, part: &str) {
    assert!(out.contains(part), "missing {part:?} in:\n{out}");
}

// CSV through test presets ----------------------------------------------------

#[test]
fn a_csv_with_running_balances_is_detected_and_shows_its_checks() {
    let scratch = Scratch::new();
    scratch.add("Savings", "savings", "GBP");
    let presets = mappings();
    let file = fixture("csv/debit-credit-windows-1252.csv");

    // Detected by its header row after the preamble, among six presets.
    let out = scratch.ok_with(&presets, &[&file, "--dry-run"]);
    assert_has(&out, "File:      debit-credit-windows-1252.csv\n");
    assert_has(
        &out,
        "Format:    CSV, preset uk-example-savings (Example Bank (savings))\n",
    );
    assert_has(&out, "Account:   1 Savings (savings, GBP)\n");
    assert_has(&out, "Rows:      3 new, 0 already imported, 2 flagged\n");
    assert_has(
        &out,
        "Balances:\n  2026-10-31  0.00 GBP  opening, from the running balances, new\n",
    );
    assert_has(
        &out,
        "Reconciliation: reconciled: every balance check passed\n  \
         running balances (oldest first): 2 checked, 0 wrong\n",
    );
    assert_has(
        &out,
        "Flagged rows (not imported; recorded with the import):\n  \
         line 8: the amount can't be read: both the money-out and money-in columns have an amount\n  \
         line 9: the amount can't be read: a money-in or money-out column has a negative amount\n",
    );
    assert!(
        out.ends_with(
            "Dry run: nothing was written.\nImporting it needs --yes to accept flagged rows.\n"
        ),
        "{out}"
    );

    let error = scratch.err_with(&presets, &[&file]);
    assert!(
        error.starts_with("not imported: the import has flagged rows"),
        "{error}"
    );
    let out = scratch.ok_with(&presets, &[&file, "--yes"]);
    assert_has(&out, "as batch 1: 3 transactions, 1 balance, 2 flags.\n");

    // Again: found by fingerprint, the opening already recorded, and no
    // --yes needed for nothing.
    let out = scratch.ok_with(&presets, &[&file]);
    assert_has(&out, "Rows:      0 new, 3 already imported, 2 flagged\n");
    assert_has(
        &out,
        "  2026-10-31  0.00 GBP  opening, from the running balances, already recorded (balance 1)\n",
    );
    assert!(out.ends_with("this file.\n"), "{out}");
    let out = scratch.ok_with(&presets, &[&file, "--dry-run"]);
    assert!(out.ends_with("Dry run: nothing was written.\n"), "{out}");
}

#[test]
fn a_newest_first_export_and_its_overlapping_successor() {
    let scratch = Scratch::new();
    scratch.add("Current", "current", "GBP");
    let presets = mappings();
    let out = scratch.ok_with(&presets, &[&fixture("csv/newest-first-tab.tsv")]);
    assert_has(&out, "Format:    CSV, preset uk-example-tab");
    assert_has(
        &out,
        "running balances (newest first): 6 checked, 0 wrong\n",
    );
    assert_has(
        &out,
        "  2026-11-09  1000.00 GBP  opening, from the running balances, new\n",
    );
    assert_has(&out, "as batch 1: 7 transactions, 1 balance, 0 flags.\n");

    let out = scratch.ok_with(&presets, &[&fixture("csv/newest-first-tab-later.tsv")]);
    assert_has(&out, "Rows:      3 new, 2 already imported, 0 flagged\n");
    assert_has(
        &out,
        "Reconciliation: reconciled: every balance check passed\n",
    );
    assert_has(
        &out,
        "  2026-11-19  2202.30 GBP (from this file): passes, counting from 2026-11-09\n",
    );
    assert_has(&out, "as batch 2: 3 transactions, 1 balance, 0 flags.\n");
}

#[test]
fn a_headerless_file_needs_its_preset_named_and_a_mismatch_needs_yes() {
    let scratch = Scratch::new();
    scratch.add("Everyday", "current", "GBP");
    let presets = mappings();
    let file = fixture("csv/headerless-preamble.csv");

    // Without a header nothing recognises it, so it isn't guessed at.
    let error = scratch.err_with(&presets, &[&file]);
    assert!(
        error.starts_with("couldn't tell what kind of file headerless-preamble.csv is"),
        "{error}"
    );
    assert_has(
        &error,
        "  uk-example-headerless  Example Bank (no header)\n",
    );

    let named = [file.as_str(), "--preset", "uk-example-headerless"];
    let out = scratch.ok_with(&presets, &[&named[..], &["--dry-run"]].concat());
    assert_has(
        &out,
        "Reconciliation: MISMATCH: a balance doesn't agree with the transactions\n  \
         running balances (oldest first): 3 checked, 1 wrong\n    \
         line 5: states 470.00 GBP, but the rows before it give 477.50 GBP \
         (the row is imported, its balance is flagged)\n",
    );
    assert!(
        out.ends_with("Importing it needs --yes to accept a balance mismatch.\n"),
        "{out}"
    );
    let error = scratch.err_with(&presets, &named);
    assert!(
        error.starts_with("not imported: the import has a balance mismatch"),
        "{error}"
    );
    assert_has(&error, "the mismatch stays flagged on the account");
    let out = scratch.ok_with(&presets, &[&named[..], &["--yes"]].concat());
    assert_has(&out, "as batch 1: 4 transactions, 1 balance, 1 flag.\n");
}

#[test]
fn flagged_rows_and_a_mismatch_together_need_yes_for_both() {
    let scratch = Scratch::new();
    scratch.add("Everyday", "current", "GBP");
    let presets = mappings();
    // The headerless file read with a stray line added: a flagged row and
    // the running mismatch at once.
    let text = std::fs::read_to_string(fixture("csv/headerless-preamble.csv")).unwrap();
    let file = scratch._dir.path().join("statement.csv");
    std::fs::write(
        &file,
        format!("{text}not a date,DEB,FAKE ROW,-1.00,457.00\n"),
    )
    .unwrap();
    let file = file.to_string_lossy().into_owned();

    let error = scratch.err_with(&presets, &[&file, "--preset", "uk-example-headerless"]);
    assert!(
        error.starts_with("not imported: the import has flagged rows and a balance mismatch"),
        "{error}"
    );
    let out = scratch.ok_with(
        &presets,
        &[&file, "--preset", "uk-example-headerless", "--yes"],
    );
    assert_has(&out, "as batch 1: 4 transactions, 1 balance, 2 flags.\n");
}

#[test]
fn presets_with_the_same_columns_tie_and_need_naming() {
    let scratch = Scratch::new();
    scratch.add("Current", "current", "GBP");
    let tab = std::fs::read_to_string(fixture("csv/mappings/uk/example-tab.toml")).unwrap();
    let twin = tab
        .replace("uk-example-tab", "uk-twin")
        .replace("Example Bank (tab-separated)", "Twin Bank");
    let presets = Presets::from_files([
        ("uk/example-tab.toml", tab.as_str()),
        ("uk/twin.toml", &twin),
    ])
    .unwrap();
    let file = fixture("csv/newest-first-tab.tsv");

    let error = scratch.err_with(&presets, &[&file]);
    assert_eq!(
        error,
        "newest-first-tab.tsv fits more than one importer equally well; choose its preset \
         with --preset:\n  uk-example-tab  Example Bank (tab-separated)\n  uk-twin  Twin Bank"
    );
    let out = scratch.ok_with(&presets, &[&file, "--preset", "uk-twin"]);
    assert_has(&out, "Format:    CSV, preset uk-twin (Twin Bank)\n");
}

#[test]
fn a_named_preset_that_doesnt_fit_fails_without_the_files_text() {
    let scratch = Scratch::new();
    scratch.add("Current", "current", "GBP");
    let presets = mappings();
    let error = scratch.err_with(
        &presets,
        &[
            &fixture("csv/newest-first-tab.tsv"),
            "--preset",
            "uk-example-current",
        ],
    );
    // That preset states `,`, so the tab-separated header is one cell.
    assert_eq!(
        error,
        "couldn't read newest-first-tab.tsv with the uk-example-current preset: \
         the file has no “Date” column"
    );
    // An OFX file named with a preset is read as CSV, and fails as one.
    let error = scratch.err_with(
        &presets,
        &[&fixture("ofx/bank-sgml.ofx"), "--preset", "uk-example-tab"],
    );
    assert!(
        error.starts_with("couldn't read bank-sgml.ofx with the uk-example-tab preset"),
        "{error}"
    );
    assert!(!error.contains("Corner"), "{error}");
}

// OFX, QFX and credit cards ------------------------------------------------------

#[test]
fn qfx_and_credit_card_files_go_to_the_account_named() {
    let scratch = Scratch::new();
    scratch.add("Checking", "current", "USD");
    scratch.add("Card", "credit_card", "USD");
    let qfx = fixture("ofx/intuit.qfx");

    let error = scratch.err(&["import", &qfx]);
    assert_eq!(
        error,
        "choose the account to import into with --account <ID or name>:\n  \
         1 Checking (current, USD)\n  2 Card (credit_card, USD)"
    );
    let out = scratch.ok(&["import", &qfx, "--account", "Checking"]);
    assert_has(&out, "Format:    OFX\n");
    assert_has(&out, "  2026-11-30  2845.79 USD  stated by the file, new\n");
    assert_has(&out, "Reconciliation: starting point");
    assert_has(&out, "as batch 1: 3 transactions, 1 balance, 0 flags.\n");

    let card = fixture("ofx/creditcard-xml.ofx");
    let out = scratch.ok(&["import", &card, "--account", "2", "--statement", "1"]);
    assert_has(&out, "Format:    OFX, statement 1 of 1\n");
    assert_has(&out, "Account:   2 Card (credit_card, USD)\n");
    assert_has(
        &out,
        "  2026-11-30  -1234.56 USD  stated by the file, new\n",
    );
    assert_has(&out, "as batch 2: 5 transactions, 1 balance, 0 flags.\n");

    assert_eq!(
        scratch.err(&["import", &card, "--account", "2", "--statement", "2"]),
        "creditcard-xml.ofx holds 1 statement; there is no statement 2:\n  \
         1  credit card, ending 0000, USD, 5 transactions"
    );
}

#[test]
fn an_ofx_balance_against_the_next_file_reconciles() {
    let scratch = Scratch::new();
    scratch.add("Current", "current", "GBP");
    scratch.ok(&["import", &fixture("ofx/bank-sgml.ofx")]);
    let next = fixture("ofx/bank-sgml-next.ofx");
    let out = scratch.ok(&["import", &next, "--dry-run"]);
    assert_has(&out, "Rows:      3 new, 2 already imported, 1 flagged\n");
    assert_has(
        &out,
        "  2026-12-31  3806.98 GBP (from this file): passes, counting from 2026-11-30\n",
    );
    assert_has(
        &out,
        "  entry 1: the account already has transaction 5 with this ID, on another date or for another amount\n",
    );
    let out = scratch.ok(&["import", &next, "--yes"]);
    assert_has(&out, "as batch 2: 3 transactions, 1 balance, 1 flag.\n");
}

// Undo -------------------------------------------------------------------------

#[test]
fn undo_lists_balances_and_flags_and_removes_them() {
    let scratch = Scratch::new();
    scratch.add("Girokonto", "current", "EUR");
    scratch.add("Current", "current", "GBP");
    scratch.ok(&["import", &fixture("ofx/edge-cases.ofx"), "--yes"]);
    scratch.ok(&["import", &fixture("ofx/bank-sgml.ofx")]);

    let out = scratch.ok(&["undo-import", "1"]);
    assert_has(
        &out,
        "Undoing it removes:\n  4 transactions\n  0 balances\n  9 flags recorded with it\n",
    );
    let out = scratch.ok(&["undo-import", "1", "--yes"]);
    assert!(
        out.ends_with("Undid import 1: removed 4 transactions, 0 balances and 9 flags.\n"),
        "{out}"
    );

    let out = scratch.ok(&["undo-import", "2"]);
    assert!(
        out.starts_with("Import 2: bank-sgml.ofx into 2 Current (current, GBP), imported "),
        "{out}"
    );
    assert_has(
        &out,
        "  7 transactions\n  1 balance\n  0 flags recorded with it\n",
    );
    assert!(!out.contains("Later imports"), "{out}");
    let out = scratch.ok(&["undo-import", "2", "--yes"]);
    assert_has(&out, "removed 7 transactions, 1 balance and 0 flags.\n");

    // Re-imported, the balance is new again.
    let out = scratch.ok(&["import", &fixture("ofx/bank-sgml.ofx")]);
    assert_has(&out, "  2026-11-30  2381.47 GBP  stated by the file, new\n");
}

#[test]
fn undo_says_a_transfer_pair_is_dissolved() {
    let scratch = Scratch::new();
    scratch.add("Current", "current", "GBP");
    scratch.add("Savings", "savings", "GBP");
    let presets = mappings();
    scratch.ok_with(
        &presets,
        &[&fixture("csv/newest-first-tab.tsv"), "--account", "Current"],
    );
    let out = scratch.ok(&[
        "import",
        &fixture("ofx/multi-statement.ofx"),
        "--statement",
        "2",
        "--account",
        "Savings",
    ]);
    assert_has(&out, "Transfer pairs suggested: 1.\n");

    let out = scratch.ok(&["undo-import", "2"]);
    assert_has(
        &out,
        "1 transfer pair is dissolved: the other side stays as an ordinary transaction.\n",
    );
    scratch.ok(&["undo-import", "2", "--yes"]);
    let out = scratch.ok(&["undo-import", "1"]);
    assert!(!out.contains("transfer pair"), "{out}");
}

// No transaction text in any output ---------------------------------------------

/// Every text field of every readable row of a fixture, at least five
/// characters long (shorter ones like `CR` could match ordinary words).
fn texts(statement: &coffer_core::statement::Statement) -> Vec<String> {
    statement
        .lines()
        .flat_map(|(_, line)| {
            [
                Some(line.description_raw.clone()),
                line.merchant.clone(),
                line.note.clone(),
                line.external_id.clone(),
            ]
        })
        .flatten()
        .filter(|text| text.chars().count() >= 5)
        .collect()
}

/// The CLI prints positions, counts and core's reasons, never a row's text
/// (decision record "rules engine, transfer detection and the CLI", point
/// 9): checked on every fixture through dry run, import, undo preview and
/// undo, flagged rows included.
#[test]
fn no_output_contains_a_fixtures_transaction_text() {
    let presets = mappings();
    let csv = [
        ("csv/signed-amount.csv", "uk-example-current", "GBP"),
        (
            "csv/debit-credit-windows-1252.csv",
            "uk-example-savings",
            "GBP",
        ),
        ("csv/semicolon-decimal-comma.csv", "eu-example-giro", "EUR"),
        ("csv/newest-first-tab.tsv", "uk-example-tab", "GBP"),
        ("csv/newest-first-tab-later.tsv", "uk-example-tab", "GBP"),
        (
            "csv/headerless-preamble.csv",
            "uk-example-headerless",
            "GBP",
        ),
        ("csv/repeated-ids-us.csv", "us-example-checking", "USD"),
    ];
    let ofx = [
        ("ofx/bank-sgml.ofx", None, "GBP"),
        ("ofx/bank-sgml-next.ofx", None, "GBP"),
        ("ofx/creditcard-xml.ofx", None, "USD"),
        ("ofx/intuit.qfx", None, "USD"),
        ("ofx/edge-cases.ofx", None, "EUR"),
        ("ofx/multi-statement.ofx", Some(1), "GBP"),
        ("ofx/multi-statement.ofx", Some(2), "GBP"),
        ("ofx/multi-statement.ofx", Some(3), "GBP"),
    ];
    // Text in rows the importer flags, which `texts` can't see.
    let flagged_text = [
        "BAD DATE ROW",
        "Odd Shop",
        "TOO PRECISE ROW",
        "Both columns filled",
        "Negative money out",
        "Hotel Beispiel",
        "Shelf brackets",
        "No date",
        "Thirtieth of February",
        "Second of a pair",
        "Two amounts",
        "Unknown currency",
    ];

    let scratch = Scratch::new();
    for currency in ["GBP", "EUR", "USD"] {
        scratch.add(&format!("Account {currency}"), "current", currency);
    }
    let account = |currency: &str| format!("Account {currency}");
    let mut outputs: Vec<String> = Vec::new();
    let mut forbidden: Vec<String> = flagged_text.iter().map(|s| (*s).to_owned()).collect();

    for (file, preset, currency) in csv {
        let source = SourceFile::read(Path::new(&fixture(file))).unwrap();
        forbidden.extend(texts(
            &presets
                .get(preset)
                .unwrap()
                .importer()
                .import(&source)
                .unwrap(),
        ));
        let path = fixture(file);
        let args = [
            path.as_str(),
            "--preset",
            preset,
            "--account",
            &account(currency),
        ];
        outputs.push(scratch.ok_with(&presets, &[&args[..], &["--dry-run"]].concat()));
        outputs.push(scratch.ok_with(&presets, &[&args[..], &["--yes"]].concat()));
    }
    for (file, statement, currency) in ofx {
        let source = SourceFile::read(Path::new(&fixture(file))).unwrap();
        let importer = match statement {
            Some(n) => OfxImporter::for_statement(n - 1),
            None => OfxImporter::new(),
        };
        forbidden.extend(texts(&importer.import(&source).unwrap()));
        let path = fixture(file);
        let number = statement.map(|n: usize| n.to_string());
        let mut args = vec!["import", path.as_str(), "--account"];
        let name = account(currency);
        args.push(&name);
        if let Some(number) = &number {
            args.extend(["--statement", number.as_str()]);
        }
        outputs.push(scratch.ok(&[&args[..], &["--dry-run"]].concat()));
        outputs.push(scratch.ok(&[&args[..], &["--yes"]].concat()));
    }
    // Errors that name the file, then the history and undo.
    outputs.push(scratch.err(&["import", &fixture("ofx/multi-statement.ofx")]));
    outputs.push(scratch.ok(&["accounts"]));
    let batches = 15;
    for batch in 1..=batches {
        let batch = batch.to_string();
        outputs.push(scratch.ok(&["undo-import", &batch]));
    }
    assert!(scratch.run(&["undo-import", "16"]).is_err(), "15 imports");
    for batch in (1..=batches).rev() {
        let batch = batch.to_string();
        if let Ok(out) = scratch.run(&["undo-import", &batch, "--yes"]) {
            outputs.push(out);
        }
    }

    assert!(forbidden.len() > 60, "{} texts", forbidden.len());
    for out in &outputs {
        for text in &forbidden {
            assert!(!out.contains(text.as_str()), "{text:?} in:\n{out}");
        }
    }
}
