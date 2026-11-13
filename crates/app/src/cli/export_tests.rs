//! `coffer export` against scratch data dirs. Nothing here touches the
//! real data dir.

use std::path::{Path, PathBuf};

use clap::Parser;
use coffer_core::category::CategoryKey;
use coffer_core::db::Database;
use coffer_core::export::{self, Options};
use coffer_core::id::{AccountId, TransactionId};
use coffer_core::manual::{self, ManualTransaction};
use coffer_core::money::{Currency, Money};
use coffer_core::split::{self, SplitLine};
use coffer_core::{note, tag};
use tempfile::TempDir;

use super::*;

const PRIVATE_TAG: &str = "Secret";
const USER_NOTE: &str = "my own words";
const LINE_NOTE: &str = "line words";
/// A note typed in Monzo's app, in the fixture's "Notes and #tags".
const BANK_NOTE: &str = "Team dinner";

fn fixture(path: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(path)
        .to_string_lossy()
        .into_owned()
}

fn day(d: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

struct Scratch {
    dir: TempDir,
    db: PathBuf,
}

impl Scratch {
    fn new() -> Scratch {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("coffer.db");
        Scratch { dir, db }
    }

    fn run(&self, args: &[&str]) -> Result<String, String> {
        let cli = Cli::try_parse_from(std::iter::once("coffer").chain(args.iter().copied()))
            .unwrap_or_else(|error| panic!("{args:?} doesn't parse: {error}"));
        let mut out = Vec::new();
        let result = super::run(cli.command.expect("a subcommand"), &self.db, &mut out);
        let out = String::from_utf8(out).unwrap();
        result.map(|()| out).map_err(|error| format!("{error:#}"))
    }

    fn ok(&self, args: &[&str]) -> String {
        self.run(args)
            .unwrap_or_else(|error| panic!("{args:?} failed: {error}"))
    }

    fn err(&self, args: &[&str]) -> String {
        match self.run(args) {
            Ok(out) => panic!("{args:?} succeeded:\n{out}"),
            Err(error) => error,
        }
    }

    fn json(&self, args: &[&str]) -> serde_json::Value {
        let out = self.ok(&[&["export", "--format", "json"], args].concat());
        serde_json::from_str(&out).unwrap()
    }

    /// `(transaction_id, line_no)` of each exported row.
    fn ids(&self, args: &[&str]) -> Vec<(i64, Option<i64>)> {
        self.json(args)["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row["transaction_id"].as_i64().unwrap(),
                    row["line_no"].as_i64(),
                )
            })
            .collect()
    }

    /// Monzo (1, current, the Monzo fixture imported: October 2026, with
    /// its pots after the card) and Card (2, credit card). On the card: a lunch on 2 November, tagged
    /// "Trip" and the private tag, with a user note; a shop on the 3rd split
    /// into groceries (with a line note) and household.
    fn with_ledger() -> (Scratch, Ids) {
        let scratch = Scratch::new();
        for (name, kind) in [("Monzo", "current"), ("Card", "credit_card")] {
            scratch.ok(&[
                "accounts",
                "add",
                "--name",
                name,
                "--kind",
                kind,
                "--currency",
                "GBP",
            ]);
        }
        scratch.ok(&[
            "import",
            &fixture("presets/uk/monzo.csv"),
            "--account",
            "Monzo",
        ]);

        let mut db = Database::open(&scratch.db).unwrap();
        let card = AccountId::new(2);
        let mut lunch = ManualTransaction::new(card, day(2), gbp(-1_234), "LUNCH");
        lunch.category = Some(CategoryKey::new("food.eating_out").unwrap());
        let lunch = manual::create(&mut db, &lunch).unwrap();
        let trip = tag::create(&mut db, &tag::TagName::new("Trip").unwrap(), false).unwrap();
        let secret = tag::create(&mut db, &tag::TagName::new(PRIVATE_TAG).unwrap(), true).unwrap();
        tag::add_to_transaction(&mut db, lunch, trip).unwrap();
        tag::add_to_transaction(&mut db, lunch, secret).unwrap();
        note::set(&mut db, lunch, USER_NOTE).unwrap();

        let shop = manual::create(
            &mut db,
            &ManualTransaction::new(card, day(3), gbp(-5_000), "SHOP"),
        )
        .unwrap();
        let mut groceries = SplitLine::new(
            gbp(-3_000),
            Some(CategoryKey::new("food.groceries").unwrap()),
        );
        groceries.user_note = Some(LINE_NOTE.into());
        split::set(
            &mut db,
            shop,
            &[
                groceries,
                SplitLine::new(gbp(-2_000), Some(CategoryKey::new("home").unwrap())),
            ],
        )
        .unwrap();
        (scratch, Ids { lunch, shop })
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
}

struct Ids {
    lunch: TransactionId,
    shop: TransactionId,
}

#[test]
fn parses_export() {
    let cli = Cli::try_parse_from([
        "coffer",
        "export",
        "--format",
        "csv",
        "-o",
        "out.csv",
        "--force",
        "--from",
        "2026-10-01",
        "--to",
        "2026-10-31",
        "--account",
        "1",
        "--account",
        "Card",
        "--category",
        "food",
        "--tag",
        "Trip",
        "--include-notes",
        "--include-private-tags",
    ])
    .unwrap();
    let Some(Command::Export(args)) = cli.command else {
        panic!("not an export: {:?}", cli.command);
    };
    assert_eq!(args.format, ExportFormat::Csv);
    assert_eq!(args.output, Some(PathBuf::from("out.csv")));
    assert!(args.force && args.include_notes && args.include_private_tags);
    assert_eq!(args.from, chrono::NaiveDate::from_ymd_opt(2026, 10, 1));
    assert_eq!(args.account, ["1", "Card"]);
    assert_eq!(args.category, ["food"]);
    assert_eq!(args.tag, ["Trip"]);

    let fails = |args: &[&str]| {
        Cli::try_parse_from(std::iter::once("coffer").chain(args.iter().copied())).is_err()
    };
    assert!(fails(&["export"]), "--format is required");
    assert!(fails(&["export", "--format", "xlsx"]));
    assert!(
        fails(&["export", "--format", "csv", "--force"]),
        "--force needs --output"
    );
    assert!(fails(&["export", "--format", "csv", "--from", "1/10/2026"]));
}

#[test]
fn needs_an_existing_database() {
    let scratch = Scratch::new();
    let error = scratch.err(&["export", "--format", "csv"]);
    assert!(error.contains("there is no Coffer database"), "{error}");
    assert!(!scratch.db.exists());
}

#[test]
fn csv_to_stdout_is_the_export_and_nothing_else() {
    let (scratch, _) = Scratch::with_ledger();
    let out = scratch.ok(&["export", "--format", "csv"]);
    let db = Database::open(&scratch.db).unwrap();
    let mut expected = Vec::new();
    export::write(&db, export::Format::Csv, &Options::default(), &mut expected).unwrap();
    assert_eq!(out, String::from_utf8(expected).unwrap());
    let header = out.lines().next().unwrap();
    assert_eq!(header, export::columns(&Options::default()).join(","));
    // 14 Monzo rows, 4 mirrored into its pots, the lunch and two split
    // lines.
    assert_eq!(out.lines().count(), 1 + 14 + 4 + 1 + 2);
}

#[test]
fn notes_and_private_tags_only_with_their_flags() {
    let (scratch, _) = Scratch::with_ledger();
    let secrets = [PRIVATE_TAG, USER_NOTE, LINE_NOTE, BANK_NOTE];
    for format in ["csv", "json"] {
        let out = scratch.ok(&["export", "--format", format]);
        for secret in secrets {
            assert!(!out.contains(secret), "{format} has {secret:?}");
        }
        assert!(out.contains("Trip"), "public tags are in");

        let out = scratch.ok(&["export", "--format", format, "--include-notes"]);
        for secret in [USER_NOTE, LINE_NOTE, BANK_NOTE] {
            assert!(out.contains(secret), "{format} lacks {secret:?}");
        }
        assert!(!out.contains(PRIVATE_TAG));

        let out = scratch.ok(&["export", "--format", format, "--include-private-tags"]);
        assert!(out.contains(PRIVATE_TAG));
        for secret in [USER_NOTE, LINE_NOTE, BANK_NOTE] {
            assert!(!out.contains(secret), "{format} has {secret:?}");
        }
    }
}

#[test]
fn split_lines_carry_their_parent() {
    let (scratch, ids) = Scratch::with_ledger();
    let json = scratch.json(&["--account", "Card"]);
    assert_eq!(json["schema_version"], 1);
    let rows = json["rows"].as_array().unwrap();
    let shop = ids.shop.get();
    let lines: Vec<_> = rows
        .iter()
        .filter(|row| row["transaction_id"] == shop)
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["line_no"], 1);
    assert_eq!(lines[1]["line_no"], 2);
    assert!(lines.iter().all(|line| line["split_id"].is_i64()));
    assert!(
        lines
            .iter()
            .all(|line| line["transaction_amount"] == "-50.00")
    );
    assert_eq!(lines[0]["amount"], "-30.00");
    assert_eq!(lines[0]["amount_minor"], -3000);
    assert_eq!(lines[0]["category_key"], "food.groceries");
}

#[test]
fn filters() {
    let (scratch, ids) = Scratch::with_ledger();
    let (lunch, shop) = (ids.lunch.get(), ids.shop.get());
    let card_rows = [(lunch, None), (shop, Some(1)), (shop, Some(2))];
    assert_eq!(scratch.ids(&["--account", "Card"]), card_rows);
    assert_eq!(scratch.ids(&["--account", "2"]), card_rows);
    assert_eq!(scratch.ids(&["--from", "2026-11-01"]), card_rows);
    assert_eq!(
        scratch.ids(&["--from", "2026-11-03", "--to", "2026-11-03"]),
        [(shop, Some(1)), (shop, Some(2))]
    );
    // The Monzo rows and their pots' mirrored rows.
    assert_eq!(scratch.ids(&["--to", "2026-10-31"]).len(), 18);
    // A pot is its own account.
    assert_eq!(
        scratch
            .ids(&["--account", "Monzo", "--account", "Card"])
            .len(),
        17
    );
    // A group includes its subcategories.
    assert_eq!(
        scratch.ids(&["--account", "Card", "--category", "food"]),
        [(lunch, None), (shop, Some(1))]
    );
    assert_eq!(scratch.ids(&["--category", "home"]), [(shop, Some(2))]);
    assert_eq!(scratch.ids(&["--tag", "trip"]), [(lunch, None)]);
    assert_eq!(
        scratch.ids(&["--tag", PRIVATE_TAG, "--include-private-tags"]),
        [(lunch, None)]
    );

    let error = scratch.err(&["export", "--format", "csv", "--tag", PRIVATE_TAG]);
    assert!(error.contains("--include-private-tags"), "{error}");
    assert!(!error.contains(PRIVATE_TAG), "{error}");
    let error = scratch.err(&["export", "--format", "csv", "--tag", "Nope"]);
    assert!(error.contains("no tag"), "{error}");
    let error = scratch.err(&["export", "--format", "csv", "--category", "nope"]);
    assert!(error.contains("no category nope"), "{error}");
    let error = scratch.err(&["export", "--format", "csv", "--account", "Nope"]);
    assert!(error.contains("there is no account"), "{error}");
    let error = scratch.err(&[
        "export",
        "--format",
        "csv",
        "--from",
        "2026-11-05",
        "--to",
        "2026-11-04",
    ]);
    assert!(
        error.contains("--from 2026-11-05 is after --to 2026-11-04"),
        "{error}"
    );
}

#[test]
fn output_files_are_private_and_never_silently_replaced() {
    let (scratch, _) = Scratch::with_ledger();
    let path = scratch.path("ledger.json");
    let file = path.to_str().unwrap();
    let out = scratch.ok(&["export", "--format", "json", "--output", file]);
    assert_eq!(out, format!("Exported 21 rows to {file}.\n"));
    let first = std::fs::read_to_string(&path).unwrap();
    assert!(serde_json::from_str::<serde_json::Value>(&first).is_ok());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    let error = scratch.err(&["export", "--format", "csv", "--output", file]);
    assert!(error.contains("already exists; use --force"), "{error}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), first);

    let out = scratch.ok(&[
        "export",
        "--format",
        "csv",
        "--output",
        file,
        "--force",
        "--account",
        "Card",
    ]);
    assert_eq!(out, format!("Exported 3 rows to {file}.\n"));
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .starts_with("transaction_id,")
    );
}

#[test]
fn an_empty_ledger_exports_just_the_header() {
    let scratch = Scratch::new();
    scratch.ok(&[
        "accounts",
        "add",
        "--name",
        "Cash",
        "--kind",
        "cash",
        "--currency",
        "GBP",
    ]);
    let out = scratch.ok(&["export", "--format", "csv"]);
    assert_eq!(out.lines().count(), 1);
}
