//! `coffer budgets` against scratch data dirs. Nothing here touches the
//! real data dir.

use std::path::PathBuf;

use clap::Parser;
use coffer_core::budget::{self, BudgetGroup};
use coffer_core::db::Database;
use tempfile::TempDir;

use super::*;

/// A scratch data dir; its database doesn't exist until a command creates
/// it.
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

    /// With one GBP account.
    fn with_account() -> Scratch {
        let scratch = Scratch::new();
        scratch.add_account("Monzo", "GBP");
        scratch
    }

    fn add_account(&self, name: &str, currency: &str) {
        self.ok(&[
            "accounts",
            "add",
            "--name",
            name,
            "--kind",
            "current",
            "--currency",
            currency,
        ]);
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

    /// `(category, month, minor, currency, group, rollover)` of every budget.
    fn budgets(&self) -> Vec<(String, Option<String>, i64, String, BudgetGroup, bool)> {
        let db = Database::open(&self.db).unwrap();
        budget::list(&db)
            .unwrap()
            .into_iter()
            .map(|b| {
                (
                    b.category.to_string(),
                    b.month.map(|m| m.key()),
                    b.amount.minor(),
                    b.amount.currency().to_string(),
                    b.group,
                    b.rollover,
                )
            })
            .collect()
    }
}

#[test]
fn set_list_and_remove() {
    let scratch = Scratch::with_account();
    assert_eq!(
        scratch.ok(&["budgets"]),
        "No budgets yet. Set one with `coffer budgets set <category> <amount>`.\n"
    );

    assert_eq!(
        scratch.ok(&["budgets", "set", "food.groceries", "250"]),
        "Set the standing budget for food.groceries: 250.00 GBP (flex).\n"
    );
    assert_eq!(
        scratch.ok(&[
            "budgets",
            "set",
            "food.groceries",
            "300.5",
            "--month",
            "2026-12",
            "--group",
            "fixed",
        ]),
        "Set the 2026-12 budget for food.groceries: 300.50 GBP (fixed).\n"
    );
    assert_eq!(
        scratch.ok(&[
            "budgets",
            "set",
            "bills.insurance",
            "40",
            "--group",
            "non_monthly",
            "--currency",
            "eur",
        ]),
        "Set the standing budget for bills.insurance: 40.00 EUR (non_monthly, rolls over).\n"
    );
    assert_eq!(
        scratch.ok(&["budgets", "set", "food.groceries", "275.00"]),
        "Changed the standing budget for food.groceries: 275.00 GBP (flex).\n"
    );

    assert_eq!(
        scratch.ok(&["budgets", "list"]),
        "CATEGORY         MONTH     AMOUNT  CURRENCY  GROUP        ROLLOVER\n\
         bills.insurance  standing  40.00   EUR       non_monthly  yes\n\
         food.groceries   standing  275.00  GBP       flex         no\n\
         food.groceries   2026-12   300.50  GBP       fixed        no\n"
    );

    assert_eq!(
        scratch.ok(&["budgets", "remove", "food.groceries", "--month", "2026-12"]),
        "Removed the 2026-12 budget for food.groceries.\n"
    );
    let error = scratch.err(&["budgets", "remove", "food.groceries", "--month", "2026-12"]);
    assert!(error.contains("has no 2026-12 budget"), "{error}");
    assert_eq!(
        scratch.budgets(),
        [
            (
                "bills.insurance".into(),
                None,
                4_000,
                "EUR".into(),
                BudgetGroup::NonMonthly,
                true
            ),
            (
                "food.groceries".into(),
                None,
                27_500,
                "GBP".into(),
                BudgetGroup::Flex,
                false
            ),
        ]
    );
}

#[test]
fn refuses_what_isnt_a_budget() {
    let scratch = Scratch::with_account();
    let refused = [
        (vec!["income.salary", "100"], "spending categories only"),
        (vec!["transfer", "100"], "spending categories only"),
        (vec!["food.nope", "100"], "there is no category food.nope"),
        (vec!["food", "-5"], "can't be negative"),
        (vec!["food", "12.345"], "more decimal places"),
        (vec!["food", "lots"], "isn't an amount"),
    ];
    for (args, message) in refused {
        let mut command = vec!["budgets", "set"];
        command.extend(args);
        let error = scratch.err(&command);
        assert!(error.contains(message), "{command:?}: {error}");
    }
    assert!(scratch.budgets().is_empty());

    // Malformed keys, months and groups are refused while parsing.
    for args in [
        vec!["budgets", "set", "Food", "100"],
        vec!["budgets", "set", "food", "100", "--month", "2026-13"],
        vec!["budgets", "set", "food", "100", "--group", "monthly"],
        vec!["budgets", "set", "food", "100", "--currency", "pounds"],
    ] {
        let parsed = Cli::try_parse_from(std::iter::once("coffer").chain(args.iter().copied()));
        assert!(parsed.is_err(), "{args:?}");
    }
}

#[test]
fn the_currency_comes_from_the_accounts_or_must_be_named() {
    // One currency across every account, valuation accounts included.
    let scratch = Scratch::new();
    scratch.ok(&[
        "accounts",
        "add",
        "--name",
        "House",
        "--kind",
        "asset",
        "--currency",
        "GBP",
    ]);
    scratch.ok(&["budgets", "set", "food", "10"]);
    assert_eq!(scratch.budgets()[0].3, "GBP");

    let scratch = Scratch::with_account();
    scratch.add_account("Revolut", "EUR");
    let error = scratch.err(&["budgets", "set", "food", "10"]);
    assert!(error.contains("the accounts are in EUR, GBP"), "{error}");
    assert!(error.contains("--currency"), "{error}");
    scratch.ok(&["budgets", "set", "food", "10", "--currency", "EUR"]);
    assert_eq!(scratch.budgets()[0].3, "EUR");
}

#[test]
fn needs_a_database_and_accounts_for_the_default_currency() {
    let scratch = Scratch::new();
    let error = scratch.err(&["budgets"]);
    assert!(error.contains("there is no Coffer database"), "{error}");
    assert!(!scratch.db.exists(), "a read left a database behind");

    // A database without accounts: the currency must be named.
    Database::open(&scratch.db).unwrap();
    let error = scratch.err(&["budgets", "set", "food", "10"]);
    assert!(error.contains("no accounts"), "{error}");
    scratch.ok(&["budgets", "set", "food", "10", "--currency", "GBP"]);
}
