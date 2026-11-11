//! CSV presets: `presets/<region>/<bank>.toml`, one per bank and account
//! type.
//!
//! A preset is data, not code: a [`CsvMapping`] written in TOML, plus a
//! name, an ID and a map from the bank's own categories to Coffer's
//! category keys. The format is documented for contributors in
//! `presets/README.md`.
//!
//! - **Strict.** Unknown keys are refused, so a misspelt field fails
//!   loudly instead of being ignored. Column names, date formats, the
//!   encoding and category keys are checked as the file is read, and the
//!   finished mapping is checked by [`CsvImporter::new`].
//! - **Bundled.** Every preset in the repository's `presets/` directory is
//!   compiled into the binary ([`Presets::bundled`]), so an installed
//!   Coffer doesn't need the source tree.
//! - **IDs are stable.** The ID is recorded on every import batch
//!   (`import_batch.preset_id`), so it never changes once a preset ships.
//!   It's the region, then the file name, then optionally more:
//!   `presets/uk/monzo.toml` can be `uk-monzo` or `uk-monzo-current`.

mod raw;

use std::collections::BTreeMap;
use std::fmt;

use coffer_core::category::CategoryKey;
use coffer_core::money::DecimalMark;

use crate::AnyImporter;
use crate::csv::{
    AmountColumns, Column, CsvFile, CsvImporter, CsvMapping, DateColumn, MappingError,
    OriginalAmountColumns, SplitSigns, TextEncoding, TimeColumn,
};
use raw::{PresetColumn, RawAmount, RawColumn, RawDescription, RawPreset};

/// The presets in the repository's `presets/` directory, as
/// `(path under presets/, contents)`, sorted by path (`build.rs`).
const BUNDLED: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/bundled_presets.rs"));

/// One bank export's layout and category names.
#[derive(Clone)]
pub struct Preset {
    id: String,
    region: String,
    importer: CsvImporter,
    ignore: Vec<String>,
    category_map: BTreeMap<String, CategoryKey>,
}

/// Why a preset's TOML can't be used.
///
/// Presets are data written by contributors, not transaction data, so
/// these messages quote them freely.
#[derive(Debug, thiserror::Error)]
pub enum PresetError {
    /// Not valid TOML, an unknown or missing key, or a field that's wrong
    /// on its own (a column, date format, encoding or category key). The
    /// message points at the line.
    #[error(transparent)]
    Toml(#[from] toml::de::Error),

    #[error("the region {0:?} must be lowercase letters, such as \"uk\"")]
    Region(String),

    #[error(
        "the id {id:?} must be the region (\"{region}\"), a hyphen, then lowercase letters, \
         digits and single hyphens"
    )]
    Id { id: String, region: String },

    #[error("the name is empty")]
    EmptyName,

    #[error("`amount` needs either `column`, or `debit` and `credit` (with optional `signs`)")]
    Amount,

    #[error("`description` needs either `column` or `columns`, not both")]
    Description,

    #[error(transparent)]
    Mapping(#[from] MappingError),

    #[error("`ignore` names columns, but `header = false` says the file has no header row")]
    IgnoreWithoutHeader,

    #[error("`ignore` lists {0:?}, which the preset reads")]
    IgnoredColumnRead(String),

    #[error("`ignore` lists {0:?} more than once, or an empty name")]
    IgnoredColumnRepeated(String),

    #[error("`category_map` needs a `source_category` column to read the categories from")]
    CategoryMapWithoutSourceCategory,

    #[error(
        "`category_map` has a source category that's empty or starts or ends with a space: {0:?}"
    )]
    CategoryMapSource(String),
}

impl Preset {
    /// Reads one preset. Where it sits in `presets/` is checked by
    /// [`Presets::from_files`].
    pub fn from_toml(text: &str) -> Result<Preset, PresetError> {
        let raw: RawPreset = toml::from_str(text)?;

        let region_ok =
            !raw.region.is_empty() && raw.region.bytes().all(|b| b.is_ascii_lowercase());
        if !region_ok {
            return Err(PresetError::Region(raw.region));
        }
        let id_ok = raw
            .id
            .strip_prefix(&raw.region)
            .and_then(|rest| rest.strip_prefix('-'))
            .is_some_and(is_slug);
        if !id_ok {
            return Err(PresetError::Id {
                id: raw.id,
                region: raw.region,
            });
        }
        if raw.name.trim().is_empty() {
            return Err(PresetError::EmptyName);
        }

        let mapping = CsvMapping {
            currency: raw.currency,
            encoding: raw.encoding.map_or(TextEncoding::Auto, |e| e.0),
            delimiter: raw.delimiter.map(|d| d.0),
            skip_rows: raw.skip_rows,
            has_header: raw.header,
            date: DateColumn {
                column: raw.date.column.0,
                format: raw.date.format.0,
            },
            time: raw.time.map(|time| TimeColumn {
                column: time.column.0,
                format: time.format.0,
            }),
            amount: amount(raw.amount)?,
            decimal: raw.decimal.unwrap_or(DecimalMark::Point),
            currency_column: column(raw.row_currency),
            merchant: column(raw.merchant),
            description: description(raw.description)?,
            external_id: column(raw.external_id),
            source_category: column(raw.source_category),
            source_type: column(raw.source_type),
            original_amount: raw.original_amount.map(|original| OriginalAmountColumns {
                amount: original.column.0,
                currency: original.currency_column.0,
            }),
            note: column(raw.note),
            balance: column(raw.balance),
        };

        check_ignore(&mapping, &raw.ignore)?;
        if !raw.category_map.is_empty() && mapping.source_category.is_none() {
            return Err(PresetError::CategoryMapWithoutSourceCategory);
        }
        if let Some(source) = raw
            .category_map
            .keys()
            .find(|source| source.is_empty() || source.trim() != source.as_str())
        {
            return Err(PresetError::CategoryMapSource(source.clone()));
        }

        let importer = CsvImporter::new(raw.name, mapping)?.with_preset(&raw.id);
        Ok(Preset {
            id: raw.id,
            region: raw.region,
            importer,
            ignore: raw.ignore,
            category_map: raw.category_map,
        })
    }

    /// The stable ID recorded on import batches, e.g. `uk-monzo-current`.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// What the user sees, e.g. `Monzo (current account)`.
    pub fn name(&self) -> &str {
        crate::Detect::name(&self.importer)
    }

    /// `uk`, `us`, `eu`, …: the directory the preset lives in.
    pub fn region(&self) -> &str {
        &self.region
    }

    /// The importer that reads files with this preset; its statements carry
    /// the preset's ID.
    pub fn importer(&self) -> &CsvImporter {
        &self.importer
    }

    pub fn mapping(&self) -> &CsvMapping {
        self.importer.mapping()
    }

    /// Columns the export has that the preset deliberately doesn't read
    /// (an emoji, a receipt link, a duplicate of the amount).
    pub fn ignored_columns(&self) -> &[String] {
        &self.ignore
    }

    /// The bank's category names mapped to Coffer's category keys.
    pub fn category_map(&self) -> &BTreeMap<String, CategoryKey> {
        &self.category_map
    }

    /// The category key for a row's source category, matched exactly
    /// (case included) after trimming the row's value. `None` when the
    /// preset doesn't map it, which leaves the row for rules or the user.
    pub fn category_for(&self, source_category: &str) -> Option<&CategoryKey> {
        self.category_map.get(source_category.trim())
    }

    /// Header columns in `file` that the preset neither reads nor lists in
    /// `ignore`, in file order. A bank adding a column shows up here, so the
    /// import preview can say what isn't being imported instead of leaving
    /// it out silently. Empty for a file without a header.
    pub fn unread_columns<'a>(&self, file: &'a CsvFile) -> Vec<&'a str> {
        let read: Vec<&str> = self
            .mapping()
            .columns()
            .filter_map(|column| match column {
                Column::Named(name) => Some(name.trim()),
                Column::Index(_) => None,
            })
            .collect();
        file.header()
            .map(|header| header.fields.as_slice())
            .unwrap_or_default()
            .iter()
            .map(String::as_str)
            .filter(|cell| {
                !cell.is_empty()
                    && !read.contains(cell)
                    && !self.ignore.iter().any(|ignored| ignored.trim() == *cell)
            })
            .collect()
    }
}

/// Shows the ID; the mapping is in [`Preset::mapping`].
impl fmt::Debug for Preset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Preset")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// Lowercase ASCII letters and digits in hyphen-separated runs: `monzo`,
/// `monzo-current`.
fn is_slug(text: &str) -> bool {
    text.split('-').all(|run| {
        !run.is_empty()
            && run
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    })
}

fn column(raw: Option<RawColumn>) -> Option<Column> {
    raw.map(|raw| raw.column.0)
}

fn amount(raw: RawAmount) -> Result<AmountColumns, PresetError> {
    match raw {
        RawAmount {
            column: Some(column),
            debit: None,
            credit: None,
            signs: None,
        } => Ok(AmountColumns::Signed(column.0)),
        RawAmount {
            column: None,
            debit: Some(debit),
            credit: Some(credit),
            signs,
        } => Ok(AmountColumns::Split {
            debit: debit.0,
            credit: credit.0,
            signs: signs.map_or(SplitSigns::Unsigned, SplitSigns::from),
        }),
        _ => Err(PresetError::Amount),
    }
}

fn description(raw: RawDescription) -> Result<Vec<Column>, PresetError> {
    let columns: Vec<PresetColumn> = match raw {
        RawDescription {
            column: Some(column),
            columns: None,
        } => vec![column],
        RawDescription {
            column: None,
            columns: Some(columns),
        } => columns,
        _ => return Err(PresetError::Description),
    };
    Ok(columns.into_iter().map(|column| column.0).collect())
}

fn check_ignore(mapping: &CsvMapping, ignore: &[String]) -> Result<(), PresetError> {
    if ignore.is_empty() {
        return Ok(());
    }
    if !mapping.has_header {
        return Err(PresetError::IgnoreWithoutHeader);
    }
    for (i, name) in ignore.iter().enumerate() {
        let name = name.trim();
        if name.is_empty() || ignore[..i].iter().any(|earlier| earlier.trim() == name) {
            return Err(PresetError::IgnoredColumnRepeated(name.to_owned()));
        }
        let read = mapping
            .columns()
            .any(|column| matches!(column, Column::Named(read) if read.trim() == name));
        if read {
            return Err(PresetError::IgnoredColumnRead(name.to_owned()));
        }
    }
    Ok(())
}

/// A set of presets with distinct IDs, sorted by ID.
#[derive(Clone, Debug, Default)]
pub struct Presets {
    presets: Vec<Preset>,
}

/// Why a set of preset files can't be loaded. Each names the file, as a
/// path under `presets/`.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("preset {path}: {error}")]
    Preset { path: String, error: PresetError },

    #[error(
        "preset {path} must be at <region>/<bank>.toml, in lowercase letters, digits and hyphens"
    )]
    Location { path: String },

    #[error("preset {path} says its region is {region:?}, but it's in the {directory:?} directory")]
    Region {
        path: String,
        region: String,
        directory: String,
    },

    #[error("preset {path} has the id {id:?}, which must start with {expected:?}")]
    IdFileName {
        path: String,
        id: String,
        expected: String,
    },

    #[error("presets {first} and {second} have the same id {id:?}")]
    DuplicateId {
        id: String,
        first: String,
        second: String,
    },
}

impl Presets {
    /// The presets compiled into Coffer from the repository's `presets/`.
    /// The tests load them all, so this only fails on a build from a
    /// broken tree.
    pub fn bundled() -> Result<Presets, LoadError> {
        Presets::from_files(BUNDLED.iter().copied())
    }

    /// Loads `(path, contents)` pairs, where `path` is relative to the
    /// presets directory and uses `/`: `uk/monzo.toml`. Each file must be at
    /// `<region>/<bank>.toml`, name its directory as its region and have an
    /// ID of `<region>-<bank>` or `<region>-<bank>-…`; IDs must be unique.
    pub fn from_files<'a>(
        files: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Presets, LoadError> {
        let mut found: BTreeMap<String, (String, Preset)> = BTreeMap::new();
        for (path, text) in files {
            let location = || LoadError::Location {
                path: path.to_owned(),
            };
            let (directory, file) = path.split_once('/').ok_or_else(location)?;
            let stem = file.strip_suffix(".toml").ok_or_else(location)?;
            if !is_slug(directory) || !is_slug(stem) {
                return Err(location());
            }

            let preset = Preset::from_toml(text).map_err(|error| LoadError::Preset {
                path: path.to_owned(),
                error,
            })?;
            if preset.region != directory {
                return Err(LoadError::Region {
                    path: path.to_owned(),
                    region: preset.region,
                    directory: directory.to_owned(),
                });
            }
            let expected = format!("{directory}-{stem}");
            let id_ok = preset
                .id
                .strip_prefix(&expected)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'));
            if !id_ok {
                return Err(LoadError::IdFileName {
                    path: path.to_owned(),
                    id: preset.id,
                    expected,
                });
            }
            if let Some((first, _)) = found.get(&preset.id) {
                return Err(LoadError::DuplicateId {
                    id: preset.id,
                    first: first.clone(),
                    second: path.to_owned(),
                });
            }
            found.insert(preset.id.clone(), (path.to_owned(), preset));
        }
        Ok(Presets {
            presets: found.into_values().map(|(_, preset)| preset).collect(),
        })
    }

    pub fn get(&self, id: &str) -> Option<&Preset> {
        self.presets
            .binary_search_by(|preset| preset.id.as_str().cmp(id))
            .ok()
            .map(|index| &self.presets[index])
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Preset> {
        self.presets.iter()
    }

    pub fn len(&self) -> usize {
        self.presets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.presets.is_empty()
    }

    /// Every preset's importer, by ID, for [`detect`](crate::detect).
    pub fn importers(&self) -> Vec<&dyn AnyImporter> {
        self.presets
            .iter()
            .map(|preset| &preset.importer as &dyn AnyImporter)
            .collect()
    }
}

impl<'a> IntoIterator for &'a Presets {
    type Item = &'a Preset;
    type IntoIter = std::slice::Iter<'a, Preset>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests;
