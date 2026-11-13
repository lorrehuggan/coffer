//! `coffer categorize <ID>... <CATEGORY>` (ROADMAP M2.14): set the category
//! of transactions as the user, until the app can.
//!
//! It goes through [`own::recategorize`], the one way the user's own
//! recategorisations reach the database: all the rows in one DB
//! transaction, recorded as one undoable change batch, with the category
//! source `user`. That is also what lets Coffer notice repeated identical
//! edits and file its own `rule` suggestion ([`own`] documents when); the
//! command says when it did.
//!
//! Every ID and the category key are checked before anything is written:
//! an unknown transaction or category, an ID named twice, or a row that
//! can't take a category (a split row) writes nothing. Output is IDs and
//! category keys only: no transaction text.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use anyhow::{Context, bail};
use clap::Args;
use coffer_core::category::{CategoryKey, CategoryTree};
use coffer_core::change::{self, CategorySource, CategoryValue, Value};
use coffer_core::db::Database;
use coffer_core::id::{ChangeBatchId, TransactionId};
use coffer_core::inbox::own::{self, RecategorizeError};
use coffer_core::inbox::{
    self, COFFER_SOURCE, Filter, SuggestionId, SuggestionKind, SuggestionStatus,
};

use super::import::plural;

#[derive(Debug, Args)]
pub struct CategorizeArgs {
    /// The transactions' IDs.
    #[arg(value_name = "TRANSACTION_ID", required = true, num_args = 1..)]
    pub transactions: Vec<i64>,

    /// The category key to set, e.g. `food.groceries`.
    #[arg(value_name = "CATEGORY", required = true)]
    pub category: String,
}

/// `coffer categorize`.
pub fn run(db: &mut Database, args: &CategorizeArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    let key = CategoryKey::new(&args.category).map_err(|error| {
        anyhow::anyhow!(
            "{:?} isn't a category key ({error}); nothing was changed",
            args.category
        )
    })?;
    let categories = db.categories().context("couldn't read the categories")?;
    if categories.get(&key).is_none() {
        bail!("there is no category {key}; nothing was changed");
    }
    let mut seen = BTreeSet::new();
    let mut items = Vec::with_capacity(args.transactions.len());
    for &id in &args.transactions {
        let id = TransactionId::new(id);
        if !seen.insert(id) {
            bail!("transaction {id} is named twice; nothing was changed");
        }
        items.push((id, key.clone()));
    }

    let before = coffer_rule_suggestions(db)?;
    let batch = match own::recategorize(db, &items) {
        Ok(batch) => batch,
        Err(RecategorizeError::Change(change::Error::TransactionNotFound(id))) => {
            bail!("there is no transaction {id}; nothing was changed")
        }
        Err(error) => bail!("{error}; nothing was changed"),
    };

    match batch {
        Some(batch) => write_changes(db, &categories, batch, &key, items.len(), out)?,
        None => writeln!(
            out,
            "Nothing changed: {} already had category {key}, set by you.",
            if items.len() == 1 {
                "the transaction"
            } else {
                "every transaction"
            }
        )?,
    }

    let filed: Vec<SuggestionId> = coffer_rule_suggestions(db)?
        .difference(&before)
        .copied()
        .collect();
    for id in filed {
        writeln!(
            out,
            "Coffer suggested a rule for this merchant (suggestion {id}): \
             `coffer inbox show {id}` shows it."
        )?;
    }
    Ok(())
}

/// What batch `batch` changed, row by row, and the rows it left alone.
fn write_changes(
    db: &Database,
    categories: &CategoryTree,
    batch: ChangeBatchId,
    key: &CategoryKey,
    named: usize,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let items = change::items(db, batch).context("couldn't read the change batch")?;
    let keys: BTreeMap<i64, &CategoryKey> = categories.iter().map(|c| (c.id(), c.key())).collect();
    let changed: BTreeSet<TransactionId> = items.iter().map(|item| item.transaction).collect();
    writeln!(
        out,
        "Set the category of {} to {key} (change batch {batch}):",
        plural(changed.len(), "transaction")
    )?;
    for item in &items {
        if let (Value::Category(old), Value::Category(new)) = (&item.old, &item.new) {
            writeln!(
                out,
                "  transaction {}: {} -> {}",
                item.transaction,
                describe(old, &keys),
                describe(new, &keys)
            )?;
        }
    }
    let unchanged = named - changed.len();
    if unchanged > 0 {
        writeln!(
            out,
            "{} already had it, set by you.",
            plural(unchanged, "other transaction")
        )?;
    }
    Ok(())
}

/// A category value as `key` and who set it, or `none`.
fn describe(value: &CategoryValue, keys: &BTreeMap<i64, &CategoryKey>) -> String {
    let Some(id) = value.category_id else {
        return "none".to_owned();
    };
    let key = keys
        .get(&id)
        .map_or_else(|| format!("category {id}"), |key| key.to_string());
    match value.source {
        Some(CategorySource::User) | None => key,
        Some(CategorySource::Rule) => format!("{key} (from a rule)"),
        Some(CategorySource::PresetMap) => format!("{key} (from the bank's category)"),
        Some(CategorySource::AiApproved) => format!("{key} (an approved suggestion)"),
    }
}

/// The pending `rule` suggestions Coffer filed itself.
fn coffer_rule_suggestions(db: &Database) -> anyhow::Result<BTreeSet<SuggestionId>> {
    let pending = inbox::list(
        db,
        &Filter {
            status: Some(SuggestionStatus::Pending),
            kind: Some(SuggestionKind::Rule),
        },
    )
    .context("couldn't read the inbox")?;
    Ok(pending
        .into_iter()
        .filter(|s| s.source_client == COFFER_SOURCE)
        .map(|s| s.id)
        .collect())
}
