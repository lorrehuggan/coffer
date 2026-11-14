//! Exporting transactions as CSV or JSON (M1.29, `coffer export`; the app's
//! export dialog, M3.16). `docs/EXPORT.md` is a copy of these docs for
//! readers of an export: keep the two in step.
//!
//! An export is one row per line of the ledger (the `ledger_line` view): a
//! transaction that isn't split is one row, and a split transaction is one
//! row per split line, each carrying its parent's `transaction_id` with its
//! own `split_id` and `line_no`. Money in with confirmed links (a refund,
//! reimbursement or settlement, M1.25) is its own row for the unlinked rest
//! plus one row per link part, each with `link_id`: a refund's part has the
//! purchase's category (and one row per purchase line when the purchase is
//! split). A transaction's rows always sum to its amount. Transaction fields (date, account,
//! description, …) repeat on each line of a split; the amount, my amount
//! and category are the line's.
//!
//! The export doesn't filter by status unless asked: excluded rows and
//! transfers are included, with `excluded_reason` and `transfer_status` to
//! filter by, as in `ledger_line`. [`Options::include_excluded`] off leaves
//! the excluded rows out (the app's export dialog, M3.16). Rows are ordered by date, then transaction ID, then
//! line number, then link.
//!
//! # Filters ([`Filter`])
//!
//! Every filter given must match (AND); several values of one filter match
//! any of them (OR).
//!
//! - **Dates**: `from` and `to`, both included, on the transaction's date.
//! - **Accounts**: by ID. A pot is its own account: naming its parent
//!   doesn't include it.
//! - **Categories**: by key, on the row's own category (a split line's, or
//!   the transaction's). A group's key includes its subcategories:
//!   `food` matches `food` and `food.groceries`. Uncategorised rows match no
//!   category filter.
//! - **Tags**: by name, ignoring case like tag names do; a row matches when
//!   its transaction has any of the tags. A private tag can be filtered by
//!   only when private tags are included ([`Options::include_private_tags`]):
//!   otherwise which rows carry it would leave the app.
//!
//! # What is left out, and the opt-ins ([`Options`])
//!
//! - **Share labels never leave the app** (decision record "Data model":
//!   `share_label`), so no export has them. `my_amount` still says how much
//!   of a row is the user's.
//! - **Private tags** are left out by default: `tags` has the public ones
//!   only ([`crate::tag::public_tags_of`]). With
//!   [`Options::include_private_tags`], a `private_tags` column is added
//!   with the private ones; `tags` stays public-only, so the two are told
//!   apart.
//! - **Notes** are left out by default. With [`Options::include_notes`],
//!   four columns are added: `user_note` (the user's note on the
//!   transaction), `line_note` (a split line's note), `excluded_note` (the
//!   note on an exclusion) and `bank_note` (the note imported from the bank's
//!   export, which in e.g. Monzo's is text the user wrote in the bank's
//!   app). Without it, none of these columns is there.
//!
//! # Formats
//!
//! - **CSV**: UTF-8, comma-separated, a header row of the column names,
//!   `\n` line endings, fields quoted (with `"` doubled) only when they hold
//!   a comma, a quote or a line break. An empty field is a missing value.
//!   Booleans are `true`/`false`. A list (`tags`, `private_tags`) is its
//!   names joined with `"; "`; a tag name can itself hold `;`, so the JSON
//!   form is the exact one. The CSV has no version marker: its header row
//!   is the schema.
//! - **JSON**: one object, `{"schema": "coffer.export", "schema_version":
//!   1, "includes": {"notes": …, "private_tags": …}, "rows": [ … ]}`, each
//!   row an object with the columns below as keys, in that order. A missing
//!   value is `null`, a list is an array of strings, a boolean is
//!   `true`/`false`, and numbers are integers only (IDs, line numbers and
//!   `*_minor` amounts).
//!
//! Neither has a float: every amount is an exact decimal string in major
//! units (`-1234.56`, [`Money::to_decimal_string`]) beside its `*_minor`
//! integer, in the row's `currency` (`original_*` in `original_currency`).
//!
//! # Columns (schema version 1)
//!
//! | Column | Type | Meaning | Present |
//! |---|---|---|---|
//! | `transaction_id` | integer | The transaction's ID; a split line's parent | always |
//! | `split_id` | integer or empty | The split line's ID | split lines |
//! | `line_no` | integer or empty | The split line's number, from 1 | split lines |
//! | `date` | `YYYY-MM-DD` | The transaction's date | always |
//! | `time` | `HH:MM:SS` or empty | The transaction's time, when the bank gave one | when known |
//! | `account_id` | integer | The account's ID | always |
//! | `account_name` | text | The account's name | always |
//! | `currency` | ISO 4217 code | The currency of every amount but `original_*` | always |
//! | `amount` | decimal text | The row's amount (the line's, or the transaction's); negative is money out | always |
//! | `amount_minor` | integer | `amount` in minor units | always |
//! | `my_amount` | decimal text | The user's part of `amount` (all of it unless a share is set) | always |
//! | `my_amount_minor` | integer | `my_amount` in minor units | always |
//! | `transaction_amount` | decimal text | The whole transaction's amount; equals `amount` unless split | always |
//! | `transaction_amount_minor` | integer | `transaction_amount` in minor units | always |
//! | `original_amount` | decimal text or empty | The whole transaction in the currency spent, when it differs | foreign spending |
//! | `original_amount_minor` | integer or empty | `original_amount` in minor units | foreign spending |
//! | `original_currency` | ISO 4217 code or empty | The currency spent | foreign spending |
//! | `merchant` | text or empty | The merchant as the bank gave it | when given |
//! | `merchant_display` | text or empty | The cleaned merchant name the app shows | when known |
//! | `description` | text | The bank's description, as imported (or as entered) | always |
//! | `category_key` | text or empty | The row's category key, e.g. `food.groceries` | categorised rows |
//! | `category_label` | text or empty | The category's label | categorised rows |
//! | `category_source` | `rule`, `preset_map`, `user`, `ai_approved` or empty | What set the transaction's category | categorised, unsplit rows |
//! | `tags` | list | The transaction's public tags, by name | always (may be empty) |
//! | `reimbursable` | boolean | Whether the user expects the money back | always |
//! | `excluded_reason` | `duplicate`, `bank_error`, `disputed`, `other` or empty | Why the transaction is excluded from reports (only `duplicate` also leaves the balance) | excluded rows |
//! | `excluded_at` | UTC timestamp or empty | When it was excluded | excluded rows |
//! | `transfer_group_id` | integer or empty | The transfer pair the transaction is in | paired rows |
//! | `transfer_status` | `suggested`, `confirmed`, `rejected` or empty | That pair's status | paired rows |
//! | `origin` | `imported`, `mirrored`, `manual` | Where the transaction came from (`mirrored`: the pot side of a pot transfer) | always |
//! | `batch_id` | integer or empty | The import batch that brought it | imported and mirrored rows |
//! | `external_id` | text or empty | The bank's own ID for the transaction | when the file had one |
//! | `source_category` | text or empty | The bank's category, as exported | when the file had one |
//! | `source_type` | text or empty | The bank's transaction type, as exported | when the file had one |
//! | `created_at` | UTC timestamp | When the transaction entered the database | always |
//! | `link_id` | integer or empty | The confirmed link this row is a part of | link parts |
//! | `link_kind` | `refund`, `reimbursement`, `settlement` or empty | That link's kind | link parts |
//! | `linked_transaction_id` | integer or empty | The purchase a refund or reimbursement part nets against | refund and reimbursement parts |
//! | `in_reports` | boolean | Whether the row counts in spending, income and reports: not excluded, not a confirmed transfer (for a link part, nor is its purchase) | always |
//! | `personal_amount` | decimal text | What the row counts as the user's own spending or income: `my_amount`, or 0 when not in reports or reimbursable | always |
//! | `personal_amount_minor` | integer | `personal_amount` in minor units | always |
//! | `user_note` | text or empty | The user's note on the transaction | with notes |
//! | `line_note` | text or empty | The user's note on the split line | with notes |
//! | `excluded_note` | text or empty | The user's note on the exclusion | with notes |
//! | `bank_note` | text or empty | The note imported from the bank's export | with notes |
//! | `private_tags` | list | The transaction's private tags, by name | with private tags |
//!
//! Timestamps are `YYYY-MM-DDTHH:MM:SS.SSSZ`. A later schema version may add
//! columns at the end; a change to an existing column's meaning or type
//! bumps [`SCHEMA_VERSION`].
//!
//! Exports are personal data: [`to_file`] creates the file `0600` (on Unix).
//! Nothing here logs row text.

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, Row, params_from_iter};

use crate::account::{self, date_key};
use crate::category::{CategoryKey, CategoryTree};
use crate::db::Database;
use crate::id::{AccountId, BatchId, LinkId, SplitId, TransactionId, TransferGroupId};
use crate::money::{Currency, Money};
use crate::tag::{self, TagName};

/// The export's schema version: the `schema_version` of a JSON export.
pub const SCHEMA_VERSION: u32 = 1;

/// The `schema` of a JSON export.
pub const SCHEMA_NAME: &str = "coffer.export";

/// The file format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    Csv,
    Json,
}

/// Which rows to export (module docs, "Filters"). The default exports
/// everything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    /// The first date, included.
    pub from: Option<NaiveDate>,
    /// The last date, included.
    pub to: Option<NaiveDate>,
    /// Empty: every account.
    pub accounts: Vec<AccountId>,
    /// Empty: every row, categorised or not. A group includes its
    /// subcategories.
    pub categories: Vec<CategoryKey>,
    /// Empty: every row, tagged or not.
    pub tags: Vec<TagName>,
}

/// What to export. The default exports every row, excluded ones included,
/// with neither notes nor private tags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub filter: Filter,
    /// Adds `user_note`, `line_note`, `excluded_note` and `bank_note`.
    pub include_notes: bool,
    /// Adds `private_tags`, and allows filtering by a private tag.
    pub include_private_tags: bool,
    /// Keeps the rows of excluded transactions, with their
    /// `excluded_reason`. On by default (and always for `coffer export`);
    /// the app's export dialog starts with it off (M3.16).
    pub include_excluded: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            filter: Filter::default(),
            include_notes: false,
            include_private_tags: false,
            include_excluded: true,
        }
    }
}

/// How much an export holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Distinct transactions.
    pub transactions: usize,
    /// Rows: a split transaction has one per line, a linked one per part.
    pub rows: usize,
}

/// One exported row: a transaction, or one line of a split one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportRow {
    pub transaction_id: TransactionId,
    pub split_id: Option<SplitId>,
    pub line_no: Option<i64>,
    pub date: NaiveDate,
    /// `HH:MM:SS`.
    pub time: Option<String>,
    pub account_id: AccountId,
    pub account_name: String,
    pub amount: Money,
    pub my_amount: Money,
    pub transaction_amount: Money,
    pub original_amount: Option<Money>,
    pub merchant: Option<String>,
    pub merchant_display: Option<String>,
    pub description: String,
    pub category_key: Option<CategoryKey>,
    pub category_label: Option<String>,
    pub category_source: Option<String>,
    /// Public tags, by name.
    pub tags: Vec<String>,
    pub reimbursable: bool,
    pub excluded_reason: Option<String>,
    pub excluded_at: Option<String>,
    pub transfer_group_id: Option<TransferGroupId>,
    pub transfer_status: Option<String>,
    pub origin: String,
    pub batch_id: Option<BatchId>,
    pub external_id: Option<String>,
    pub source_category: Option<String>,
    pub source_type: Option<String>,
    pub created_at: String,
    /// The confirmed link this row is a part of (M1.25).
    pub link_id: Option<LinkId>,
    pub link_kind: Option<String>,
    /// The purchase a refund or reimbursement part nets against.
    pub linked_transaction_id: Option<TransactionId>,
    pub in_reports: bool,
    pub personal_amount: Money,
    /// `None` unless notes are included.
    pub notes: Option<Notes>,
    /// `None` unless private tags are included.
    pub private_tags: Option<Vec<String>>,
}

/// A row's notes, read only when they are asked for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Notes {
    pub user_note: Option<String>,
    pub line_note: Option<String>,
    pub excluded_note: Option<String>,
    pub bank_note: Option<String>,
}

/// Why an export failed. Messages never contain row text or tag names.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the start date {from} is after the end date {to}")]
    EmptyRange { from: NaiveDate, to: NaiveDate },

    #[error("there is no account {0}")]
    UnknownAccount(AccountId),

    #[error("there is no category {0}")]
    UnknownCategory(CategoryKey),

    #[error("there is no tag with that name")]
    UnknownTag,

    #[error("that tag is private, and private tags stay out of exports unless they are included")]
    PrivateTag,

    #[error("{} already exists; choose another file or allow overwriting it", .0.display())]
    FileExists(PathBuf),

    #[error("couldn't write {}: {source}", path.display())]
    File { path: PathBuf, source: io::Error },

    #[error("{} is Coffer's database (or one of its files); choose another file for the export", .0.display())]
    IsDatabase(PathBuf),

    #[error("couldn't write the export: {0}")]
    Write(#[source] io::Error),

    #[error(transparent)]
    Tag(#[from] tag::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// The column names an export with `options` has, in order.
pub fn columns(options: &Options) -> Vec<&'static str> {
    Column::for_options(options)
        .into_iter()
        .map(Column::name)
        .collect()
}

/// The rows `options` selects, in order (module docs).
pub fn rows(db: &Database, options: &Options) -> Result<Vec<ExportRow>, Error> {
    let conn = db.conn();
    let filter = Resolved::new(db, options)?;
    let tags = tag_names(conn)?;

    let mut sql = String::from(
        "SELECT l.transaction_id, l.split_id, l.line_no, l.account_id, l.batch_id, l.date, \
                l.amount_minor, l.currency, l.my_amount_minor, l.reimbursable, l.merchant, \
                l.merchant_display, l.excluded_reason, l.transfer_group_id, l.transfer_status, \
                l.origin, l.link_id, l.link_kind, l.target_id, l.in_reports, l.personal_minor, \
                t.time, t.amount_minor AS transaction_amount_minor, t.original_amount_minor, \
                t.original_currency, t.description_raw, \
                CASE WHEN l.link_id IS NULL THEN t.category_source END AS category_source, \
                t.excluded_at, \
                t.external_id, t.source_category, t.source_type, t.created_at, \
                a.name AS account_name, c.key AS category_key, c.label AS category_label",
    );
    if options.include_notes {
        sql.push_str(
            ", t.user_note, s.user_note AS line_note, t.excluded_note, t.note AS bank_note",
        );
    }
    sql.push_str(
        " FROM ledger_line l \
          JOIN \"transaction\" t ON t.id = l.transaction_id \
          JOIN account a ON a.id = l.account_id \
          LEFT JOIN category c ON c.id = l.category_id \
          LEFT JOIN transaction_split s ON s.id = l.split_id \
          WHERE 1",
    );
    let mut values: Vec<SqlValue> = Vec::new();
    if let Some(from) = options.filter.from {
        sql.push_str(" AND l.date >= ?");
        values.push(SqlValue::Text(date_key(from)));
    }
    if let Some(to) = options.filter.to {
        sql.push_str(" AND l.date <= ?");
        values.push(SqlValue::Text(date_key(to)));
    }
    any_of(
        &mut sql,
        &mut values,
        "l.account_id IN ({})",
        &filter.accounts,
    );
    any_of(
        &mut sql,
        &mut values,
        "l.category_id IN ({})",
        &filter.categories,
    );
    any_of(
        &mut sql,
        &mut values,
        "EXISTS (SELECT 1 FROM transaction_tag tt \
         WHERE tt.transaction_id = l.transaction_id AND tt.tag_id IN ({}))",
        &filter.tags,
    );
    if !options.include_excluded {
        sql.push_str(" AND l.excluded_reason IS NULL");
    }
    sql.push_str(" ORDER BY l.date, l.transaction_id, l.line_no, l.link_id, l.target_split_id");

    let mut statement = conn.prepare(&sql)?;
    let mut query = statement.query(params_from_iter(values))?;
    let mut rows = Vec::new();
    while let Some(row) = query.next()? {
        rows.push(read_row(row, options, &tags)?);
    }
    Ok(rows)
}

/// Adds ` AND <condition>` for `ids`, when there are any, with `{}` in
/// `condition` replaced by a placeholder per ID.
fn any_of(sql: &mut String, values: &mut Vec<SqlValue>, condition: &str, ids: &[i64]) {
    if ids.is_empty() {
        return;
    }
    let marks = vec!["?"; ids.len()].join(", ");
    sql.push_str(" AND ");
    sql.push_str(&condition.replace("{}", &marks));
    values.extend(ids.iter().map(|&id| SqlValue::Integer(id)));
}

/// The first and last dates of the ledger, `None` when it's empty: the
/// export dialog's dates start there, so it exports everything unless
/// narrowed.
pub fn date_span(db: &Database) -> Result<Option<(NaiveDate, NaiveDate)>, Error> {
    let span: (Option<String>, Option<String>) =
        db.conn()
            .query_row("SELECT min(date), max(date) FROM ledger_line", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?;
    let parse = |text: Option<String>| {
        text.and_then(|text| NaiveDate::parse_from_str(&text, "%Y-%m-%d").ok())
    };
    Ok(parse(span.0).zip(parse(span.1)))
}

/// How many transactions and rows `options` selects: the app's export
/// dialog says so before writing anything.
pub fn count(db: &Database, options: &Options) -> Result<Counts, Error> {
    let rows = rows(db, options)?;
    let transactions = rows
        .iter()
        .map(|row| row.transaction_id)
        .collect::<std::collections::HashSet<_>>()
        .len();
    Ok(Counts {
        transactions,
        rows: rows.len(),
    })
}

/// Exports the rows `options` selects to `out` and returns how many there
/// were.
pub fn write(
    db: &Database,
    format: Format,
    options: &Options,
    out: &mut dyn Write,
) -> Result<usize, Error> {
    let rows = rows(db, options)?;
    write_rows(&rows, format, options, out).map_err(Error::Write)?;
    Ok(rows.len())
}

/// Exports to a new file at `path`, created `0600` on Unix, and returns how
/// many rows it has. An existing file is refused ([`Error::FileExists`])
/// unless `overwrite`; then it is replaced whole (written beside it, then
/// renamed over it), so it ends up `0600` too and is never half-written.
/// The rows are read before the file is touched, so a filter error leaves
/// nothing behind.
pub fn to_file(
    db: &Database,
    format: Format,
    options: &Options,
    path: &Path,
    overwrite: bool,
) -> Result<usize, Error> {
    if is_database_file(db.path(), path) {
        return Err(Error::IsDatabase(path.to_path_buf()));
    }
    let rows = rows(db, options)?;
    let file_error = |path: &Path| {
        let path = path.to_path_buf();
        move |source| Error::File { path, source }
    };
    let target = if overwrite {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        path.with_file_name(format!(".{name}.coffer-export.tmp"))
    } else {
        path.to_path_buf()
    };
    if overwrite {
        remove_if_exists(&target).map_err(file_error(&target))?;
    }
    let file = match create_private(&target) {
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && !overwrite => {
            return Err(Error::FileExists(path.to_path_buf()));
        }
        result => result.map_err(file_error(&target))?,
    };
    let written = (|| {
        let mut out = BufWriter::new(file);
        write_rows(&rows, format, options, &mut out)?;
        out.into_inner()
            .map_err(io::IntoInnerError::into_error)?
            .sync_all()?;
        if overwrite {
            fs::rename(&target, path)?;
        }
        Ok(())
    })();
    if let Err(error) = written {
        let _ = fs::remove_file(&target);
        return Err(file_error(path)(error));
    }
    Ok(rows.len())
}

/// Whether `path` is the database at `database` or one of its `-wal`,
/// `-shm` or `-journal` files: an export must never replace them. Compared
/// by their directory's canonical path, so a link to the data directory
/// is caught too.
fn is_database_file(database: &Path, path: &Path) -> bool {
    let canonical = |path: &Path| -> Option<PathBuf> {
        let dir = match path.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => dir,
            _ => Path::new("."),
        };
        Some(dir.canonicalize().ok()?.join(path.file_name()?))
    };
    let (Some(database), Some(path)) = (canonical(database), canonical(path)) else {
        return false;
    };
    ["", "-wal", "-shm", "-journal"].iter().any(|suffix| {
        let mut name = database.clone().into_os_string();
        name.push(suffix);
        name == path.as_os_str()
    })
}

fn create_private(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Writes `rows` in `format`.
pub fn write_rows(
    rows: &[ExportRow],
    format: Format,
    options: &Options,
    out: &mut dyn Write,
) -> io::Result<()> {
    let columns = Column::for_options(options);
    match format {
        Format::Csv => write_csv(rows, &columns, out),
        Format::Json => write_json(rows, &columns, options, out),
    }
}

fn write_csv(rows: &[ExportRow], columns: &[Column], out: &mut dyn Write) -> io::Result<()> {
    let header: Vec<String> = columns
        .iter()
        .map(|column| csv_field(column.name()))
        .collect();
    writeln!(out, "{}", header.join(","))?;
    for row in rows {
        let fields: Vec<String> = columns
            .iter()
            .map(|column| match column.cell(row) {
                Cell::Null => String::new(),
                Cell::Int(value) => value.to_string(),
                Cell::Bool(value) => value.to_string(),
                Cell::Text(text) => csv_field(&text),
                Cell::List(items) => csv_field(&items.join("; ")),
            })
            .collect();
        writeln!(out, "{}", fields.join(","))?;
    }
    Ok(())
}

/// Quotes a CSV field when it needs it, doubling its quotes.
fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_owned()
    }
}

fn write_json(
    rows: &[ExportRow],
    columns: &[Column],
    options: &Options,
    out: &mut dyn Write,
) -> io::Result<()> {
    writeln!(out, "{{")?;
    writeln!(out, "  \"schema\": {},", json_string(SCHEMA_NAME))?;
    writeln!(out, "  \"schema_version\": {SCHEMA_VERSION},")?;
    writeln!(
        out,
        "  \"includes\": {{\"notes\": {}, \"private_tags\": {}}},",
        options.include_notes, options.include_private_tags
    )?;
    write!(out, "  \"rows\": [")?;
    for (index, row) in rows.iter().enumerate() {
        write!(out, "{}\n    {{", if index == 0 { "" } else { "," })?;
        for (position, column) in columns.iter().enumerate() {
            let value = match column.cell(row) {
                Cell::Null => "null".to_owned(),
                Cell::Int(value) => value.to_string(),
                Cell::Bool(value) => value.to_string(),
                Cell::Text(text) => json_string(&text),
                Cell::List(items) => format!(
                    "[{}]",
                    items
                        .iter()
                        .map(|item| json_string(item))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            };
            let separator = if position == 0 { "" } else { ", " };
            write!(out, "{separator}{}: {value}", json_string(column.name()))?;
        }
        write!(out, "}}")?;
    }
    writeln!(out, "{}]", if rows.is_empty() { "" } else { "\n  " })?;
    writeln!(out, "}}")?;
    Ok(())
}

fn json_string(text: &str) -> String {
    serde_json::to_string(text).expect("a string always serialises")
}

/// One value of an exported row, before it's written.
enum Cell {
    Null,
    Int(i64),
    Bool(bool),
    Text(String),
    List(Vec<String>),
}

impl Cell {
    fn text(text: Option<&str>) -> Cell {
        text.map_or(Cell::Null, |text| Cell::Text(text.to_owned()))
    }

    fn int(value: Option<i64>) -> Cell {
        value.map_or(Cell::Null, Cell::Int)
    }
}

/// Every column, in the order an export has them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Column {
    TransactionId,
    SplitId,
    LineNo,
    Date,
    Time,
    AccountId,
    AccountName,
    Currency,
    Amount,
    AmountMinor,
    MyAmount,
    MyAmountMinor,
    TransactionAmount,
    TransactionAmountMinor,
    OriginalAmount,
    OriginalAmountMinor,
    OriginalCurrency,
    Merchant,
    MerchantDisplay,
    Description,
    CategoryKey,
    CategoryLabel,
    CategorySource,
    Tags,
    Reimbursable,
    ExcludedReason,
    ExcludedAt,
    TransferGroupId,
    TransferStatus,
    Origin,
    BatchId,
    ExternalId,
    SourceCategory,
    SourceType,
    CreatedAt,
    LinkId,
    LinkKind,
    LinkedTransactionId,
    InReports,
    PersonalAmount,
    PersonalAmountMinor,
    UserNote,
    LineNote,
    ExcludedNote,
    BankNote,
    PrivateTags,
}

impl Column {
    const ALWAYS: [Column; 41] = [
        Column::TransactionId,
        Column::SplitId,
        Column::LineNo,
        Column::Date,
        Column::Time,
        Column::AccountId,
        Column::AccountName,
        Column::Currency,
        Column::Amount,
        Column::AmountMinor,
        Column::MyAmount,
        Column::MyAmountMinor,
        Column::TransactionAmount,
        Column::TransactionAmountMinor,
        Column::OriginalAmount,
        Column::OriginalAmountMinor,
        Column::OriginalCurrency,
        Column::Merchant,
        Column::MerchantDisplay,
        Column::Description,
        Column::CategoryKey,
        Column::CategoryLabel,
        Column::CategorySource,
        Column::Tags,
        Column::Reimbursable,
        Column::ExcludedReason,
        Column::ExcludedAt,
        Column::TransferGroupId,
        Column::TransferStatus,
        Column::Origin,
        Column::BatchId,
        Column::ExternalId,
        Column::SourceCategory,
        Column::SourceType,
        Column::CreatedAt,
        Column::LinkId,
        Column::LinkKind,
        Column::LinkedTransactionId,
        Column::InReports,
        Column::PersonalAmount,
        Column::PersonalAmountMinor,
    ];

    const NOTES: [Column; 4] = [
        Column::UserNote,
        Column::LineNote,
        Column::ExcludedNote,
        Column::BankNote,
    ];

    fn for_options(options: &Options) -> Vec<Column> {
        let mut columns = Column::ALWAYS.to_vec();
        if options.include_notes {
            columns.extend(Column::NOTES);
        }
        if options.include_private_tags {
            columns.push(Column::PrivateTags);
        }
        columns
    }

    const fn name(self) -> &'static str {
        match self {
            Column::TransactionId => "transaction_id",
            Column::SplitId => "split_id",
            Column::LineNo => "line_no",
            Column::Date => "date",
            Column::Time => "time",
            Column::AccountId => "account_id",
            Column::AccountName => "account_name",
            Column::Currency => "currency",
            Column::Amount => "amount",
            Column::AmountMinor => "amount_minor",
            Column::MyAmount => "my_amount",
            Column::MyAmountMinor => "my_amount_minor",
            Column::TransactionAmount => "transaction_amount",
            Column::TransactionAmountMinor => "transaction_amount_minor",
            Column::OriginalAmount => "original_amount",
            Column::OriginalAmountMinor => "original_amount_minor",
            Column::OriginalCurrency => "original_currency",
            Column::Merchant => "merchant",
            Column::MerchantDisplay => "merchant_display",
            Column::Description => "description",
            Column::CategoryKey => "category_key",
            Column::CategoryLabel => "category_label",
            Column::CategorySource => "category_source",
            Column::Tags => "tags",
            Column::Reimbursable => "reimbursable",
            Column::ExcludedReason => "excluded_reason",
            Column::ExcludedAt => "excluded_at",
            Column::TransferGroupId => "transfer_group_id",
            Column::TransferStatus => "transfer_status",
            Column::Origin => "origin",
            Column::BatchId => "batch_id",
            Column::ExternalId => "external_id",
            Column::SourceCategory => "source_category",
            Column::SourceType => "source_type",
            Column::CreatedAt => "created_at",
            Column::LinkId => "link_id",
            Column::LinkKind => "link_kind",
            Column::LinkedTransactionId => "linked_transaction_id",
            Column::InReports => "in_reports",
            Column::PersonalAmount => "personal_amount",
            Column::PersonalAmountMinor => "personal_amount_minor",
            Column::UserNote => "user_note",
            Column::LineNote => "line_note",
            Column::ExcludedNote => "excluded_note",
            Column::BankNote => "bank_note",
            Column::PrivateTags => "private_tags",
        }
    }

    fn cell(self, row: &ExportRow) -> Cell {
        let notes = row.notes.as_ref();
        match self {
            Column::TransactionId => Cell::Int(row.transaction_id.get()),
            Column::SplitId => Cell::int(row.split_id.map(SplitId::get)),
            Column::LineNo => Cell::int(row.line_no),
            Column::Date => Cell::Text(date_key(row.date)),
            Column::Time => Cell::text(row.time.as_deref()),
            Column::AccountId => Cell::Int(row.account_id.get()),
            Column::AccountName => Cell::Text(row.account_name.clone()),
            Column::Currency => Cell::Text(row.amount.currency().to_string()),
            Column::Amount => Cell::Text(row.amount.to_decimal_string()),
            Column::AmountMinor => Cell::Int(row.amount.minor()),
            Column::MyAmount => Cell::Text(row.my_amount.to_decimal_string()),
            Column::MyAmountMinor => Cell::Int(row.my_amount.minor()),
            Column::TransactionAmount => Cell::Text(row.transaction_amount.to_decimal_string()),
            Column::TransactionAmountMinor => Cell::Int(row.transaction_amount.minor()),
            Column::OriginalAmount => Cell::text(
                row.original_amount
                    .map(|m| m.to_decimal_string())
                    .as_deref(),
            ),
            Column::OriginalAmountMinor => Cell::int(row.original_amount.map(|m| m.minor())),
            Column::OriginalCurrency => Cell::text(
                row.original_amount
                    .map(|m| m.currency().to_string())
                    .as_deref(),
            ),
            Column::Merchant => Cell::text(row.merchant.as_deref()),
            Column::MerchantDisplay => Cell::text(row.merchant_display.as_deref()),
            Column::Description => Cell::Text(row.description.clone()),
            Column::CategoryKey => Cell::text(row.category_key.as_ref().map(CategoryKey::as_str)),
            Column::CategoryLabel => Cell::text(row.category_label.as_deref()),
            Column::CategorySource => Cell::text(row.category_source.as_deref()),
            Column::Tags => Cell::List(row.tags.clone()),
            Column::Reimbursable => Cell::Bool(row.reimbursable),
            Column::ExcludedReason => Cell::text(row.excluded_reason.as_deref()),
            Column::ExcludedAt => Cell::text(row.excluded_at.as_deref()),
            Column::TransferGroupId => Cell::int(row.transfer_group_id.map(TransferGroupId::get)),
            Column::TransferStatus => Cell::text(row.transfer_status.as_deref()),
            Column::Origin => Cell::Text(row.origin.clone()),
            Column::BatchId => Cell::int(row.batch_id.map(BatchId::get)),
            Column::ExternalId => Cell::text(row.external_id.as_deref()),
            Column::SourceCategory => Cell::text(row.source_category.as_deref()),
            Column::SourceType => Cell::text(row.source_type.as_deref()),
            Column::CreatedAt => Cell::Text(row.created_at.clone()),
            Column::LinkId => Cell::int(row.link_id.map(LinkId::get)),
            Column::LinkKind => Cell::text(row.link_kind.as_deref()),
            Column::LinkedTransactionId => {
                Cell::int(row.linked_transaction_id.map(TransactionId::get))
            }
            Column::InReports => Cell::Bool(row.in_reports),
            Column::PersonalAmount => Cell::Text(row.personal_amount.to_decimal_string()),
            Column::PersonalAmountMinor => Cell::Int(row.personal_amount.minor()),
            Column::UserNote => Cell::text(notes.and_then(|n| n.user_note.as_deref())),
            Column::LineNote => Cell::text(notes.and_then(|n| n.line_note.as_deref())),
            Column::ExcludedNote => Cell::text(notes.and_then(|n| n.excluded_note.as_deref())),
            Column::BankNote => Cell::text(notes.and_then(|n| n.bank_note.as_deref())),
            Column::PrivateTags => Cell::List(row.private_tags.clone().unwrap_or_default()),
        }
    }
}

/// The filter, with keys and names turned into row IDs.
struct Resolved {
    accounts: Vec<i64>,
    /// Every category a key names, a group's subcategories included.
    categories: Vec<i64>,
    tags: Vec<i64>,
}

impl Resolved {
    fn new(db: &Database, options: &Options) -> Result<Resolved, Error> {
        let filter = &options.filter;
        if let (Some(from), Some(to)) = (filter.from, filter.to)
            && from > to
        {
            return Err(Error::EmptyRange { from, to });
        }
        let mut accounts = Vec::new();
        for &id in &filter.accounts {
            account::find(db.conn(), id)?.ok_or(Error::UnknownAccount(id))?;
            accounts.push(id.get());
        }
        let tree = CategoryTree::load(db.conn())?;
        let mut categories = Vec::new();
        for key in &filter.categories {
            let category = tree
                .get(key)
                .ok_or_else(|| Error::UnknownCategory(key.clone()))?;
            categories.push(category.id());
            if category.is_group()
                && let Some(group) = tree.group_of(key)
            {
                categories.extend(group.children().iter().map(|child| child.id()));
            }
        }
        let mut tags = Vec::new();
        for name in &filter.tags {
            let tag = tag::find(db, name)?.ok_or(Error::UnknownTag)?;
            if tag.private && !options.include_private_tags {
                return Err(Error::PrivateTag);
            }
            tags.push(tag.id.get());
        }
        Ok(Resolved {
            accounts,
            categories,
            tags,
        })
    }
}

/// Every transaction's tags: `(name, private)`, ordered by name.
type TagNames = HashMap<TransactionId, Vec<(String, bool)>>;

fn tag_names(conn: &Connection) -> rusqlite::Result<TagNames> {
    let mut statement = conn.prepare(
        "SELECT tt.transaction_id, tag.name, tag.private FROM transaction_tag tt \
         JOIN tag ON tag.id = tt.tag_id ORDER BY tag.name, tag.id",
    )?;
    let mut names = TagNames::new();
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, TransactionId>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, bool>(2)?,
        ))
    })?;
    for row in rows {
        let (transaction, name, private) = row?;
        names.entry(transaction).or_default().push((name, private));
    }
    Ok(names)
}

fn read_row(row: &Row<'_>, options: &Options, tags: &TagNames) -> Result<ExportRow, Error> {
    let currency: Currency = row.get("currency")?;
    let money =
        |column: &str| -> rusqlite::Result<Money> { Ok(Money::new(row.get(column)?, currency)) };
    let original_minor: Option<i64> = row.get("original_amount_minor")?;
    let original_currency: Option<Currency> = row.get("original_currency")?;
    let transaction_id: TransactionId = row.get("transaction_id")?;
    let own_tags = tags.get(&transaction_id).map(Vec::as_slice).unwrap_or(&[]);
    let names = |private: bool| -> Vec<String> {
        own_tags
            .iter()
            .filter(|(_, is_private)| *is_private == private)
            .map(|(name, _)| name.clone())
            .collect()
    };
    let notes = if options.include_notes {
        Some(Notes {
            user_note: row.get("user_note")?,
            line_note: row.get("line_note")?,
            excluded_note: row.get("excluded_note")?,
            bank_note: row.get("bank_note")?,
        })
    } else {
        None
    };
    Ok(ExportRow {
        transaction_id,
        split_id: row.get("split_id")?,
        line_no: row.get("line_no")?,
        date: account::date(row, "date")?,
        time: row.get("time")?,
        account_id: row.get("account_id")?,
        account_name: row.get("account_name")?,
        amount: money("amount_minor")?,
        my_amount: money("my_amount_minor")?,
        transaction_amount: money("transaction_amount_minor")?,
        original_amount: original_minor
            .zip(original_currency)
            .map(|(minor, currency)| Money::new(minor, currency)),
        merchant: row.get("merchant")?,
        merchant_display: row.get("merchant_display")?,
        description: row.get("description_raw")?,
        category_key: row.get("category_key")?,
        category_label: row.get("category_label")?,
        category_source: row.get("category_source")?,
        tags: names(false),
        reimbursable: row.get("reimbursable")?,
        excluded_reason: row.get("excluded_reason")?,
        excluded_at: row.get("excluded_at")?,
        transfer_group_id: row.get("transfer_group_id")?,
        transfer_status: row.get("transfer_status")?,
        origin: row.get("origin")?,
        batch_id: row.get("batch_id")?,
        external_id: row.get("external_id")?,
        source_category: row.get("source_category")?,
        source_type: row.get("source_type")?,
        created_at: row.get("created_at")?,
        link_id: row.get("link_id")?,
        link_kind: row.get("link_kind")?,
        linked_transaction_id: row.get("target_id")?,
        in_reports: row.get("in_reports")?,
        personal_amount: money("personal_minor")?,
        notes,
        private_tags: options.include_private_tags.then(|| names(true)),
    })
}
