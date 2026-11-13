//! A bank file's contents, normalized: what an importer hands to core.
//!
//! Importers (`coffer-import`) read a file in its own format and produce a
//! [`Statement`]: the file's rows in order, each either a normalized
//! [`StatementLine`] or a [`RowProblem`] saying why it couldn't be read, plus
//! any balances the file states. Deduplication, reconciliation and the commit
//! work on this type, so it lives here rather than in the import crate.
//!
//! A statement isn't tied to an account yet: the user picks the account when
//! importing, and the commit checks that the currencies agree.
//!
//! Nothing in a file is dropped. A row that can't be read stays in the
//! statement as flagged, in its place, so the preview can show it and the
//! user can decide (`AGENTS.md` → Invariants).

use std::fmt;

use chrono::{NaiveDate, NaiveTime};

use crate::category::CategoryKey;

use crate::money::{Currency, Money, ParseAmountError};

/// The file format a statement came from, as `import_batch.format` stores it.
///
/// QFX is OFX. Every planned format is listed, matching the schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SourceFormat {
    Csv,
    Ofx,
    Qif,
    Camt053,
    Mt940,
}

impl SourceFormat {
    /// The key stored in `import_batch.format`.
    pub const fn as_str(self) -> &'static str {
        match self {
            SourceFormat::Csv => "csv",
            SourceFormat::Ofx => "ofx",
            SourceFormat::Qif => "qif",
            SourceFormat::Camt053 => "camt053",
            SourceFormat::Mt940 => "mt940",
        }
    }
}

impl fmt::Display for SourceFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a row sits in its file, so a flagged row can be found again.
///
/// It's a position only, never the row's text: locations end up in errors
/// and logs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SourceLocation {
    /// A 1-based line number in a text file (CSV, QIF).
    Line(u64),
    /// The 1-based position of an entry in a structured file (the n-th
    /// `STMTTRN` in OFX).
    Entry(u64),
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceLocation::Line(n) => write!(f, "line {n}"),
            SourceLocation::Entry(n) => write!(f, "entry {n}"),
        }
    }
}

/// A part of a row that an importer reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    Date,
    Time,
    Amount,
    Currency,
    Merchant,
    Description,
    ExternalId,
    SourceCategory,
    SourceType,
    OriginalAmount,
    OriginalCurrency,
    Note,
    Balance,
}

impl Field {
    pub const fn as_str(self) -> &'static str {
        match self {
            Field::Date => "date",
            Field::Time => "time",
            Field::Amount => "amount",
            Field::Currency => "currency",
            Field::Merchant => "merchant",
            Field::Description => "description",
            Field::ExternalId => "external ID",
            Field::SourceCategory => "source category",
            Field::SourceType => "source type",
            Field::OriginalAmount => "original amount",
            Field::OriginalCurrency => "original currency",
            Field::Note => "note",
            Field::Balance => "balance",
        }
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a row couldn't be normalized. The row is flagged, not skipped.
///
/// Like [`ParseAmountError`], it never contains the row's text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RowProblem {
    #[error("the {0} is missing")]
    Missing(Field),

    #[error("the {field} can't be read: {reason}")]
    Unreadable { field: Field, reason: &'static str },

    #[error("the {field} can't be read: {error}")]
    Amount {
        field: Field,
        #[source]
        error: ParseAmountError,
    },

    #[error("the {field} is in {found}, but the statement is in {expected}")]
    CurrencyMismatch {
        field: Field,
        expected: Currency,
        found: Currency,
    },

    /// A text row split into a different number of columns from the
    /// header, so its fields may have shifted (an unquoted delimiter in a
    /// description). None of it is trusted.
    #[error("the row has {found} columns, but the file has {expected}")]
    ColumnCount { expected: usize, found: usize },

    /// A text row runs over `lines` physical lines (a quoted field with
    /// line breaks) in a file whose rows are one line each, so a stray or
    /// unclosed quote has probably merged the rows after it into this one.
    /// None of it is trusted; the row sits at the line it starts on.
    #[error(
        "the row runs over {lines} lines, but each row should be one line: \
         a quote in it may not be closed"
    )]
    MultiLineRow { lines: u64 },

    /// A quoted field is still open at the end of the file, so everything
    /// from the row's start to the end, `lines` physical lines, was read
    /// into it. Never read as text, whatever the file allows.
    #[error(
        "a quote in the row is never closed, so the row takes in the rest of the file \
         ({lines} {})",
        if *lines == 1 { "line" } else { "lines" }
    )]
    UnclosedQuote { lines: u64 },

    /// The row has the same external ID as an earlier row of the file, so
    /// the ID doesn't identify it and dedupe can't rely on it. The earlier
    /// row is read as usual; this one waits for the user.
    #[error("the external ID is the same as {first}'s")]
    DuplicateExternalId { first: SourceLocation },

    /// The row is something Coffer doesn't import as a transaction, such
    /// as an OFX correction to an earlier transaction.
    #[error("Coffer can't import this row: {reason}")]
    Unsupported { reason: &'static str },

    /// The preset recognises the row as a transfer to or from a sub-account
    /// (a pot), but the column that identifies the sub-account (its ID, or
    /// for Monzo its name) is blank. Without it the pot side can't be
    /// recorded, and half a transfer isn't imported.
    #[error("the row is a {kind} transfer, but doesn't say which {kind}")]
    SubAccountKeyMissing { kind: SubAccountKind },
}

/// One transaction as the file states it, normalized.
///
/// These become the imported source fields of a `transaction` row, which
/// are never edited afterwards. Text fields are kept as the file has them;
/// an importer turns an empty optional field into `None`, and never an
/// empty `external_id` into `Some`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatementLine {
    pub date: NaiveDate,
    pub time: Option<NaiveTime>,
    /// Signed: negative is money out of the account.
    pub amount: Money,
    /// The bank's merchant name, when the file has one apart from the
    /// description (Monzo's "Name").
    pub merchant: Option<String>,
    pub description_raw: String,
    /// The file's own ID for the row (OFX `FITID`, a CSV ID column): the
    /// first dedupe key.
    pub external_id: Option<String>,
    pub source_category: Option<String>,
    pub source_type: Option<String>,
    /// The amount in the currency spent, when it differs (a card used
    /// abroad).
    pub original_amount: Option<Money>,
    /// The note from the file; read-only once imported.
    pub note: Option<String>,
    /// The account's balance after this row, when the file has a running
    /// balance.
    pub balance_after: Option<Money>,
    /// The category the preset's category map gives `source_category`, if
    /// it maps it. Not a source field: the commit stores it as the row's
    /// category with `category_source = 'preset_map'`, and a matching rule
    /// overrides it.
    pub category: Option<CategoryKey>,
    /// The sub-account (pot) at the other end, when the preset recognises
    /// the row as a transfer between the account and one of its pots. Not
    /// a source field: the commit mirrors the row onto the pot
    /// ([`crate::sub_account`]).
    pub sub_account: Option<SubAccountRef>,
}

impl StatementLine {
    /// The fields every line has; the optional ones start empty.
    pub fn new(date: NaiveDate, amount: Money, description_raw: impl Into<String>) -> Self {
        StatementLine {
            date,
            time: None,
            amount,
            merchant: None,
            description_raw: description_raw.into(),
            external_id: None,
            source_category: None,
            source_type: None,
            original_amount: None,
            note: None,
            balance_after: None,
            category: None,
            sub_account: None,
        }
    }

    /// The description as dedupe and rules compare it
    /// (`transaction.description_normalized`).
    pub fn description_normalized(&self) -> String {
        normalize_description(&self.description_raw)
    }
}

/// What kind of sub-account a transfer row names. Only pots so far; Starling
/// Spaces and Revolut Vaults will be pots too, read by their own presets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SubAccountKind {
    /// A Monzo Pot (and the like): an `account` of kind `pot` under the
    /// account the file is imported into.
    Pot,
}

impl SubAccountKind {
    pub const ALL: [SubAccountKind; 1] = [SubAccountKind::Pot];

    /// The key a preset writes (`kind = "pot"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            SubAccountKind::Pot => "pot",
        }
    }

    pub fn from_key(key: &str) -> Option<SubAccountKind> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == key)
    }
}

impl fmt::Display for SubAccountKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The sub-account at the other end of a transfer row: which pot, by its
/// key at the bank.
///
/// `key` is how the pot is found again (`account.sub_account_key`, unique
/// under its parent); `name` is only what a new pot is called. Pots are
/// never matched by name, since the user can rename them.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SubAccountRef {
    pub kind: SubAccountKind,
    /// What identifies the pot at the bank, trimmed and never empty: a
    /// stable ID where the export has one, else its name (Monzo's export
    /// has no pot ID).
    pub key: String,
    /// The pot's name on this row, when the file gives one. It can differ
    /// between rows for the same key (the user renamed the pot).
    pub name: Option<String>,
}

/// Whitespace collapsed to single spaces and trimmed, then uppercased:
/// `"  Tesco  Stores\t1234 "` is `"TESCO STORES 1234"`.
///
/// The fingerprint depends on this, so changing it changes which rows count
/// as already imported.
pub fn normalize_description(raw: &str) -> String {
    let mut normalized = String::with_capacity(raw.len());
    for word in raw.split_whitespace() {
        if !normalized.is_empty() {
            normalized.push(' ');
        }
        normalized.extend(word.chars().flat_map(char::to_uppercase));
    }
    normalized
}

/// A balance the file states for the end of a day (OFX `LEDGERBAL`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DatedBalance {
    pub date: NaiveDate,
    pub balance: Money,
}

/// One row of a statement: read, or flagged with the reason it couldn't be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatementRow {
    pub location: SourceLocation,
    pub outcome: Result<StatementLine, RowProblem>,
}

/// The normalized contents of one file, for one account in one currency.
///
/// Rows keep the file's order: the dedupe fingerprint counts identical rows
/// in order, and running balances only make sense in it. Every amount and
/// balance is in the statement's currency; [`Statement::push`] flags a line
/// that isn't, and [`Statement::push_balance`] refuses such a balance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Statement {
    format: SourceFormat,
    preset_id: Option<String>,
    currency: Currency,
    rows: Vec<StatementRow>,
    balances: Vec<DatedBalance>,
}

impl Statement {
    pub fn new(format: SourceFormat, currency: Currency) -> Self {
        Statement {
            format,
            preset_id: None,
            currency,
            rows: Vec::new(),
            balances: Vec::new(),
        }
    }

    /// Records the CSV preset that read the file (`import_batch.preset_id`).
    pub fn with_preset(mut self, preset_id: impl Into<String>) -> Self {
        self.preset_id = Some(preset_id.into());
        self
    }

    pub fn format(&self) -> SourceFormat {
        self.format
    }

    pub fn preset_id(&self) -> Option<&str> {
        self.preset_id.as_deref()
    }

    pub fn currency(&self) -> Currency {
        self.currency
    }

    /// Appends the next row. A line whose amount or running balance is in
    /// another currency is flagged instead.
    pub fn push(&mut self, location: SourceLocation, outcome: Result<StatementLine, RowProblem>) {
        let outcome = outcome.and_then(|line| self.check_line(line));
        self.rows.push(StatementRow { location, outcome });
    }

    fn check_line(&self, line: StatementLine) -> Result<StatementLine, RowProblem> {
        let amounts = [
            (Field::Amount, Some(line.amount)),
            (Field::Balance, line.balance_after),
        ];
        for (field, amount) in amounts {
            if let Some(amount) = amount
                && amount.currency() != self.currency
            {
                return Err(RowProblem::CurrencyMismatch {
                    field,
                    expected: self.currency,
                    found: amount.currency(),
                });
            }
        }
        Ok(line)
    }

    /// Adds a balance the file states. One in another currency is an error:
    /// it says the file isn't what the importer thinks it is.
    pub fn push_balance(&mut self, balance: DatedBalance) -> Result<(), BalanceCurrencyMismatch> {
        if balance.balance.currency() != self.currency {
            return Err(BalanceCurrencyMismatch {
                expected: self.currency,
                found: balance.balance.currency(),
            });
        }
        self.balances.push(balance);
        Ok(())
    }

    /// Every row in file order, read or flagged.
    pub fn rows(&self) -> &[StatementRow] {
        &self.rows
    }

    /// The rows that were read.
    pub fn lines(&self) -> impl Iterator<Item = (SourceLocation, &StatementLine)> {
        self.rows
            .iter()
            .filter_map(|row| row.outcome.as_ref().ok().map(|line| (row.location, line)))
    }

    /// The rows that couldn't be read, with the reason.
    pub fn flagged(&self) -> impl Iterator<Item = (SourceLocation, &RowProblem)> {
        self.rows.iter().filter_map(|row| {
            row.outcome
                .as_ref()
                .err()
                .map(|problem| (row.location, problem))
        })
    }

    /// Whether any row is flagged. A statement with flagged rows isn't
    /// committed without the user seeing them.
    pub fn has_flagged(&self) -> bool {
        self.flagged().next().is_some()
    }

    /// Balances the file states apart from running balances on rows.
    pub fn balances(&self) -> &[DatedBalance] {
        &self.balances
    }
}

/// A statement-level balance in a different currency from the statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the file states a balance in {found}, but its transactions are in {expected}")]
pub struct BalanceCurrencyMismatch {
    pub expected: Currency,
    pub found: Currency,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn gbp(minor: i64) -> Money {
        Money::new(minor, Currency::GBP)
    }

    fn eur(minor: i64) -> Money {
        Money::new(minor, Currency::EUR)
    }

    #[test]
    fn source_format_keys_match_the_schema() {
        let keys: Vec<_> = [
            SourceFormat::Csv,
            SourceFormat::Ofx,
            SourceFormat::Qif,
            SourceFormat::Camt053,
            SourceFormat::Mt940,
        ]
        .map(SourceFormat::as_str)
        .into();
        assert_eq!(keys, ["csv", "ofx", "qif", "camt053", "mt940"]);
        assert_eq!(SourceFormat::Ofx.to_string(), "ofx");
    }

    #[test]
    fn description_is_collapsed_trimmed_and_uppercased() {
        assert_eq!(
            normalize_description("  Tesco  Stores\t1234 \n"),
            "TESCO STORES 1234"
        );
        assert_eq!(normalize_description("café\u{A0}Straße"), "CAFÉ STRASSE");
        assert_eq!(normalize_description(""), "");
        assert_eq!(normalize_description(" \t "), "");

        let line = StatementLine::new(date(2026, 11, 2), gbp(-1240), "card  payment");
        assert_eq!(line.description_normalized(), "CARD PAYMENT");
    }

    #[test]
    fn rows_keep_file_order_and_flagged_rows_stay() {
        let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
        statement.push(
            SourceLocation::Line(2),
            Ok(StatementLine::new(date(2026, 11, 1), gbp(-500), "A")),
        );
        statement.push(
            SourceLocation::Line(3),
            Err(RowProblem::Missing(Field::Date)),
        );
        statement.push(
            SourceLocation::Line(4),
            Ok(StatementLine::new(date(2026, 11, 2), gbp(1000), "B")),
        );

        let locations: Vec<_> = statement.rows().iter().map(|row| row.location).collect();
        assert_eq!(
            locations,
            [2, 3, 4].map(SourceLocation::Line),
            "every row stays, in order"
        );
        let read: Vec<_> = statement.lines().map(|(at, _)| at).collect();
        assert_eq!(read, [SourceLocation::Line(2), SourceLocation::Line(4)]);
        let flagged: Vec<_> = statement.flagged().collect();
        assert_eq!(
            flagged,
            [(SourceLocation::Line(3), &RowProblem::Missing(Field::Date))]
        );
        assert!(statement.has_flagged());
    }

    #[test]
    fn a_line_in_another_currency_is_flagged() {
        let mut statement = Statement::new(SourceFormat::Ofx, Currency::GBP);
        statement.push(
            SourceLocation::Entry(1),
            Ok(StatementLine::new(date(2026, 11, 1), eur(-500), "A")),
        );
        let mut line = StatementLine::new(date(2026, 11, 1), gbp(-500), "B");
        line.balance_after = Some(eur(10_000));
        statement.push(SourceLocation::Entry(2), Ok(line));

        let problems: Vec<_> = statement
            .flagged()
            .map(|(_, problem)| problem.clone())
            .collect();
        assert_eq!(
            problems,
            [Field::Amount, Field::Balance].map(|field| RowProblem::CurrencyMismatch {
                field,
                expected: Currency::GBP,
                found: Currency::EUR,
            })
        );
        assert_eq!(statement.lines().count(), 0);
    }

    #[test]
    fn original_amount_may_be_in_another_currency() {
        let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
        let mut line = StatementLine::new(date(2026, 11, 1), gbp(-860), "Café");
        line.original_amount = Some(eur(-1000));
        statement.push(SourceLocation::Line(2), Ok(line));
        assert!(!statement.has_flagged());
    }

    #[test]
    fn balances_must_be_in_the_statement_currency() {
        let mut statement = Statement::new(SourceFormat::Ofx, Currency::GBP);
        let closing = DatedBalance {
            date: date(2026, 11, 30),
            balance: gbp(12_345),
        };
        statement.push_balance(closing).unwrap();
        assert_eq!(statement.balances(), [closing]);

        let wrong = DatedBalance {
            date: date(2026, 11, 30),
            balance: eur(1),
        };
        assert_eq!(
            statement.push_balance(wrong),
            Err(BalanceCurrencyMismatch {
                expected: Currency::GBP,
                found: Currency::EUR,
            })
        );
        assert_eq!(statement.balances().len(), 1);
    }

    #[test]
    fn preset_is_recorded() {
        let statement =
            Statement::new(SourceFormat::Csv, Currency::GBP).with_preset("uk-monzo-current");
        assert_eq!(statement.preset_id(), Some("uk-monzo-current"));
        assert_eq!(statement.format(), SourceFormat::Csv);
        assert_eq!(statement.currency(), Currency::GBP);
    }

    #[test]
    fn problems_name_the_field_but_not_the_text() {
        let problem = RowProblem::Unreadable {
            field: Field::Date,
            reason: "doesn't match the date format",
        };
        assert_eq!(
            problem.to_string(),
            "the date can't be read: doesn't match the date format"
        );
        assert_eq!(
            RowProblem::ColumnCount {
                expected: 9,
                found: 10
            }
            .to_string(),
            "the row has 10 columns, but the file has 9"
        );
        assert_eq!(
            RowProblem::MultiLineRow { lines: 3 }.to_string(),
            "the row runs over 3 lines, but each row should be one line: \
             a quote in it may not be closed"
        );
        assert_eq!(
            RowProblem::UnclosedQuote { lines: 1 }.to_string(),
            "a quote in the row is never closed, so the row takes in the rest of the file (1 line)"
        );
        assert_eq!(
            RowProblem::UnclosedQuote { lines: 4 }.to_string(),
            "a quote in the row is never closed, so the row takes in the rest of the file (4 lines)"
        );
        assert_eq!(
            RowProblem::DuplicateExternalId {
                first: SourceLocation::Entry(2)
            }
            .to_string(),
            "the external ID is the same as entry 2's"
        );
        assert_eq!(
            RowProblem::Unsupported {
                reason: "it corrects an earlier transaction"
            }
            .to_string(),
            "Coffer can't import this row: it corrects an earlier transaction"
        );
        assert_eq!(SourceLocation::Line(14).to_string(), "line 14");
        assert_eq!(SourceLocation::Entry(3).to_string(), "entry 3");
    }
}
