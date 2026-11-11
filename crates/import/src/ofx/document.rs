//! The statements in an OFX document, before normalizing.

use std::fmt;

use coffer_core::money::Currency;
use coffer_core::statement::{SourceFormat, SourceLocation};

use super::markup::Element;
use crate::ImportError;
use crate::csv::TextEncoding;

/// An OFX file read into its statements: the
/// [`OfxImporter`](super::OfxImporter)'s `Parsed` type.
///
/// A file usually holds one statement, but a download can cover several
/// accounts. [`statements`](OfxFile::statements) lists them so the user can
/// pick one ([`OfxImporter::for_statement`](super::OfxImporter::for_statement)).
#[derive(Clone)]
pub struct OfxFile {
    pub(super) version: Option<u32>,
    pub(super) encoding: TextEncoding,
    pub(super) statements: Vec<OfxStatement>,
}

impl OfxFile {
    /// The OFX version the header states (`102`, `220`); `None` without a
    /// header.
    pub fn version(&self) -> Option<u32> {
        self.version
    }

    /// The encoding the file was decoded with: its byte order mark, or the
    /// one its header states. Never [`TextEncoding::Auto`].
    pub fn encoding(&self) -> TextEncoding {
        self.encoding
    }

    /// The statements in file order.
    pub fn statements(&self) -> &[OfxStatement] {
        &self.statements
    }
}

/// Shows the version, encoding and statements only.
impl fmt::Debug for OfxFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OfxFile")
            .field("version", &self.version)
            .field("encoding", &self.encoding)
            .field("statements", &self.statements)
            .finish()
    }
}

/// What kind of account a statement is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OfxStatementKind {
    /// A bank account (`STMTRS`).
    Bank,
    /// A credit card (`CCSTMTRS`).
    CreditCard,
    /// An investment account (`INVSTMTRS`). Listed so that nothing in the
    /// file goes unmentioned, but not imported.
    Investment,
}

/// One statement in an OFX file, before normalizing.
#[derive(Clone)]
pub struct OfxStatement {
    pub(super) kind: OfxStatementKind,
    pub(super) line: u64,
    /// `CURDEF`, as written.
    pub(super) currency: Option<String>,
    /// `ACCTID`, as written. Only its end is ever shown.
    pub(super) account_id: Option<String>,
    pub(super) transactions: Vec<Transaction>,
    pub(super) ledger_balance: Option<Element>,
}

impl OfxStatement {
    pub fn kind(&self) -> OfxStatementKind {
        self.kind
    }

    /// The statement's currency (`CURDEF`), if it names a known one.
    pub fn currency(&self) -> Option<Currency> {
        self.currency.as_deref()?.trim().parse().ok()
    }

    /// How many transactions (`STMTTRN`) it holds.
    pub fn transaction_count(&self) -> usize {
        self.transactions.len()
    }

    /// The last four characters of the account or card number, to tell
    /// statements apart when choosing one. The rest is never exposed.
    pub fn account_suffix(&self) -> Option<&str> {
        let id = self.account_id.as_deref()?.trim();
        let start = id.char_indices().rev().nth(3).map_or(0, |(index, _)| index);
        (!id.is_empty()).then(|| &id[start..])
    }

    /// Where the statement starts in the file.
    pub fn location(&self) -> SourceLocation {
        SourceLocation::Line(self.line)
    }
}

/// Shows the kind, currency and counts: no account number or transaction
/// contents.
impl fmt::Debug for OfxStatement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OfxStatement")
            .field("kind", &self.kind)
            .field("line", &self.line)
            .field("currency", &self.currency())
            .field("transactions", &self.transactions.len())
            .field("ledger_balance", &self.ledger_balance.is_some())
            .finish_non_exhaustive()
    }
}

/// A `STMTTRN` element and its position among all of the file's.
#[derive(Clone, Debug)]
pub(super) struct Transaction {
    /// 1-based, counted across the whole file in document order.
    pub entry: u64,
    pub element: Element,
}

fn malformed(line: u64, reason: &'static str) -> ImportError {
    ImportError::Malformed {
        format: SourceFormat::Ofx,
        location: Some(SourceLocation::Line(line)),
        reason,
    }
}

/// Finds the statements under the `<OFX>` root, wherever their message set
/// puts them, and the transactions in each.
///
/// A transaction outside any statement, a statement inside another, or a
/// statement-level value given twice fails the file: either would mean
/// guessing where a transaction belongs.
pub(super) fn statements(root: Element) -> Result<Vec<OfxStatement>, ImportError> {
    let mut found = Vec::new();
    let mut entries = 0;
    walk(root, &mut found, &mut entries)?;
    Ok(found)
}

fn statement_kind(name: &str) -> Option<OfxStatementKind> {
    match name {
        "STMTRS" => Some(OfxStatementKind::Bank),
        "CCSTMTRS" => Some(OfxStatementKind::CreditCard),
        "INVSTMTRS" => Some(OfxStatementKind::Investment),
        _ => None,
    }
}

/// Walks everything outside statements.
fn walk(
    element: Element,
    found: &mut Vec<OfxStatement>,
    entries: &mut u64,
) -> Result<(), ImportError> {
    if element.name == "STMTTRN" {
        return Err(malformed(
            element.line,
            "a transaction (STMTTRN) is outside any statement",
        ));
    }
    if let Some(kind) = statement_kind(&element.name) {
        found.push(statement(element, kind, entries)?);
        return Ok(());
    }
    if let super::markup::Content::Children(children) = element.content {
        for child in children {
            walk(child, found, entries)?;
        }
    }
    Ok(())
}

fn statement(
    element: Element,
    kind: OfxStatementKind,
    entries: &mut u64,
) -> Result<OfxStatement, ImportError> {
    let single_value = |name: &'static str| -> Result<Option<String>, ImportError> {
        let mut matches = element.children_named(name);
        match (matches.next(), matches.next()) {
            (None, _) => Ok(None),
            (Some(_), Some(second)) => Err(malformed(
                second.line,
                "a statement value is given more than once",
            )),
            (Some(one), None) => match one.value() {
                Some(value) => Ok(Some(value.to_owned()).filter(|value| !value.is_empty())),
                None => Err(malformed(one.line, "a statement value holds elements")),
            },
        }
    };
    let currency = single_value("CURDEF")?;

    let account = element.children().iter().find(|child| {
        matches!(
            child.name.as_str(),
            "BANKACCTFROM" | "CCACCTFROM" | "INVACCTFROM"
        )
    });
    let account_id = account
        .and_then(|account| account.children_named("ACCTID").next())
        .and_then(Element::value)
        .map(str::to_owned);

    let ledgers: Vec<&Element> = element.children_named("LEDGERBAL").collect();
    let ledger_balance = match ledgers.as_slice() {
        [] => None,
        [ledger] => Some((*ledger).clone()),
        [_, second, ..] => {
            return Err(malformed(
                second.line,
                "the statement has more than one ledger balance (LEDGERBAL)",
            ));
        }
    };

    let line = element.line;
    let mut transactions = Vec::new();
    collect_transactions(element, &mut transactions, entries)?;
    Ok(OfxStatement {
        kind,
        line,
        currency,
        account_id,
        transactions,
        ledger_balance,
    })
}

/// Collects the `STMTTRN`s under a statement in document order, wherever
/// they are (`BANKTRANLIST`, or `INVBANKTRAN` in an investment statement).
fn collect_transactions(
    element: Element,
    transactions: &mut Vec<Transaction>,
    entries: &mut u64,
) -> Result<(), ImportError> {
    for child in element.children() {
        if statement_kind(&child.name).is_some() {
            return Err(malformed(child.line, "a statement is inside another"));
        }
    }
    let super::markup::Content::Children(children) = element.content else {
        return Ok(());
    };
    for child in children {
        if child.name == "STMTTRN" {
            if let Some(inner) = child.children_named("STMTTRN").next() {
                return Err(malformed(inner.line, "a transaction is inside another"));
            }
            *entries += 1;
            transactions.push(Transaction {
                entry: *entries,
                element: child,
            });
        } else {
            collect_transactions(child, transactions, entries)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ofx::markup;

    fn read(text: &str) -> Result<Vec<OfxStatement>, String> {
        statements(markup::read(text, 1).unwrap()).map_err(|error| error.to_string())
    }

    #[test]
    fn statements_anywhere_in_file_order() {
        let text = "<OFX>\
            <BANKMSGSRSV1><STMTTRNRS><STMTRS><CURDEF>GBP\
              <BANKACCTFROM><ACCTID>00001234</BANKACCTFROM>\
              <BANKTRANLIST><STMTTRN><FITID>1</STMTTRN><STMTTRN><FITID>2</STMTTRN></BANKTRANLIST>\
              <LEDGERBAL><BALAMT>1<DTASOF>20261130</LEDGERBAL>\
            </STMTRS></STMTTRNRS></BANKMSGSRSV1>\
            <CREDITCARDMSGSRSV1><CCSTMTTRNRS><CCSTMTRS><CURDEF>USD\
              <CCACCTFROM><ACCTID>XXXX-XXXX-XXXX-9876</CCACCTFROM>\
              <BANKTRANLIST><STMTTRN><FITID>3</STMTTRN></BANKTRANLIST>\
            </CCSTMTRS></CCSTMTTRNRS></CREDITCARDMSGSRSV1>\
            <INVSTMTMSGSRSV1><INVSTMTTRNRS><INVSTMTRS><CURDEF>USD\
              <INVTRANLIST><INVBANKTRAN><STMTTRN><FITID>4</STMTTRN></INVBANKTRAN></INVTRANLIST>\
            </INVSTMTRS></INVSTMTTRNRS></INVSTMTMSGSRSV1>\
            </OFX>";
        let found = read(text).unwrap();
        let summary: Vec<_> = found
            .iter()
            .map(|statement| {
                (
                    statement.kind(),
                    statement.currency(),
                    statement
                        .transactions
                        .iter()
                        .map(|transaction| transaction.entry)
                        .collect::<Vec<_>>(),
                    statement.account_suffix(),
                    statement.ledger_balance.is_some(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                (
                    OfxStatementKind::Bank,
                    Some(Currency::GBP),
                    vec![1, 2],
                    Some("1234"),
                    true
                ),
                (
                    OfxStatementKind::CreditCard,
                    Some(Currency::USD),
                    vec![3],
                    Some("9876"),
                    false
                ),
                (
                    OfxStatementKind::Investment,
                    Some(Currency::USD),
                    vec![4],
                    None,
                    false
                ),
            ]
        );
    }

    #[test]
    fn short_account_ids_are_shown_whole() {
        let found =
            read("<OFX><STMTRS><BANKACCTFROM><ACCTID>12</BANKACCTFROM></STMTRS></OFX>").unwrap();
        assert_eq!(found[0].account_suffix(), Some("12"));
    }

    #[test]
    fn misplaced_structure_fails_the_file() {
        assert_eq!(
            read("<OFX><BANKTRANLIST><STMTTRN><FITID>1</STMTTRN></BANKTRANLIST></OFX>")
                .unwrap_err(),
            "the file isn't a readable ofx file (line 1): a transaction (STMTTRN) is outside any statement"
        );
        assert_eq!(
            read("<OFX><STMTRS><CURDEF>GBP<CURDEF>EUR</STMTRS></OFX>").unwrap_err(),
            "the file isn't a readable ofx file (line 1): a statement value is given more than once"
        );
        assert_eq!(
            read("<OFX><STMTRS><LEDGERBAL></LEDGERBAL><LEDGERBAL></LEDGERBAL></STMTRS></OFX>")
                .unwrap_err(),
            "the file isn't a readable ofx file (line 1): the statement has more than one ledger balance (LEDGERBAL)"
        );
        assert_eq!(
            read("<OFX><STMTRS><BANKTRANLIST><STMTRS></STMTRS></BANKTRANLIST></STMTRS></OFX>")
                .unwrap_err(),
            "the file isn't a readable ofx file (line 1): a statement is inside another"
        );
    }

    #[test]
    fn debug_leaves_the_account_out() {
        let found = read(
            "<OFX><STMTRS><CURDEF>GBP<BANKACCTFROM><ACCTID>55554444</BANKACCTFROM></STMTRS></OFX>",
        )
        .unwrap();
        let debug = format!("{found:?}");
        assert!(!debug.contains("5555"), "{debug}");
        assert!(!debug.contains("4444"), "{debug}");
    }
}
