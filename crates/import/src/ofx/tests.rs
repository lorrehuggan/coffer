//! End-to-end tests: files through [`OfxImporter`], including the fixtures
//! in `fixtures/ofx/`.

use chrono::NaiveDate;
use coffer_core::money::{Currency, Money, ParseAmountError};
use coffer_core::statement::{
    DatedBalance, Field, RowProblem, SourceLocation, Statement, StatementLine,
};

use super::*;
use crate::csv::{
    AmountColumns, Column, CsvImporter, CsvMapping, DateColumn, DateFormat, TextEncoding,
};
use crate::{AnyImporter, detect};

macro_rules! fixture {
    ($name:literal) => {
        SourceFile::from_bytes(
            $name,
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../fixtures/ofx/",
                $name
            ))
            .as_slice(),
        )
    };
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

fn import(file: &SourceFile) -> Result<Statement, ImportError> {
    OfxImporter::new().import(file)
}

fn import_text(text: &str) -> Result<Statement, String> {
    import(&SourceFile::from_bytes("s.ofx", text)).map_err(|error| error.to_string())
}

fn entry(location: SourceLocation) -> u64 {
    match location {
        SourceLocation::Entry(n) => n,
        SourceLocation::Line(_) => unreachable!("OFX transactions are entries"),
    }
}

/// The read lines, one string each:
/// `entry date [time] amount | merchant | description | external ID | type`,
/// with `-` for a missing optional field.
fn summary(statement: &Statement) -> Vec<String> {
    let optional = |text: &Option<String>| text.clone().unwrap_or_else(|| "-".to_owned());
    statement
        .lines()
        .map(|(location, line)| {
            let time = line.time.map(|time| format!(" {time}")).unwrap_or_default();
            format!(
                "{} {}{time} {} | {} | {} | {} | {}",
                entry(location),
                line.date,
                line.amount.to_decimal_string(),
                optional(&line.merchant),
                line.description_raw,
                optional(&line.external_id),
                optional(&line.source_type),
            )
        })
        .collect()
}

fn problems(statement: &Statement) -> Vec<(u64, RowProblem)> {
    statement
        .flagged()
        .map(|(location, problem)| (entry(location), problem.clone()))
        .collect()
}

/// A minimal OFX 1.x bank statement around `transactions`.
fn sgml_statement(currency: &str, transactions: &str) -> String {
    format!(
        "OFXHEADER:100\nDATA:OFXSGML\nVERSION:102\nENCODING:UTF-8\nCHARSET:NONE\n\n\
         <OFX><BANKMSGSRSV1><STMTTRNRS><STMTRS><CURDEF>{currency}\
         <BANKTRANLIST>{transactions}</BANKTRANLIST></STMTRS></STMTTRNRS></BANKMSGSRSV1></OFX>"
    )
}

#[test]
fn sgml_bank_fixture() {
    let file = fixture!("bank-sgml.ofx");
    let parsed = OfxImporter::new().parse(&file).unwrap();
    assert_eq!(parsed.version(), Some(102));
    assert_eq!(parsed.encoding(), TextEncoding::Windows1252);
    assert_eq!(parsed.statements().len(), 1);
    assert_eq!(parsed.statements()[0].kind(), OfxStatementKind::Bank);
    assert_eq!(parsed.statements()[0].account_suffix(), Some("0000"));

    let statement = OfxImporter::new().normalize(parsed).unwrap();
    assert_eq!(statement.format(), SourceFormat::Ofx);
    assert_eq!(statement.preset_id(), None);
    assert_eq!(statement.currency(), Currency::GBP);
    assert!(!statement.has_flagged());
    assert_eq!(
        summary(&statement),
        [
            "1 2026-11-02 -12.40 | Corner Café | Corner Café CONTACTLESS | EX-0001 | DEBIT",
            "2 2026-11-03 1500.00 | EXAMPLE EMPLOYER LTD | EXAMPLE EMPLOYER LTD SALARY NOV | EX-0002 | CREDIT",
            "3 2026-11-05 -45.00 | Example Energy & Gas | Example Energy & Gas | EX-0003 | DIRECTDEBIT",
            "4 2026-11-10 -8.99 | SANDWICH EXAMPLE LONDON GB 1234 | SANDWICH EXAMPLE LONDON GB 12345678 VISA | EX-0004 | POS",
            "5 2026-11-12 -50.00 | CASH £50 EXAMPLE STREET | CASH £50 EXAMPLE STREET | EX-0005 | ATM",
            "6 2026-11-20 -17.24 | Boulangerie Exemple | Boulangerie Exemple | EX-0006 | DEBIT",
            "7 2026-11-25 -120.00 | CHEQUE 000101 | CHEQUE 000101 | EX-0007 | CHECK",
        ]
    );
    // ORIGCURRENCY says the amount is already in GBP; the original amount
    // would need the rate and rounding, so it isn't derived.
    assert!(
        statement
            .lines()
            .all(|(_, line)| line.original_amount.is_none())
    );
    assert!(statement.lines().all(|(_, line)| line.note.is_none()));
    assert_eq!(
        statement.balances(),
        [DatedBalance {
            date: date(2026, 11, 30),
            balance: Money::new(238_147, Currency::GBP),
        }],
        "LEDGERBAL only; AVAILBAL isn't a ledger balance"
    );
}

#[test]
fn xml_credit_card_fixture() {
    let statement = import(&fixture!("creditcard-xml.ofx")).unwrap();
    assert_eq!(statement.currency(), Currency::USD);
    assert!(!statement.has_flagged());
    assert_eq!(
        summary(&statement),
        [
            "1 2026-11-03 12:00:00 -23.50 | Example Books | Example Books | CC-2026-0001 | DEBIT",
            "2 2026-11-05 15:30:12 -4.75 | Example Coffee Co | Example Coffee Co Store 42 | CC-2026-0002 | DEBIT",
            "3 2026-11-10 200.00 | PAYMENT - THANK YOU | PAYMENT - THANK YOU | CC-2026-0003 | CREDIT",
            "4 2026-11-18 00:00:00 -61.20 | Example Hardware & Garden | Example Hardware & Garden | CC-2026-0004 | DEBIT",
            "5 2026-11-25 00:00:00 -9.99 | Example Streaming | Example Streaming Service Monthly | CC-2026-0005 | DEBIT",
        ]
    );
    assert_eq!(
        statement.balances(),
        [DatedBalance {
            date: date(2026, 11, 30),
            balance: Money::new(-123_456, Currency::USD),
        }],
        "a card balance owed is negative, as the file states it"
    );
    // The payee's address is never read.
    for (_, line) in statement.lines() {
        assert!(!line.description_raw.contains("Fake Street"));
    }
}

#[test]
fn qfx_fixture() {
    let file = fixture!("intuit.qfx");
    assert_eq!(file.extension().as_deref(), Some("qfx"));
    let parsed = OfxImporter::new().parse(&file).unwrap();
    assert_eq!(parsed.version(), Some(102));
    let statement = OfxImporter::new().normalize(parsed).unwrap();
    assert_eq!(statement.currency(), Currency::USD);
    assert!(!statement.has_flagged());
    assert_eq!(
        summary(&statement),
        [
            "1 2026-11-01 12:00:00 -54.21 | EXAMPLE MARKET #0000 | EXAMPLE MARKET #0000 POS PURCHASE | 2026110100001 | DEBIT",
            "2 2026-11-02 12:00:00 1200.00 | EXAMPLE PAYROLL | EXAMPLE PAYROLL DIRECT DEP | 2026110200001 | CREDIT",
            "3 2026-11-04 12:00:00 -300.00 | CHECK 1001 | CHECK 1001 | 2026110400001 | CHECK",
        ]
    );
    assert_eq!(
        statement.balances(),
        [DatedBalance {
            date: date(2026, 11, 30),
            balance: Money::new(284_579, Currency::USD),
        }]
    );
}

#[test]
fn edge_cases_fixture() {
    let file = fixture!("edge-cases.ofx");
    let parsed = OfxImporter::new().parse(&file).unwrap();
    assert_eq!(parsed.version(), Some(103));
    assert_eq!(parsed.encoding(), TextEncoding::Utf8);
    let statement = OfxImporter::new().normalize(parsed).unwrap();
    assert_eq!(statement.currency(), Currency::EUR);
    assert_eq!(statement.rows().len(), 13, "every transaction is kept");
    let entries: Vec<_> = statement
        .rows()
        .iter()
        .map(|row| entry(row.location))
        .collect();
    assert_eq!(entries, (1..=13).collect::<Vec<_>>(), "in file order");

    assert_eq!(
        summary(&statement),
        [
            "1 2026-11-01 09:15:00 -3.50 | - | Bäckerei Beispiel | - | POS",
            "5 2026-11-05 -2.00 | First of a pair | First of a pair | EDGE-5 | DEBIT",
            "11 2026-11-15 10:30:00 -4.20 | Same currency | Same currency | EDGE-11 | DEBIT",
            "13 2026-11-17 0.01 | - |  | EDGE-13 | OTHER",
        ]
    );
    assert_eq!(
        problems(&statement),
        [
            (2, RowProblem::Missing(Field::Date)),
            (
                3,
                RowProblem::Unreadable {
                    field: Field::Date,
                    reason: "isn't a real date",
                }
            ),
            (
                4,
                RowProblem::Amount {
                    field: Field::Amount,
                    error: ParseAmountError::TooPrecise {
                        currency: Currency::EUR
                    },
                }
            ),
            (
                6,
                RowProblem::DuplicateExternalId {
                    first: SourceLocation::Entry(5)
                }
            ),
            (
                7,
                RowProblem::Unsupported {
                    reason: "it corrects or deletes an earlier transaction (OFX CORRECTACTION)",
                }
            ),
            (
                8,
                RowProblem::CurrencyMismatch {
                    field: Field::Amount,
                    expected: Currency::EUR,
                    found: Currency::USD,
                }
            ),
            (
                9,
                RowProblem::Unreadable {
                    field: Field::Amount,
                    reason: "is given more than once",
                }
            ),
            (10, RowProblem::Missing(Field::Amount)),
            (
                12,
                RowProblem::Unreadable {
                    field: Field::Currency,
                    reason: "isn't a known ISO 4217 code",
                }
            ),
        ]
    );
    assert!(statement.balances().is_empty(), "the file has no LEDGERBAL");
}

#[test]
fn several_statements_are_listed_and_one_is_chosen() {
    let file = fixture!("multi-statement.ofx");
    let parsed = OfxImporter::new().parse(&file).unwrap();
    assert_eq!(parsed.version(), Some(211));
    let listed: Vec<_> = parsed
        .statements()
        .iter()
        .map(|statement| {
            (
                statement.kind(),
                statement.currency(),
                statement.account_suffix(),
                statement.transaction_count(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            (OfxStatementKind::Bank, Some(Currency::GBP), Some("1111"), 2),
            (OfxStatementKind::Bank, Some(Currency::GBP), Some("2222"), 1),
            (
                OfxStatementKind::CreditCard,
                Some(Currency::GBP),
                Some("3333"),
                1
            ),
        ]
    );

    assert_eq!(
        import(&file).unwrap_err().to_string(),
        "the file holds 3 statements; choose which one to import"
    );

    let savings = OfxImporter::for_statement(1).import(&file).unwrap();
    assert_eq!(
        summary(&savings),
        ["3 2026-11-15 250.00 | Savings in | Savings in | M-3 | OTHER"],
        "entries count across the whole file"
    );
    assert_eq!(
        savings.balances(),
        [DatedBalance {
            date: date(2026, 11, 30),
            balance: Money::new(525_000, Currency::GBP),
        }]
    );

    let card = OfxImporter::for_statement(2).import(&file).unwrap();
    assert_eq!(
        card.lines().map(|(at, _)| entry(at)).collect::<Vec<_>>(),
        [4]
    );

    assert_eq!(
        OfxImporter::for_statement(3)
            .import(&file)
            .unwrap_err()
            .to_string(),
        "the file isn't a readable ofx file: the file doesn't have the statement chosen"
    );
}

#[test]
fn detection() {
    let importer = OfxImporter::new();
    for file in [
        fixture!("bank-sgml.ofx"),
        fixture!("creditcard-xml.ofx"),
        fixture!("intuit.qfx"),
        fixture!("edge-cases.ofx"),
        fixture!("multi-statement.ofx"),
    ] {
        assert_eq!(importer.detect(&file), Detection::Format, "{}", file.name());
    }
    // Recognised by its contents whatever the name.
    let renamed = SourceFile::from_bytes("download.txt", fixture!("intuit.qfx").bytes());
    assert_eq!(importer.detect(&renamed), Detection::Format);

    assert_eq!(
        importer.detect(&SourceFile::from_bytes("Statement.QFX", "not ofx")),
        Detection::Extension
    );
    assert_eq!(
        importer.detect(&SourceFile::from_bytes("s.ofx", "")),
        Detection::Extension
    );
    assert_eq!(
        importer.detect(&SourceFile::from_bytes("s.csv", "Date,Amount\n")),
        Detection::No
    );
    assert_eq!(importer.name(), "OFX / QFX");
    assert_eq!(importer.format(), SourceFormat::Ofx);
}

#[test]
fn ofx_files_rank_above_a_generic_csv_mapping() {
    let csv = CsvImporter::new(
        "Generic CSV",
        CsvMapping::new(
            Currency::GBP,
            DateColumn {
                column: Column::named("Date"),
                format: DateFormat::new("%d/%m/%Y").unwrap(),
            },
            AmountColumns::Signed(Column::named("Amount")),
            Column::named("Description"),
        ),
    )
    .unwrap();
    let ofx = OfxImporter::new();
    let importers: [&dyn AnyImporter; 2] = [&csv, &ofx];
    for file in [
        fixture!("bank-sgml.ofx"),
        fixture!("creditcard-xml.ofx"),
        fixture!("intuit.qfx"),
        fixture!("edge-cases.ofx"),
        fixture!("multi-statement.ofx"),
    ] {
        let candidates = detect(&importers, &file);
        let ranked: Vec<_> = candidates
            .iter()
            .map(|candidate| (candidate.importer.name(), candidate.detection))
            .collect();
        assert_eq!(
            ranked.first(),
            Some(&("OFX / QFX", Detection::Format)),
            "{}",
            file.name()
        );
        assert!(
            ranked
                .get(1)
                .is_none_or(|(_, detection)| *detection < Detection::Format),
            "{}: no tie at the top: {ranked:?}",
            file.name()
        );
    }
}

#[test]
fn no_header_utf16_and_crlf() {
    let body = sgml_statement(
        "GBP",
        "<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20261102<TRNAMT>-1.00<FITID>A<NAME>Café</STMTTRN>",
    );
    let bare = &body[body.find("<OFX>").unwrap()..];
    let statement = import_text(bare).unwrap();
    assert_eq!(statement.lines().count(), 1);

    let utf16: Vec<u8> = [0xFF, 0xFE]
        .into_iter()
        .chain(
            body.replace('\n', "\r\n")
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        )
        .collect();
    let parsed = OfxImporter::new()
        .parse(&SourceFile::from_bytes("s.ofx", utf16))
        .unwrap();
    assert_eq!(parsed.encoding(), TextEncoding::Utf16Le);
    let statement = OfxImporter::new().normalize(parsed).unwrap();
    let (_, line) = statement.lines().next().unwrap();
    assert_eq!(line.description_raw, "Café");
}

#[test]
fn a_statement_without_transactions() {
    let statement = import_text(&sgml_statement("GBP", "")).unwrap();
    assert!(statement.rows().is_empty());
    let text =
        "<OFX><STMTRS><CURDEF>GBP<LEDGERBAL><BALAMT>0<DTASOF>20261130</LEDGERBAL></STMTRS></OFX>";
    let statement = import_text(text).unwrap();
    assert!(statement.rows().is_empty());
    assert_eq!(statement.balances().len(), 1);
}

#[test]
fn payee_and_name() {
    let text = sgml_statement(
        "GBP",
        "<STMTTRN><DTPOSTED>20261102<TRNAMT>-1.00<FITID>A\
           <NAME>Short Name<PAYEE><NAME>Payee Name<ADDR1>1 Fake Street</PAYEE></STMTTRN>",
    );
    let statement = import_text(&text).unwrap();
    let (_, line) = statement.lines().next().unwrap();
    assert_eq!(line.merchant.as_deref(), Some("Short Name"), "NAME first");
    assert_eq!(line.source_type, None, "TRNTYPE missing is allowed");
}

#[test]
fn files_that_fail_as_a_whole() {
    let error = |text: &str| import_text(text).unwrap_err();
    assert_eq!(
        error("<OFX><SIGNONMSGSRSV1></SIGNONMSGSRSV1></OFX>"),
        "the file isn't a readable ofx file: the file has no bank or credit card statement"
    );
    assert_eq!(
        error("<OFX><INVSTMTMSGSRSV1><INVSTMTRS><CURDEF>USD</INVSTMTRS></INVSTMTMSGSRSV1></OFX>"),
        "Coffer can't import investment statements"
    );
    assert_eq!(
        error(&sgml_statement("", "")),
        "the file isn't a readable ofx file (line 7): the statement has no currency (CURDEF)"
    );
    assert_eq!(
        error(&sgml_statement("ZZZ", "")),
        "the file isn't a readable ofx file (line 7): the statement's currency (CURDEF) isn't a known ISO 4217 code"
    );
    assert_eq!(
        error(
            "<OFX><STMTRS><CURDEF>GBP<LEDGERBAL><BALAMT>1.005<DTASOF>20261130</LEDGERBAL></STMTRS></OFX>"
        ),
        "the file isn't a readable ofx file (line 1): the ledger balance (LEDGERBAL) can't be read"
    );
    assert_eq!(
        error("<OFX><STMTRS><CURDEF>GBP<LEDGERBAL><BALAMT>1.00</LEDGERBAL></STMTRS></OFX>"),
        "the file isn't a readable ofx file (line 1): the ledger balance (LEDGERBAL) can't be read"
    );
    assert_eq!(
        error("Date,Amount\n01/11/2026,1.00\n"),
        "the file isn't a readable ofx file: the file doesn't start with an OFX header"
    );
}

#[test]
fn a_truncated_download_fails_instead_of_losing_transactions() {
    let file = fixture!("bank-sgml.ofx");
    let cut = &file.bytes()[..file.bytes().len() / 2];
    let error = import(&SourceFile::from_bytes("s.ofx", cut))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("the file ends before this element is closed"),
        "{error}"
    );
}

#[test]
fn errors_and_debug_leave_the_contents_out() {
    let text = sgml_statement(
        "GBP",
        "<STMTTRN><DTPOSTED>secret-date<TRNAMT>secret-amount<NAME>Secret Merchant</STMTTRN>",
    );
    let statement = import_text(&text).unwrap();
    for (_, problem) in statement.flagged() {
        assert!(!problem.to_string().contains("secret"), "{problem}");
    }
    let error = import_text("<OFX>\n<STMTTRN>Secret Merchant\n</OFX>").unwrap_err();
    assert!(!error.contains("Secret"), "{error}");

    let parsed = OfxImporter::new()
        .parse(&fixture!("bank-sgml.ofx"))
        .unwrap();
    let debug = format!("{parsed:?}");
    for secret in ["Corner", "EX-0001", "00000000", "-12.40"] {
        assert!(!debug.contains(secret), "{secret} in {debug}");
    }
    assert!(debug.contains("transactions: 7"), "{debug}");
}

#[test]
fn every_line_is_in_the_statement_currency() {
    // `Statement::push` flags the rest; the importer only reads them in
    // the currency the file names.
    let statement = import(&fixture!("edge-cases.ofx")).unwrap();
    assert!(
        statement
            .lines()
            .all(|(_, line): (_, &StatementLine)| line.amount.currency() == Currency::EUR)
    );
}
