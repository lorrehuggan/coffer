//! Turning OFX statements into a [`Statement`].

use std::collections::HashMap;

use chrono::{NaiveDate, NaiveTime};
use coffer_core::money::{Currency, DecimalMark, Money};
use coffer_core::statement::{
    DatedBalance, Field, RowProblem, SourceFormat, SourceLocation, Statement, StatementLine,
};

use super::document::{OfxStatement, OfxStatementKind, Transaction};
use super::markup::Element;
use crate::ImportError;

fn malformed(line: u64, reason: &'static str) -> ImportError {
    ImportError::Malformed {
        format: SourceFormat::Ofx,
        location: Some(SourceLocation::Line(line)),
        reason,
    }
}

/// Normalizes one statement. Transactions that can't be read are flagged in
/// their place; a statement without a known currency, or with a ledger
/// balance that can't be read, fails the file.
pub(super) fn statement(ofx: &OfxStatement) -> Result<Statement, ImportError> {
    if ofx.kind == OfxStatementKind::Investment {
        return Err(ImportError::Unsupported {
            what: "investment statements",
        });
    }
    let currency_text = ofx
        .currency
        .as_deref()
        .ok_or_else(|| malformed(ofx.line, "the statement has no currency (CURDEF)"))?;
    let currency: Currency = currency_text.trim().parse().map_err(|_| {
        malformed(
            ofx.line,
            "the statement's currency (CURDEF) isn't a known ISO 4217 code",
        )
    })?;

    let mut statement = Statement::new(SourceFormat::Ofx, currency);
    let mut first_seen: HashMap<&str, SourceLocation> = HashMap::new();
    for transaction in &ofx.transactions {
        let location = SourceLocation::Entry(transaction.entry);
        let mut line = line(transaction, currency);
        // A repeated FITID can't identify the row. The first one stands
        // (read or not); later ones are flagged unless already flagged.
        if let Ok(Some(id)) = external_id(&transaction.element) {
            match first_seen.get(id) {
                Some(&first) if line.is_ok() => {
                    line = Err(RowProblem::DuplicateExternalId { first });
                }
                Some(_) => {}
                None => {
                    first_seen.insert(id, location);
                }
            }
        }
        statement.push(location, line);
    }

    if let Some(ledger) = &ofx.ledger_balance {
        statement.push_balance(ledger_balance(ledger, currency)?)?;
    }
    Ok(statement)
}

/// The one value element called `name` directly inside `element`: `None`
/// when it's missing or empty, flagged when it's given twice or holds
/// elements.
fn single<'a>(
    element: &'a Element,
    name: &str,
    field: Field,
) -> Result<Option<&'a str>, RowProblem> {
    let mut matches = element.children_named(name);
    match (matches.next(), matches.next()) {
        (None, _) => Ok(None),
        (Some(_), Some(_)) => Err(RowProblem::Unreadable {
            field,
            reason: "is given more than once",
        }),
        (Some(one), None) => match one.value() {
            Some("") => Ok(None),
            Some(value) => Ok(Some(value)),
            None => Err(RowProblem::Unreadable {
                field,
                reason: "holds elements instead of a value",
            }),
        },
    }
}

/// The one aggregate called `name` directly inside `element`, if any.
/// `PAYEE`, `CURRENCY` and `ORIGCURRENCY` are always aggregates: the
/// markup reader fails a file that gives one a value.
fn single_aggregate<'a>(
    element: &'a Element,
    name: &str,
    field: Field,
) -> Result<Option<&'a Element>, RowProblem> {
    let mut matches = element.children_named(name);
    match (matches.next(), matches.next()) {
        (_, Some(_)) => Err(RowProblem::Unreadable {
            field,
            reason: "is given more than once",
        }),
        (one, None) => Ok(one),
    }
}

fn external_id(element: &Element) -> Result<Option<&str>, RowProblem> {
    single(element, "FITID", Field::ExternalId)
}

/// Normalizes one `STMTTRN`, or says why it can't be.
fn line(
    transaction: &Transaction,
    statement_currency: Currency,
) -> Result<StatementLine, RowProblem> {
    let element = &transaction.element;

    // A correction replaces or deletes an earlier transaction; importing
    // it as a new one would count it twice.
    if element.children_named("CORRECTACTION").next().is_some() {
        return Err(RowProblem::Unsupported {
            reason: "it corrects or deletes an earlier transaction (OFX CORRECTACTION)",
        });
    }

    let posted =
        single(element, "DTPOSTED", Field::Date)?.ok_or(RowProblem::Missing(Field::Date))?;
    let (date, time) = parse_date_time(posted).map_err(|reason| RowProblem::Unreadable {
        field: Field::Date,
        reason,
    })?;

    let currency = transaction_currency(element, statement_currency)?;
    let amount_text =
        single(element, "TRNAMT", Field::Amount)?.ok_or(RowProblem::Missing(Field::Amount))?;
    let amount = parse_amount(amount_text, currency).map_err(|error| RowProblem::Amount {
        field: Field::Amount,
        error,
    })?;

    let name = single(element, "NAME", Field::Merchant)?;
    let payee_name = match single_aggregate(element, "PAYEE", Field::Merchant)? {
        Some(payee) => single(payee, "NAME", Field::Merchant)?,
        None => None,
    };
    let merchant = name.or(payee_name);
    let extended_name = single(element, "EXTDNAME", Field::Description)?;
    let memo = single(element, "MEMO", Field::Description)?;

    let mut line = StatementLine::new(date, amount, describe([merchant, extended_name, memo]));
    line.time = time;
    line.merchant = merchant.map(str::to_owned);
    line.external_id = external_id(element)?.map(str::to_owned);
    line.source_type = single(element, "TRNTYPE", Field::SourceType)?.map(str::to_owned);
    Ok(line)
}

/// The currency `TRNAMT` is in: the `CURRENCY` aggregate's `CURSYM` when
/// there is one (OFX then states the amount in that currency), otherwise
/// the statement's. `ORIGCURRENCY` says the amount was already converted to
/// the statement's currency, so it doesn't change it.
fn transaction_currency(element: &Element, statement: Currency) -> Result<Currency, RowProblem> {
    let currency = single_aggregate(element, "CURRENCY", Field::Currency)?;
    let original = single_aggregate(element, "ORIGCURRENCY", Field::Currency)?;
    match (currency, original) {
        (Some(_), Some(_)) => Err(RowProblem::Unreadable {
            field: Field::Currency,
            reason: "has both CURRENCY and ORIGCURRENCY",
        }),
        (Some(currency), None) => single(currency, "CURSYM", Field::Currency)?
            .ok_or(RowProblem::Missing(Field::Currency))?
            .parse()
            .map_err(|_| RowProblem::Unreadable {
                field: Field::Currency,
                reason: "isn't a known ISO 4217 code",
            }),
        (None, _) => Ok(statement),
    }
}

/// The description from the name, extended name and memo, in that order:
/// each piece is added after a space, unless it repeats the text so far
/// (the name is often the memo cut short, or the same).
fn describe(pieces: [Option<&str>; 3]) -> String {
    let mut description = String::new();
    for piece in pieces.into_iter().flatten() {
        if piece.starts_with(description.as_str()) {
            description = piece.to_owned();
        } else if !description.starts_with(piece) {
            description.push(' ');
            description.push_str(piece);
        }
    }
    description
}

/// Reads an OFX amount exactly. OFX allows `.` or `,` as the decimal mark
/// and no grouping: a value with a comma and no point uses the comma.
fn parse_amount(
    text: &str,
    currency: Currency,
) -> Result<Money, coffer_core::money::ParseAmountError> {
    let mark = if text.contains(',') && !text.contains('.') {
        DecimalMark::Comma
    } else {
        DecimalMark::Point
    };
    Money::parse_decimal(text, currency, mark)
}

/// Reads an OFX date and time: `YYYYMMDD`, optionally followed by `HHMM`,
/// `HHMMSS` or `HHMMSS.XXX`, then optionally a `[offset:zone]`.
///
/// The date and time are taken as written, in the bank's time zone. The
/// offset isn't applied: converting to UTC would move evening transactions
/// to another day from the one on the bank's statement (and in its CSV
/// exports). Fractions of a second are dropped.
pub(super) fn parse_date_time(text: &str) -> Result<(NaiveDate, Option<NaiveTime>), &'static str> {
    const SHAPE: &str = "isn't an OFX date (YYYYMMDD, optionally HHMMSS and a time zone)";
    let text = text.trim();
    let main = match text.split_once('[') {
        Some((main, zone)) => {
            let inner = zone.strip_suffix(']').ok_or(SHAPE)?;
            if inner.is_empty() || inner.contains(['[', ']']) {
                return Err(SHAPE);
            }
            main
        }
        None => text,
    };
    let (digits, fraction) = match main.split_once('.') {
        Some((digits, fraction)) => (digits, Some(fraction)),
        None => (main, None),
    };
    if !digits.bytes().all(|b| b.is_ascii_digit())
        || !matches!(digits.len(), 8 | 12 | 14)
        || fraction.is_some_and(|fraction| {
            digits.len() != 14
                || fraction.is_empty()
                || !fraction.bytes().all(|b| b.is_ascii_digit())
        })
    {
        return Err(SHAPE);
    }
    let number = |range: std::ops::Range<usize>| -> u32 {
        digits[range].parse().expect("checked to be digits")
    };
    let year = i32::try_from(number(0..4)).expect("four digits fit");
    let date =
        NaiveDate::from_ymd_opt(year, number(4..6), number(6..8)).ok_or("isn't a real date")?;
    let time = match digits.len() {
        8 => None,
        12 => Some(
            NaiveTime::from_hms_opt(number(8..10), number(10..12), 0).ok_or("isn't a real time")?,
        ),
        _ => Some(
            NaiveTime::from_hms_opt(number(8..10), number(10..12), number(12..14))
                .ok_or("isn't a real time")?,
        ),
    };
    Ok((date, time))
}

/// The ledger balance (`LEDGERBAL`: `BALAMT` as of `DTASOF`), dated by
/// `DTASOF`'s date as written.
fn ledger_balance(ledger: &Element, currency: Currency) -> Result<DatedBalance, ImportError> {
    let unreadable = || malformed(ledger.line, "the ledger balance (LEDGERBAL) can't be read");
    let amount = single(ledger, "BALAMT", Field::Balance)
        .ok()
        .flatten()
        .ok_or_else(unreadable)?;
    let as_of = single(ledger, "DTASOF", Field::Date)
        .ok()
        .flatten()
        .ok_or_else(unreadable)?;
    let (date, _) = parse_date_time(as_of).map_err(|_| unreadable())?;
    let balance = parse_amount(amount, currency).map_err(|_| unreadable())?;
    Ok(DatedBalance { date, balance })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn time(h: u32, m: u32, s: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, s).unwrap()
    }

    #[test]
    fn dates_and_times_as_written() {
        for (text, expected) in [
            ("20261102", (date(2026, 11, 2), None)),
            ("20261102[0:GMT]", (date(2026, 11, 2), None)),
            ("202611021530", (date(2026, 11, 2), Some(time(15, 30, 0)))),
            (
                "20261102153045",
                (date(2026, 11, 2), Some(time(15, 30, 45))),
            ),
            (
                "20261102153045.123",
                (date(2026, 11, 2), Some(time(15, 30, 45))),
            ),
            // The offset isn't applied: still the 2nd, not the 3rd in UTC.
            (
                "20261102230000.000[-5:EST]",
                (date(2026, 11, 2), Some(time(23, 0, 0))),
            ),
            (
                "20261102000000[+5.30:IST]",
                (date(2026, 11, 2), Some(time(0, 0, 0))),
            ),
            (
                "20261102120000[-8]",
                (date(2026, 11, 2), Some(time(12, 0, 0))),
            ),
            (" 20261102 ", (date(2026, 11, 2), None)),
        ] {
            assert_eq!(parse_date_time(text), Ok(expected), "{text}");
        }
    }

    #[test]
    fn unreadable_dates() {
        let shape = "isn't an OFX date (YYYYMMDD, optionally HHMMSS and a time zone)";
        for (text, reason) in [
            ("", shape),
            ("2026-11-02", shape),
            ("02/11/2026", shape),
            ("2026110", shape),
            ("20261102 1530", shape),
            ("2026110215", shape),
            ("20261102.5", shape),
            ("20261102153045.", shape),
            ("20261102[0:GMT", shape),
            ("20261102[]", shape),
            ("20260230", "isn't a real date"),
            ("20261302", "isn't a real date"),
            ("20261102250000", "isn't a real time"),
            ("20261102236000", "isn't a real time"),
        ] {
            assert_eq!(parse_date_time(text), Err(reason), "{text}");
        }
    }

    #[test]
    fn amounts_take_a_point_or_a_comma() {
        let gbp = |text| parse_amount(text, Currency::GBP).map(|money| money.minor());
        assert_eq!(gbp("-12.40"), Ok(-1240));
        assert_eq!(gbp("+1500"), Ok(150_000));
        assert_eq!(gbp("12,5"), Ok(1250));
        assert_eq!(gbp(".99"), Ok(99));
        assert!(gbp("1.005").is_err(), "never rounded");
        assert!(
            gbp("1,234").is_err(),
            "a lone comma is a decimal mark: too precise"
        );
        assert!(gbp("£12.40").is_err());
        assert!(gbp("12.40-").is_err());
        assert_eq!(
            parse_amount("-1500", "JPY".parse().unwrap()).map(|money| money.minor()),
            Ok(-1500)
        );
    }

    #[test]
    fn descriptions_join_without_repeating() {
        assert_eq!(
            describe([Some("SHOP"), None, Some("CARD 1234")]),
            "SHOP CARD 1234"
        );
        assert_eq!(
            describe([
                Some("GREENGROCER LTD LONDO"),
                None,
                Some("GREENGROCER LTD LONDON GB")
            ]),
            "GREENGROCER LTD LONDON GB",
            "a name cut short by the 32-character limit"
        );
        assert_eq!(describe([Some("SHOP"), None, Some("SHOP")]), "SHOP");
        assert_eq!(
            describe([Some("SHOP LONDON"), None, Some("SHOP")]),
            "SHOP LONDON"
        );
        assert_eq!(
            describe([Some("SHOP"), Some("SHOP AND CAFE"), Some("REF 9")]),
            "SHOP AND CAFE REF 9"
        );
        assert_eq!(describe([None, None, Some("MEMO ONLY")]), "MEMO ONLY");
        assert_eq!(describe([None, None, None]), "");
    }
}
