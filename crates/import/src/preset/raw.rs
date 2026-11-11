//! The TOML form of a preset, as serde reads it.
//!
//! Field values that can be checked on their own (column references, date
//! formats, the encoding, the delimiter, category keys) are checked while
//! deserializing, so their errors point at the line in the file. Checks
//! across fields happen when the raw preset becomes a
//! [`Preset`](super::Preset).

use std::collections::BTreeMap;
use std::fmt;

use coffer_core::category::CategoryKey;
use coffer_core::money::{Currency, DecimalMark};
use serde::Deserialize;
use serde::de::{self, Deserializer, Visitor};

use crate::csv::{Column, DateFormat, Delimiter, SplitSigns, TextEncoding, TimeFormat};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawPreset {
    pub id: String,
    pub name: String,
    pub region: String,
    pub currency: Currency,
    pub encoding: Option<PresetEncoding>,
    pub delimiter: Option<PresetDelimiter>,
    #[serde(default)]
    pub skip_rows: usize,
    #[serde(default = "yes")]
    pub header: bool,
    pub date: RawDate,
    pub time: Option<RawTime>,
    pub amount: RawAmount,
    pub decimal: Option<DecimalMark>,
    pub row_currency: Option<RawColumn>,
    pub merchant: Option<RawColumn>,
    pub description: RawDescription,
    pub external_id: Option<RawColumn>,
    pub source_category: Option<RawColumn>,
    pub source_type: Option<RawColumn>,
    pub original_amount: Option<RawOriginalAmount>,
    pub note: Option<RawColumn>,
    pub balance: Option<RawColumn>,
    #[serde(default)]
    pub ignore: Vec<String>,
    #[serde(default)]
    pub category_map: BTreeMap<String, CategoryKey>,
}

fn yes() -> bool {
    true
}

/// `{ column = "Name" }`
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawColumn {
    pub column: PresetColumn,
}

/// `{ column = "Date", format = "%d/%m/%Y" }`
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawDate {
    pub column: PresetColumn,
    pub format: PresetDateFormat,
}

/// `{ column = "Time", format = "%H:%M:%S" }`
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawTime {
    pub column: PresetColumn,
    pub format: PresetTimeFormat,
}

/// `{ column = "Amount" }` or `{ debit = "Paid out", credit = "Paid in" }`,
/// optionally with `signs = "signed"`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawAmount {
    pub column: Option<PresetColumn>,
    pub debit: Option<PresetColumn>,
    pub credit: Option<PresetColumn>,
    pub signs: Option<RawSigns>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum RawSigns {
    Unsigned,
    Signed,
}

impl From<RawSigns> for SplitSigns {
    fn from(signs: RawSigns) -> SplitSigns {
        match signs {
            RawSigns::Unsigned => SplitSigns::Unsigned,
            RawSigns::Signed => SplitSigns::Signed,
        }
    }
}

/// `{ column = "Description" }` or `{ columns = ["Payee", "Reference"] }`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawDescription {
    pub column: Option<PresetColumn>,
    pub columns: Option<Vec<PresetColumn>>,
}

/// `{ column = "Local amount", currency_column = "Local currency" }`
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawOriginalAmount {
    pub column: PresetColumn,
    pub currency_column: PresetColumn,
}

/// A column as a preset writes it: a header name (a string) or a 1-based
/// position (an integer), for files without a header.
pub(super) struct PresetColumn(pub Column);

impl<'de> Deserialize<'de> for PresetColumn {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<PresetColumn, D::Error> {
        struct ColumnVisitor;

        impl Visitor<'_> for ColumnVisitor {
            type Value = PresetColumn;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a column name, or a column position counted from 1")
            }

            fn visit_str<E: de::Error>(self, name: &str) -> Result<PresetColumn, E> {
                if name.trim().is_empty() {
                    return Err(E::custom("a column name can't be empty"));
                }
                Ok(PresetColumn(Column::named(name)))
            }

            fn visit_i64<E: de::Error>(self, position: i64) -> Result<PresetColumn, E> {
                let index = usize::try_from(position)
                    .ok()
                    .and_then(|position| position.checked_sub(1))
                    .ok_or_else(|| E::custom("column positions count from 1"))?;
                Ok(PresetColumn(Column::Index(index)))
            }

            fn visit_u64<E: de::Error>(self, position: u64) -> Result<PresetColumn, E> {
                let position = i64::try_from(position)
                    .map_err(|_| E::custom("that column position is too large"))?;
                self.visit_i64(position)
            }
        }

        deserializer.deserialize_any(ColumnVisitor)
    }
}

/// Checked with [`DateFormat::new`].
pub(super) struct PresetDateFormat(pub DateFormat);

impl<'de> Deserialize<'de> for PresetDateFormat {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<PresetDateFormat, D::Error> {
        let pattern = String::deserialize(deserializer)?;
        DateFormat::new(&pattern)
            .map(PresetDateFormat)
            .map_err(de::Error::custom)
    }
}

/// Checked with [`TimeFormat::new`].
pub(super) struct PresetTimeFormat(pub TimeFormat);

impl<'de> Deserialize<'de> for PresetTimeFormat {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<PresetTimeFormat, D::Error> {
        let pattern = String::deserialize(deserializer)?;
        TimeFormat::new(&pattern)
            .map(PresetTimeFormat)
            .map_err(de::Error::custom)
    }
}

/// `auto` or an encoding label ([`TextEncoding::from_label`]).
pub(super) struct PresetEncoding(pub TextEncoding);

impl<'de> Deserialize<'de> for PresetEncoding {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<PresetEncoding, D::Error> {
        let label = String::deserialize(deserializer)?;
        TextEncoding::from_label(&label)
            .map(PresetEncoding)
            .ok_or_else(|| {
                de::Error::custom(format!(
                    "unknown encoding {label:?}: use \"auto\", \"utf-8\", \"utf-16le\", \
                     \"utf-16be\", \"windows-1252\" or \"iso-8859-15\""
                ))
            })
    }
}

/// One of `,` `;` `\t` `|`.
pub(super) struct PresetDelimiter(pub Delimiter);

impl<'de> Deserialize<'de> for PresetDelimiter {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<PresetDelimiter, D::Error> {
        let text = String::deserialize(deserializer)?;
        Delimiter::ALL
            .into_iter()
            .find(|delimiter| text.as_bytes() == [delimiter.byte()])
            .map(PresetDelimiter)
            .ok_or_else(|| {
                de::Error::custom(r#"the delimiter must be one of ",", ";", "\t" or "|""#)
            })
    }
}
