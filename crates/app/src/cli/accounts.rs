//! `coffer accounts`: list the accounts, and add one to import into.

use std::io::Write;

use anyhow::{bail, ensure};
use clap::builder::{PossibleValuesParser, TypedValueParser};
use clap::{Args, Subcommand};
use coffer_core::account::{self, Account, AccountKind, NewAccount};
use coffer_core::db::Database;
use coffer_core::money::Currency;

use super::write_table;

#[derive(Debug, Args)]
pub struct AccountsArgs {
    /// What to do; lists the accounts when left out.
    #[command(subcommand)]
    pub command: Option<AccountsCommand>,
}

#[derive(Debug, Subcommand)]
pub enum AccountsCommand {
    /// List the accounts (the default).
    List,
    /// Add an account.
    Add(AddAccountArgs),
}

#[derive(Debug, Args)]
pub struct AddAccountArgs {
    /// The account's name, e.g. "Monzo current account".
    #[arg(long)]
    pub name: String,

    /// What kind of account it is. A pot is a Monzo Pot, Starling Space or
    /// similar, under a current or savings account; asset and liability
    /// accounts hold valuations, not imports.
    #[arg(long, value_parser = kind_parser())]
    pub kind: AccountKind,

    /// Its ISO 4217 currency code, e.g. GBP.
    #[arg(long, value_parser = parse_currency)]
    pub currency: Currency,

    /// The bank or provider, e.g. Monzo.
    #[arg(long)]
    pub institution: Option<String>,

    /// For a pot: the current or savings account it belongs to (ID or exact
    /// name).
    #[arg(long, value_name = "ID|NAME")]
    pub parent: Option<String>,
}

fn kind_parser() -> impl TypedValueParser<Value = AccountKind> {
    PossibleValuesParser::new(AccountKind::ALL.map(AccountKind::as_str))
        .map(|key| AccountKind::from_key(&key).expect("the parser only accepts the kinds' keys"))
}

/// Accepts the code in either case: `gbp` is `GBP`.
fn parse_currency(code: &str) -> Result<Currency, String> {
    Currency::new(&code.trim().to_ascii_uppercase())
        .map_err(|_| format!("{code:?} isn't an ISO 4217 currency code (e.g. GBP, EUR, USD)"))
}

/// `coffer accounts`: every account, in the order they were added.
pub fn list(db: &Database, out: &mut dyn Write) -> anyhow::Result<()> {
    let accounts = account::list(db)?;
    if accounts.is_empty() {
        writeln!(
            out,
            "No accounts yet. Add one with \
             `coffer accounts add --name <name> --kind <kind> --currency <code>`."
        )?;
        return Ok(());
    }
    let rows: Vec<Vec<String>> = accounts
        .iter()
        .map(|account| {
            vec![
                account.id.to_string(),
                account.name.clone(),
                account.kind.as_str().to_owned(),
                account.currency.to_string(),
                account.institution.clone().unwrap_or_else(|| "-".into()),
                account
                    .parent
                    .map_or_else(|| "-".into(), |parent| parent.to_string()),
                account
                    .reconciled_through
                    .map_or_else(|| "-".into(), |date| date.to_string()),
            ]
        })
        .collect();
    write_table(
        out,
        &[
            "ID",
            "NAME",
            "KIND",
            "CURRENCY",
            "INSTITUTION",
            "PARENT",
            "LOCKED THROUGH",
        ],
        &rows,
    )?;
    Ok(())
}

/// `coffer accounts add`.
pub fn add(db: &mut Database, args: &AddAccountArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    let name = args.name.trim();
    ensure!(!name.is_empty(), "the account's name can't be blank");
    let institution = args
        .institution
        .as_deref()
        .map(str::trim)
        .filter(|institution| !institution.is_empty());

    // The schema enforces these too; checking first gives a message that
    // says what to change instead of a constraint error.
    let parent = match (&args.parent, args.kind) {
        (None, AccountKind::Pot) => {
            bail!("a pot belongs to a current or savings account; name it with --parent")
        }
        (Some(_), kind) if kind != AccountKind::Pot => {
            bail!("only a pot has a parent account; leave out --parent")
        }
        (None, _) => None,
        (Some(wanted), _) => {
            let accounts = account::list(db)?;
            let parent = find(&accounts, wanted)?;
            ensure!(
                matches!(parent.kind, AccountKind::Current | AccountKind::Savings),
                "a pot belongs to a current or savings account, and {} is a {} account",
                describe(parent),
                parent.kind.as_str()
            );
            ensure!(
                parent.currency == args.currency,
                "a pot is in its account's currency, and {} is in {}",
                describe(parent),
                parent.currency
            );
            Some(parent.id)
        }
    };

    let id = account::create(
        db,
        &NewAccount {
            name: name.to_owned(),
            institution: institution.map(str::to_owned),
            kind: args.kind,
            currency: args.currency,
            parent,
        },
    )?;
    writeln!(
        out,
        "Added account {id}: {name} ({}, {})",
        args.kind.as_str(),
        args.currency
    )?;
    Ok(())
}

/// The account the user means by `wanted`: its ID or its exact name.
pub(super) fn find<'a>(accounts: &'a [Account], wanted: &str) -> anyhow::Result<&'a Account> {
    let by_id = wanted
        .trim()
        .parse::<i64>()
        .ok()
        .and_then(|id| accounts.iter().find(|account| account.id.get() == id));
    let by_name: Vec<&Account> = accounts
        .iter()
        .filter(|account| account.name == wanted)
        .collect();
    match (by_id, by_name.as_slice()) {
        (Some(account), []) | (None, &[account]) => Ok(account),
        (Some(account), [named]) if account.id == named.id => Ok(account),
        (None, []) => bail!(
            "there is no account {wanted:?}; the accounts are:\n{}",
            list_lines(accounts.iter())
        ),
        (by_id, by_name) => bail!(
            "{wanted:?} could mean more than one account; use its ID:\n{}",
            list_lines(by_id.into_iter().chain(by_name.iter().copied()))
        ),
    }
}

/// `3 Monzo (current, GBP)`.
pub(super) fn describe(account: &Account) -> String {
    format!(
        "{} {} ({}, {})",
        account.id,
        account.name,
        account.kind.as_str(),
        account.currency
    )
}

/// One indented [`describe`] line per account, for error messages.
pub(super) fn list_lines<'a>(accounts: impl Iterator<Item = &'a Account>) -> String {
    accounts
        .map(|account| format!("  {}", describe(account)))
        .collect::<Vec<_>>()
        .join("\n")
}
