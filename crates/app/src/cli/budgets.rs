//! `coffer budgets`: list, set and remove budgets ([`coffer_core::budget`]).
//!
//! A budget is standing (every month) unless `--month` names the one month
//! it is for, where it overrides the standing one. The group defaults to
//! `flex`; rollover follows the group (only `non_monthly` budgets roll
//! over until M4.9). The currency defaults to the one every account is in;
//! with no accounts, or accounts in several currencies, `--currency` is
//! required.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, bail};
use clap::builder::{PossibleValuesParser, TypedValueParser};
use clap::{Args, Subcommand};
use coffer_core::account;
use coffer_core::budget::{self, BudgetGroup, NewBudget, Saved};
use coffer_core::category::CategoryKey;
use coffer_core::db::Database;
use coffer_core::money::{Currency, DecimalMark, Money};
use coffer_core::reporting::Period;

use super::write_table;

#[derive(Debug, Args)]
pub struct BudgetsArgs {
    /// What to do; lists the budgets when left out.
    #[command(subcommand)]
    pub command: Option<BudgetsCommand>,
}

#[derive(Debug, Subcommand)]
pub enum BudgetsCommand {
    /// List the budgets (the default): standing ones first, then by month.
    List,
    /// Set a category's budget, replacing the one it has.
    Set(SetBudgetArgs),
    /// Remove a category's budget.
    Remove(RemoveBudgetArgs),
}

#[derive(Debug, Args)]
pub struct SetBudgetArgs {
    /// The spending category's key, e.g. food.groceries (see the app's
    /// categories, or `list_categories` over MCP).
    #[arg(value_name = "CATEGORY", value_parser = parse_category)]
    pub category: CategoryKey,

    /// What may be spent each month, e.g. 250 or 250.00. Not negative.
    #[arg(value_name = "AMOUNT", allow_hyphen_values = true)]
    pub amount: String,

    /// The one month this budget is for (YYYY-MM); it overrides the
    /// standing budget that month. Without it, the budget is standing.
    #[arg(long, value_name = "YYYY-MM", value_parser = parse_month)]
    pub month: Option<Period>,

    /// fixed: the same every month; flex: day-to-day; non_monthly: spent a
    /// few times a year, set aside monthly (always rolls over).
    #[arg(long, value_parser = group_parser(), default_value = "flex")]
    pub group: BudgetGroup,

    /// Its ISO 4217 currency code. Defaults to the currency every account
    /// is in.
    #[arg(long, value_name = "CODE", value_parser = parse_currency)]
    pub currency: Option<Currency>,
}

#[derive(Debug, Args)]
pub struct RemoveBudgetArgs {
    /// The category's key, e.g. food.groceries.
    #[arg(value_name = "CATEGORY", value_parser = parse_category)]
    pub category: CategoryKey,

    /// Remove that month's budget (YYYY-MM) instead of the standing one.
    #[arg(long, value_name = "YYYY-MM", value_parser = parse_month)]
    pub month: Option<Period>,
}

fn parse_category(key: &str) -> Result<CategoryKey, String> {
    CategoryKey::new(key.trim()).map_err(|error| error.to_string())
}

fn parse_month(text: &str) -> Result<Period, String> {
    text.trim()
        .parse()
        .map_err(|error: coffer_core::reporting::Error| error.to_string())
}

fn group_parser() -> impl TypedValueParser<Value = BudgetGroup> {
    PossibleValuesParser::new(BudgetGroup::ALL.map(BudgetGroup::as_str))
        .map(|key| BudgetGroup::from_key(&key).expect("the parser only accepts the groups' keys"))
}

/// Accepts the code in either case: `gbp` is `GBP`.
fn parse_currency(code: &str) -> Result<Currency, String> {
    Currency::new(&code.trim().to_ascii_uppercase())
        .map_err(|_| format!("{code:?} isn't an ISO 4217 currency code (e.g. GBP, EUR, USD)"))
}

/// `coffer budgets …` on the existing database at `database`.
pub fn run(database: &Path, args: BudgetsArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    let mut db = super::open_existing(database)?;
    match args.command {
        None | Some(BudgetsCommand::List) => list(&db, out),
        Some(BudgetsCommand::Set(args)) => set(&mut db, &args, out),
        Some(BudgetsCommand::Remove(args)) => remove(&mut db, &args, out),
    }
}

/// `coffer budgets`, `coffer budgets list`.
pub fn list(db: &Database, out: &mut dyn Write) -> anyhow::Result<()> {
    let budgets = budget::list(db)?;
    if budgets.is_empty() {
        writeln!(
            out,
            "No budgets yet. Set one with `coffer budgets set <category> <amount>`."
        )?;
        return Ok(());
    }
    let rows: Vec<Vec<String>> = budgets
        .iter()
        .map(|budget| {
            vec![
                budget.category.to_string(),
                month_label(budget.month),
                budget.amount.to_decimal_string(),
                budget.amount.currency().to_string(),
                budget.group.as_str().to_owned(),
                if budget.rollover { "yes" } else { "no" }.to_owned(),
            ]
        })
        .collect();
    write_table(
        out,
        &[
            "CATEGORY", "MONTH", "AMOUNT", "CURRENCY", "GROUP", "ROLLOVER",
        ],
        &rows,
    )?;
    Ok(())
}

/// `coffer budgets set`.
pub fn set(db: &mut Database, args: &SetBudgetArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    let currency = match args.currency {
        Some(currency) => currency,
        None => default_currency(db)?,
    };
    let amount = Money::parse_decimal(&args.amount, currency, DecimalMark::Point)
        .with_context(|| format!("{:?} isn't an amount in {currency}", args.amount))?;
    if amount.is_negative() {
        bail!("a budget can't be negative");
    }
    let budget = NewBudget {
        category: args.category.clone(),
        month: args.month,
        amount,
        group: args.group,
    };
    let saved = budget::set(db, &budget)?;
    let verb = match saved {
        Saved::Added => "Set",
        Saved::Replaced => "Changed",
    };
    writeln!(
        out,
        "{verb} the {} budget for {}: {} {} ({}{}).",
        month_label(args.month),
        args.category,
        amount.to_decimal_string(),
        currency,
        args.group.as_str(),
        if args.group.rollover() {
            ", rolls over"
        } else {
            ""
        },
    )?;
    Ok(())
}

/// `coffer budgets remove`.
pub fn remove(
    db: &mut Database,
    args: &RemoveBudgetArgs,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    if !budget::remove(db, &args.category, args.month)? {
        bail!(
            "{} has no {} budget; `coffer budgets list` shows the budgets",
            args.category,
            month_label(args.month)
        );
    }
    writeln!(
        out,
        "Removed the {} budget for {}.",
        month_label(args.month),
        args.category
    )?;
    Ok(())
}

/// `standing`, or the month.
fn month_label(month: Option<Period>) -> String {
    month.map_or_else(|| "standing".to_owned(), |month| month.key())
}

/// The one currency every account is in.
fn default_currency(db: &Database) -> anyhow::Result<Currency> {
    let mut currencies: Vec<Currency> = account::list(db)?
        .iter()
        .map(|account| account.currency)
        .collect();
    currencies.sort_by_key(|currency| currency.code().to_owned());
    currencies.dedup();
    match currencies.as_slice() {
        [currency] => Ok(*currency),
        [] => bail!("there are no accounts to take a currency from; name one with --currency"),
        several => bail!(
            "the accounts are in {}; name the budget's currency with --currency",
            several
                .iter()
                .map(Currency::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}
