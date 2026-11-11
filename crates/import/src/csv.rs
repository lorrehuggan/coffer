//! The CSV importer: delimited text read through a [`CsvMapping`].
//!
//! A mapping says how one kind of export is laid out: encoding, delimiter,
//! lines to skip, whether there's a header, and which columns hold the
//! date, amount, description and the optional fields. Presets
//! (`crate::preset`, M1.8) and the mapping screen both
//! produce one; [`CsvImporter`] reads files with it.
//!
//! - **Encoding** is strict: a byte order mark, otherwise the stated
//!   encoding, otherwise UTF-8. A file that doesn't decode fails as a
//!   whole; nothing is replaced or guessed ([`TextEncoding`]).
//! - **The delimiter** is stated or detected from the first rows; a tie
//!   is refused rather than guessed.
//! - **Dates** use a `strftime` format and must round-trip through it
//!   ([`DateFormat`]); [`DateFormat::candidates`] lists the formats a set
//!   of values fits, so `DD/MM` against `MM/DD` is the user's call.
//! - **Amounts** are read exactly with the mapping's decimal mark, from a
//!   signed column or a money-out/money-in pair.
//! - **Rows are never dropped.** A row that splits into the wrong number of
//!   columns, or whose fields can't be read, is flagged in its place.

mod dates;
mod encoding;
mod mapping;
mod normalize;
mod reader;

use coffer_core::statement::{SourceFormat, Statement};

pub use dates::{DateFormat, FormatError, TimeFormat};
pub use encoding::TextEncoding;
pub use mapping::{
    AmountColumns, Column, CsvMapping, DateColumn, Delimiter, MappingError, OriginalAmountColumns,
    SplitSigns, TimeColumn,
};
pub use reader::{CsvFile, CsvRecord};

use crate::{Detect, Detection, ImportError, Importer, SourceFile};
use normalize::Positions;

/// Extensions a CSV export can have.
const EXTENSIONS: [&str; 3] = ["csv", "tsv", "txt"];

/// Reads CSV files laid out as its [`CsvMapping`] says.
#[derive(Clone, Debug)]
pub struct CsvImporter {
    name: String,
    preset_id: Option<String>,
    mapping: CsvMapping,
}

impl CsvImporter {
    /// An importer called `name` (what the user sees when choosing one).
    /// Fails if the mapping can't work for any file.
    pub fn new(name: impl Into<String>, mapping: CsvMapping) -> Result<CsvImporter, MappingError> {
        mapping.validate()?;
        Ok(CsvImporter {
            name: name.into(),
            preset_id: None,
            mapping,
        })
    }

    /// Records the preset the mapping came from on every statement.
    pub fn with_preset(mut self, preset_id: impl Into<String>) -> CsvImporter {
        self.preset_id = Some(preset_id.into());
        self
    }

    pub fn mapping(&self) -> &CsvMapping {
        &self.mapping
    }

    pub fn preset_id(&self) -> Option<&str> {
        self.preset_id.as_deref()
    }
}

impl Detect for CsvImporter {
    fn name(&self) -> &str {
        &self.name
    }

    fn format(&self) -> SourceFormat {
        SourceFormat::Csv
    }

    /// [`Detection::Source`] when the file has every column the mapping
    /// names in its header; [`Detection::Format`] when it reads as CSV with
    /// at least two columns (and, without a header, every column the
    /// mapping counts on); [`Detection::Extension`] for a `.csv`, `.tsv`
    /// or `.txt` file that doesn't.
    fn detect(&self, file: &SourceFile) -> Detection {
        let by_extension = match file.extension() {
            Some(extension) if EXTENSIONS.contains(&extension.as_str()) => Detection::Extension,
            _ => Detection::No,
        };
        let Ok(parsed) = self.parse(file) else {
            return by_extension;
        };
        let found = Positions::find(&self.mapping, &parsed).is_ok();
        match (self.mapping.has_header, found) {
            (true, true) => Detection::Source,
            (false, true) => Detection::Format,
            (true, false) if parsed.width() >= 2 => Detection::Format,
            _ => by_extension,
        }
    }
}

impl Importer for CsvImporter {
    type Parsed = CsvFile;

    fn parse(&self, file: &SourceFile) -> Result<CsvFile, ImportError> {
        reader::read(file.bytes(), &self.mapping)
    }

    fn normalize(&self, parsed: CsvFile) -> Result<Statement, ImportError> {
        let positions = Positions::find(&self.mapping, &parsed)?;
        let mut statement = Statement::new(SourceFormat::Csv, self.mapping.currency);
        if let Some(preset_id) = &self.preset_id {
            statement = statement.with_preset(preset_id);
        }
        for record in parsed.records() {
            let line = positions.line(&self.mapping, &record.fields);
            statement.push(record.location(), line);
        }
        Ok(statement)
    }
}

#[cfg(test)]
mod tests;
