//! End-to-end tests: files through [`CsvImporter`], including the
//! fixtures in `fixtures/csv/`.

use chrono::{NaiveDate, NaiveTime};
use coffer_core::money::{Currency, DecimalMark, Money, ParseAmountError};
use coffer_core::statement::{Field, RowProblem, SourceLocation, Statement, StatementLine};

use super::*;
use crate::{AnyImporter, detect};

macro_rules! fixture {
    ($name:literal) => {
        SourceFile::from_bytes(
            $name,
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../fixtures/csv/",
                $name
            ))
            .as_slice(),
        )
    };
}

fn named(name: &str) -> Column {
    Column::named(name)
}

fn date_column(column: Column, format: &str) -> DateColumn {
    DateColumn {
        column,
        format: DateFormat::new(format).unwrap(),
    }
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn eur(minor: i64) -> Money {
    Money::new(minor, Currency::EUR)
}

/// A comma-separated mapping with `Date` (`%d/%m/%Y`), `Amount` and
/// `Description`, in GBP.
fn simple_mapping() -> CsvMapping {
    CsvMapping::new(
        Currency::GBP,
        date_column(named("Date"), "%d/%m/%Y"),
        AmountColumns::Signed(named("Amount")),
        named("Description"),
    )
}

fn importer(mapping: CsvMapping) -> CsvImporter {
    CsvImporter::new("Test", mapping).unwrap()
}

fn import(mapping: CsvMapping, text: &str) -> Result<Statement, ImportError> {
    importer(mapping).import(&SourceFile::from_bytes("s.csv", text))
}

fn problems(statement: &Statement) -> Vec<(u64, RowProblem)> {
    statement
        .flagged()
        .map(|(location, problem)| match location {
            SourceLocation::Line(line) => (line, problem.clone()),
            SourceLocation::Entry(_) => unreachable!("CSV rows are lines"),
        })
        .collect()
}

fn lines(statement: &Statement) -> Vec<(u64, StatementLine)> {
    statement
        .lines()
        .map(|(location, line)| match location {
            SourceLocation::Line(n) => (n, line.clone()),
            SourceLocation::Entry(_) => unreachable!("CSV rows are lines"),
        })
        .collect()
}

fn signed_amount_mapping() -> CsvMapping {
    let mut mapping = simple_mapping();
    mapping.time = Some(TimeColumn {
        column: named("Time"),
        format: TimeFormat::new("%H:%M:%S").unwrap(),
    });
    mapping.currency_column = Some(named("Currency"));
    mapping.merchant = Some(named("Name"));
    mapping.external_id = Some(named("Transaction ID"));
    mapping.source_category = Some(named("Category"));
    mapping.source_type = Some(named("Type"));
    mapping.original_amount = Some(OriginalAmountColumns {
        amount: named("Local amount"),
        currency: named("Local currency"),
    });
    mapping.note = Some(named("Notes"));
    mapping
}

#[test]
fn signed_amount_fixture() {
    let importer = importer(signed_amount_mapping()).with_preset("test-signed");
    let statement = importer.import(&fixture!("signed-amount.csv")).unwrap();

    assert_eq!(statement.preset_id(), Some("test-signed"));
    assert_eq!(statement.currency(), Currency::GBP);
    assert_eq!(statement.rows().len(), 10, "every row is kept");
    assert!(statement.balances().is_empty());

    let read = lines(&statement);
    let summary: Vec<_> = read
        .iter()
        .map(|(n, line)| {
            (
                *n,
                line.external_id.as_deref().unwrap(),
                line.amount.minor(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (2, "tx_0001", -340),
            (3, "tx_0002", -860),
            (4, "tx_0003", 200_000),
            (5, "tx_0004", -120),
            (6, "tx_0005", -120),
            (7, "tx_0006", 340),
            (11, "tx_0010", -4500),
        ],
        "identical same-day rows both stay; a positive amount is a refund"
    );

    let mut expected = StatementLine::new(date(2026, 11, 1), gbp(-860), "CAFE LUMIERE PARIS FRA");
    expected.time = NaiveTime::from_hms_opt(12, 30, 0);
    expected.merchant = Some("Café Lumière".to_owned());
    expected.external_id = Some("tx_0002".to_owned());
    expected.source_category = Some("Eating out".to_owned());
    expected.source_type = Some("Card payment".to_owned());
    expected.original_amount = Some(eur(-1000));
    expected.note = Some("lunch".to_owned());
    assert_eq!(read[1].1, expected, "every field of a foreign card payment");

    assert_eq!(
        read[0].1.original_amount, None,
        "a local amount that repeats the amount isn't kept"
    );
    assert_eq!(read[0].1.note, None, "a blank note is None");
    assert_eq!(read[6].1.time, None, "a blank time is None");

    assert_eq!(
        problems(&statement),
        [
            (
                8,
                RowProblem::Unreadable {
                    field: Field::Date,
                    reason: "doesn't match the date format",
                }
            ),
            (
                9,
                RowProblem::ColumnCount {
                    expected: 12,
                    found: 13
                }
            ),
            (
                10,
                RowProblem::Amount {
                    field: Field::Amount,
                    error: ParseAmountError::TooPrecise {
                        currency: Currency::GBP
                    },
                }
            ),
        ]
    );
}

fn debit_credit_mapping() -> CsvMapping {
    let mut mapping = CsvMapping::new(
        Currency::GBP,
        date_column(named("Date"), "%d/%m/%Y"),
        AmountColumns::Split {
            debit: named("Paid out"),
            credit: named("Paid in"),
            signs: SplitSigns::Unsigned,
        },
        named("Description"),
    );
    mapping.encoding = TextEncoding::Windows1252;
    mapping.delimiter = Some(Delimiter::Comma);
    mapping.skip_rows = 3;
    mapping.balance = Some(named("Balance"));
    mapping
}

#[test]
fn debit_credit_fixture() {
    let importer = importer(debit_credit_mapping());
    let file = fixture!("debit-credit-windows-1252.csv");

    let parsed = importer.parse(&file).unwrap();
    assert_eq!(parsed.encoding(), TextEncoding::Windows1252);
    assert_eq!(
        parsed.header().unwrap().line,
        4,
        "after three preamble lines"
    );
    assert_eq!(parsed.width(), 5);

    let statement = importer.normalize(parsed).unwrap();
    let read: Vec<_> = lines(&statement)
        .into_iter()
        .map(|(n, line)| {
            (
                n,
                line.description_raw,
                line.amount.minor(),
                line.balance_after.map(|balance| balance.minor()),
            )
        })
        .collect();
    assert_eq!(
        read,
        [
            (5, "Opening transfer".to_owned(), 100_000, Some(100_000)),
            (6, "Café Rouge £5 deal".to_owned(), -1240, Some(98_760)),
            (7, "Refund Café Rouge".to_owned(), 240, Some(99_000)),
        ],
        "decoded from Windows-1252, money out negated, CRLF handled"
    );
    assert_eq!(
        problems(&statement),
        [
            (
                8,
                RowProblem::Unreadable {
                    field: Field::Amount,
                    reason: "both the money-out and money-in columns have an amount",
                }
            ),
            (
                9,
                RowProblem::Unreadable {
                    field: Field::Amount,
                    reason: "a money-in or money-out column has a negative amount",
                }
            ),
        ]
    );
}

#[test]
fn a_non_utf8_file_needs_its_encoding_stated() {
    let mut mapping = debit_credit_mapping();
    mapping.encoding = TextEncoding::Auto;
    let importer = importer(mapping);
    let file = fixture!("debit-credit-windows-1252.csv");

    let error = importer.import(&file).unwrap_err();
    assert_eq!(error.to_string(), "the file isn't valid UTF-8 text");
    assert_eq!(importer.detect(&file), Detection::Extension);
}

#[test]
fn semicolon_decimal_comma_fixture() {
    let mut mapping = CsvMapping::new(
        Currency::EUR,
        date_column(named("Buchungstag"), "%d.%m.%Y"),
        AmountColumns::Signed(named("Betrag")),
        named("Auftraggeber"),
    );
    mapping.description.push(named("Verwendungszweck"));
    mapping.merchant = Some(named("Auftraggeber"));
    mapping.decimal = DecimalMark::Comma;
    mapping.currency_column = Some(named("Währung"));
    let importer = importer(mapping);
    let file = fixture!("semicolon-decimal-comma.csv");

    let parsed = importer.parse(&file).unwrap();
    assert_eq!(parsed.encoding(), TextEncoding::Utf8);
    assert_eq!(parsed.delimiter(), Delimiter::Semicolon, "detected");
    assert_eq!(
        parsed.header().unwrap().fields[0],
        "Buchungstag",
        "the BOM is gone"
    );

    let statement = importer.normalize(parsed).unwrap();
    let read: Vec<_> = lines(&statement)
        .into_iter()
        .map(|(n, line)| (n, line.description_raw, line.amount))
        .collect();
    assert_eq!(
        read,
        [
            (2, "Muster GmbH Gehalt November".to_owned(), eur(250_000)),
            (3, "Bäckerei Beispiel Kartenzahlung".to_owned(), eur(-450)),
            (
                4,
                "Vermietung Beispiel Miete, November".to_owned(),
                eur(-123_456)
            ),
        ]
    );
    assert_eq!(
        problems(&statement),
        [(
            5,
            RowProblem::CurrencyMismatch {
                field: Field::Amount,
                expected: Currency::EUR,
                found: Currency::USD,
            }
        )]
    );
}

#[test]
fn files_without_a_header_use_positions() {
    let mut mapping = CsvMapping::new(
        Currency::GBP,
        date_column(Column::Index(0), "%d/%m/%Y"),
        AmountColumns::Signed(Column::Index(1)),
        Column::Index(2),
    );
    mapping.has_header = false;
    let statement = import(
        mapping.clone(),
        "01/11/2026,-3.40,Coffee\n02/11/2026,10.00,Refund\n",
    )
    .unwrap();
    let read: Vec<_> = lines(&statement)
        .into_iter()
        .map(|(n, line)| (n, line.date, line.amount.minor()))
        .collect();
    assert_eq!(
        read,
        [(1, date(2026, 11, 1), -340), (2, date(2026, 11, 2), 1000)]
    );

    mapping.balance = Some(Column::Index(4));
    assert_eq!(
        import(mapping, "01/11/2026,-3.40,Coffee\n")
            .unwrap_err()
            .to_string(),
        "the file has no column 5"
    );
}

#[test]
fn a_date_column_can_carry_the_time() {
    let mut mapping = simple_mapping();
    mapping.date = date_column(named("Date"), "%Y-%m-%dT%H:%M:%S");
    let statement = import(
        mapping.clone(),
        "Date,Amount,Description\n2026-11-01T09:05:33,-1.00,A\n",
    )
    .unwrap();
    let (_, line) = &lines(&statement)[0];
    assert_eq!(
        (line.date, line.time),
        (date(2026, 11, 1), NaiveTime::from_hms_opt(9, 5, 33))
    );

    mapping.time = Some(TimeColumn {
        column: named("Time"),
        format: TimeFormat::new("%H:%M").unwrap(),
    });
    let statement = import(
        mapping,
        "Date,Time,Amount,Description\n2026-11-01T09:05:33,10:00,-1.00,A\n2026-11-01T09:05:33,,-1.00,B\n",
    )
    .unwrap();
    let times: Vec<_> = lines(&statement)
        .into_iter()
        .map(|(_, line)| line.time)
        .collect();
    assert_eq!(
        times,
        [
            NaiveTime::from_hms_opt(10, 0, 0),
            NaiveTime::from_hms_opt(9, 5, 33)
        ],
        "a filled time column wins; a blank one keeps the date's time"
    );
}

#[test]
fn blank_required_fields_are_flagged() {
    let statement = import(
        simple_mapping(),
        "Date,Amount,Description\n,-1.00,A\n01/11/2026,,B\n01/11/2026, -1.00 ,\n01/11/2026,-1.00\n",
    )
    .unwrap();
    assert_eq!(
        problems(&statement),
        [
            (2, RowProblem::Missing(Field::Date)),
            (3, RowProblem::Missing(Field::Amount)),
            (
                5,
                RowProblem::ColumnCount {
                    expected: 3,
                    found: 2
                }
            ),
        ]
    );
    let (_, line) = &lines(&statement)[0];
    assert_eq!(
        (line.amount.minor(), line.description_raw.as_str()),
        (-100, ""),
        "a blank description is allowed; amounts are trimmed"
    );
}

#[test]
fn trailing_delimiters_on_rows_are_fine() {
    let statement = import(
        simple_mapping(),
        "Date,Amount,Description\n01/11/2026,-1.00,A,\n01/11/2026,-2.00,B,,\n",
    )
    .unwrap();
    assert!(!statement.has_flagged());
    assert_eq!(statement.lines().count(), 2);
}

#[test]
fn a_header_ending_with_a_delimiter_fits_the_rows() {
    let statement = import(
        simple_mapping(),
        "Date,Amount,Description,\n01/11/2026,-1.00,A\n01/11/2026,-2.00,B,\n01/11/2026,-3.00,C,x\n",
    )
    .unwrap();
    assert_eq!(
        statement.lines().count(),
        2,
        "with or without the empty field"
    );
    assert_eq!(
        problems(&statement),
        [(
            4,
            RowProblem::ColumnCount {
                expected: 3,
                found: 4
            }
        )],
        "a value under the unnamed column isn't dropped silently"
    );

    let parsed = importer(simple_mapping())
        .parse(&SourceFile::from_bytes(
            "s.csv",
            "Date,,Amount,Description,,\n",
        ))
        .unwrap();
    assert_eq!(parsed.width(), 4, "only trailing empty cells are left out");

    let mut mapping = simple_mapping();
    mapping.balance = Some(Column::Index(3));
    assert_eq!(
        import(mapping, "Date,Amount,Description,\n")
            .unwrap_err()
            .to_string(),
        "the file has no column 4"
    );
}

/// The rows a stray quote swallows aren't read as text: the row it starts
/// in is flagged, at its own line, with how far it runs.
#[test]
fn a_stray_quote_flags_the_rows_it_merges() {
    let unclosed = "Date,Amount,Description\n\
                    01/11/2026,-1.00,\"oops\n\
                    02/11/2026,-2.00,b\n\
                    03/11/2026,-3.00,c\n";
    let statement = import(simple_mapping(), unclosed).unwrap();
    assert_eq!(statement.lines().count(), 0);
    assert_eq!(
        problems(&statement),
        [(2, RowProblem::UnclosedQuote { lines: 3 })]
    );

    let between_two = "Date,Amount,Description\n\
                       01/11/2026,-1.00,\"a\n\
                       02/11/2026,-2.00,b\n\
                       03/11/2026,-3.00,c\"\n\
                       04/11/2026,-4.00,d\n";
    let statement = import(simple_mapping(), between_two).unwrap();
    assert_eq!(
        problems(&statement),
        [(2, RowProblem::MultiLineRow { lines: 3 })],
        "even with the right number of columns"
    );
    let read: Vec<_> = lines(&statement)
        .into_iter()
        .map(|(n, line)| (n, line.amount.minor()))
        .collect();
    assert_eq!(read, [(5, -400)]);

    let mut mapping = simple_mapping();
    mapping.delimiter = Some(Delimiter::Comma);
    let statement = import(mapping, "Date,Amount,Description\r01/11/2026,-1.00,\"open").unwrap();
    assert_eq!(
        problems(&statement),
        [(2, RowProblem::UnclosedQuote { lines: 1 })],
        "an unclosed quote on the last line is flagged too"
    );
}

#[test]
fn multiline_fields_reads_line_breaks_in_quoted_fields() {
    let mut mapping = simple_mapping();
    mapping.multiline_fields = true;
    let text = "Date,Amount,Description\r\n\
                01/11/2026,-1.00,\"two\r\nlines\"\r\n\
                02/11/2026,-2.00,b\r\n";
    let statement = import(mapping.clone(), text).unwrap();
    assert!(!statement.has_flagged());
    let read: Vec<_> = lines(&statement)
        .into_iter()
        .map(|(n, line)| (n, line.description_raw))
        .collect();
    assert_eq!(read, [(2, "two\r\nlines".to_owned()), (4, "b".to_owned())]);

    let unclosed = "Date,Amount,Description\n01/11/2026,-1.00,\"oops\n02/11/2026,-2.00,b\n";
    assert_eq!(
        problems(&import(mapping, unclosed).unwrap()),
        [(2, RowProblem::UnclosedQuote { lines: 2 })],
        "a quote still open at the end of the file is never text"
    );
}

#[test]
fn a_quote_inside_an_unquoted_field_is_a_character() {
    let statement = import(
        simple_mapping(),
        "Date,Amount,Description\n01/11/2026,-1.00,6\" pizza\n02/11/2026,-2.00,12\" sub",
    )
    .unwrap();
    assert!(!statement.has_flagged());
    let read: Vec<_> = lines(&statement)
        .into_iter()
        .map(|(_, line)| line.description_raw)
        .collect();
    assert_eq!(read, ["6\" pizza", "12\" sub"]);
}

#[test]
fn a_broken_header_fails_the_file() {
    assert_eq!(
        import(
            simple_mapping(),
            "Date,Amount,\"Description\n01/11/2026,-1.00,a\n"
        )
        .unwrap_err()
        .to_string(),
        "the file isn't a readable csv file: no delimiter splits the rows into columns"
    );
    let mut mapping = simple_mapping();
    mapping.delimiter = Some(Delimiter::Comma);
    assert_eq!(
        import(
            mapping.clone(),
            "Date,Amount,\"Description\n01/11/2026,-1.00,a\n"
        )
        .unwrap_err()
        .to_string(),
        "the file isn't a readable csv file (line 1): the header row has a quote that's never closed"
    );
    let two_line_header = "Date,Amount,\"Desc\nription\"\n01/11/2026,-1.00,a\n";
    assert_eq!(
        import(mapping.clone(), two_line_header)
            .unwrap_err()
            .to_string(),
        "the file isn't a readable csv file (line 1): the header row runs over more than one line"
    );
    mapping.multiline_fields = true;
    mapping.description = vec![named("Desc\nription")];
    assert!(import(mapping, two_line_header).is_ok());
}

#[test]
fn lone_carriage_returns_end_lines() {
    let text = "Date,Amount,Description\r01/11/2026,-1.00,a\r\r02/11/2026,-2.00,b\r";
    let statement = import(simple_mapping(), text).unwrap();
    let read: Vec<_> = lines(&statement)
        .into_iter()
        .map(|(n, line)| (n, line.amount.minor()))
        .collect();
    assert_eq!(read, [(2, -100), (4, -200)]);

    let mut mapping = simple_mapping();
    mapping.skip_rows = 2;
    let statement = import(mapping, &format!("Account\rSummary\r{text}")).unwrap();
    let read: Vec<_> = lines(&statement).into_iter().map(|(n, _)| n).collect();
    assert_eq!(read, [4, 6], "skipped lines are counted");
}

/// `skip_rows` counts physical lines, so a summary cell with a line break
/// in it is two. Counted as one, the skip would end inside the cell and
/// the rest of it would be read as the header or the first row: the file
/// fails instead, at the line the cell opens on.
#[test]
fn a_skip_ending_inside_a_quoted_field_fails_the_file() {
    let text = "Account,\"Example\nCurrent Account\"\n\
                Date,Amount,Description\n\
                01/11/2026,-1.00,a\n";
    let mut mapping = simple_mapping();
    mapping.skip_rows = 2;
    let read: Vec<_> = lines(&import(mapping.clone(), text).unwrap())
        .into_iter()
        .map(|(n, line)| (n, line.amount.minor()))
        .collect();
    assert_eq!(read, [(4, -100)]);

    mapping.skip_rows = 1;
    let error = "the file isn't a readable csv file (line 1): \
                 the lines to skip end inside a quoted field";
    assert_eq!(
        import(mapping.clone(), text).unwrap_err().to_string(),
        error
    );
    mapping.delimiter = Some(Delimiter::Comma);
    assert_eq!(import(mapping, text).unwrap_err().to_string(), error);

    // Without a header, the cell's closing quote opened a field that took
    // in the first rows, flagged at a summary line.
    let mut mapping = CsvMapping::new(
        Currency::GBP,
        date_column(Column::Index(0), "%d/%m/%Y"),
        AmountColumns::Signed(Column::Index(1)),
        Column::Index(2),
    );
    mapping.has_header = false;
    mapping.skip_rows = 1;
    let text = "Summary,\"Example\n\"\n\
                01/11/2026,-1.00,a\n\
                02/11/2026,-2.00,\"b\"\n\
                03/11/2026,-3.00,c\n";
    assert_eq!(
        import(mapping.clone(), text).unwrap_err().to_string(),
        error
    );
    mapping.skip_rows = 2;
    let read: Vec<_> = lines(&import(mapping, text).unwrap())
        .into_iter()
        .map(|(n, line)| (n, line.amount.minor()))
        .collect();
    assert_eq!(read, [(3, -100), (4, -200), (5, -300)]);
}

#[test]
fn quotes_in_skipped_free_text_are_characters() {
    let text = "Account: 6\" pizza fund\r\n\
                Note: a \"quoted\" word, \"unclosed\r\n\
                Date,Amount,Description\r\n\
                01/11/2026,-1.00,a\r\n";
    let mut mapping = simple_mapping();
    mapping.skip_rows = 2;
    let read: Vec<_> = lines(&import(mapping, text).unwrap())
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert_eq!(read, [4]);
}

#[test]
fn header_only_and_empty_files() {
    let statement = import(simple_mapping(), "Date,Amount,Description\n").unwrap();
    assert!(statement.rows().is_empty(), "no transactions in the period");

    let mut mapping = simple_mapping();
    mapping.delimiter = Some(Delimiter::Comma);
    assert_eq!(
        import(mapping.clone(), "").unwrap_err().to_string(),
        "the file isn't a readable csv file: the file has no rows"
    );
    mapping.skip_rows = 3;
    assert_eq!(
        import(mapping, "Summary\n").unwrap_err().to_string(),
        "the file isn't a readable csv file: the file ends within the lines to skip"
    );
}

#[test]
fn columns_must_be_found_exactly_once() {
    assert_eq!(
        import(simple_mapping(), "When,Amount,Description\n")
            .unwrap_err()
            .to_string(),
        "the file has no “Date” column"
    );
    assert_eq!(
        import(simple_mapping(), "Date,Date,Amount,Description\n")
            .unwrap_err()
            .to_string(),
        "the file has more than one “Date” column"
    );
    assert!(
        import(simple_mapping(), " Date , Amount ,Description\n").is_ok(),
        "header cells are trimmed"
    );
}

#[test]
fn unusable_mappings_are_refused() {
    let mut mapping = simple_mapping();
    mapping.description.clear();
    assert_eq!(
        CsvImporter::new("x", mapping).unwrap_err(),
        MappingError::NoDescription
    );

    let mut mapping = simple_mapping();
    mapping.amount = AmountColumns::Split {
        debit: named("Amount"),
        credit: named("Amount"),
        signs: SplitSigns::Unsigned,
    };
    assert_eq!(
        CsvImporter::new("x", mapping).unwrap_err(),
        MappingError::SameDebitAndCredit
    );

    let mut mapping = simple_mapping();
    mapping.has_header = false;
    assert_eq!(
        CsvImporter::new("x", mapping).unwrap_err().to_string(),
        "the column “Date” is named, but the file has no header row to find it in"
    );
}

#[test]
fn detection() {
    let signed = importer(signed_amount_mapping());
    let simple = CsvImporter::new("Simple", simple_mapping()).unwrap();
    let importers: [&dyn AnyImporter; 2] = [&simple, &signed];
    let ranked = |file: &SourceFile| -> Vec<(String, Detection)> {
        detect(&importers, file)
            .into_iter()
            .map(|candidate| (candidate.importer.name().to_owned(), candidate.detection))
            .collect()
    };

    assert_eq!(
        ranked(&fixture!("signed-amount.csv")),
        [
            ("Simple".to_owned(), Detection::Source),
            ("Test".to_owned(), Detection::Source),
        ],
        "both find all their columns; the caller has to choose"
    );
    assert_eq!(
        ranked(&fixture!("semicolon-decimal-comma.csv")),
        [
            ("Simple".to_owned(), Detection::Format),
            ("Test".to_owned(), Detection::Format),
        ]
    );
    assert!(
        ranked(&SourceFile::from_bytes(
            "x.pdf",
            b"%PDF-1.7\n\x00\xff".as_slice()
        ))
        .is_empty()
    );
    assert_eq!(
        ranked(&SourceFile::from_bytes("x.txt", "just text")),
        [
            ("Simple".to_owned(), Detection::Extension),
            ("Test".to_owned(), Detection::Extension),
        ]
    );

    // A stray quote in a row doesn't change what the header says.
    assert_eq!(
        ranked(&SourceFile::from_bytes(
            "s.csv",
            "Date,Amount,Description\n01/11/2026,-1.00,\"oops\n02/11/2026,-2.00,b\n"
        ))[0],
        ("Simple".to_owned(), Detection::Source)
    );
    // Without a header, a merged row is all the importer has to go on.
    let mut headerless = CsvMapping::new(
        Currency::GBP,
        date_column(Column::Index(0), "%d/%m/%Y"),
        AmountColumns::Signed(Column::Index(1)),
        Column::Index(2),
    );
    headerless.has_header = false;
    headerless.delimiter = Some(Delimiter::Comma);
    let headerless = importer(headerless);
    assert_eq!(
        headerless.detect(&SourceFile::from_bytes(
            "s.csv",
            "01/11/2026,-1.00,\"oops\n02/11/2026,-2.00,b\n"
        )),
        Detection::Extension
    );
    assert_eq!(
        headerless.detect(&SourceFile::from_bytes(
            "s.csv",
            "01/11/2026,-1.00,\"a\n02/11/2026\"\n03/11/2026,-3.00,c\n"
        )),
        Detection::Format,
        "the width comes from the first row that's one line"
    );
    assert_eq!(signed.name(), "Test");
    assert_eq!(signed.format(), SourceFormat::Csv);
}

#[test]
fn debug_output_leaves_out_transaction_text() {
    let importer = importer(signed_amount_mapping());
    let parsed = importer.parse(&fixture!("signed-amount.csv")).unwrap();
    let debug = format!("{parsed:?} {:?}", parsed.records()[0]);
    assert!(!debug.contains("Bean"), "{debug}");
    assert!(!debug.contains("tx_0001"), "{debug}");
    assert!(debug.contains("records: 10"), "{debug}");
}
