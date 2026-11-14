//! The mapping screen's working copy of a mapping (M3.4).
//!
//! A [`MappingDraft`] is a [`CsvMapping`] whose required columns may still
//! be unchosen: the user picks the date, amount and description columns
//! (and the optional ones) on the Map step, and [`MappingDraft::build`]
//! turns the choices into a mapping once they're complete, saying which
//! one is missing otherwise.
//!
//! [`MappingDraft::suggest`] fills a draft in from a file the presets don't
//! recognise: columns by their header names (`Date`, `Paid out`, …) or,
//! without a match, by their values, and the date format from the date
//! column's values. It's deterministic and only a starting point: the user
//! sees every choice and the live preview before anything is imported. A
//! date format that reads the values in more than one way (`01/02/2026`)
//! is left unchosen, never guessed.

use coffer_core::money::{Currency, DecimalMark, Money};

use super::dates::{DateFormat, TimeFormat};
use super::mapping::{
    AmountColumns, Column, CsvMapping, DateColumn, FileLayout, MappingError, OriginalAmountColumns,
    SplitSigns, SubAccountTransfer, TimeColumn,
};
use super::reader::CsvFile;

/// Where the amount comes from, while it's being chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AmountDraft {
    /// One signed column (negative is money out).
    Signed(Option<Column>),
    /// A money-out and a money-in column.
    Split {
        debit: Option<Column>,
        credit: Option<Column>,
        signs: SplitSigns,
    },
}

/// A choice [`MappingDraft::build`] still needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Needed {
    DateColumn,
    DateFormat,
    TimeFormat,
    Amount,
    MoneyOut,
    MoneyIn,
    Description,
}

impl Needed {
    /// What to choose, for "Choose {the date column}".
    pub const fn as_str(self) -> &'static str {
        match self {
            Needed::DateColumn => "the date column",
            Needed::DateFormat => "the date format",
            Needed::TimeFormat => "the time format",
            Needed::Amount => "the amount column",
            Needed::MoneyOut => "the money-out column",
            Needed::MoneyIn => "the money-in column",
            Needed::Description => "the details column",
        }
    }
}

/// Why a draft isn't a mapping yet.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DraftError {
    #[error("choose {}", .0.as_str())]
    Needed(Needed),
    #[error(transparent)]
    Mapping(#[from] MappingError),
}

/// A [`CsvMapping`] being chosen on the mapping screen.
///
/// The screen shows the layout, the decimal mark, the date and its format,
/// the time, the amount, the merchant, the details (description), the ID
/// and the balance. The other fields come from the preset the draft
/// started from ([`MappingDraft::from_mapping`]) and are kept as they are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MappingDraft {
    pub layout: FileLayout,
    pub decimal: DecimalMark,
    pub date: Option<Column>,
    pub date_format: Option<DateFormat>,
    pub time: Option<Column>,
    pub time_format: Option<TimeFormat>,
    pub amount: AmountDraft,
    pub merchant: Option<Column>,
    /// The details: one column, or several joined with a space.
    pub description: Vec<Column>,
    pub external_id: Option<Column>,
    pub balance: Option<Column>,
    pub currency_column: Option<Column>,
    pub source_category: Option<Column>,
    pub source_type: Option<Column>,
    pub original_amount: Option<OriginalAmountColumns>,
    pub note: Option<Column>,
    pub sub_account_transfer: Option<SubAccountTransfer>,
}

impl MappingDraft {
    /// Nothing chosen yet, read as `layout` says.
    pub fn new(layout: FileLayout) -> MappingDraft {
        MappingDraft {
            layout,
            decimal: DecimalMark::Point,
            date: None,
            date_format: None,
            time: None,
            time_format: None,
            amount: AmountDraft::Signed(None),
            merchant: None,
            description: Vec::new(),
            external_id: None,
            balance: None,
            currency_column: None,
            source_category: None,
            source_type: None,
            original_amount: None,
            note: None,
            sub_account_transfer: None,
        }
    }

    /// Every choice `mapping` makes, to start from a preset.
    pub fn from_mapping(mapping: &CsvMapping) -> MappingDraft {
        MappingDraft {
            layout: mapping.layout(),
            decimal: mapping.decimal,
            date: Some(mapping.date.column.clone()),
            date_format: Some(mapping.date.format.clone()),
            time: mapping.time.as_ref().map(|time| time.column.clone()),
            time_format: mapping.time.as_ref().map(|time| time.format.clone()),
            amount: match &mapping.amount {
                AmountColumns::Signed(column) => AmountDraft::Signed(Some(column.clone())),
                AmountColumns::Split {
                    debit,
                    credit,
                    signs,
                } => AmountDraft::Split {
                    debit: Some(debit.clone()),
                    credit: Some(credit.clone()),
                    signs: *signs,
                },
            },
            merchant: mapping.merchant.clone(),
            description: mapping.description.clone(),
            external_id: mapping.external_id.clone(),
            balance: mapping.balance.clone(),
            currency_column: mapping.currency_column.clone(),
            source_category: mapping.source_category.clone(),
            source_type: mapping.source_type.clone(),
            original_amount: mapping.original_amount.clone(),
            note: mapping.note.clone(),
            sub_account_transfer: mapping.sub_account_transfer.clone(),
        }
    }

    /// The mapping, reading amounts in `currency` (the account's), once
    /// every required choice is made. The first missing choice, in screen
    /// order, is the error.
    pub fn build(&self, currency: Currency) -> Result<CsvMapping, DraftError> {
        let need = |needed| DraftError::Needed(needed);
        let date = self.date.clone().ok_or(need(Needed::DateColumn))?;
        let format = self.date_format.clone().ok_or(need(Needed::DateFormat))?;
        let time = match (&self.time, &self.time_format) {
            (None, _) => None,
            (Some(column), Some(format)) => Some(TimeColumn {
                column: column.clone(),
                format: format.clone(),
            }),
            (Some(_), None) => return Err(need(Needed::TimeFormat)),
        };
        let amount = match &self.amount {
            AmountDraft::Signed(column) => {
                AmountColumns::Signed(column.clone().ok_or(need(Needed::Amount))?)
            }
            AmountDraft::Split {
                debit,
                credit,
                signs,
            } => AmountColumns::Split {
                debit: debit.clone().ok_or(need(Needed::MoneyOut))?,
                credit: credit.clone().ok_or(need(Needed::MoneyIn))?,
                signs: *signs,
            },
        };
        if self.description.is_empty() {
            return Err(need(Needed::Description));
        }

        let mut mapping = CsvMapping::new(
            currency,
            DateColumn {
                column: date,
                format,
            },
            amount,
            self.description[0].clone(),
        );
        mapping.set_layout(self.layout.clone());
        mapping.description = self.description.clone();
        mapping.time = time;
        mapping.decimal = self.decimal;
        mapping.merchant = self.merchant.clone();
        mapping.external_id = self.external_id.clone();
        mapping.balance = self.balance.clone();
        mapping.currency_column = self.currency_column.clone();
        mapping.source_category = self.source_category.clone();
        mapping.source_type = self.source_type.clone();
        mapping.original_amount = self.original_amount.clone();
        mapping.note = self.note.clone();
        mapping.sub_account_transfer = self.sub_account_transfer.clone();
        mapping.validate()?;
        Ok(mapping)
    }

    /// A draft for `file` (read with `layout`) from its header names, or,
    /// where no name fits, from its values; see the module docs.
    pub fn suggest(file: &CsvFile, layout: FileLayout) -> MappingDraft {
        let mut draft = MappingDraft::new(layout);
        let columns = columns(file);
        let named = |names: &[&str]| -> Option<Column> {
            columns
                .iter()
                .find(|column| match column {
                    Column::Named(name) => names.contains(&simplify(name).as_str()),
                    Column::Index(_) => false,
                })
                .cloned()
        };

        draft.date = named(DATE_NAMES).or_else(|| {
            columns
                .iter()
                .find(|column| !date_formats(file, column).is_empty())
                .cloned()
        });
        if let Some(date) = draft.date.clone() {
            draft.set_date(file, Some(date));
        }
        draft.time = named(TIME_NAMES).filter(|time| Some(time) != draft.date.as_ref());
        if let Some(time) = draft.time.clone() {
            draft.set_time(file, Some(time));
        }

        let decimal = suggest_decimal(file, &columns);
        draft.decimal = decimal;
        let reads_amounts = |column: &&Column| {
            let values = values(file, column);
            values.iter().any(|value| !value.trim().is_empty())
                && values.iter().all(|value| reads_as_amount(value, decimal))
        };
        draft.amount = match (named(DEBIT_NAMES), named(CREDIT_NAMES), named(AMOUNT_NAMES)) {
            (_, _, Some(amount)) => AmountDraft::Signed(Some(amount)),
            (Some(debit), Some(credit), None) => AmountDraft::Split {
                signs: split_signs(file, &debit, decimal),
                debit: Some(debit),
                credit: Some(credit),
            },
            _ => AmountDraft::Signed(
                columns
                    .iter()
                    .filter(|column| Some(*column) != draft.date.as_ref())
                    .find(reads_amounts)
                    .cloned(),
            ),
        };

        draft.merchant = named(MERCHANT_NAMES);
        draft.description = named(DESCRIPTION_NAMES)
            .or_else(|| draft.merchant.clone())
            .into_iter()
            .collect();
        draft.external_id = named(ID_NAMES);
        draft.balance = named(BALANCE_NAMES);
        draft
    }

    /// Chooses the date column, and its format when the column's values
    /// say which: the current format if it still reads them, else the only
    /// one that does. Several that read them differently leave it for the
    /// user.
    pub fn set_date(&mut self, file: &CsvFile, column: Option<Column>) {
        self.date = column;
        let formats = match &self.date {
            Some(column) => date_formats(file, column),
            None => Vec::new(),
        };
        let keep = self
            .date_format
            .as_ref()
            .is_some_and(|current| formats.contains(current));
        if !keep {
            self.date_format = match formats.as_slice() {
                [only] => Some(only.clone()),
                _ => None,
            };
        }
    }

    /// Chooses the time column, and its format as [`set_date`](Self::set_date)
    /// does.
    pub fn set_time(&mut self, file: &CsvFile, column: Option<Column>) {
        self.time = column;
        let formats = match &self.time {
            Some(column) => time_formats(file, column),
            None => Vec::new(),
        };
        let keep = self
            .time_format
            .as_ref()
            .is_some_and(|current| formats.contains(current));
        if !keep {
            self.time_format = formats.into_iter().next();
        }
    }

    /// Every column the draft reads, for marking columns as in use.
    pub fn columns(&self) -> Vec<&Column> {
        let amount: Vec<&Column> = match &self.amount {
            AmountDraft::Signed(column) => column.iter().collect(),
            AmountDraft::Split { debit, credit, .. } => debit.iter().chain(credit).collect(),
        };
        self.date
            .iter()
            .chain(&self.time)
            .chain(amount)
            .chain(&self.merchant)
            .chain(&self.description)
            .chain(&self.external_id)
            .chain(&self.balance)
            .chain(&self.currency_column)
            .chain(&self.source_category)
            .chain(&self.source_type)
            .chain(
                self.original_amount
                    .iter()
                    .flat_map(|original| [&original.amount, &original.currency]),
            )
            .chain(&self.note)
            .chain(self.sub_account_transfer.iter().flat_map(|transfer| {
                [&transfer.match_column, &transfer.key]
                    .into_iter()
                    .chain(&transfer.name)
            }))
            .collect()
    }
}

/// The columns a file offers: its header cells by name (blank and repeated
/// names left out, since they can't be found by name), or positions
/// without a header.
pub fn columns(file: &CsvFile) -> Vec<Column> {
    match file.header() {
        Some(header) => {
            let cells = &header.fields[..file.width().min(header.fields.len())];
            cells
                .iter()
                .enumerate()
                .filter(|(index, cell)| {
                    !cell.is_empty() && !cells[..*index].contains(cell) && {
                        let later = &cells[index + 1..];
                        !later.contains(cell)
                    }
                })
                .map(|(_, cell)| Column::named(cell.as_str()))
                .collect()
        }
        None => (0..file.width()).map(Column::Index).collect(),
    }
}

/// The values in `column`, from rows whose split can be trusted. Empty when
/// the file has no such column.
pub fn values<'a>(file: &'a CsvFile, column: &Column) -> Vec<&'a str> {
    let Some(index) = position(file, column) else {
        return Vec::new();
    };
    file.records()
        .iter()
        .filter(|record| record.structure_problem(true).is_none())
        .filter_map(|record| record.fields.get(index).map(String::as_str))
        .collect()
}

/// Where `column` is in `file`'s rows.
fn position(file: &CsvFile, column: &Column) -> Option<usize> {
    match column {
        Column::Index(index) => (*index < file.width()).then_some(*index),
        Column::Named(name) => file
            .header()?
            .fields
            .iter()
            .position(|cell| cell == name.trim()),
    }
}

/// The date formats that read every value in `column`, most common first,
/// leaving out any that reads every value as the same dates as one before
/// it (`%d/%m/%Y` and `%-d/%-m/%Y` on days from the 10th). More than one
/// left means the values are ambiguous.
pub fn date_formats(file: &CsvFile, column: &Column) -> Vec<DateFormat> {
    let values = values(file, column);
    let mut kept: Vec<(DateFormat, Vec<String>)> = Vec::new();
    for format in DateFormat::candidates(values.iter().copied()) {
        let read: Vec<String> = values
            .iter()
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!("{:?}", format.parse(value)))
            .collect();
        if !kept.iter().any(|(_, earlier)| *earlier == read) {
            kept.push((format, read));
        }
    }
    kept.into_iter().map(|(format, _)| format).collect()
}

/// The time formats that read every value in `column`, most common first.
pub fn time_formats(file: &CsvFile, column: &Column) -> Vec<TimeFormat> {
    TimeFormat::candidates(values(file, column))
}

// Suggesting ---------------------------------------------------------------

const DATE_NAMES: &[&str] = &[
    "date",
    "transaction date",
    "posting date",
    "posted date",
    "booking date",
    "completed date",
    "trans date",
    "value date",
];
const TIME_NAMES: &[&str] = &["time", "transaction time"];
const AMOUNT_NAMES: &[&str] = &["amount", "transaction amount", "value"];
const DEBIT_NAMES: &[&str] = &[
    "paid out",
    "money out",
    "debit",
    "debit amount",
    "debits",
    "withdrawal",
    "withdrawals",
    "out",
];
const CREDIT_NAMES: &[&str] = &[
    "paid in",
    "money in",
    "credit",
    "credit amount",
    "credits",
    "deposit",
    "deposits",
    "in",
];
const DESCRIPTION_NAMES: &[&str] = &[
    "description",
    "details",
    "transaction description",
    "transaction details",
    "narrative",
    "memo",
    "reference",
];
const MERCHANT_NAMES: &[&str] = &["name", "merchant", "merchant name", "payee", "counterparty"];
const ID_NAMES: &[&str] = &["transaction id", "id", "transaction reference", "fitid"];
const BALANCE_NAMES: &[&str] = &["balance", "running balance"];

/// A header name lowercased, with anything but letters and digits as one
/// space: `Paid Out (£)` → `paid out`.
fn simplify(name: &str) -> String {
    let mut out = String::new();
    for word in name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
    {
        // Currency codes and symbols after the name say nothing about it.
        if matches!(word.to_lowercase().as_str(), "gbp" | "usd" | "eur") {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&word.to_lowercase());
    }
    out
}

/// Whether `text` is blank or an amount with `decimal`, as two-decimal
/// money: only to tell amount columns apart.
fn reads_as_amount(text: &str, decimal: DecimalMark) -> bool {
    text.trim().is_empty() || Money::parse_decimal(text, Currency::GBP, decimal).is_ok()
}

/// The decimal comma when more of the file's values read with it than with
/// the point; otherwise the point.
fn suggest_decimal(file: &CsvFile, columns: &[Column]) -> DecimalMark {
    let count = |decimal| {
        columns
            .iter()
            .flat_map(|column| values(file, column))
            .filter(|value| !value.trim().is_empty())
            .filter(|value| {
                Money::parse_decimal(value, Currency::GBP, decimal).is_ok()
                    && value.contains(match decimal {
                        DecimalMark::Point => '.',
                        DecimalMark::Comma => ',',
                    })
            })
            .count()
    };
    if count(DecimalMark::Comma) > count(DecimalMark::Point) {
        DecimalMark::Comma
    } else {
        DecimalMark::Point
    }
}

/// [`SplitSigns::Signed`] when a money-out value carries a minus sign.
fn split_signs(file: &CsvFile, debit: &Column, decimal: DecimalMark) -> SplitSigns {
    let negative = values(file, debit).iter().any(|value| {
        Money::parse_decimal(value, Currency::GBP, decimal).is_ok_and(|money| money.is_negative())
    });
    if negative {
        SplitSigns::Signed
    } else {
        SplitSigns::Unsigned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SourceFile;

    fn read(text: &str) -> CsvFile {
        CsvFile::read(
            &SourceFile::from_bytes("x.csv", text),
            &FileLayout::default(),
        )
        .unwrap()
    }

    fn named(name: &str) -> Column {
        Column::named(name)
    }

    #[test]
    fn suggests_a_signed_layout_from_its_header() {
        let file = read(
            "Transaction ID,Date,Time,Name,Description,Amount,Balance\n\
             tx_1,23/11/2026,09:15:00,Example Cafe,CAFE 1,-3.40,96.60\n\
             tx_2,24/11/2026,18:02:11,Example Shop,SHOP 2,-10.00,86.60\n",
        );
        let draft = MappingDraft::suggest(&file, FileLayout::default());
        assert_eq!(draft.date, Some(named("Date")));
        assert_eq!(draft.date_format.as_ref().unwrap().pattern(), "%d/%m/%Y");
        assert_eq!(draft.time, Some(named("Time")));
        assert_eq!(draft.time_format.as_ref().unwrap().pattern(), "%H:%M:%S");
        assert_eq!(draft.amount, AmountDraft::Signed(Some(named("Amount"))));
        assert_eq!(draft.merchant, Some(named("Name")));
        assert_eq!(draft.description, vec![named("Description")]);
        assert_eq!(draft.external_id, Some(named("Transaction ID")));
        assert_eq!(draft.balance, Some(named("Balance")));

        let mapping = draft.build(Currency::GBP).unwrap();
        assert_eq!(mapping.balance, Some(named("Balance")));
        assert_eq!(mapping.layout(), FileLayout::default());
    }

    #[test]
    fn suggests_a_money_out_money_in_pair() {
        let file = read(
            "Date,Details,Paid out (GBP),Paid in (GBP)\n\
             23/11/2026,SHOP,10.00,\n\
             24/11/2026,SALARY,,1200.00\n",
        );
        let draft = MappingDraft::suggest(&file, FileLayout::default());
        assert_eq!(
            draft.amount,
            AmountDraft::Split {
                debit: Some(named("Paid out (GBP)")),
                credit: Some(named("Paid in (GBP)")),
                signs: SplitSigns::Unsigned,
            }
        );
        assert_eq!(draft.description, vec![named("Details")]);
    }

    #[test]
    fn a_signed_money_out_column_is_noticed() {
        let file = read("Date,Memo,Debit,Credit\n23/11/2026,SHOP,-10.00,\n");
        let draft = MappingDraft::suggest(&file, FileLayout::default());
        assert!(matches!(
            draft.amount,
            AmountDraft::Split {
                signs: SplitSigns::Signed,
                ..
            }
        ));
    }

    #[test]
    fn an_ambiguous_date_is_left_for_the_user() {
        let file = read("Date,Description,Amount\n01/02/2026,A,-1.00\n03/04/2026,B,-2.00\n");
        let draft = MappingDraft::suggest(&file, FileLayout::default());
        assert_eq!(draft.date, Some(named("Date")));
        assert_eq!(draft.date_format, None);
        let formats: Vec<_> = date_formats(&file, &named("Date"))
            .iter()
            .map(|format| format.pattern().to_owned())
            .collect();
        assert_eq!(formats, ["%d/%m/%Y", "%m/%d/%Y"]);
        assert_eq!(
            draft.build(Currency::GBP),
            Err(DraftError::Needed(Needed::DateFormat))
        );
    }

    #[test]
    fn equivalent_date_formats_count_once() {
        let file = read("Date,Description,Amount\n23/11/2026,A,-1.00\n24/11/2026,B,-2.00\n");
        let formats: Vec<_> = date_formats(&file, &named("Date"))
            .iter()
            .map(|format| format.pattern().to_owned())
            .collect();
        assert_eq!(formats, ["%d/%m/%Y"]);
    }

    #[test]
    fn without_a_header_columns_are_found_by_their_values() {
        let layout = FileLayout {
            has_header: false,
            ..FileLayout::default()
        };
        let file = CsvFile::read(
            &SourceFile::from_bytes("x.csv", "SHOP,2026-11-23,-10.00\nCAFE,2026-11-24,-3.40\n"),
            &layout,
        )
        .unwrap();
        let draft = MappingDraft::suggest(&file, layout);
        assert_eq!(draft.date, Some(Column::Index(1)));
        assert_eq!(draft.date_format.as_ref().unwrap().pattern(), "%Y-%m-%d");
        assert_eq!(draft.amount, AmountDraft::Signed(Some(Column::Index(2))));
        assert!(draft.description.is_empty());
        assert_eq!(
            draft.build(Currency::GBP),
            Err(DraftError::Needed(Needed::Description))
        );
    }

    #[test]
    fn a_decimal_comma_is_suggested_when_the_values_use_it() {
        let file = CsvFile::read(
            &SourceFile::from_bytes("x.csv", "Datum;Omschrijving;Bedrag\n23-11-2026;A;-8,99\n"),
            &FileLayout::default(),
        )
        .unwrap();
        let draft = MappingDraft::suggest(&file, FileLayout::default());
        assert_eq!(draft.decimal, DecimalMark::Comma);
        assert_eq!(draft.amount, AmountDraft::Signed(Some(named("Bedrag"))));
    }

    #[test]
    fn build_names_the_first_missing_choice() {
        let mut draft = MappingDraft::new(FileLayout::default());
        assert_eq!(
            draft.build(Currency::GBP),
            Err(DraftError::Needed(Needed::DateColumn))
        );
        draft.date = Some(named("Date"));
        draft.date_format = Some(DateFormat::new("%d/%m/%Y").unwrap());
        draft.amount = AmountDraft::Split {
            debit: Some(named("Out")),
            credit: None,
            signs: SplitSigns::Unsigned,
        };
        assert_eq!(
            draft.build(Currency::GBP),
            Err(DraftError::Needed(Needed::MoneyIn))
        );
        draft.amount = AmountDraft::Split {
            debit: Some(named("Out")),
            credit: Some(named("Out")),
            signs: SplitSigns::Unsigned,
        };
        draft.description = vec![named("Details")];
        assert_eq!(
            draft.build(Currency::GBP),
            Err(DraftError::Mapping(MappingError::SameDebitAndCredit))
        );
        draft.time = Some(named("Time"));
        assert_eq!(
            draft.build(Currency::GBP),
            Err(DraftError::Needed(Needed::TimeFormat))
        );
    }

    #[test]
    fn a_preset_mapping_comes_back_unchanged() {
        let presets = crate::preset::Presets::bundled().unwrap();
        let mapping = presets.get("uk-monzo").unwrap().mapping();
        let draft = MappingDraft::from_mapping(mapping);
        assert_eq!(&draft.build(mapping.currency).unwrap(), mapping);
    }

    #[test]
    fn repeated_and_blank_header_names_are_not_offered() {
        let file = read("Date,Amount,Amount,,Description\n23/11/2026,1,2,,A\n");
        assert_eq!(columns(&file), vec![named("Date"), named("Description")]);
    }

    #[test]
    fn simplify_drops_punctuation_and_currency() {
        assert_eq!(simplify("Paid Out (£)"), "paid out");
        assert_eq!(simplify("  Amount GBP "), "amount");
        assert_eq!(simplify("Transaction_ID"), "transaction id");
    }
}
