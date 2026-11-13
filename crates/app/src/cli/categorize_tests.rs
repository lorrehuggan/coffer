//! `coffer categorize` against scratch data dirs with synthetic rows.
//! Nothing here touches the real data dir.

use std::path::PathBuf;

use clap::Parser;
use coffer_core::account::{self, AccountKind, NewAccount};
use coffer_core::category::CategoryKey;
use coffer_core::change;
use coffer_core::db::Database;
use coffer_core::id::{AccountId, TransactionId};
use coffer_core::inbox::own::RULE_SUGGESTION_THRESHOLD;
use coffer_core::inbox::{self, COFFER_SOURCE, Filter, SuggestionKind, SuggestionStatus};
use coffer_core::manual::{self, ManualTransaction};
use coffer_core::money::{Currency, Money};
use coffer_core::split::{self, SplitLine};
use tempfile::TempDir;

use super::*;

struct Scratch {
    _dir: TempDir,
    path: PathBuf,
    account: AccountId,
}

impl Scratch {
    fn new() -> Scratch {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("coffer.db");
        let mut db = Database::open(&path).unwrap();
        let account = account::create(
            &mut db,
            &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        Scratch {
            _dir: dir,
            path,
            account,
        }
    }

    fn db(&self) -> Database {
        Database::open(&self.path).unwrap()
    }

    fn run(&self, args: &[&str]) -> Result<String, String> {
        let cli = Cli::try_parse_from(std::iter::once("coffer").chain(args.iter().copied()))
            .unwrap_or_else(|error| panic!("{args:?} doesn't parse: {error}"));
        let mut out = Vec::new();
        let result = super::run(cli.command.expect("a subcommand"), &self.path, &mut out);
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

    /// A manual row of `minor` pence at `merchant` on day `d`, uncategorised.
    fn row(&self, d: u32, minor: i64, merchant: &str) -> TransactionId {
        let mut entry = ManualTransaction::new(
            self.account,
            chrono::NaiveDate::from_ymd_opt(2026, 11, d).unwrap(),
            Money::new(minor, Currency::GBP),
            merchant,
        );
        entry.merchant = Some(merchant.to_owned());
        manual::create(&mut self.db(), &entry).unwrap()
    }

    fn category(&self, row: TransactionId) -> Option<String> {
        manual::get(&self.db(), row)
            .unwrap()
            .category
            .map(|key| key.as_str().to_owned())
    }

    fn batches(&self) -> usize {
        change::list(&self.db()).unwrap().len()
    }

    /// Coffer's own `rule` suggestions, whatever their status.
    fn coffer_rules(&self) -> Vec<inbox::Suggestion> {
        inbox::list(
            &self.db(),
            &Filter {
                status: None,
                kind: Some(SuggestionKind::Rule),
            },
        )
        .unwrap()
        .into_iter()
        .filter(|s| s.source_client == COFFER_SOURCE)
        .collect()
    }
}

#[test]
fn parses_categorize() {
    let cli = Cli::try_parse_from(["coffer", "categorize", "12", "13", "food.groceries"]).unwrap();
    let Some(Command::Categorize(args)) = cli.command else {
        panic!("not categorize");
    };
    assert_eq!(args.transactions, [12, 13]);
    assert_eq!(args.category, "food.groceries");
    let cli = Cli::try_parse_from(["coffer", "categorize", "7", "food.groceries"]).unwrap();
    let Some(Command::Categorize(args)) = cli.command else {
        panic!("not categorize");
    };
    assert_eq!(args.transactions, [7]);

    for bad in [
        &["coffer", "categorize"][..],
        &["coffer", "categorize", "12"],
        &["coffer", "categorize", "food.groceries"],
        &["coffer", "categorize", "twelve", "food.groceries"],
    ] {
        assert!(Cli::try_parse_from(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn sets_the_category_as_one_change_batch() {
    let scratch = Scratch::new();
    let tesco = scratch.row(1, -2000, "Tesco");
    let aldi = scratch.row(2, -1500, "Aldi");

    let out = scratch.ok(&[
        "categorize",
        &tesco.to_string(),
        &aldi.to_string(),
        "food.groceries",
    ]);
    let batch = change::list(&scratch.db()).unwrap()[0].id;
    assert_eq!(
        out,
        format!(
            "Set the category of 2 transactions to food.groceries (change batch {batch}):\n  \
             transaction {tesco}: none -> food.groceries\n  \
             transaction {aldi}: none -> food.groceries\n"
        )
    );
    assert_eq!(scratch.category(tesco).as_deref(), Some("food.groceries"));
    assert_eq!(scratch.category(aldi).as_deref(), Some("food.groceries"));
    // No transaction text in the output.
    assert!(!out.contains("Tesco") && !out.contains("Aldi"), "{out}");

    // The batch undoes like any other.
    change::undo(&mut scratch.db(), batch).unwrap();
    assert_eq!(scratch.category(tesco), None);

    // Again, with one row already in the category.
    let out = scratch.ok(&["categorize", &tesco.to_string(), "food.eating_out"]);
    assert!(out.contains(&format!("transaction {tesco}: none -> food.eating_out")));
    let out = scratch.ok(&[
        "categorize",
        &tesco.to_string(),
        &aldi.to_string(),
        "food.eating_out",
    ]);
    assert!(
        out.contains(&format!("transaction {aldi}: none -> food.eating_out\n")),
        "{out}"
    );
    assert!(
        out.ends_with("1 other transaction already had it, set by you.\n"),
        "{out}"
    );
    let batches = scratch.batches();
    let out = scratch.ok(&["categorize", &tesco.to_string(), "food.eating_out"]);
    assert_eq!(
        out,
        "Nothing changed: the transaction already had category food.eating_out, set by you.\n"
    );
    assert_eq!(scratch.batches(), batches);
}

#[test]
fn refuses_unknown_ids_and_keys_and_writes_nothing() {
    let scratch = Scratch::new();
    let tesco = scratch.row(1, -2000, "Tesco");
    let split_row = scratch.row(2, -1000, "Market");
    split::set(
        &mut scratch.db(),
        split_row,
        &[
            SplitLine::new(Money::new(-600, Currency::GBP), None),
            SplitLine::new(Money::new(-400, Currency::GBP), None),
        ],
    )
    .unwrap();
    let tesco_id = tesco.to_string();
    let split_id = split_row.to_string();

    for (args, message) in [
        (
            vec!["categorize", &tesco_id, "9999", "food.groceries"],
            "there is no transaction 9999; nothing was changed".to_owned(),
        ),
        (
            vec!["categorize", &tesco_id, "food.nonsense"],
            "there is no category food.nonsense; nothing was changed".to_owned(),
        ),
        (
            vec!["categorize", &tesco_id, &tesco_id, "food.groceries"],
            format!("transaction {tesco} is named twice; nothing was changed"),
        ),
    ] {
        assert_eq!(scratch.err(&args), message, "{args:?}");
    }
    let bad_key = scratch.err(&["categorize", &tesco_id, "Food Groceries"]);
    assert!(
        bad_key.starts_with("\"Food Groceries\" isn't a category key")
            && bad_key.ends_with("; nothing was changed"),
        "{bad_key}"
    );
    let split = scratch.err(&["categorize", &tesco_id, &split_id, "food.groceries"]);
    assert!(split.ends_with("; nothing was changed"), "{split}");
    assert!(split.contains(&split_id), "{split}");

    assert_eq!(scratch.category(tesco), None);
    assert_eq!(scratch.batches(), 0);

    // Without a database, it creates none.
    let empty = tempfile::tempdir().unwrap();
    let path = empty.path().join("coffer.db");
    let cli = Cli::try_parse_from(["coffer", "categorize", "1", "food.groceries"]).unwrap();
    let error = super::run(cli.command.unwrap(), &path, &mut Vec::new()).unwrap_err();
    assert!(error.to_string().contains("there is no Coffer database"));
    assert!(!path.exists());
}

#[test]
fn the_third_identical_edit_files_coffers_rule_suggestion() {
    assert_eq!(RULE_SUGGESTION_THRESHOLD, 3);
    let scratch = Scratch::new();
    let rows: Vec<TransactionId> = (1..=3)
        .map(|d| scratch.row(d, -350, "Bean There"))
        .collect();

    for row in &rows[..2] {
        let out = scratch.ok(&["categorize", &row.to_string(), "food.eating_out"]);
        assert!(!out.contains("Coffer suggested"), "{out}");
        assert!(scratch.coffer_rules().is_empty());
    }

    let out = scratch.ok(&["categorize", &rows[2].to_string(), "food.eating_out"]);
    let filed = scratch.coffer_rules();
    assert_eq!(filed.len(), 1);
    let suggestion = &filed[0];
    assert_eq!(suggestion.status, SuggestionStatus::Pending);
    assert_eq!(suggestion.kind, SuggestionKind::Rule);
    assert!(
        out.ends_with(&format!(
            "Coffer suggested a rule for this merchant (suggestion {id}): \
             `coffer inbox show {id}` shows it.\n",
            id = suggestion.id
        )),
        "{out}"
    );
    assert!(!out.contains("Bean"), "{out}");
    let inbox::Payload::Rule(rule) = &suggestion.payload else {
        panic!("not a rule: {:?}", suggestion.payload);
    };
    assert_eq!(rule.pattern().as_str(), "Bean There");
    assert_eq!(
        rule.actions(),
        [coffer_core::rule::RuleAction::SetCategory(
            CategoryKey::new("food.eating_out").unwrap()
        )]
    );

    // It shows in the inbox, from Coffer.
    let list = scratch.ok(&["inbox"]);
    assert!(list.contains("Coffer"), "{list}");
}
