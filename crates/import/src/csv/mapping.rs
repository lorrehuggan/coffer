//! How a CSV file's columns map onto a statement line.

use std::fmt;

use coffer_core::money::{Currency, DecimalMark};
use coffer_core::statement::SubAccountKind;

use super::dates::{DateFormat, TimeFormat};
use super::encoding::TextEncoding;

/// A column, by its header name or by position.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Column {
    /// The header cell's text, matched exactly after trimming whitespace.
    Named(String),
    /// The 0-based position, for files without a header row.
    Index(usize),
}

impl Column {
    pub fn named(name: impl Into<String>) -> Column {
        Column::Named(name.into())
    }
}

/// `“Date” column` or `column 3` (1-based, as a user counts).
impl fmt::Display for Column {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Column::Named(name) => write!(f, "“{name}” column"),
            Column::Index(index) => write!(f, "column {}", index + 1),
        }
    }
}

/// The character between columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Delimiter {
    Comma,
    Semicolon,
    Tab,
    Pipe,
}

impl Delimiter {
    /// In the order [`CsvMapping::delimiter`] detection prefers them on a
    /// tie, which it then refuses anyway.
    pub const ALL: [Delimiter; 4] = [
        Delimiter::Comma,
        Delimiter::Semicolon,
        Delimiter::Tab,
        Delimiter::Pipe,
    ];

    pub fn byte(self) -> u8 {
        match self {
            Delimiter::Comma => b',',
            Delimiter::Semicolon => b';',
            Delimiter::Tab => b'\t',
            Delimiter::Pipe => b'|',
        }
    }
}

/// Where the transaction amount comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AmountColumns {
    /// One column with the sign: negative is money out. Preferred when a
    /// file has both forms (Monzo puts some refunds in "Money Out" as
    /// positive values).
    Signed(Column),
    /// A money-out and a money-in column; a row fills one of them, or both
    /// with one of them zero.
    Split {
        debit: Column,
        credit: Column,
        signs: SplitSigns,
    },
}

/// How a debit/credit pair writes its amounts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SplitSigns {
    /// Both columns hold positive amounts; money out is negated. A negative
    /// value in either column is flagged, since what it means is unclear.
    #[default]
    Unsigned,
    /// Each column already carries its sign (money out is negative), and
    /// the value is taken as written.
    Signed,
}

/// A date column and its format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DateColumn {
    pub column: Column,
    pub format: DateFormat,
}

/// A separate time-of-day column and its format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeColumn {
    pub column: Column,
    pub format: TimeFormat,
}

/// The amount in the currency spent (a card used abroad), and that
/// currency's code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginalAmountColumns {
    pub amount: Column,
    pub currency: Column,
}

/// How an export marks a transfer between the account and one of its
/// sub-accounts (a Monzo Pot), and which columns say which sub-account.
///
/// A row is such a transfer when its `match_column`, trimmed, is exactly
/// `match_value` (case included). Its line then carries a
/// [`SubAccountRef`](coffer_core::statement::SubAccountRef) with the
/// trimmed `key` and `name`; a matching row with a blank key is flagged
/// ([`RowProblem::SubAccountKeyMissing`]). Other rows are read as usual.
///
/// [`RowProblem::SubAccountKeyMissing`]: coffer_core::statement::RowProblem::SubAccountKeyMissing
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubAccountTransfer {
    pub kind: SubAccountKind,
    /// The column that says a row is a sub-account transfer (Monzo's
    /// `Type`).
    pub match_column: Column,
    /// Its value on such a row (`Pot transfer`): not empty, and without
    /// spaces at either end.
    pub match_value: String,
    /// What identifies the sub-account at the bank, which finds the pot
    /// again on every import: a stable ID where the export has one, else
    /// its name (Monzo's `Name`; its export has no pot ID).
    pub key: Column,
    /// The sub-account's name (Monzo's `Name`), for a pot the import
    /// creates. Optional.
    pub name: Option<Column>,
}

/// How a CSV file is split into rows, before any column is chosen: what a
/// mapping screen needs to read a file it has no mapping for yet
/// ([`CsvFile::read`](super::CsvFile::read)). [`CsvMapping::layout`] gives
/// a mapping's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileLayout {
    pub encoding: TextEncoding,
    /// `None` detects it from the file.
    pub delimiter: Option<Delimiter>,
    pub skip_rows: usize,
    pub has_header: bool,
    pub multiline_fields: bool,
}

/// Automatic encoding, a detected delimiter, nothing skipped, a header row,
/// one line per row: what [`CsvMapping::new`] starts with.
impl Default for FileLayout {
    fn default() -> FileLayout {
        FileLayout {
            encoding: TextEncoding::Auto,
            delimiter: None,
            skip_rows: 0,
            has_header: true,
            multiline_fields: false,
        }
    }
}

/// How to read one kind of CSV export: the shape a preset
/// ([`crate::preset`]) or the mapping screen produces.
///
/// [`CsvMapping::new`] gives the required parts and sensible defaults for
/// the rest; set the other fields directly. [`CsvImporter::new`] checks the
/// whole mapping.
///
/// [`CsvImporter::new`]: super::CsvImporter::new
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CsvMapping {
    /// The account's currency: every amount is read in it unless
    /// [`currency_column`](Self::currency_column) is set.
    pub currency: Currency,
    pub encoding: TextEncoding,
    /// `None` detects it from the file.
    pub delimiter: Option<Delimiter>,
    /// Lines before the header (or before the first row, without one), such
    /// as an account summary.
    pub skip_rows: usize,
    /// Whether the first line after `skip_rows` names the columns. Without
    /// a header, every column must be a [`Column::Index`].
    pub has_header: bool,
    /// Whether the export really has line breaks inside quoted fields (a
    /// note over several lines). Off by default: a row over several lines
    /// is then flagged ([`RowProblem::MultiLineRow`]), since it's usually
    /// a stray or unclosed quote that has merged the rows after it. On or
    /// off, a quote still open at the end of the file is flagged
    /// ([`RowProblem::UnclosedQuote`]).
    ///
    /// [`RowProblem::MultiLineRow`]: coffer_core::statement::RowProblem::MultiLineRow
    /// [`RowProblem::UnclosedQuote`]: coffer_core::statement::RowProblem::UnclosedQuote
    pub multiline_fields: bool,
    pub date: DateColumn,
    /// When filled in, overrides any time in the date format.
    pub time: Option<TimeColumn>,
    pub amount: AmountColumns,
    pub decimal: DecimalMark,
    /// A per-row currency code. Amounts are read in it; a row not in
    /// [`currency`](Self::currency) is flagged.
    pub currency_column: Option<Column>,
    /// The bank's merchant name, apart from the description.
    pub merchant: Option<Column>,
    /// The raw description: one column, or several joined with a space
    /// (empty ones left out).
    pub description: Vec<Column>,
    /// The file's own ID for each row, the first dedupe key.
    pub external_id: Option<Column>,
    pub source_category: Option<Column>,
    pub source_type: Option<Column>,
    pub original_amount: Option<OriginalAmountColumns>,
    pub note: Option<Column>,
    /// A running balance after each row.
    pub balance: Option<Column>,
    /// Rows that are transfers to or from a sub-account (a pot), if the
    /// export has them.
    pub sub_account_transfer: Option<SubAccountTransfer>,
}

impl CsvMapping {
    /// A mapping with a header row, a detected delimiter, automatic
    /// encoding, one line per row, `.` as the decimal mark and no optional
    /// columns.
    pub fn new(
        currency: Currency,
        date: DateColumn,
        amount: AmountColumns,
        description: Column,
    ) -> CsvMapping {
        CsvMapping {
            currency,
            encoding: TextEncoding::Auto,
            delimiter: None,
            skip_rows: 0,
            has_header: true,
            multiline_fields: false,
            date,
            time: None,
            amount,
            decimal: DecimalMark::Point,
            currency_column: None,
            merchant: None,
            description: vec![description],
            external_id: None,
            source_category: None,
            source_type: None,
            original_amount: None,
            note: None,
            balance: None,
            sub_account_transfer: None,
        }
    }

    /// How the file is split into rows.
    pub fn layout(&self) -> FileLayout {
        FileLayout {
            encoding: self.encoding,
            delimiter: self.delimiter,
            skip_rows: self.skip_rows,
            has_header: self.has_header,
            multiline_fields: self.multiline_fields,
        }
    }

    /// Sets how the file is split into rows.
    pub fn set_layout(&mut self, layout: FileLayout) {
        self.encoding = layout.encoding;
        self.delimiter = layout.delimiter;
        self.skip_rows = layout.skip_rows;
        self.has_header = layout.has_header;
        self.multiline_fields = layout.multiline_fields;
    }

    /// Every column the mapping reads.
    pub(crate) fn columns(&self) -> impl Iterator<Item = &Column> {
        let amount: Vec<&Column> = match &self.amount {
            AmountColumns::Signed(column) => vec![column],
            AmountColumns::Split { debit, credit, .. } => vec![debit, credit],
        };
        let original = self
            .original_amount
            .iter()
            .flat_map(|original| [&original.amount, &original.currency]);
        let sub_account = self.sub_account_transfer.iter().flat_map(|transfer| {
            [&transfer.match_column, &transfer.key]
                .into_iter()
                .chain(&transfer.name)
        });
        std::iter::once(&self.date.column)
            .chain(self.time.as_ref().map(|time| &time.column))
            .chain(amount)
            .chain(&self.currency_column)
            .chain(&self.merchant)
            .chain(&self.description)
            .chain(&self.external_id)
            .chain(&self.source_category)
            .chain(&self.source_type)
            .chain(original)
            .chain(&self.note)
            .chain(&self.balance)
            .chain(sub_account)
    }

    pub(super) fn validate(&self) -> Result<(), MappingError> {
        if self.description.is_empty() {
            return Err(MappingError::NoDescription);
        }
        if let AmountColumns::Split { debit, credit, .. } = &self.amount
            && debit == credit
        {
            return Err(MappingError::SameDebitAndCredit);
        }
        if let Some(transfer) = &self.sub_account_transfer
            && (transfer.match_value.is_empty()
                || transfer.match_value.trim() != transfer.match_value)
        {
            return Err(MappingError::SubAccountMatchValue(
                transfer.match_value.clone(),
            ));
        }
        if !self.has_header
            && let Some(Column::Named(name)) = self
                .columns()
                .find(|column| matches!(column, Column::Named(_)))
        {
            return Err(MappingError::NamedColumnWithoutHeader(name.clone()));
        }
        Ok(())
    }
}

/// A mapping that can't be used, whatever the file.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MappingError {
    #[error("the mapping needs at least one description column")]
    NoDescription,

    #[error("the money-out and money-in columns are the same column")]
    SameDebitAndCredit,

    #[error("the column “{0}” is named, but the file has no header row to find it in")]
    NamedColumnWithoutHeader(String),

    #[error(
        "the value that marks a sub-account transfer is empty or starts or ends with a space: {0:?}"
    )]
    SubAccountMatchValue(String),
}
