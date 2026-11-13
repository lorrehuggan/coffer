//! The CLI against scratch data dirs and the synthetic fixtures. Nothing
//! here opens a window or touches the real data dir.

use std::path::{Path, PathBuf};

use clap::Parser;
use coffer_core::account::{self, AccountKind};
use coffer_core::db::Database;
use coffer_core::id::{AccountId, BatchId};
use coffer_core::import::{self, BatchStatus};
use coffer_core::money::Currency;
use tempfile::TempDir;

use super::*;

fn fixture(path: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(path)
        .to_string_lossy()
        .into_owned()
}

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

    /// Runs `coffer <args>` and returns its output, or the error message
    /// (as `main` prints it, without the `error: `).
    fn run(&self, args: &[&str]) -> Result<String, String> {
        let cli = Cli::try_parse_from(std::iter::once("coffer").chain(args.iter().copied()))
            .unwrap_or_else(|error| panic!("{args:?} doesn't parse: {error}"));
        let command = cli.command.expect("a subcommand");
        let mut out = Vec::new();
        let result = super::run(command, &self.db, &mut out);
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

    fn open(&self) -> Database {
        Database::open(&self.db).unwrap()
    }

    fn batches(&self, account: i64) -> Vec<import::BatchInfo> {
        import::batches(&self.open(), AccountId::new(account)).unwrap()
    }

    /// A GBP current account called Monzo (1), and an EUR one (2).
    fn with_accounts() -> Scratch {
        let scratch = Scratch::new();
        scratch.ok(&[
            "accounts",
            "add",
            "--name",
            "Monzo",
            "--kind",
            "current",
            "--currency",
            "GBP",
        ]);
        scratch.ok(&[
            "accounts",
            "add",
            "--name",
            "Euro",
            "--kind",
            "current",
            "--currency",
            "EUR",
        ]);
        scratch
    }
}

// Parsing ----------------------------------------------------------------------

#[test]
fn no_subcommand_opens_the_gui() {
    let cli = Cli::try_parse_from(["coffer"]).unwrap();
    assert!(cli.command.is_none());
    assert!(!cli.showcase);

    let cli = Cli::try_parse_from(["coffer", "--showcase"]).unwrap();
    assert!(cli.command.is_none());
    assert!(cli.showcase);
}

#[test]
fn showcase_doesnt_mix_with_a_subcommand() {
    assert!(Cli::try_parse_from(["coffer", "--showcase", "accounts"]).is_err());
}

#[test]
fn parses_import() {
    let cli = Cli::try_parse_from([
        "coffer",
        "import",
        "statement.csv",
        "--account",
        "Monzo",
        "--preset",
        "uk-monzo",
        "--dry-run",
        "--yes",
    ])
    .unwrap();
    let Some(Command::Import(args)) = cli.command else {
        panic!("not an import: {:?}", cli.command);
    };
    assert_eq!(args.file, PathBuf::from("statement.csv"));
    assert_eq!(args.account.as_deref(), Some("Monzo"));
    assert_eq!(args.preset.as_deref(), Some("uk-monzo"));
    assert_eq!(args.statement, None);
    assert!(args.dry_run && args.yes);

    let cli = Cli::try_parse_from(["coffer", "import", "a.ofx", "--statement", "2"]).unwrap();
    let Some(Command::Import(args)) = cli.command else {
        panic!("not an import");
    };
    assert_eq!(args.statement.map(|n| n.get()), Some(2));
    assert!(!args.dry_run && !args.yes);

    assert!(
        Cli::try_parse_from(["coffer", "import"]).is_err(),
        "needs a file"
    );
    assert!(
        Cli::try_parse_from(["coffer", "import", "a.ofx", "--statement", "0"]).is_err(),
        "statements count from 1"
    );
}

#[test]
fn parses_accounts() {
    let cli = Cli::try_parse_from(["coffer", "accounts"]).unwrap();
    assert!(matches!(
        cli.command,
        Some(Command::Accounts(AccountsArgs { command: None }))
    ));
    let cli = Cli::try_parse_from(["coffer", "accounts", "list"]).unwrap();
    assert!(matches!(
        cli.command,
        Some(Command::Accounts(AccountsArgs {
            command: Some(AccountsCommand::List)
        }))
    ));

    let cli = Cli::try_parse_from([
        "coffer",
        "accounts",
        "add",
        "--name",
        "Pot",
        "--kind",
        "pot",
        "--currency",
        "gbp",
        "--institution",
        "Monzo",
        "--parent",
        "1",
    ])
    .unwrap();
    let Some(Command::Accounts(AccountsArgs {
        command: Some(AccountsCommand::Add(args)),
    })) = cli.command
    else {
        panic!("not accounts add");
    };
    assert_eq!(args.name, "Pot");
    assert_eq!(args.kind, AccountKind::Pot);
    assert_eq!(args.currency, Currency::GBP, "the code in any case");
    assert_eq!(args.institution.as_deref(), Some("Monzo"));
    assert_eq!(args.parent.as_deref(), Some("1"));

    for kind in AccountKind::ALL {
        let parsed = Cli::try_parse_from([
            "coffer",
            "accounts",
            "add",
            "--name",
            "A",
            "--kind",
            kind.as_str(),
            "--currency",
            "EUR",
        ]);
        assert!(parsed.is_ok(), "{}", kind.as_str());
    }
    let base = ["coffer", "accounts", "add", "--name", "A"];
    assert!(
        Cli::try_parse_from([&base[..], &["--kind", "chequing", "--currency", "GBP"]].concat())
            .is_err()
    );
    assert!(
        Cli::try_parse_from([&base[..], &["--kind", "cash", "--currency", "XYZ"]].concat())
            .is_err()
    );
    assert!(Cli::try_parse_from([&base[..], &["--kind", "cash"]].concat()).is_err());
}

#[test]
fn parses_undo_import() {
    let cli = Cli::try_parse_from(["coffer", "undo-import", "12"]).unwrap();
    let Some(Command::UndoImport(args)) = cli.command else {
        panic!("not undo-import");
    };
    assert_eq!((args.batch, args.yes), (12, false));

    let cli = Cli::try_parse_from(["coffer", "undo-import", "12", "--yes"]).unwrap();
    let Some(Command::UndoImport(args)) = cli.command else {
        panic!("not undo-import");
    };
    assert!(args.yes);
    assert!(Cli::try_parse_from(["coffer", "undo-import", "twelve"]).is_err());
}

#[test]
fn later_commands_arent_there_yet() {
    assert!(Cli::try_parse_from(["coffer", "inbox"]).is_err());
}

#[test]
fn parses_mcp() {
    let cli = Cli::try_parse_from(["coffer", "mcp", "--client", "claude-code"]).unwrap();
    let Some(Command::Mcp(args)) = cli.command else {
        panic!("not mcp");
    };
    assert_eq!(args.client.as_str(), "claude-code");

    for bad in [
        &["coffer", "mcp"][..],
        &["coffer", "mcp", "--client", ""],
        &["coffer", "mcp", "--client", "claude code"],
        &["coffer", "mcp", "--client", "coffer"],
    ] {
        assert!(Cli::try_parse_from(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn mcp_needs_an_existing_database_and_creates_none() {
    let scratch = Scratch::new();
    let error = scratch.err(&["mcp", "--client", "claude-code"]);
    assert!(error.contains("couldn't start the MCP server"), "{error}");
    assert!(error.contains("there is no Coffer database"), "{error}");
    assert!(!scratch.db.exists(), "the MCP server creates nothing");
}

// Accounts ---------------------------------------------------------------------

#[test]
fn read_only_commands_need_an_existing_database() {
    let scratch = Scratch::new();
    let error = scratch.err(&["accounts"]);
    assert!(error.contains("there is no Coffer database"), "{error}");
    assert!(error.contains("coffer accounts add"), "{error}");
    assert!(
        scratch
            .err(&["undo-import", "1"])
            .contains("no Coffer database")
    );
    assert!(!scratch.db.exists(), "a read-only command creates nothing");
}

#[test]
fn accounts_add_creates_the_database_and_list_shows_them() {
    let scratch = Scratch::new();
    let out = scratch.ok(&[
        "accounts",
        "add",
        "--name",
        "  Monzo  ",
        "--kind",
        "current",
        "--currency",
        "gbp",
        "--institution",
        "Monzo",
    ]);
    assert_eq!(out, "Added account 1: Monzo (current, GBP)\n");
    let out = scratch.ok(&[
        "accounts",
        "add",
        "--name",
        "Rainy Day",
        "--kind",
        "pot",
        "--currency",
        "GBP",
        "--parent",
        "Monzo",
    ]);
    assert_eq!(out, "Added account 2: Rainy Day (pot, GBP)\n");

    let accounts = account::list(&scratch.open()).unwrap();
    assert_eq!(accounts.len(), 2);
    assert_eq!(accounts[0].name, "Monzo");
    assert_eq!(accounts[0].institution.as_deref(), Some("Monzo"));
    assert_eq!(accounts[1].parent, Some(AccountId::new(1)));

    assert_eq!(
        scratch.ok(&["accounts"]),
        "ID  NAME       KIND     CURRENCY  INSTITUTION  PARENT  LOCKED THROUGH\n\
         1   Monzo      current  GBP       Monzo        -       -\n\
         2   Rainy Day  pot      GBP       -            1       -\n"
    );
    assert_eq!(scratch.ok(&["accounts", "list"]), scratch.ok(&["accounts"]));
}

#[test]
fn accounts_list_says_when_there_are_none() {
    let scratch = Scratch::new();
    Database::open(&scratch.db).unwrap();
    assert!(scratch.ok(&["accounts"]).starts_with("No accounts yet."));
}

#[test]
fn accounts_add_checks_pots_and_parents() {
    let scratch = Scratch::with_accounts();
    let add = |extra: &[&str]| {
        let mut args = vec!["accounts", "add", "--name", "Pot", "--currency", "GBP"];
        args.extend_from_slice(extra);
        scratch.err(&args)
    };
    assert!(add(&["--kind", "pot"]).contains("--parent"));
    assert!(add(&["--kind", "savings", "--parent", "1"]).contains("only a pot"));
    assert!(add(&["--kind", "pot", "--parent", "9"]).contains("there is no account \"9\""));
    assert!(add(&["--kind", "pot", "--parent", "Euro"]).contains("in EUR"));
    assert!(
        scratch
            .err(&[
                "accounts",
                "add",
                "--name",
                " ",
                "--kind",
                "cash",
                "--currency",
                "GBP"
            ])
            .contains("blank")
    );
    assert_eq!(account::list(&scratch.open()).unwrap().len(), 2);
}

// Import -----------------------------------------------------------------------

#[test]
fn dry_run_writes_nothing() {
    let scratch = Scratch::with_accounts();
    let monzo = fixture("presets/uk/monzo.csv");
    let out = scratch.ok(&["import", &monzo, "--account", "Monzo", "--dry-run"]);
    assert!(out.contains("File:      monzo.csv\n"), "{out}");
    assert!(
        out.contains("Format:    CSV, preset uk-monzo (Monzo)\n"),
        "{out}"
    );
    assert!(out.contains("Account:   1 Monzo (current, GBP)\n"), "{out}");
    assert!(
        out.contains("Rows:      14 new, 0 already imported, 0 flagged\n"),
        "{out}"
    );
    assert!(out.contains("Reconciliation: nothing to check"), "{out}");
    assert!(out.ends_with("Dry run: nothing was written.\n"), "{out}");
    assert!(scratch.batches(1).is_empty());

    // The dry run changed nothing: the real import finds the same rows new.
    let out = scratch.ok(&["import", &monzo, "--account", "Monzo"]);
    assert!(out.contains("14 new"), "{out}");
}

#[test]
fn import_commits_and_again_finds_nothing_new() {
    let scratch = Scratch::with_accounts();
    let monzo = fixture("presets/uk/monzo.csv");
    // The only GBP account takes it without --account.
    let out = scratch.ok(&["import", &monzo]);
    assert!(
        out.contains("Imported monzo.csv as batch 1: 14 transactions, 0 balances, 0 flags.\n"),
        "{out}"
    );
    // Ten of the fixture's Monzo categories are in the preset's map.
    assert!(
        out.contains("Categorised: 12 by the preset, 0 by rules. Transfer pairs suggested: 0.\n"),
        "{out}"
    );
    assert!(out.ends_with("To undo it: coffer undo-import 1\n"), "{out}");
    let batches = scratch.batches(1);
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].preset_id.as_deref(), Some("uk-monzo"));
    // The 14 imported rows and the 4 pot transfers mirrored onto pots.
    assert_eq!(batches[0].transactions, 18);

    let out = scratch.ok(&["import", &monzo, "--account", "1"]);
    assert!(
        out.contains("0 new, 14 already imported, 0 flagged"),
        "{out}"
    );
    assert!(
        out.ends_with(
            "Nothing new to import: the account already has every row and balance in this file.\n"
        ),
        "{out}"
    );
    assert_eq!(scratch.batches(1).len(), 1, "no new batch");
}

#[test]
fn flagged_rows_need_yes() {
    let scratch = Scratch::with_accounts();
    let edge = fixture("ofx/edge-cases.ofx");

    let error = scratch.err(&["import", &edge]);
    assert!(
        error.starts_with("not imported: the import has flagged rows"),
        "{error}"
    );
    assert!(error.contains("--yes"), "{error}");
    assert!(scratch.batches(2).is_empty());

    // A dry run says what --yes would accept, and succeeds.
    let out = scratch.ok(&["import", &edge, "--dry-run"]);
    assert!(out.contains("Account:   2 Euro (current, EUR)"), "{out}");
    assert!(
        out.contains("4 new, 0 already imported, 9 flagged"),
        "{out}"
    );
    assert!(out.contains("Flagged rows (not imported; recorded with the import):\n  entry 2: the date is missing\n"), "{out}");
    assert!(
        out.ends_with("Importing it needs --yes to accept flagged rows.\n"),
        "{out}"
    );

    let out = scratch.ok(&["import", &edge, "--yes"]);
    assert!(
        out.contains("as batch 1: 4 transactions, 0 balances, 9 flags."),
        "{out}"
    );
    assert_eq!(
        import::batch_flags(&scratch.open(), BatchId::new(1))
            .unwrap()
            .len(),
        9
    );
}

#[test]
fn a_balance_mismatch_needs_yes() {
    let scratch = Scratch::with_accounts();
    let multi = fixture("ofx/multi-statement.ofx");
    scratch.ok(&["import", &multi, "--account", "1", "--statement", "1"]);

    // The savings statement's balance, for the same day, into the same account.
    let second = ["import", &multi, "--account", "1", "--statement", "2"];
    let out = scratch.ok(&[&second[..], &["--dry-run"]].concat());
    assert!(out.contains("Reconciliation: MISMATCH"), "{out}");
    assert!(
        out.contains("  2026-11-30  5250.00 GBP (from this file): FAILS: "),
        "{out}"
    );
    assert!(
        out.ends_with("Importing it needs --yes to accept a balance mismatch.\n"),
        "{out}"
    );

    let error = scratch.err(&second);
    assert!(
        error.starts_with("not imported: the import has a balance mismatch"),
        "{error}"
    );
    assert_eq!(scratch.batches(1).len(), 1);

    let out = scratch.ok(&[&second[..], &["--yes"]].concat());
    assert!(
        out.contains("as batch 2: 1 transaction, 1 balance, 0 flags."),
        "{out}"
    );
}

#[test]
fn flags_show_positions_and_reasons_only() {
    let scratch = Scratch::with_accounts();
    let out = scratch.ok(&["import", &fixture("ofx/edge-cases.ofx"), "--dry-run"]);
    // Descriptions from the fixture's rows never appear.
    for text in ["Bäckerei", "First of a pair", "Same currency"] {
        assert!(!out.contains(text), "{text} in:\n{out}");
    }
}

#[test]
fn several_ofx_statements_need_choosing() {
    let scratch = Scratch::with_accounts();
    let multi = fixture("ofx/multi-statement.ofx");
    let error = scratch.err(&["import", &multi, "--account", "Monzo"]);
    assert_eq!(
        error,
        "multi-statement.ofx holds 3 statements, one per account; choose one with --statement:\n  \
         1  bank account, ending 1111, GBP, 2 transactions\n  \
         2  bank account, ending 2222, GBP, 1 transaction\n  \
         3  credit card, ending 3333, GBP, 1 transaction"
    );
    assert!(
        scratch
            .err(&["import", &multi, "--account", "Monzo", "--statement", "4"])
            .contains("there is no statement 4")
    );

    let out = scratch.ok(&["import", &multi, "--account", "Monzo", "--statement", "1"]);
    assert!(out.contains("Format:    OFX, statement 1 of 3\n"), "{out}");
    assert!(
        out.contains("  2026-11-30  100.00 GBP  stated by the file, new\n"),
        "{out}"
    );
    assert!(out.contains("Reconciliation: starting point"), "{out}");
    assert!(
        out.contains("as batch 1: 2 transactions, 1 balance, 0 flags."),
        "{out}"
    );
}

#[test]
fn statement_is_only_for_ofx() {
    let scratch = Scratch::with_accounts();
    let error = scratch.err(&[
        "import",
        &fixture("presets/uk/monzo.csv"),
        "--statement",
        "1",
    ]);
    assert!(error.contains("--statement is for OFX files"), "{error}");
}

#[test]
fn import_errors_name_the_choices() {
    let scratch = Scratch::with_accounts();
    let monzo = fixture("presets/uk/monzo.csv");

    let error = scratch.err(&["import", &monzo, "--account", "Barclays"]);
    assert_eq!(
        error,
        "there is no account \"Barclays\"; the accounts are:\n  \
         1 Monzo (current, GBP)\n  2 Euro (current, EUR)"
    );
    assert!(
        scratch
            .err(&["import", &monzo, "--account", "7"])
            .contains("there is no account \"7\"")
    );

    let error = scratch.err(&["import", &monzo, "--preset", "uk-nope"]);
    assert_eq!(
        error,
        "there is no preset \"uk-nope\"; the bundled presets are:\n  uk-monzo  Monzo"
    );

    // A CSV no preset recognises isn't guessed at.
    let error = scratch.err(&["import", &fixture("csv/signed-amount.csv")]);
    assert!(
        error.starts_with("couldn't tell what kind of file signed-amount.csv is"),
        "{error}"
    );
    assert!(error.contains("--preset:\n  uk-monzo  Monzo\n"), "{error}");

    // Naming a preset that doesn't fit fails on reading, without the file's text.
    let error = scratch.err(&[
        "import",
        &fixture("csv/signed-amount.csv"),
        "--preset",
        "uk-monzo",
    ]);
    assert!(
        error.starts_with("couldn't read signed-amount.csv with the uk-monzo preset: "),
        "{error}"
    );

    let error = scratch.err(&["import", &fixture("csv/no-such-file.csv")]);
    assert!(error.starts_with("couldn't read "), "{error}");

    assert!(scratch.batches(1).is_empty() && scratch.batches(2).is_empty());
}

#[test]
fn import_needs_an_account_that_fits() {
    let scratch = Scratch::new();
    let monzo = fixture("presets/uk/monzo.csv");
    let error = scratch.err(&["import", &monzo]);
    assert!(error.starts_with("there are no accounts yet"), "{error}");
    assert!(error.contains("--currency GBP"), "{error}");

    scratch.ok(&[
        "accounts",
        "add",
        "--name",
        "Euro",
        "--kind",
        "current",
        "--currency",
        "EUR",
    ]);
    assert!(
        scratch
            .err(&["import", &monzo])
            .starts_with("no open account in GBP")
    );

    // Naming an account in another currency is core's error.
    let error = scratch.err(&["import", &monzo, "--account", "Euro"]);
    assert!(
        error.contains("the file is in GBP, but the account is in EUR"),
        "{error}"
    );

    scratch.ok(&[
        "accounts",
        "add",
        "--name",
        "A",
        "--kind",
        "current",
        "--currency",
        "GBP",
    ]);
    scratch.ok(&[
        "accounts",
        "add",
        "--name",
        "A",
        "--kind",
        "savings",
        "--currency",
        "GBP",
    ]);
    let error = scratch.err(&["import", &monzo]);
    assert!(
        error.starts_with("choose the account to import into with --account"),
        "{error}"
    );
    let error = scratch.err(&["import", &monzo, "--account", "A"]);
    assert_eq!(
        error,
        "\"A\" could mean more than one account; use its ID:\n  \
         2 A (current, GBP)\n  3 A (savings, GBP)"
    );
    scratch.ok(&["import", &monzo, "--account", "3"]);
}

#[test]
fn pot_transfers_show_in_the_preview_and_the_summary() {
    let scratch = Scratch::with_accounts();
    let monzo = fixture("presets/uk/monzo.csv");
    let out = scratch.ok(&["import", &monzo, "--dry-run"]);
    assert!(
        out.contains(
            "Pots:      4 transfers mirrored onto 2 pots as confirmed transfers\n  \
             creates pot \"Rainy Day\"\n  creates pot \"Holiday Fund\"\n"
        ),
        "{out}"
    );

    let out = scratch.ok(&["import", &monzo]);
    assert!(
        out.contains(
            "Pots: 4 transfers mirrored onto pots as confirmed transfers, 2 pots created.\n"
        ),
        "{out}"
    );
    assert_eq!(account::list(&scratch.open()).unwrap().len(), 4);

    // The pots the import created don't make the next import ambiguous:
    // the Monzo account is still the only one that takes the file.
    let out = scratch.ok(&["import", &monzo]);
    assert!(out.contains("Account:   1 Monzo (current, GBP)\n"), "{out}");
    assert!(out.contains("0 new, 14 already imported"), "{out}");
    assert!(!out.contains("Pots:"), "{out}");
}

// Undo -------------------------------------------------------------------------

#[test]
fn undo_previews_then_undoes_with_yes() {
    let scratch = Scratch::with_accounts();
    let monzo = fixture("presets/uk/monzo.csv");
    scratch.ok(&["import", &monzo]);

    let out = scratch.ok(&["undo-import", "1"]);
    assert!(
        out.starts_with("Import 1: monzo.csv into 1 Monzo (current, GBP), imported "),
        "{out}"
    );
    assert!(
        out.contains(
            "Undoing it removes:\n  14 transactions\n  4 transactions it created in pots\n  \
             0 balances\n  0 flags recorded with it\n"
        ),
        "{out}"
    );
    assert!(
        out.ends_with("Nothing was undone. Run the command again with --yes to undo it.\n"),
        "{out}"
    );
    assert_eq!(scratch.batches(1)[0].status, BatchStatus::Committed);

    let out = scratch.ok(&["undo-import", "1", "--yes"]);
    assert!(
        out.ends_with("Undid import 1: removed 18 transactions, 0 balances and 0 flags.\n"),
        "{out}"
    );
    assert_eq!(scratch.batches(1)[0].status, BatchStatus::Undone);

    assert_eq!(
        scratch.err(&["undo-import", "1", "--yes"]),
        "import batch 1 is already undone"
    );
    assert_eq!(
        scratch.err(&["undo-import", "9"]),
        "there is no import batch 9"
    );

    // The rows are gone, so importing the file again finds them new.
    assert!(scratch.ok(&["import", &monzo]).contains("14 new"));
}

#[test]
fn undo_names_later_imports_that_relied_on_it() {
    let scratch = Scratch::with_accounts();
    scratch.ok(&["import", &fixture("presets/uk/monzo.csv")]);
    let multi = fixture("ofx/multi-statement.ofx");
    scratch.ok(&["import", &multi, "--account", "1", "--statement", "1"]);
    let out = scratch.ok(&["undo-import", "1"]);
    assert!(
        out.contains("Later imports into this account relied on it: 2."),
        "{out}"
    );
}

#[test]
fn import_reports_transfer_pairs_it_suggests() {
    let scratch = Scratch::with_accounts();
    scratch.ok(&[
        "accounts",
        "add",
        "--name",
        "Savings",
        "--kind",
        "savings",
        "--currency",
        "GBP",
    ]);
    scratch.ok(&[
        "import",
        &fixture("presets/uk/monzo.csv"),
        "--account",
        "Monzo",
    ]);

    // A synthetic Monzo-format export for the savings account: the fixture's
    // 24.00 payment out on 4 October arriving the next day.
    let header = std::fs::read_to_string(fixture("presets/uk/monzo.csv"))
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_owned();
    let file = scratch._dir.path().join("savings.csv");
    std::fs::write(
        &file,
        format!(
            "{header}\ntx_0000FAKE900000000000001,05/10/2026,09:00:00,Faster payment,\
             Example Transfer,,,24.00,GBP,24.00,GBP,,,,FAKE TRANSFER IN,,,24.00\n"
        ),
    )
    .unwrap();
    let out = scratch.ok(&["import", &file.to_string_lossy(), "--account", "Savings"]);
    assert!(out.contains("Transfer pairs suggested: 1.\n"), "{out}");
}
