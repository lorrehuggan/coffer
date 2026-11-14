//! Writing a preset back out as TOML (M3.5): what "Save as preset…" stores
//! and "Save a copy…" exports.
//!
//! The text is the format `presets/README.md` documents, keys in its order,
//! defaults left out, so a saved preset reads like a bundled one and can be
//! contributed to `presets/` as it is. [`Preset::new`] builds a preset by
//! writing its TOML and reading it back with [`Preset::from_toml`], so a
//! preset made on the mapping screen passes exactly the checks a file
//! does, and what is saved is what was checked.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use coffer_core::category::CategoryKey;
use coffer_core::money::DecimalMark;

use super::{Preset, PresetError};
use crate::csv::{AmountColumns, Column, CsvMapping, SplitSigns, TextEncoding};

/// What a new preset is made of.
#[derive(Clone, Debug)]
pub struct PresetParts {
    /// `<region>-<bank>…`, as [`Preset::id`].
    pub id: String,
    pub name: String,
    pub region: String,
    pub mapping: CsvMapping,
    /// Header columns read on purpose not at all.
    pub ignore: Vec<String>,
    pub category_map: BTreeMap<String, CategoryKey>,
    pub sub_account_category: Option<CategoryKey>,
}

/// The first line of a preset Coffer writes.
const SAVED_HEADER: &str = "# Saved from Coffer's column mapping. The fields are described in \
                            presets/README.md.\n";

impl Preset {
    /// A preset from its parts, checked as [`Preset::from_toml`] checks a
    /// file (it is written out and read back).
    pub fn new(parts: PresetParts) -> Result<Preset, PresetError> {
        Preset::from_toml(&write(&parts))
    }

    /// The preset as TOML that [`Preset::from_toml`] reads back to the same
    /// preset.
    pub fn to_toml(&self) -> String {
        write(&PresetParts {
            id: self.id.clone(),
            name: self.name().to_owned(),
            region: self.region.clone(),
            mapping: self.mapping().clone(),
            ignore: self.ignore.clone(),
            category_map: self.category_map().clone(),
            sub_account_category: self.sub_account_category().cloned(),
        })
    }
}

/// `parts` in the preset format.
fn write(parts: &PresetParts) -> String {
    let mapping = &parts.mapping;
    let mut lines: Vec<String> = vec![SAVED_HEADER.trim_end().to_owned()];

    lines.push(format!("id = {}", quote(&parts.id)));
    lines.push(format!("name = {}", quote(&parts.name)));
    lines.push(format!("region = {}", quote(&parts.region)));
    lines.push(format!("currency = {}", quote(mapping.currency.code())));
    lines.push(String::new());

    if mapping.encoding != TextEncoding::Auto {
        lines.push(format!(
            "encoding = {}",
            quote(&mapping.encoding.name().to_ascii_lowercase())
        ));
    }
    if let Some(delimiter) = mapping.delimiter {
        lines.push(format!(
            "delimiter = {}",
            quote(&char::from(delimiter.byte()).to_string())
        ));
    }
    if mapping.skip_rows != 0 {
        lines.push(format!("skip_rows = {}", mapping.skip_rows));
    }
    if !mapping.has_header {
        lines.push("header = false".to_owned());
    }
    if mapping.multiline_fields {
        lines.push("multiline_fields = true".to_owned());
    }
    lines.push(String::new());

    if let Some(column) = &mapping.external_id {
        lines.push(format!(
            "external_id = {}",
            table(&[("column", col(column))])
        ));
    }
    lines.push(format!(
        "date = {}",
        table(&[
            ("column", col(&mapping.date.column)),
            ("format", quote(mapping.date.format.pattern())),
        ])
    ));
    if let Some(time) = &mapping.time {
        lines.push(format!(
            "time = {}",
            table(&[
                ("column", col(&time.column)),
                ("format", quote(time.format.pattern())),
            ])
        ));
    }
    lines.push(format!(
        "amount = {}",
        match &mapping.amount {
            AmountColumns::Signed(column) => table(&[("column", col(column))]),
            AmountColumns::Split {
                debit,
                credit,
                signs,
            } => {
                let mut fields = vec![("debit", col(debit)), ("credit", col(credit))];
                if *signs == SplitSigns::Signed {
                    fields.push(("signs", quote("signed")));
                }
                table(&fields)
            }
        }
    ));
    if mapping.decimal == DecimalMark::Comma {
        lines.push(format!("decimal = {}", quote(",")));
    }
    let optional = |key: &str, column: &Option<Column>| {
        column
            .as_ref()
            .map(|column| format!("{key} = {}", table(&[("column", col(column))])))
    };
    lines.extend(optional("row_currency", &mapping.currency_column));
    if let Some(original) = &mapping.original_amount {
        lines.push(format!(
            "original_amount = {}",
            table(&[
                ("column", col(&original.amount)),
                ("currency_column", col(&original.currency)),
            ])
        ));
    }
    lines.extend(optional("merchant", &mapping.merchant));
    lines.push(format!(
        "description = {}",
        match mapping.description.as_slice() {
            [only] => table(&[("column", col(only))]),
            several => table(&[(
                "columns",
                format!(
                    "[{}]",
                    several.iter().map(col).collect::<Vec<_>>().join(", ")
                ),
            )]),
        }
    ));
    lines.extend(optional("source_category", &mapping.source_category));
    lines.extend(optional("source_type", &mapping.source_type));
    lines.extend(optional("note", &mapping.note));
    lines.extend(optional("balance", &mapping.balance));
    if !parts.ignore.is_empty() {
        lines.push(format!(
            "ignore = [{}]",
            parts
                .ignore
                .iter()
                .map(|name| quote(name))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    if !parts.category_map.is_empty() {
        lines.push(String::new());
        lines.push("[category_map]".to_owned());
        for (source, key) in &parts.category_map {
            lines.push(format!("{} = {}", quote(source), quote(key.as_str())));
        }
    }

    if let Some(transfer) = &mapping.sub_account_transfer {
        lines.push(String::new());
        lines.push("[sub_account_transfer]".to_owned());
        lines.push(format!("kind = {}", quote(transfer.kind.as_str())));
        lines.push(format!(
            "when = {}",
            table(&[
                ("column", col(&transfer.match_column)),
                ("is", quote(&transfer.match_value)),
            ])
        ));
        lines.push(format!(
            "key = {}",
            table(&[("column", col(&transfer.key))])
        ));
        if let Some(name) = &transfer.name {
            lines.push(format!("name = {}", table(&[("column", col(name))])));
        }
        if let Some(category) = &parts.sub_account_category {
            lines.push(format!("category = {}", quote(category.as_str())));
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// `{ key = value, … }`.
fn table(fields: &[(&str, String)]) -> String {
    let inner = fields
        .iter()
        .map(|(key, value)| format!("{key} = {value}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{ {inner} }}")
}

/// A column as a preset writes it: its name, or its position from 1.
fn col(column: &Column) -> String {
    match column {
        Column::Named(name) => quote(name),
        Column::Index(index) => (index + 1).to_string(),
    }
}

/// A TOML basic string.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => {
                write!(out, "\\u{:04X}", u32::from(c)).expect("in memory");
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A preset's bank part from its name: lowercase ASCII letters and digits,
/// anything else as single hyphens (`Example Bank (savings)` →
/// `example-bank-savings`). `None` when the name has no letter or digit
/// Coffer can use.
pub fn slug(name: &str) -> Option<String> {
    let mut out = String::new();
    let mut gap = false;
    for c in name.chars() {
        let c = fold(c);
        if c.is_ascii_alphanumeric() {
            if gap && !out.is_empty() {
                out.push('-');
            }
            gap = false;
            out.push(c.to_ascii_lowercase());
        } else {
            gap = true;
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Common accented Latin letters as their base letter, so `Crédit` is
/// `credit`, not `cr-dit`.
fn fold(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => 'a',
        'ç' | 'Ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' | 'È' | 'É' | 'Ê' | 'Ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'Ì' | 'Í' | 'Î' | 'Ï' => 'i',
        'ñ' | 'Ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' => 'o',
        'ù' | 'ú' | 'û' | 'ü' | 'Ù' | 'Ú' | 'Û' | 'Ü' => 'u',
        'ý' | 'ÿ' | 'Ý' => 'y',
        'ß' => 's',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use coffer_core::money::Currency;

    use super::*;
    use crate::csv::{
        DateColumn, DateFormat, Delimiter, OriginalAmountColumns, TimeColumn, TimeFormat,
    };
    use crate::preset::Presets;

    /// Everything about two presets that a file can say.
    fn same(a: &Preset, b: &Preset) {
        assert_eq!(a.id(), b.id());
        assert_eq!(a.name(), b.name());
        assert_eq!(a.region(), b.region());
        assert_eq!(a.mapping(), b.mapping());
        assert_eq!(a.ignored_columns(), b.ignored_columns());
        assert_eq!(a.category_map(), b.category_map());
        assert_eq!(a.sub_account_category(), b.sub_account_category());
    }

    #[test]
    fn every_bundled_preset_round_trips() {
        for preset in &Presets::bundled().unwrap() {
            let text = preset.to_toml();
            let again = Preset::from_toml(&text)
                .unwrap_or_else(|error| panic!("{}: {error}\n{text}", preset.id()));
            same(preset, &again);
            assert_eq!(again.to_toml(), text, "{} isn't stable", preset.id());
        }
    }

    #[test]
    fn every_field_round_trips() {
        let mut mapping = CsvMapping::new(
            Currency::EUR,
            DateColumn {
                column: Column::Index(0),
                format: DateFormat::new("%d.%m.%Y").unwrap(),
            },
            AmountColumns::Split {
                debit: Column::Index(3),
                credit: Column::Index(4),
                signs: SplitSigns::Signed,
            },
            Column::Index(2),
        );
        mapping.description.push(Column::Index(5));
        mapping.encoding = TextEncoding::Iso8859_15;
        mapping.delimiter = Some(Delimiter::Tab);
        mapping.skip_rows = 3;
        mapping.has_header = false;
        mapping.multiline_fields = true;
        mapping.decimal = DecimalMark::Comma;
        mapping.time = Some(TimeColumn {
            column: Column::Index(1),
            format: TimeFormat::new("%H:%M").unwrap(),
        });
        mapping.currency_column = Some(Column::Index(6));
        mapping.original_amount = Some(OriginalAmountColumns {
            amount: Column::Index(7),
            currency: Column::Index(8),
        });
        mapping.merchant = Some(Column::Index(9));
        mapping.external_id = Some(Column::Index(10));
        mapping.source_type = Some(Column::Index(11));
        mapping.note = Some(Column::Index(12));
        mapping.balance = Some(Column::Index(13));

        let preset = Preset::new(PresetParts {
            id: "eu-example-giro".to_owned(),
            name: "Example \"Giro\" \\ Konto".to_owned(),
            region: "eu".to_owned(),
            mapping: mapping.clone(),
            ignore: Vec::new(),
            category_map: BTreeMap::new(),
            sub_account_category: None,
        })
        .unwrap();
        assert_eq!(preset.mapping(), &mapping);
        assert_eq!(preset.name(), "Example \"Giro\" \\ Konto");
        same(&preset, &Preset::from_toml(&preset.to_toml()).unwrap());
    }

    #[test]
    fn a_new_preset_is_checked_like_a_file() {
        let presets = Presets::bundled().unwrap();
        let monzo = presets.get("uk-monzo").unwrap();
        let parts = |id: &str, region: &str| PresetParts {
            id: id.to_owned(),
            name: "Mine".to_owned(),
            region: region.to_owned(),
            mapping: monzo.mapping().clone(),
            ignore: vec!["Amount".to_owned()],
            category_map: BTreeMap::new(),
            sub_account_category: None,
        };
        assert!(matches!(
            Preset::new(parts("uk-mine", "uk")),
            Err(PresetError::IgnoredColumnRead(_))
        ));
        let mut ok = parts("uk-mine", "uk");
        ok.ignore.clear();
        assert!(Preset::new(ok.clone()).is_ok());
        ok.id = "us-mine".to_owned();
        assert!(matches!(Preset::new(ok), Err(PresetError::Id { .. })));
    }

    #[test]
    fn the_saved_text_reads_like_a_bundled_preset() {
        let presets = Presets::bundled().unwrap();
        let text = presets.get("uk-monzo").unwrap().to_toml();
        assert!(text.starts_with("# Saved from Coffer"));
        assert!(text.contains("\ndate = { column = \"Date\", format = \"%d/%m/%Y\" }\n"));
        assert!(text.contains("\n[sub_account_transfer]\nkind = \"pot\"\n"));
    }

    #[test]
    fn slugs() {
        assert_eq!(
            slug("Example Bank (savings)").as_deref(),
            Some("example-bank-savings")
        );
        assert_eq!(
            slug("  Crédit Agricole ").as_deref(),
            Some("credit-agricole")
        );
        assert_eq!(slug("N26").as_deref(), Some("n26"));
        assert_eq!(slug("—").as_deref(), None);
    }

    #[test]
    fn quoting() {
        assert_eq!(quote("a\"b\\c\td\u{1}"), "\"a\\\"b\\\\c\\td\\u0001\"");
    }
}
