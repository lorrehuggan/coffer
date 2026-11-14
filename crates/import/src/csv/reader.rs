//! Splitting decoded CSV text into records.

use std::fmt;

use coffer_core::statement::{RowProblem, SourceFormat, SourceLocation};

use super::encoding::{TextEncoding, decode};
use super::mapping::{Delimiter, FileLayout};
use crate::ImportError;

/// How many records delimiter detection looks at.
const SNIFF_RECORDS: usize = 50;

/// One record of a CSV file, the line it starts on and how it was split.
#[derive(Clone, PartialEq, Eq)]
pub struct CsvRecord {
    /// The 1-based physical line the record starts on.
    pub line: u64,
    /// How many physical lines the record takes: more than one when a
    /// quoted field has line breaks in it, or when a stray quote has
    /// merged the following lines into it.
    pub lines: u64,
    /// Whether a quoted field is still open at the end of the file, so the
    /// record took in everything after its start.
    pub unclosed_quote: bool,
    pub fields: Vec<String>,
}

impl CsvRecord {
    pub fn location(&self) -> SourceLocation {
        SourceLocation::Line(self.line)
    }

    /// Why the record's split can't be trusted, whatever its fields hold:
    /// a quote still open at the end of the file, or a record over several
    /// lines when `multiline_fields` (the mapping's
    /// [`multiline_fields`](super::CsvMapping::multiline_fields)) says rows
    /// are one line each.
    pub fn structure_problem(&self, multiline_fields: bool) -> Option<RowProblem> {
        if self.unclosed_quote {
            Some(RowProblem::UnclosedQuote { lines: self.lines })
        } else if self.lines > 1 && !multiline_fields {
            Some(RowProblem::MultiLineRow { lines: self.lines })
        } else {
            None
        }
    }
}

/// Shows the position and shape only: the fields are transaction data.
impl fmt::Debug for CsvRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CsvRecord")
            .field("line", &self.line)
            .field("lines", &self.lines)
            .field("unclosed_quote", &self.unclosed_quote)
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
    multiline_fields: bool,
    header: Option<CsvRecord>,
    records: Vec<CsvRecord>,
}

impl CsvFile {
    /// Reads `file` laid out as `layout` says, before any column is
    /// mapped: what the mapping screen shows. A
    /// [`CsvImporter`](super::CsvImporter) reads files the same way.
    pub fn read(file: &crate::SourceFile, layout: &FileLayout) -> Result<CsvFile, ImportError> {
        read(file.bytes(), layout)
    }

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

    /// How many columns a row should have.
    ///
    /// With a header, the header's cells up to its last non-empty one: a
    /// header that ends with a delimiter when the rows don't still fits
    /// them, and rows that do carry the empty field fit too (extra trailing
    /// empty fields always do). Without one, the first row's count, taken from the
    /// first row whose split can be trusted
    /// ([`CsvRecord::structure_problem`]), or the first row if none can.
    pub fn width(&self) -> usize {
        match &self.header {
            Some(header) => header
                .fields
                .iter()
                .rposition(|cell| !cell.is_empty())
                .map_or(0, |last| last + 1),
            None => self
                .first_sound_record()
                .or(self.records.first())
                .map_or(0, |record| record.fields.len()),
        }
    }

    /// The first data row whose split can be trusted.
    pub(super) fn first_sound_record(&self) -> Option<&CsvRecord> {
        self.records
            .iter()
            .find(|record| record.structure_problem(self.multiline_fields).is_none())
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

/// Reads `bytes` as the CSV file `mapping` (its layout) describes.
///
/// A header row with a quote that's never closed, or one over several
/// lines when the mapping doesn't allow line breaks in fields, fails the
/// file: its columns can't be trusted. Data rows like that are kept, and
/// flagged when normalizing.
pub(super) fn read(bytes: &[u8], mapping: &FileLayout) -> Result<CsvFile, ImportError> {
    let (text, encoding) = decode(bytes, mapping.encoding)?;
    let body = skip_lines(&text, mapping.skip_rows)?;
    let preamble = &text[..text.len() - body.len()];
    let delimiter = match mapping.delimiter {
        Some(delimiter) => delimiter,
        None => match sniff_delimiter(body, mapping.multiline_fields) {
            Ok(delimiter) => delimiter,
            Err(error) => {
                // A skip ending inside a quoted field is the likelier
                // reason, and says more.
                check_preamble(preamble, &Delimiter::ALL)?;
                return Err(error);
            }
        },
    };
    check_preamble(preamble, &[delimiter])?;

    let skipped = u64::try_from(mapping.skip_rows).expect("usize fits u64");
    let mut records = split(body, delimiter, None)?;
    for record in &mut records {
        record.line += skipped;
    }
    if records.is_empty() {
        return Err(malformed(None, "the file has no rows"));
    }
    let header = if mapping.has_header {
        let mut header = records.remove(0);
        match header.structure_problem(mapping.multiline_fields) {
            None => {}
            Some(RowProblem::UnclosedQuote { .. }) => {
                return Err(malformed(
                    Some(header.location()),
                    "the header row has a quote that's never closed",
                ));
            }
            Some(_) => {
                return Err(malformed(
                    Some(header.location()),
                    "the header row runs over more than one line",
                ));
            }
        }
        for cell in &mut header.fields {
            *cell = cell.trim().to_owned();
        }
        Some(header)
    } else {
        None
    };
    Ok(CsvFile {
        encoding,
        delimiter,
        multiline_fields: mapping.multiline_fields,
        header,
        records,
    })
}

/// `text` after its first `count` lines. `\r\n`, `\n` and a lone `\r` each
/// end a line. The lines are physical lines, whatever their quotes: a
/// preamble is often not CSV at all ([`check_preamble`] catches a skip that
/// ends inside a quoted field).
fn skip_lines(text: &str, count: usize) -> Result<&str, ImportError> {
    let mut rest = text;
    for _ in 0..count {
        let end = rest
            .find(['\r', '\n'])
            .ok_or_else(|| malformed(None, "the file ends within the lines to skip"))?;
        let after = if rest[end..].starts_with("\r\n") {
            end + 2
        } else {
            end + 1
        };
        rest = &rest[after..];
    }
    Ok(rest)
}

/// Fails the file when the skipped lines, `preamble`, end inside a quoted
/// field under any of `delimiters`, at the line the field opens on: a
/// summary cell with a line break in it, counted as one line in
/// `skip_rows`. The rest of the field would otherwise be read as the header
/// or the first row.
///
/// Quotes follow [`open_quote_start`]: only a `"` at the start of a field
/// opens one, so a `"` inside free text (`6" pizza fund`) never counts.
/// Where a field starts depends on the delimiter, so this checks the one
/// the file is read with; a closing quote at the start of a line opens a
/// field under any other. Only when the delimiter can't be sniffed (and a
/// broken skip is a likely reason) does any candidate count.
fn check_preamble(preamble: &str, delimiters: &[Delimiter]) -> Result<(), ImportError> {
    let bytes = preamble.as_bytes();
    let open = delimiters
        .iter()
        .filter_map(|delimiter| open_quote_start(bytes, delimiter.byte()))
        .min();
    match open {
        Some(at) => Err(malformed(
            Some(SourceLocation::Line(1 + line_breaks(bytes, 0, at))),
            "the lines to skip end inside a quoted field",
        )),
        None => Ok(()),
    }
}

/// Whether `byte` ends a line: `\r` or `\n`.
fn is_line_break(byte: u8) -> bool {
    matches!(byte, b'\r' | b'\n')
}

/// The line breaks in `text[from..to]`: `\r\n`, `\n` and a lone `\r` count
/// once each. A `\r` at `to - 1` followed by `\n` at `to` isn't counted,
/// so that the `\n` counts once in the range after.
fn line_breaks(text: &[u8], from: usize, to: usize) -> u64 {
    let count = (from..to)
        .filter(|&at| match text[at] {
            b'\n' => true,
            b'\r' => text.get(at + 1) != Some(&b'\n'),
            _ => false,
        })
        .count();
    u64::try_from(count).expect("usize fits u64")
}

/// Whether a record's text (without its line ending) ends inside a quoted
/// field: [`open_quote_start`] for one record.
fn ends_in_open_quote(record: &[u8], delimiter: u8) -> bool {
    open_quote_start(record, delimiter).is_some()
}

/// Where the quoted field still open at the end of `text` starts (the
/// offset of its opening `"`), or `None` if none is. Follows the `csv`
/// crate's rules: a field is quoted only when it starts with `"`; inside
/// one, `""` is a quote and line breaks are text; after the closing quote,
/// the rest of the field is read as written. A `"` inside an unquoted field
/// (`6" pizza`) is just a character. Outside a quoted field, a line break
/// ends the record.
fn open_quote_start(text: &[u8], delimiter: u8) -> Option<usize> {
    enum State {
        FieldStart,
        Unquoted,
        Quoted(usize),
        QuoteInQuoted(usize),
    }
    let mut state = State::FieldStart;
    for (at, &byte) in text.iter().enumerate() {
        state = match state {
            State::FieldStart if byte == b'"' => State::Quoted(at),
            State::FieldStart | State::Unquoted | State::QuoteInQuoted(_)
                if byte == delimiter || is_line_break(byte) =>
            {
                State::FieldStart
            }
            State::QuoteInQuoted(start) if byte == b'"' => State::Quoted(start),
            State::FieldStart | State::Unquoted | State::QuoteInQuoted(_) => State::Unquoted,
            State::Quoted(start) if byte == b'"' => State::QuoteInQuoted(start),
            State::Quoted(start) => State::Quoted(start),
        };
    }
    match state {
        State::Quoted(start) => Some(start),
        _ => None,
    }
}

/// The records of `text`, at most `limit` of them. Line numbers are
/// 1-based within `text`.
fn split(
    text: &str,
    delimiter: Delimiter,
    limit: Option<usize>,
) -> Result<Vec<CsvRecord>, ImportError> {
    let bytes = text.as_bytes();
    let offset =
        |position: u64| usize::try_from(position).map_or(bytes.len(), |at| at.min(bytes.len()));
    let mut reader = ::csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter.byte())
        .from_reader(bytes);
    let mut records = Vec::new();
    let mut lines = LineCounter::new(text);
    let mut record = ::csv::StringRecord::new();
    while limit.is_none_or(|limit| records.len() < limit) {
        let more = reader.read_record(&mut record).map_err(|error| {
            let location = error
                .position()
                .map(|position| SourceLocation::Line(lines.line_at(position.byte())));
            malformed(location, "the CSV structure can't be read")
        })?;
        if !more {
            break;
        }
        // `csv` gives the record's start as the end of the previous one, and
        // its end as just past its first line-ending byte: trim both to the
        // record's own text.
        let position = record.position().map_or(0, ::csv::Position::byte);
        let start = offset(position);
        let end = offset(reader.position().byte()).max(start);
        let mut from = start;
        while from < end && is_line_break(bytes[from]) {
            from += 1;
        }
        let mut to = end;
        while to > from && is_line_break(bytes[to - 1]) {
            to -= 1;
        }
        records.push(CsvRecord {
            line: lines.line_at(position),
            lines: 1 + line_breaks(bytes, from, to),
            unclosed_quote: end == bytes.len()
                && ends_in_open_quote(&bytes[from..to], delimiter.byte()),
            fields: record.iter().map(str::to_owned).collect(),
        });
    }
    Ok(records)
}

/// Line numbers from byte offsets. The `csv` crate counts lines itself, but
/// only `\n`s, so count line breaks the way [`line_breaks`] does: `\r\n`,
/// `\n` or a lone `\r` (old Mac exports).
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
        while self.text.get(byte).copied().is_some_and(is_line_break) {
            byte += 1;
        }
        if byte > self.offset {
            self.line += line_breaks(self.text, self.offset, byte);
            self.offset = byte;
        }
        self.line
    }
}

/// The delimiter that splits the first rows most consistently.
///
/// Each candidate splits the first [`SNIFF_RECORDS`] records. Records whose
/// split can't be trusted ([`CsvRecord::structure_problem`]: an unclosed
/// quote, or several lines when `multiline_fields` is off) are left out:
/// how a stray quote merges lines depends on the delimiter, so they say
/// nothing about it. The first remaining record sets the width, which must
/// be at least two. The candidate under which most of the others have the
/// same shape wins (the same count, or one has extra trailing empty
/// fields: [`fits_width`] either way round), then the one giving more
/// columns. A tie is refused rather than guessed.
pub(super) fn sniff_delimiter(
    text: &str,
    multiline_fields: bool,
) -> Result<Delimiter, ImportError> {
    let mut scored = Vec::new();
    for delimiter in Delimiter::ALL {
        let records = split(text, delimiter, Some(SNIFF_RECORDS))?;
        if records.is_empty() {
            return Err(malformed(None, "the file has no rows"));
        }
        let mut sound = records
            .iter()
            .filter(|record| record.structure_problem(multiline_fields).is_none());
        let Some(first) = sound.next() else {
            continue;
        };
        let width = first.fields.len();
        if width < 2 {
            continue;
        }
        let fitting = sound
            .filter(|record| {
                fits_width(&record.fields, width) || fits_width(&first.fields, record.fields.len())
            })
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
        sniff_delimiter(text, false).map_err(|error| error.to_string())
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
    fn a_lone_carriage_return_ends_a_line() {
        let text = "a,b\r1,x\r\r2,\"multi\rline\"\r3,y\r\n4,z\n5,w";
        let records = split(text, Delimiter::Comma, None).unwrap();
        let shape: Vec<_> = records
            .iter()
            .map(|record| (record.line, record.lines))
            .collect();
        assert_eq!(
            shape,
            [(1, 1), (2, 1), (4, 2), (6, 1), (7, 1), (8, 1)],
            "CR, CRLF and LF each end one line, mixed or not"
        );

        assert_eq!(skip_lines("x\ry\rz", 2).ok(), Some("z"));
        assert_eq!(skip_lines("x\r\ny\nz", 2).ok(), Some("z"));
        assert_eq!(
            skip_lines("x\r\n\rz", 2).ok(),
            Some("z"),
            "CRLF is one line"
        );
        assert!(skip_lines("x\ry", 2).is_err());
    }

    #[test]
    fn records_say_how_many_lines_they_take() {
        let shape = |text: &str| -> Vec<(u64, u64, bool, usize)> {
            split(text, Delimiter::Comma, None)
                .unwrap()
                .iter()
                .map(|r| (r.line, r.lines, r.unclosed_quote, r.fields.len()))
                .collect()
        };
        assert_eq!(
            shape("a,b\n1,\"oops\n2,x\n3,y\n"),
            [(1, 1, false, 2), (2, 3, true, 2)],
            "an unclosed quote takes in the rest of the file"
        );
        assert_eq!(
            shape("a,b\n1,\"oops\r\n2,x\r\n"),
            [(1, 1, false, 2), (2, 2, true, 2)]
        );
        assert_eq!(
            shape("a,b\n1,\"x\n2,y\"\n3,z"),
            [(1, 1, false, 2), (2, 2, false, 2), (4, 1, false, 2)],
            "two stray quotes merge the lines between them"
        );
        assert_eq!(
            shape("a,b\n1,\"open"),
            [(1, 1, false, 2), (2, 1, true, 2)],
            "open on the last line, without a line ending"
        );
        assert_eq!(
            shape("a,b\n1,6\" pizza\n2,\"say \"\"hi\"\"\"\n3,\"x\"y"),
            [
                (1, 1, false, 2),
                (2, 1, false, 2),
                (3, 1, false, 2),
                (4, 1, false, 2)
            ],
            "quotes inside or after a field are characters; doubled ones are escapes"
        );
    }

    #[test]
    fn open_quotes_follow_the_csv_rules() {
        let open = |text: &str| ends_in_open_quote(text.as_bytes(), b',');
        assert!(open("a,\"b"));
        assert!(open("\"a"));
        assert!(open("a,\"b\"\"c"), "a doubled quote doesn't close it");
        assert!(open("a,\"b,\"\"\"\"\n"));
        assert!(!open("a,\"b\""));
        assert!(!open("a,6\" pizza"));
        assert!(
            !open("a, \"b"),
            "a quote after a space isn't an opening quote"
        );
        assert!(
            !open("\"a\"b\"c"),
            "after the closing quote, the rest is literal"
        );
        assert!(!open(""));
    }

    #[test]
    fn open_quotes_run_over_line_breaks() {
        let start = |text: &str| open_quote_start(text.as_bytes(), b',');
        assert_eq!(start("a,\"b\nc"), Some(2), "a line break inside is text");
        assert_eq!(start("a,\"b\nc\"\n"), None);
        assert_eq!(start("a,\"b\"\"\r\nc"), Some(2), "a doubled quote is text");
        assert_eq!(
            start("a,b\n\"c"),
            Some(4),
            "a line break outside one starts a field"
        );
        assert_eq!(start("a,6\" pizza\nb\n"), None);
        assert_eq!(
            start("\"a\"b\"\nc"),
            None,
            "after the closing quote, the rest is literal"
        );
    }

    #[test]
    fn a_preamble_must_not_end_inside_a_quoted_field() {
        fn check(preamble: &str, delimiters: &[Delimiter]) -> Result<(), String> {
            check_preamble(preamble, delimiters).map_err(|error| error.to_string())
        }
        let error = |line: u64| {
            format!(
                "the file isn't a readable csv file (line {line}): \
                 the lines to skip end inside a quoted field"
            )
        };
        let comma = [Delimiter::Comma];
        assert_eq!(check("Name,\"Example\nAccount\"\n", &comma), Ok(()));
        assert_eq!(check("Name,\"Example\n", &comma), Err(error(1)));
        assert_eq!(
            check("Summary\r\nName,\"Ex\rample\r\n", &comma),
            Err(error(2)),
            "the line the field opens on, CR and CRLF one line each"
        );
        assert_eq!(
            check(
                "Account: 6\" pizza fund\nNote: a \"quoted\" word, \"open\n",
                &comma
            ),
            Ok(()),
            "a quote that doesn't start a field is a character"
        );

        let semicolons = "Konto;\"Beispiel\n";
        assert_eq!(
            check(semicolons, &comma),
            Ok(()),
            "only the delimiter starts fields"
        );
        assert_eq!(check(semicolons, &Delimiter::ALL), Err(error(1)));
        let closed_on_its_own_line = "Name,\"Example\n\"\n";
        assert_eq!(check(closed_on_its_own_line, &comma), Ok(()));
        assert_eq!(
            check(closed_on_its_own_line, &[Delimiter::Semicolon]),
            Err(error(2)),
            "under the wrong delimiter, the closing quote opens a field"
        );
    }

    #[test]
    fn records_merged_by_a_stray_quote_dont_sniff() {
        assert_eq!(
            sniff("Date,Amount,Text\n01,-1,\"oops\n02,-2,b\n03,-3,c\n"),
            Ok(Delimiter::Comma),
            "the header still sets the width; the merged record isn't scored"
        );
        let whole_file = "\"Date,Amount,Text\n01,-1,a\n02,-2,b\n";
        assert_eq!(
            sniff(whole_file),
            Err(
                "the file isn't a readable csv file: no delimiter splits the rows into columns"
                    .to_owned()
            ),
            "an unclosed quote on the first line leaves nothing to go on"
        );
        let multiline = "a,b\n\"x\ny\",1\n2,3\n";
        assert_eq!(
            sniff_delimiter(multiline, true).ok(),
            Some(Delimiter::Comma)
        );
        assert_eq!(sniff(multiline), Ok(Delimiter::Comma));
    }

    #[test]
    fn sniffing_allows_a_header_ending_with_a_delimiter() {
        assert_eq!(
            sniff("Date,Amount,Text,\n01,-1,a;b\n02,-2,c;d\n"),
            Ok(Delimiter::Comma)
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
