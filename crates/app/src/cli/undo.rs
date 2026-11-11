//! `coffer undo-import <batch>`: say what undoing an import removes, and
//! with `--yes`, remove it.

use std::io::Write;

use clap::Args;
use coffer_core::account;
use coffer_core::db::Database;
use coffer_core::id::BatchId;
use coffer_core::import::{self, UndoPreview};

use super::accounts;
use super::import::plural;

#[derive(Debug, Args)]
pub struct UndoImportArgs {
    /// The import's batch ID, as `coffer import` printed it.
    #[arg(value_name = "BATCH")]
    pub batch: i64,

    /// Undo it. Without this, only shows what undoing would remove.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

/// `coffer undo-import`. Without `--yes` it only previews, and succeeds.
pub fn run(db: &mut Database, args: &UndoImportArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    let batch = BatchId::new(args.batch);
    let preview = import::undo_preview(db, batch)?;
    let target = account::get(db, preview.batch.account)?;

    writeln!(
        out,
        "Import {}: {} into {}, imported {}",
        preview.batch.id,
        preview.batch.source_file,
        accounts::describe(&target),
        preview.batch.imported_at
    )?;
    writeln!(out, "Undoing it removes:")?;
    write_counts(out, &preview)?;

    if !args.yes {
        writeln!(out)?;
        writeln!(
            out,
            "Nothing was undone. Run the command again with --yes to undo it."
        )?;
        return Ok(());
    }

    let undone = import::undo(db, batch)?;
    writeln!(out)?;
    writeln!(
        out,
        "Undid import {}: removed {}, {} and {}.",
        undone.batch.id,
        plural(undone.transactions + undone.mirrored, "transaction"),
        plural(undone.balances, "balance"),
        plural(undone.flags, "flag")
    )?;
    Ok(())
}

fn write_counts(out: &mut dyn Write, preview: &UndoPreview) -> std::io::Result<()> {
    writeln!(out, "  {}", plural(preview.transactions, "transaction"))?;
    if preview.mirrored > 0 {
        writeln!(
            out,
            "  {} it created in pots",
            plural(preview.mirrored, "transaction")
        )?;
    }
    writeln!(out, "  {}", plural(preview.balances, "balance"))?;
    writeln!(out, "  {} recorded with it", plural(preview.flags, "flag"))?;
    if preview.edited > 0 {
        writeln!(
            out,
            "{} your edits (category, note, share, exclusion). The edits go with them; \
             importing the file again brings the rows back without them.",
            match preview.edited {
                1 => "1 of these transactions has".to_owned(),
                n => format!("{n} of these transactions have"),
            }
        )?;
    }
    if preview.transfers > 0 {
        writeln!(
            out,
            "{} {} dissolved: the other side stays as an ordinary transaction.",
            plural(preview.transfers, "transfer pair"),
            if preview.transfers == 1 { "is" } else { "are" }
        )?;
    }
    if !preview.later_batches.is_empty() {
        let ids: Vec<String> = preview
            .later_batches
            .iter()
            .map(ToString::to_string)
            .collect();
        writeln!(
            out,
            "Later imports into this account relied on it: {}. Rows they skipped as already \
             imported go too; import those files again to bring them back.",
            ids.join(", ")
        )?;
    }
    Ok(())
}
