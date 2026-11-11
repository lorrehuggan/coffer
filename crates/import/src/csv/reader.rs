//! Splitting decoded CSV text into records.

use std::fmt;

use coffer_core::statement::{SourceFormat, SourceLocation};

use super::encoding::{TextEncoding, decode};
use super::mapping::{CsvMapping, Delimiter};
use crate::ImportError;

/// How many records delimiter detection looks at.
const SNIFF_RECORDS: usize = 50;

/// One record of a CSV file and the line it starts on.
#[derive(Clone, PartialEq, Eq)]
pub struct CsvRecord {
    pub line: u64,
    pub fields: Vec<String>,
}

impl CsvRecord {
    pub fn location(&self) -> SourceLocation {
        SourceLocation::Line(self.line)
    }
}

/// Shows the line and field count only: the fields are transaction data.
impl fmt::Debug for CsvRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CsvRecord")
            .field("line", &self.line)
            .field("fields", &self.fields.len())
            .finish()
    }
}

/// A CSV file split into records, before normalizing: the
/// [`CsvImporter`](super::CsvImporter)'s `Parsed` type, and what a mapping
/// screen shows.
#[derive(Clone)]
pub struct CsvFile {
    encoding: TextEncoding,
    delimiter: Delimiter,
    header: Option<CsvRecord>,
    records: Vec<CsvRecord>,
}

impl CsvFile {
    /// The encoding the file was decoded with: the stated one, or the one
    /// its byte order mark names. Never [`TextEncoding::Auto`].
    pub fn encoding(&self) -> TextEncoding {
        self.encoding
    }

    /// The delimiter, as stated or as detected.
    pub fn delimiter(&self) -> Delimiter {
        self.delimiter
    }

    /// The header row, its cells trimmed. `None` when the mapping says the
    /// file has none.
    pub fn header(&self) -> Option<&CsvRecord> {
        self.header.as_ref()
    }

    /// The data rows in file order, untrimmed. Blank lines aren't records.
    pub fn records(&self) -> &[CsvRecord] {
        &self.records
    }

    /// How many columns a row should have: the header's, or the first
    /// row's without a header.
    pub fn width(&self) -> usize {
        self.header
            .as_ref()
            .or(self.records.first())
            .map_or(0, |record| record.fields.len())
    }
}

impl fmt::Debug for CsvFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CsvFile")
            .field("encoding", &self.encoding)
            .field("delimiter", &self.delimiter)
            .field("width", &self.width())
            .field("records", &self.records.len())
            .finish_non_exhaustive()
    }
}

fn malformed(location: Option<SourceLocation>, reason: &'static str) -> ImportError {
    ImportError::Malformed {
        format: SourceFormat::Csv,
        location,
        reason,
    }
}

/// Whether a record of `found` fields fits a file `width` columns wide:
/// the same count, or extra trailing fields that are all empty (some banks
/// end every row, but not the header, with a delimiter).
pub(super) fn fits_width(fields: &[String], width: usize) -> bool {
    fields.len() == width
        || (fields.len() > width && fields[width..].iter().all(|field| field.trim().is_empty()))
}

/// Reads `bytes` as the CSV file `mapping` describes.
pub(super) fn read(bytes: &[u8], mapping: &CsvMapping) -> Result<CsvFile, ImportError> {
    let (text, encoding) = decode(bytes, mapping.encoding)?;
    let body = skip_lines(&text, mapping.skip_rows)?;
    let delimiter = match mapping.delimiter {
        Some(delimiter) => delimiter,
        None => sniff_delimiter(body)?,
    };

    let skipped = u64::try_from(mapping.skip_rows).expect("usize fits u64");
    let mut records = split(body, delimiter, None)?;
    for record in &mut records {
        record.line += skipped;
    }
    if records.is_empty() {
        return Err(malformed(None, "the file has no rows"));
    }
    let header = mapping.has_header.then(|| {
        let mut header = records.remove(0);
        for cell in &mut header.fields {
            *cell = cell.trim().to_owned();
        }
        header
    });
    Ok(CsvFile {
        encoding,
        delimiter,
        header,
        records,
    })
}

/// `text` after its first `count` lines.
fn skip_lines(text: &str, count: usize) -> Result<&str, ImportError> {
    let mut rest = text;
    for _ in 0..count {
        let (_, after) = rest
            .split_once('\n')
            .ok_or_else(|| malformed(None, "the file ends within the lines to skip"))?;
        rest = after;
    }
    Ok(rest)
}

/// The records of `text`, at most `limit` of them. Line numbers are
/// 1-based within `text`.
fn split(
    text: &str,
    delimiter: Delimiter,
    limit: Option<usize>,
) -> Result<Vec<CsvRecord>, ImportError> {
    let mut reader = ::csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter.byte())
        .from_reader(text.as_bytes());
    let mut records = Vec::new();
    let mut lines = LineCounter::new(text);
    for record in reader.records().take(limit.unwrap_or(usize::MAX)) {
        let record = record.map_err(|error| {
            let location = error
                .position()
                .map(|position| SourceLocation::Line(lines.line_at(position.byte())));
            malformed(location, "the CSV structure can't be read")
        })?;
        let start = record.position().map_or(0, ::csv::Position::byte);
        records.push(CsvRecord {
            line: lines.line_at(start),
            fields: record.iter().map(str::to_owned).collect(),
        });
    }
    Ok(records)
}

/// Line numbers from byte offsets. The `csv` crate counts lines itself, but
/// not reliably across CRLF line endings, so count the `\n`s instead.
struct LineCounter<'a> {
    text: &'a [u8],
    offset: usize,
    line: u64,
}

impl<'a> LineCounter<'a> {
    fn new(text: &'a str) -> Self {
        LineCounter {
            text: text.as_bytes(),
            offset: 0,
            line: 1,
        }
    }

    /// The 1-based line of the first content at or after `byte`. `csv`
    /// gives a record's start as the end of the previous one, before the
    /// line ending and any blank lines, so those are stepped over (a record
    /// can't start with a line break). Offsets must not go backwards.
    fn line_at(&mut self, byte: u64) -> u64 {
        let mut byte = usize::try_from(byte)
            .unwrap_or(usize::MAX)
            .min(self.text.len());
        while self
            .text
            .get(byte)
            .is_some_and(|b| matches!(b, b'\r' | b'\n'))
        {
            byte += 1;
        }
        if byte > self.offset {
            let newlines = self.text[self.offset..byte]
                .iter()
                .filter(|&&b| b == b'\n')
                .count();
            self.line += u64::try_from(newlines).expect("usize fits u64");
            self.offset = byte;
        }
        self.line
    }
}

/// The delimiter that splits the first rows most consistently.
///
/// Each candidate splits the first [`SNIFF_RECORDS`] records; the first one
/// sets the width, which must be at least two. The candidate under which
/// most rows fit that width wins, then the one giving more columns. A tie
/// is refused rather than guessed.
pub(super) fn sniff_delimiter(text: &str) -> Result<Delimiter, ImportError> {
    let mut scored = Vec::new();
    for delimiter in Delimiter::ALL {
        let records = split(text, delimiter, Some(SNIFF_RECORDS))?;
        let Some((first, rest)) = records.split_first() else {
            return Err(malformed(None, "the file has no rows"));
        };
        let width = first.fields.len();
        if width < 2 {
            continue;
        }
        let fitting = rest
            .iter()
            .filter(|record| fits_width(&record.fields, width))
            .count();
        scored.push(((fitting, width), delimiter));
    }
    scored.sort_by_key(|&(score, _)| std::cmp::Reverse(score));
    match scored.as_slice() {
        [] => Err(malformed(None, "no delimiter splits the rows into columns")),
        [(best, _), (second, _), ..] if best == second => Err(malformed(
            None,
            "more than one delimiter fits; the mapping has to say which",
        )),
        [(_, delimiter), ..] => Ok(*delimiter),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sniff(text: &str) -> Result<Delimiter, String> {
        sniff_delimiter(text).map_err(|error| error.to_string())
    }

    #[test]
    fn sniffs_common_delimiters() {
        assert_eq!(sniff("Date,Amount,Text\n1,2,3\n"), Ok(Delimiter::Comma));
        assert_eq!(
            sniff("Datum;Betrag;Text\n01.11.2026;-1.234,56;Kaffee, groß\n"),
            Ok(Delimiter::Semicolon),
            "commas inside fields don't count"
        );
        assert_eq!(sniff("Date\tAmount\n1\t2\n"), Ok(Delimiter::Tab));
        assert_eq!(sniff("Date|Amount\n1|2\n"), Ok(Delimiter::Pipe));
        assert_eq!(
            sniff("Date,Amount,Text\n1,2,\"a;b\"\n"),
            Ok(Delimiter::Comma),
            "quoted delimiters don't count"
        );
    }

    #[test]
    fn prefers_consistent_rows_then_more_columns() {
        // Each row has one comma in its text but splits the same way on `;`.
        let text = "Date;Text,Kind;Amount\n\
                    01;a,b;1\n\
                    02;c;2\n";
        assert_eq!(sniff(text), Ok(Delimiter::Semicolon));
    }

    #[test]
    fn refuses_to_guess() {
        assert_eq!(
            sniff("just one column\nanother\n"),
            Err(
                "the file isn't a readable csv file: no delimiter splits the rows into columns"
                    .to_owned()
            )
        );
        assert_eq!(
            sniff("a,b;c\n1,2;3\n"),
            Err(
                "the file isn't a readable csv file: more than one delimiter fits; \
                 the mapping has to say which"
                    .to_owned()
            )
        );
    }

    #[test]
    fn line_numbers_are_physical_lines() {
        let text = "a,b\r\n\r\n1,\"multi\r\nline\"\r\n2,x\r\n3,y\n\n\n4,z";
        let lines: Vec<_> = split(text, Delimiter::Comma, None)
            .unwrap()
            .iter()
            .map(|record| record.line)
            .collect();
        assert_eq!(
            lines,
            [1, 3, 5, 6, 9],
            "blank lines skipped, quoted line breaks counted, CRLF or LF"
        );
    }

    #[test]
    fn fits_width_allows_trailing_empty_fields() {
        let fields = |items: &[&str]| -> Vec<String> {
            items.iter().map(|item| (*item).to_owned()).collect()
        };
        assert!(fits_width(&fields(&["a", "b"]), 2));
        assert!(fits_width(&fields(&["a", "b", ""]), 2));
        assert!(fits_width(&fields(&["a", "b", " ", ""]), 2));
        assert!(!fits_width(&fields(&["a", "b", "c"]), 2));
        assert!(!fits_width(&fields(&["a"]), 2));
    }
}
