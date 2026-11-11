//! The `Importer` trait: detect → parse → normalize.

use std::cmp::Reverse;

use coffer_core::statement::{BalanceCurrencyMismatch, SourceFormat, SourceLocation, Statement};

use crate::SourceFile;

/// How sure an importer is that it can read a file. Ordered from weakest to
/// strongest, so the best candidate is the largest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Detection {
    /// Not a file this importer reads.
    No,
    /// Only the extension fits (`.csv`, `.qfx`); the contents weren't
    /// recognised.
    Extension,
    /// The contents are in this importer's format (an OFX header, text that
    /// splits into CSV columns).
    Format,
    /// The contents match this importer's particular source, such as a
    /// preset's exact header row.
    Source,
}

/// Why a file couldn't be imported at all.
///
/// A problem with one row isn't an `ImportError`: that row is flagged in
/// the [`Statement`] and the rest are read. Like row problems, these never
/// contain the file's text, since errors get logged.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("the file isn't valid {encoding} text")]
    Encoding { encoding: &'static str },

    #[error(
        "the file isn't a readable {format} file{}: {reason}",
        location.map(|at| format!(" ({at})")).unwrap_or_default()
    )]
    Malformed {
        format: SourceFormat,
        location: Option<SourceLocation>,
        reason: &'static str,
    },

    /// The mapping names a column the file doesn't have. The name comes from
    /// the mapping, not the file, so it's safe to show and log.
    #[error("the file has no {column}")]
    MissingColumn { column: String },

    /// The mapping names a column that appears more than once in the header,
    /// so which one it means is unclear.
    #[error("the file has more than one {column}")]
    DuplicateColumn { column: String },

    #[error(transparent)]
    BalanceCurrency(#[from] BalanceCurrencyMismatch),

    /// The file holds more than one statement (several accounts in one
    /// OFX download). A statement is one account, so the caller asks which
    /// to import.
    #[error("the file holds {count} statements; choose which one to import")]
    MultipleStatements { count: usize },

    /// The file is readable, but holds something Coffer doesn't import,
    /// such as an OFX investment statement.
    #[error("Coffer can't import {what}")]
    Unsupported { what: &'static str },
}

/// What an importer is and which files it reads: the part of an importer
/// that choosing between importers needs.
pub trait Detect {
    /// What the user sees when choosing an importer, e.g.
    /// `Monzo (current account)`.
    fn name(&self) -> &str;

    /// The format recorded on the import batch.
    fn format(&self) -> SourceFormat;

    /// How well `file` fits, from its name and contents. Cheap, and never
    /// fails: a file this importer can't make sense of is [`Detection::No`].
    fn detect(&self, file: &SourceFile) -> Detection;
}

/// Reads one kind of bank file into a [`Statement`].
///
/// Import runs in three steps, all deterministic (no model or network
/// calls):
///
/// 1. [`detect`](Detect::detect) looks at the name and the contents and
///    says how well the file fits.
/// 2. [`parse`](Importer::parse) reads the whole file in its own format:
///    decoding, splitting into records, the format's structure.
/// 3. [`normalize`](Importer::normalize) turns those records into
///    [`StatementLine`](coffer_core::statement::StatementLine)s: dates, exact
///    amounts in minor units, which text goes where.
///
/// Only a file that can't be read as a whole is an error. A record that
/// can't be normalized is pushed as a flagged row in its place, never
/// skipped. An importer holds its configuration (a CSV preset, a mapping)
/// and can be used for many files.
///
/// Every `Importer` is an [`AnyImporter`], which runs both steps with
/// [`import`](AnyImporter::import) and lets different importers sit in one
/// list.
pub trait Importer: Detect {
    /// The file as parsed, before normalizing: CSV records, an OFX document.
    type Parsed;

    fn parse(&self, file: &SourceFile) -> Result<Self::Parsed, ImportError>;

    fn normalize(&self, parsed: Self::Parsed) -> Result<Statement, ImportError>;
}

/// An [`Importer`] without its `Parsed` type, so different importers can be
/// used as `&dyn AnyImporter`. Implemented for every `Importer`.
pub trait AnyImporter: Detect {
    /// Parses and normalizes `file`.
    fn import(&self, file: &SourceFile) -> Result<Statement, ImportError>;
}

impl<T: Importer> AnyImporter for T {
    fn import(&self, file: &SourceFile) -> Result<Statement, ImportError> {
        let parsed = self.parse(file)?;
        self.normalize(parsed)
    }
}

/// An importer that recognised a file, and how well.
#[derive(Clone, Copy)]
pub struct Candidate<'a> {
    pub importer: &'a dyn AnyImporter,
    pub detection: Detection,
}

/// The importers that recognise `file`, best first. Importers that detect
/// equally well keep their order in `importers`.
///
/// Choosing is the caller's: with no candidate or a tie at the top, ask the
/// user (or take `--preset`) rather than guess.
pub fn detect<'a>(importers: &[&'a dyn AnyImporter], file: &SourceFile) -> Vec<Candidate<'a>> {
    let mut candidates: Vec<_> = importers
        .iter()
        .map(|&importer| Candidate {
            importer,
            detection: importer.detect(file),
        })
        .filter(|candidate| candidate.detection > Detection::No)
        .collect();
    candidates.sort_by_key(|candidate| Reverse(candidate.detection));
    candidates
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use coffer_core::money::{Currency, DecimalMark, Money};
    use coffer_core::statement::{DatedBalance, Field, RowProblem, StatementLine};

    use super::*;

    /// A toy format: a `TOY <currency>` header, then `date|amount|description`
    /// rows and an optional `BALANCE|date|amount` row.
    struct Toy {
        name: &'static str,
        /// A header that marks this source in particular.
        signature: Option<&'static str>,
    }

    const TOY: Toy = Toy {
        name: "Toy",
        signature: None,
    };

    struct ToyFile {
        currency: Currency,
        rows: Vec<(SourceLocation, Vec<String>)>,
    }

    impl Detect for Toy {
        fn name(&self) -> &str {
            self.name
        }

        fn format(&self) -> SourceFormat {
            SourceFormat::Csv
        }

        fn detect(&self, file: &SourceFile) -> Detection {
            let header = file
                .bytes()
                .split(|&b| b == b'\n')
                .next()
                .unwrap_or_default();
            match self.signature {
                Some(signature) if header == signature.as_bytes() => Detection::Source,
                _ if header.starts_with(b"TOY ") => Detection::Format,
                _ if file.extension().as_deref() == Some("toy") => Detection::Extension,
                _ => Detection::No,
            }
        }
    }

    impl Importer for Toy {
        type Parsed = ToyFile;

        fn parse(&self, file: &SourceFile) -> Result<ToyFile, ImportError> {
            let text = std::str::from_utf8(file.bytes())
                .map_err(|_| ImportError::Encoding { encoding: "UTF-8" })?;
            let mut lines = text.lines();
            let malformed = |line, reason| ImportError::Malformed {
                format: SourceFormat::Csv,
                location: Some(SourceLocation::Line(line)),
                reason,
            };
            let currency = lines
                .next()
                .and_then(|header| header.strip_prefix("TOY "))
                .ok_or_else(|| malformed(1, "no TOY header"))?
                .parse()
                .map_err(|_| malformed(1, "unknown currency"))?;
            let rows = (2..)
                .zip(lines)
                .map(|(n, line)| {
                    let fields = line.split('|').map(str::to_owned).collect();
                    (SourceLocation::Line(n), fields)
                })
                .collect();
            Ok(ToyFile { currency, rows })
        }

        fn normalize(&self, parsed: ToyFile) -> Result<Statement, ImportError> {
            let currency = parsed.currency;
            let date = |text: &str| {
                NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|_| RowProblem::Unreadable {
                    field: Field::Date,
                    reason: "isn't YYYY-MM-DD",
                })
            };
            let amount = |field, text: &str| {
                Money::parse_decimal(text, currency, DecimalMark::Point)
                    .map_err(|error| RowProblem::Amount { field, error })
            };

            let mut statement = Statement::new(SourceFormat::Csv, currency);
            for (location, fields) in parsed.rows {
                match fields.as_slice() {
                    [tag, day, balance] if tag == "BALANCE" => {
                        let balance = DatedBalance {
                            date: date(day).map_err(|_| ImportError::Malformed {
                                format: SourceFormat::Csv,
                                location: Some(location),
                                reason: "bad balance date",
                            })?,
                            balance: amount(Field::Balance, balance).map_err(|_| {
                                ImportError::Malformed {
                                    format: SourceFormat::Csv,
                                    location: Some(location),
                                    reason: "bad balance",
                                }
                            })?,
                        };
                        statement.push_balance(balance)?;
                    }
                    [day, value, description] => {
                        let line = date(day).and_then(|day| {
                            Ok(StatementLine::new(
                                day,
                                amount(Field::Amount, value)?,
                                description.as_str(),
                            ))
                        });
                        statement.push(location, line);
                    }
                    _ => statement.push(location, Err(RowProblem::Missing(Field::Description))),
                }
            }
            Ok(statement)
        }
    }

    fn file(name: &str, text: &str) -> SourceFile {
        SourceFile::from_bytes(name, text)
    }

    #[test]
    fn detection_strength_is_ordered() {
        assert!(Detection::No < Detection::Extension);
        assert!(Detection::Extension < Detection::Format);
        assert!(Detection::Format < Detection::Source);
    }

    #[test]
    fn detect_ranks_candidates_and_drops_non_matches() {
        let generic = Toy {
            name: "generic",
            signature: None,
        };
        let other = Toy {
            name: "other bank",
            signature: Some("TOY EUR"),
        };
        let bank = Toy {
            name: "bank",
            signature: Some("TOY GBP"),
        };
        let importers: [&dyn AnyImporter; 3] = [&generic, &other, &bank];

        let ranked = |file: &SourceFile| -> Vec<(String, Detection)> {
            detect(&importers, file)
                .into_iter()
                .map(|c| (c.importer.name().to_owned(), c.detection))
                .collect()
        };

        assert_eq!(
            ranked(&file("a.txt", "TOY GBP\n")),
            [
                ("bank".to_owned(), Detection::Source),
                ("generic".to_owned(), Detection::Format),
                ("other bank".to_owned(), Detection::Format),
            ],
            "best first; ties keep the given order"
        );
        assert_eq!(
            ranked(&file("a.toy", "something else")).len(),
            3,
            "extension only"
        );
        assert!(ranked(&file("a.csv", "Date,Amount\n")).is_empty());
    }

    #[test]
    fn import_parses_then_normalizes_in_file_order() {
        let text = "TOY GBP\n\
                    2026-11-01|-12.40|Coffee\n\
                    01/11/2026|-3.00|Bad date\n\
                    2026-11-02|1.005|Too precise\n\
                    just one field\n\
                    2026-11-03|2,000.00|Salary\n\
                    BALANCE|2026-11-03|1987.60\n";
        let statement = TOY.import(&file("s.toy", text)).unwrap();

        assert_eq!(statement.currency(), Currency::GBP);
        assert_eq!(statement.rows().len(), 5, "no row is dropped");

        let lines: Vec<_> = statement
            .lines()
            .map(|(at, line)| (at, line.amount.minor(), line.description_raw.as_str()))
            .collect();
        assert_eq!(
            lines,
            [
                (SourceLocation::Line(2), -1240, "Coffee"),
                (SourceLocation::Line(6), 200_000, "Salary"),
            ]
        );

        let flagged: Vec<_> = statement
            .flagged()
            .map(|(at, problem)| (at, problem.clone()))
            .collect();
        assert_eq!(
            flagged,
            [
                (
                    SourceLocation::Line(3),
                    RowProblem::Unreadable {
                        field: Field::Date,
                        reason: "isn't YYYY-MM-DD",
                    }
                ),
                (
                    SourceLocation::Line(4),
                    RowProblem::Amount {
                        field: Field::Amount,
                        error: coffer_core::money::ParseAmountError::TooPrecise {
                            currency: Currency::GBP
                        },
                    }
                ),
                (
                    SourceLocation::Line(5),
                    RowProblem::Missing(Field::Description)
                ),
            ]
        );

        assert_eq!(
            statement.balances(),
            [DatedBalance {
                date: NaiveDate::from_ymd_opt(2026, 11, 3).unwrap(),
                balance: Money::new(198_760, Currency::GBP),
            }]
        );
    }

    #[test]
    fn works_through_any_importer() {
        let importer: &dyn AnyImporter = &TOY;
        assert_eq!(importer.name(), "Toy");
        assert_eq!(importer.format(), SourceFormat::Csv);
        let statement = importer
            .import(&file("s.toy", "TOY EUR\n2026-11-01|-1.00|A\n"))
            .unwrap();
        assert_eq!(statement.currency(), Currency::EUR);
        assert_eq!(statement.lines().count(), 1);
    }

    #[test]
    fn unreadable_files_are_errors_without_their_text() {
        let error = TOY
            .import(&file("s.toy", "SECRET MERCHANT|-1.00\n"))
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "the file isn't a readable csv file (line 1): no TOY header"
        );

        let error = TOY
            .import(&SourceFile::from_bytes(
                "s.toy",
                b"TOY GBP\n\xff\n".as_slice(),
            ))
            .unwrap_err();
        assert_eq!(error.to_string(), "the file isn't valid UTF-8 text");
    }

    #[test]
    fn a_balance_in_another_currency_fails_the_file() {
        let mut statement = Statement::new(SourceFormat::Ofx, Currency::GBP);
        let error: ImportError = statement
            .push_balance(DatedBalance {
                date: NaiveDate::from_ymd_opt(2026, 11, 3).unwrap(),
                balance: Money::new(1, Currency::EUR),
            })
            .unwrap_err()
            .into();
        assert_eq!(
            error.to_string(),
            "the file states a balance in EUR, but its transactions are in GBP"
        );
    }
}
