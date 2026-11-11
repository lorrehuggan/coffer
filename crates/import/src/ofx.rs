//! The OFX importer: OFX 1.x (SGML) and 2.x (XML) bank and credit card
//! statements, QFX included (QFX is OFX with Intuit's extra tags).
//!
//! - **Encoding** is a byte order mark, otherwise what the header states
//!   (`ENCODING`/`CHARSET`, or the XML declaration), otherwise UTF-8.
//!   Decoding is strict, as for CSV: nothing is replaced or guessed.
//! - **Markup** is read by one tolerant reader for both syntaxes: values may
//!   be closed or not, but aggregates must nest, and a file cut short fails.
//! - **One statement per import.** A file can hold several accounts;
//!   [`OfxFile::statements`] lists them and
//!   [`OfxImporter::for_statement`] picks one. Without a choice, a file
//!   with more than one is [`ImportError::MultipleStatements`].
//! - **Transactions** (`STMTTRN`): `DTPOSTED` as written (the zone offset
//!   isn't applied), `TRNAMT` read exactly, `FITID` as the external ID,
//!   `NAME`/`PAYEE` as the merchant, name and `MEMO` as the description,
//!   `TRNTYPE` as the source type. One that can't be read is flagged in its
//!   place, as is a repeated `FITID` and a correction (`CORRECTACTION`).
//! - **Balances**: `LEDGERBAL` becomes the statement's dated balance.
//!   `AVAILBAL` isn't a ledger balance (holds, credit limits) and isn't
//!   used.

mod document;
mod header;
mod markup;
mod normalize;

use coffer_core::statement::{SourceFormat, Statement};

pub use document::{OfxFile, OfxStatement, OfxStatementKind};

use crate::{Detect, Detection, ImportError, Importer, SourceFile};

/// Extensions an OFX export can have. QFX is Quicken's name for it.
const EXTENSIONS: [&str; 2] = ["ofx", "qfx"];

/// Reads OFX and QFX files. It has no configuration apart from which
/// statement to import from a file that holds several.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OfxImporter {
    statement: Option<usize>,
}

impl OfxImporter {
    /// Imports files that hold exactly one statement.
    pub fn new() -> OfxImporter {
        OfxImporter::default()
    }

    /// Imports the statement at `index` (0-based, in
    /// [`OfxFile::statements`] order), for files that hold several.
    pub fn for_statement(index: usize) -> OfxImporter {
        OfxImporter {
            statement: Some(index),
        }
    }
}

impl Detect for OfxImporter {
    fn name(&self) -> &str {
        "OFX / QFX"
    }

    fn format(&self) -> SourceFormat {
        SourceFormat::Ofx
    }

    /// [`Detection::Format`] when the file starts like OFX (an OFX 1.x
    /// header, an XML declaration or `<?OFX …?>` followed by OFX, or a bare
    /// `<OFX>`); [`Detection::Extension`] for another `.ofx` or `.qfx` file.
    /// OFX has no per-bank variants, so never [`Detection::Source`].
    fn detect(&self, file: &SourceFile) -> Detection {
        if header::looks_like_ofx(file.bytes()) {
            return Detection::Format;
        }
        match file.extension() {
            Some(extension) if EXTENSIONS.contains(&extension.as_str()) => Detection::Extension,
            _ => Detection::No,
        }
    }
}

impl Importer for OfxImporter {
    type Parsed = OfxFile;

    fn parse(&self, file: &SourceFile) -> Result<OfxFile, ImportError> {
        let (text, encoding) = header::decode(file.bytes())?;
        let header = header::read(&text)?;
        let root = markup::read(&text[header.body_start..], header.body_line)?;
        Ok(OfxFile {
            version: header.version,
            encoding,
            statements: document::statements(root)?,
        })
    }

    fn normalize(&self, parsed: OfxFile) -> Result<Statement, ImportError> {
        let statements = parsed.statements();
        let chosen = match (self.statement, statements) {
            (None, [only]) => only,
            (None, []) => {
                return Err(ImportError::Malformed {
                    format: SourceFormat::Ofx,
                    location: None,
                    reason: "the file has no bank or credit card statement",
                });
            }
            (None, several) => {
                return Err(ImportError::MultipleStatements {
                    count: several.len(),
                });
            }
            (Some(index), statements) => statements.get(index).ok_or(ImportError::Malformed {
                format: SourceFormat::Ofx,
                location: None,
                reason: "the file doesn't have the statement chosen",
            })?,
        };
        normalize::statement(chosen)
    }
}

#[cfg(test)]
mod tests;
