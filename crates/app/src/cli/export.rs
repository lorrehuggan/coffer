//! `coffer export` (M1.29): the ledger as CSV or JSON
//! ([`coffer_core::export`], which documents the columns).
//!
//! Without `--output` the export goes to stdout and nothing else is
//! printed, so it can be piped. With `--output`, the file is created `0600`
//! (it is personal data), an existing file is refused unless `--force`, and
//! one line says how many rows were written. Notes and private tags are left
//! out unless `--include-notes` / `--include-private-tags` ask for them;
//! share labels never leave the app.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use chrono::NaiveDate;
use clap::{Args, ValueEnum};
use coffer_core::account;
use coffer_core::category::CategoryKey;
use coffer_core::db::Database;
use coffer_core::export::{self, Filter, Format, Options};
use coffer_core::tag::TagName;

#[derive(Debug, Args)]
pub struct ExportArgs {
    /// The file format.
    #[arg(long, value_enum)]
    pub format: ExportFormat,

    /// Write to this file (created private to you) instead of stdout.
    #[arg(long, short = 'o', value_name = "FILE")]
    pub output: Option<PathBuf>,

    /// Replace the --output file if it exists.
    #[arg(long, requires = "output")]
    pub force: bool,

    /// The first date to export, included (YYYY-MM-DD).
    #[arg(long, value_name = "DATE", value_parser = parse_date)]
    pub from: Option<NaiveDate>,

    /// The last date to export, included (YYYY-MM-DD).
    #[arg(long, value_name = "DATE", value_parser = parse_date)]
    pub to: Option<NaiveDate>,

    /// Only this account (ID or exact name); repeat for several.
    #[arg(long, value_name = "ID|NAME")]
    pub account: Vec<String>,

    /// Only this category, by key (e.g. food.groceries); a group's key
    /// includes its subcategories. Repeat for several.
    #[arg(long, value_name = "KEY")]
    pub category: Vec<String>,

    /// Only transactions with this tag (ignoring case); repeat for several.
    #[arg(long, value_name = "NAME")]
    pub tag: Vec<String>,

    /// Add the notes: yours on transactions, split lines and exclusions,
    /// and the bank's imported note.
    #[arg(long)]
    pub include_notes: bool,

    /// Add a private_tags column with your private tags (and allow
    /// filtering by one).
    #[arg(long)]
    pub include_private_tags: bool,
}

/// `--format`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ExportFormat {
    Csv,
    Json,
}

impl From<ExportFormat> for Format {
    fn from(format: ExportFormat) -> Format {
        match format {
            ExportFormat::Csv => Format::Csv,
            ExportFormat::Json => Format::Json,
        }
    }
}

fn parse_date(text: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d")
        .map_err(|_| format!("{text:?} isn't a date in the form YYYY-MM-DD"))
}

/// `coffer export`.
pub fn run(db: &Database, args: &ExportArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    if let Some(path) = &args.output
        && is_same_file(path, db.path())
    {
        bail!(
            "{} is the Coffer database; choose another file for the export",
            path.display()
        );
    }
    let options = options(db, args)?;
    let format = Format::from(args.format);
    let Some(path) = &args.output else {
        return export::write(db, format, &options, out)
            .map(|_| ())
            .map_err(|error| explain(error, args));
    };
    let rows = export::to_file(db, format, &options, path, args.force)
        .map_err(|error| explain(error, args))?;
    writeln!(
        out,
        "Exported {rows} {} to {}.",
        if rows == 1 { "row" } else { "rows" },
        path.display()
    )?;
    Ok(())
}

/// Whether `a` and `b` name the same existing file.
fn is_same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn options(db: &Database, args: &ExportArgs) -> anyhow::Result<Options> {
    let accounts = if args.account.is_empty() {
        Vec::new()
    } else {
        let all = account::list(db)?;
        args.account
            .iter()
            .map(|wanted| super::accounts::find(&all, wanted).map(|account| account.id))
            .collect::<anyhow::Result<_>>()?
    };
    let categories = args
        .category
        .iter()
        .map(|key| {
            CategoryKey::new(key.trim())
                .with_context(|| format!("{key:?} isn't a category key (e.g. food.groceries)"))
        })
        .collect::<anyhow::Result<_>>()?;
    let tags = args
        .tag
        .iter()
        .map(|name| TagName::new(name).context("that isn't a tag name"))
        .collect::<anyhow::Result<_>>()?;
    Ok(Options {
        filter: Filter {
            from: args.from,
            to: args.to,
            accounts,
            categories,
            tags,
        },
        include_notes: args.include_notes,
        include_private_tags: args.include_private_tags,
    })
}

/// Core's error, with the flag that changes the answer where there is one.
fn explain(error: export::Error, args: &ExportArgs) -> anyhow::Error {
    match error {
        export::Error::FileExists(path) => {
            anyhow::anyhow!(
                "{} already exists; use --force to replace it",
                path.display()
            )
        }
        export::Error::PrivateTag => anyhow::anyhow!(
            "a --tag is private; private tags stay out of exports unless you add \
             --include-private-tags"
        ),
        export::Error::UnknownTag => anyhow::anyhow!("there is no tag with a --tag name"),
        export::Error::EmptyRange { from, to } => {
            anyhow::anyhow!("--from {from} is after --to {to}")
        }
        export::Error::Write(error) if args.output.is_none() => {
            anyhow::Error::new(error).context("couldn't write the export to stdout")
        }
        other => {
            let error = anyhow::Error::new(other);
            match &args.output {
                Some(path) => error.context(format!("couldn't export to {}", path.display())),
                None => error,
            }
        }
    }
}
