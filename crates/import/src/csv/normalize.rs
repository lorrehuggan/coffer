//! Turning CSV records into statement lines.

use coffer_core::money::{Currency, DecimalMark, Money, ParseAmountError};
use coffer_core::statement::{Field, RowProblem, StatementLine};

use super::mapping::{AmountColumns, Column, CsvMapping, SplitSigns};
use super::reader::{CsvFile, fits_width};
use crate::ImportError;

/// [`AmountColumns`] with positions instead of columns.
enum AmountAt {
    Signed(usize),
    Split {
        debit: usize,
        credit: usize,
        signs: SplitSigns,
    },
}

/// A mapping's columns found in one file: positions in each record.
pub(super) struct Positions {
    width: usize,
    date: usize,
    time: Option<usize>,
    amount: AmountAt,
    currency: Option<usize>,
    merchant: Option<usize>,
    description: Vec<usize>,
    external_id: Option<usize>,
    source_category: Option<usize>,
    source_type: Option<usize>,
    original_amount: Option<(usize, usize)>,
    note: Option<usize>,
    balance: Option<usize>,
}

impl Positions {
    /// Finds every column `mapping` names in `file`.
    pub(super) fn find(mapping: &CsvMapping, file: &CsvFile) -> Result<Positions, ImportError> {
        let width = file.width();
        let header = file.header().map(|header| header.fields.as_slice());
        let at = |column: &Column| -> Result<usize, ImportError> {
            let missing = || ImportError::MissingColumn {
                column: column.to_string(),
            };
            match column {
                Column::Index(index) if *index < width => Ok(*index),
                Column::Index(_) => Err(missing()),
                Column::Named(name) => {
                    let header = header.ok_or_else(missing)?;
                    let mut matches = header
                        .iter()
                        .take(width)
                        .enumerate()
                        .filter(|(_, cell)| *cell == name.trim())
                        .map(|(index, _)| index);
                    match (matches.next(), matches.next()) {
                        (Some(index), None) => Ok(index),
                        (None, _) => Err(missing()),
                        (Some(_), Some(_)) => Err(ImportError::DuplicateColumn {
                            column: column.to_string(),
                        }),
                    }
                }
            }
        };
        let optional = |column: &Option<Column>| column.as_ref().map(at).transpose();

        Ok(Positions {
            width,
            date: at(&mapping.date.column)?,
            time: mapping
                .time
                .as_ref()
                .map(|time| at(&time.column))
                .transpose()?,
            amount: match &mapping.amount {
                AmountColumns::Signed(column) => AmountAt::Signed(at(column)?),
                AmountColumns::Split {
                    debit,
                    credit,
                    signs,
                } => AmountAt::Split {
                    debit: at(debit)?,
                    credit: at(credit)?,
                    signs: *signs,
                },
            },
            currency: optional(&mapping.currency_column)?,
            merchant: optional(&mapping.merchant)?,
            description: mapping
                .description
                .iter()
                .map(&at)
                .collect::<Result<_, _>>()?,
            external_id: optional(&mapping.external_id)?,
            source_category: optional(&mapping.source_category)?,
            source_type: optional(&mapping.source_type)?,
            original_amount: mapping
                .original_amount
                .as_ref()
                .map(|original| {
                    Ok::<_, ImportError>((at(&original.amount)?, at(&original.currency)?))
                })
                .transpose()?,
            note: optional(&mapping.note)?,
            balance: optional(&mapping.balance)?,
        })
    }

    /// Normalizes one record, or says why it can't be.
    pub(super) fn line(
        &self,
        mapping: &CsvMapping,
        fields: &[String],
    ) -> Result<StatementLine, RowProblem> {
        if !fits_width(fields, self.width) {
            return Err(RowProblem::ColumnCount {
                expected: self.width,
                found: fields.len(),
            });
        }
        let get = |index: usize| fields.get(index).map_or("", String::as_str);
        let optional = |index: Option<usize>| index.map(get);

        let date_text = get(self.date);
        if date_text.trim().is_empty() {
            return Err(RowProblem::Missing(Field::Date));
        }
        let (date, mut time) =
            mapping
                .date
                .format
                .parse(date_text)
                .ok_or(RowProblem::Unreadable {
                    field: Field::Date,
                    reason: "doesn't match the date format",
                })?;
        if let (Some(index), Some(time_column)) = (self.time, &mapping.time) {
            let text = get(index);
            if !text.trim().is_empty() {
                time = Some(
                    time_column
                        .format
                        .parse(text)
                        .ok_or(RowProblem::Unreadable {
                            field: Field::Time,
                            reason: "doesn't match the time format",
                        })?,
                );
            }
        }

        let currency = match optional(self.currency) {
            None => mapping.currency,
            Some(text) => currency(text, Field::Currency)?,
        };
        let decimal = mapping.decimal;
        let amount = match self.amount {
            AmountAt::Signed(index) => parse_amount(get(index), currency, decimal, Field::Amount)?
                .ok_or(RowProblem::Missing(Field::Amount))?,
            AmountAt::Split {
                debit,
                credit,
                signs,
            } => split_amount(
                parse_amount(get(debit), currency, decimal, Field::Amount)?,
                parse_amount(get(credit), currency, decimal, Field::Amount)?,
                signs,
            )?,
        };
        let balance_after = match optional(self.balance) {
            Some(text) => parse_amount(text, currency, decimal, Field::Balance)?,
            None => None,
        };
        let original_amount = match self.original_amount {
            Some((amount_at, currency_at)) => original(get(amount_at), get(currency_at), decimal)?
                .filter(|&original| original != amount),
            None => None,
        };

        let mut line = StatementLine::new(date, amount, description(&self.description, &get));
        line.time = time;
        line.merchant = optional(self.merchant).and_then(text);
        line.external_id = optional(self.external_id).map(str::trim).and_then(text);
        line.source_category = optional(self.source_category).and_then(text);
        line.source_type = optional(self.source_type).and_then(text);
        line.original_amount = original_amount;
        line.note = optional(self.note).and_then(text);
        line.balance_after = balance_after;
        Ok(line)
    }
}

/// The field as written, or `None` when it's blank.
fn text(field: &str) -> Option<String> {
    (!field.trim().is_empty()).then(|| field.to_owned())
}

/// One description column as written; several joined with a space, blank
/// ones left out.
fn description<'a>(columns: &[usize], get: &impl Fn(usize) -> &'a str) -> String {
    match columns {
        [only] => get(*only).to_owned(),
        _ => columns
            .iter()
            .map(|&index| get(index).trim())
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn currency(text: &str, field: Field) -> Result<Currency, RowProblem> {
    let code = text.trim();
    if code.is_empty() {
        return Err(RowProblem::Missing(field));
    }
    Currency::new(code).map_err(|_| RowProblem::Unreadable {
        field,
        reason: "isn't an ISO 4217 currency code",
    })
}

/// The amount in `text`, or `None` when it's blank.
fn parse_amount(
    text: &str,
    currency: Currency,
    decimal: DecimalMark,
    field: Field,
) -> Result<Option<Money>, RowProblem> {
    match Money::parse_decimal(text, currency, decimal) {
        Ok(money) => Ok(Some(money)),
        Err(ParseAmountError::Empty) => Ok(None),
        Err(error) => Err(RowProblem::Amount { field, error }),
    }
}

fn split_amount(
    debit: Option<Money>,
    credit: Option<Money>,
    signs: SplitSigns,
) -> Result<Money, RowProblem> {
    let unreadable = |reason| RowProblem::Unreadable {
        field: Field::Amount,
        reason,
    };
    if signs == SplitSigns::Unsigned && [debit, credit].iter().flatten().any(Money::is_negative) {
        return Err(unreadable(
            "a money-in or money-out column has a negative amount",
        ));
    }
    let money_out = |debit: Money| match signs {
        SplitSigns::Signed => Ok(debit),
        SplitSigns::Unsigned => debit.checked_neg().map_err(|_| RowProblem::Amount {
            field: Field::Amount,
            error: ParseAmountError::Overflow,
        }),
    };
    match (debit, credit) {
        (None, None) => Err(RowProblem::Missing(Field::Amount)),
        (Some(debit), None) => money_out(debit),
        (None, Some(credit)) => Ok(credit),
        (Some(debit), Some(credit)) if debit.is_zero() => Ok(credit),
        (Some(debit), Some(credit)) if credit.is_zero() => money_out(debit),
        (Some(_), Some(_)) => Err(unreadable(
            "both the money-out and money-in columns have an amount",
        )),
    }
}

/// The amount in the currency spent, or `None` when both columns are blank.
fn original(
    amount_text: &str,
    currency_text: &str,
    decimal: DecimalMark,
) -> Result<Option<Money>, RowProblem> {
    let (has_amount, has_currency) = (
        !amount_text.trim().is_empty(),
        !currency_text.trim().is_empty(),
    );
    match (has_amount, has_currency) {
        (false, false) => Ok(None),
        (false, true) => Err(RowProblem::Missing(Field::OriginalAmount)),
        (true, false) => Err(RowProblem::Missing(Field::OriginalCurrency)),
        (true, true) => {
            let currency = currency(currency_text, Field::OriginalCurrency)?;
            parse_amount(amount_text, currency, decimal, Field::OriginalAmount)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gbp(minor: i64) -> Money {
        Money::new(minor, Currency::GBP)
    }

    #[test]
    fn split_amounts() {
        use SplitSigns::{Signed, Unsigned};
        let split = |debit: Option<i64>, credit: Option<i64>, signs| {
            split_amount(debit.map(gbp), credit.map(gbp), signs).map(|money| money.minor())
        };
        assert_eq!(split(Some(1240), None, Unsigned), Ok(-1240));
        assert_eq!(split(None, Some(5000), Unsigned), Ok(5000));
        assert_eq!(split(Some(0), Some(5000), Unsigned), Ok(5000));
        assert_eq!(split(Some(1240), Some(0), Unsigned), Ok(-1240));
        assert_eq!(split(Some(0), Some(0), Unsigned), Ok(0));
        assert_eq!(split(Some(-1240), None, Signed), Ok(-1240));
        assert_eq!(
            split(Some(300), None, Signed),
            Ok(300),
            "a signed money-out column can hold a refund"
        );
        assert_eq!(
            split(None, None, Unsigned),
            Err(RowProblem::Missing(Field::Amount))
        );
        assert!(matches!(
            split(Some(-1240), None, Unsigned),
            Err(RowProblem::Unreadable { .. })
        ));
        assert!(matches!(
            split(Some(100), Some(200), Signed),
            Err(RowProblem::Unreadable { .. })
        ));
        assert_eq!(
            split_amount(Some(Money::new(i64::MIN, Currency::GBP)), None, Signed)
                .map(|money| money.minor()),
            Ok(i64::MIN)
        );
    }

    #[test]
    fn original_amounts() {
        let eur = |minor| Money::new(minor, Currency::EUR);
        assert_eq!(
            original("-10.00", "EUR", DecimalMark::Point),
            Ok(Some(eur(-1000)))
        );
        assert_eq!(original(" ", "", DecimalMark::Point), Ok(None));
        assert_eq!(
            original("", "EUR", DecimalMark::Point),
            Err(RowProblem::Missing(Field::OriginalAmount))
        );
        assert_eq!(
            original("1", "", DecimalMark::Point),
            Err(RowProblem::Missing(Field::OriginalCurrency))
        );
        assert_eq!(
            original("1", "euro", DecimalMark::Point),
            Err(RowProblem::Unreadable {
                field: Field::OriginalCurrency,
                reason: "isn't an ISO 4217 currency code",
            })
        );
        assert_eq!(
            original("100", "JPY", DecimalMark::Point),
            Ok(Some(Money::new(100, Currency::new("JPY").unwrap()))),
            "read in its own currency's minor units"
        );
    }

    #[test]
    fn descriptions() {
        let fields = ["  Card payment ", "", " TESCO "];
        let get = |index: usize| fields[index];
        assert_eq!(
            description(&[0], &get),
            "  Card payment ",
            "one column as written"
        );
        assert_eq!(description(&[0, 1, 2], &get), "Card payment TESCO");
    }
}
