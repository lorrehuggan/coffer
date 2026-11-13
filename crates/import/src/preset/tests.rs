//! Preset TOML → [`CsvMapping`], the checks on it, and loading sets of
//! presets, including the bundled ones and their fixtures.

use std::path::Path;

use coffer_core::money::{Currency, DecimalMark};
use coffer_core::statement::{RowProblem, SubAccountKind, SubAccountRef};

use super::*;
use crate::csv::{DateFormat, Delimiter, TimeFormat};
use crate::{Detect, Detection, Importer, SourceFile, detect};

fn fixture(name: &str) -> SourceFile {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/csv")
        .join(name);
    SourceFile::read(&path).unwrap()
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

fn key(key: &str) -> CategoryKey {
    CategoryKey::new(key).unwrap()
}

/// Reads `fixtures/csv/signed-amount.csv`.
const SIGNED: &str = r#"
id = "uk-example-current"
name = "Example Bank (current account)"
region = "uk"
currency = "GBP"
encoding = "utf-8"
delimiter = ","
external_id = { column = "Transaction ID" }
date = { column = "Date", format = "%d/%m/%Y" }
time = { column = "Time", format = "%H:%M:%S" }
amount = { column = "Amount" }
row_currency = { column = "Currency" }
merchant = { column = "Name" }
description = { column = "Description" }
source_category = { column = "Category" }
source_type = { column = "Type" }
original_amount = { column = "Local amount", currency_column = "Local currency" }
decimal = "."
ignore = ["Notes"]

[category_map]
"Eating out" = "food.eating_out"
"Groceries" = "food.groceries"
"Transfers" = "transfer"
"#;

#[test]
fn signed_amount_preset() {
    let preset = Preset::from_toml(SIGNED).unwrap();
    assert_eq!(preset.id(), "uk-example-current");
    assert_eq!(preset.name(), "Example Bank (current account)");
    assert_eq!(preset.region(), "uk");

    let mut expected = CsvMapping::new(
        Currency::GBP,
        date_column(named("Date"), "%d/%m/%Y"),
        AmountColumns::Signed(named("Amount")),
        named("Description"),
    );
    expected.encoding = TextEncoding::Utf8;
    expected.delimiter = Some(Delimiter::Comma);
    expected.time = Some(TimeColumn {
        column: named("Time"),
        format: TimeFormat::new("%H:%M:%S").unwrap(),
    });
    expected.currency_column = Some(named("Currency"));
    expected.merchant = Some(named("Name"));
    expected.external_id = Some(named("Transaction ID"));
    expected.source_category = Some(named("Category"));
    expected.source_type = Some(named("Type"));
    expected.original_amount = Some(OriginalAmountColumns {
        amount: named("Local amount"),
        currency: named("Local currency"),
    });
    assert_eq!(preset.mapping(), &expected);
    assert_eq!(preset.ignored_columns(), ["Notes"]);

    let file = fixture("signed-amount.csv");
    assert_eq!(preset.importer().detect(&file), Detection::Source);
    let statement = preset.importer().import(&file).unwrap();
    assert_eq!(statement.preset_id(), Some("uk-example-current"));
    assert_eq!(statement.rows().len(), 10, "every row is kept");
}

#[test]
fn category_map_matches_exactly_after_trimming() {
    let preset = Preset::from_toml(SIGNED).unwrap();
    assert_eq!(preset.category_map().len(), 3);
    assert_eq!(
        preset.category_for("Eating out"),
        Some(&key("food.eating_out"))
    );
    assert_eq!(preset.category_for(" Transfers "), Some(&key("transfer")));
    assert_eq!(preset.category_for("eating out"), None, "case counts");
    assert_eq!(preset.category_for("Bills"), None, "unmapped");
}

#[test]
fn unread_columns_are_the_ones_neither_read_nor_ignored() {
    let file = fixture("signed-amount.csv");
    let ignoring = Preset::from_toml(SIGNED).unwrap();
    let parsed = ignoring.importer().parse(&file).unwrap();
    assert_eq!(ignoring.unread_columns(&parsed), Vec::<&str>::new());

    let reading_less = Preset::from_toml(
        &SIGNED
            .replace("ignore = [\"Notes\"]", "")
            .replace("source_type = { column = \"Type\" }", ""),
    )
    .unwrap();
    assert_eq!(reading_less.unread_columns(&parsed), ["Type", "Notes"]);
}

#[test]
fn debit_credit_preset() {
    let preset = Preset::from_toml(
        r#"
        id = "uk-example-savings"
        name = "Example Bank (savings)"
        region = "uk"
        currency = "GBP"
        encoding = "windows-1252"
        skip_rows = 3
        date = { column = "Date", format = "%d/%m/%Y" }
        amount = { debit = "Paid out", credit = "Paid in" }
        description = { column = "Description" }
        balance = { column = "Balance" }
        "#,
    )
    .unwrap();

    let mut expected = CsvMapping::new(
        Currency::GBP,
        date_column(named("Date"), "%d/%m/%Y"),
        AmountColumns::Split {
            debit: named("Paid out"),
            credit: named("Paid in"),
            signs: SplitSigns::Unsigned,
        },
        named("Description"),
    );
    expected.encoding = TextEncoding::Windows1252;
    expected.skip_rows = 3;
    expected.balance = Some(named("Balance"));
    assert_eq!(preset.mapping(), &expected);
    assert!(preset.category_map().is_empty());

    let file = fixture("debit-credit-windows-1252.csv");
    assert_eq!(preset.importer().detect(&file), Detection::Source);
    assert!(preset.importer().import(&file).unwrap().lines().count() > 0);
}

#[test]
fn signed_split_columns() {
    let preset = Preset::from_toml(
        r#"
        id = "eu-example"
        name = "Example"
        region = "eu"
        currency = "EUR"
        date = { column = "Date", format = "%Y-%m-%d" }
        amount = { debit = "Out", credit = "In", signs = "signed" }
        description = { column = "Text" }
        "#,
    )
    .unwrap();
    assert_eq!(
        preset.mapping().amount,
        AmountColumns::Split {
            debit: named("Out"),
            credit: named("In"),
            signs: SplitSigns::Signed,
        }
    );
}

#[test]
fn semicolon_decimal_comma_preset() {
    let preset = Preset::from_toml(
        r#"
        id = "eu-example-giro"
        name = "Beispielbank (Girokonto)"
        region = "eu"
        currency = "EUR"
        delimiter = ";"
        decimal = ","
        date = { column = "Buchungstag", format = "%d.%m.%Y" }
        amount = { column = "Betrag" }
        row_currency = { column = "Währung" }
        merchant = { column = "Auftraggeber" }
        description = { columns = ["Auftraggeber", "Verwendungszweck"] }
        "#,
    )
    .unwrap();
    let mapping = preset.mapping();
    assert_eq!(mapping.encoding, TextEncoding::Auto, "the default");
    assert_eq!(mapping.delimiter, Some(Delimiter::Semicolon));
    assert_eq!(mapping.decimal, DecimalMark::Comma);
    assert_eq!(
        mapping.description,
        [named("Auftraggeber"), named("Verwendungszweck")]
    );

    let file = fixture("semicolon-decimal-comma.csv");
    assert_eq!(preset.importer().detect(&file), Detection::Source);
    assert!(preset.importer().import(&file).unwrap().lines().count() > 0);
}

#[test]
fn headerless_presets_count_columns_from_one() {
    let preset = Preset::from_toml(
        r#"
        id = "us-example"
        name = "Example"
        region = "us"
        currency = "USD"
        delimiter = "\t"
        header = false
        date = { column = 1, format = "%m/%d/%Y" }
        amount = { column = 3 }
        description = { column = 2 }
        "#,
    )
    .unwrap();
    let mapping = preset.mapping();
    assert!(!mapping.has_header);
    assert_eq!(mapping.delimiter, Some(Delimiter::Tab));
    assert_eq!(mapping.date.column, Column::Index(0));
    assert_eq!(mapping.amount, AmountColumns::Signed(Column::Index(2)));
    assert_eq!(mapping.description, [Column::Index(1)]);

    let statement = preset
        .importer()
        .import(&SourceFile::from_bytes(
            "s.tsv",
            "11/23/2026\tCoffee\t-3.40\n",
        ))
        .unwrap();
    assert_eq!(statement.lines().count(), 1);
}

/// The smallest valid preset, for the error tests to break.
const MINIMAL: &str = r#"
id = "uk-test"
name = "Test"
region = "uk"
currency = "GBP"
date = { column = "Date", format = "%d/%m/%Y" }
amount = { column = "Amount" }
description = { column = "Description" }
"#;

fn error(text: &str) -> PresetError {
    Preset::from_toml(text).expect_err("the preset should be refused")
}

/// The TOML error's message, which points at the line.
fn toml_error(text: &str) -> String {
    match error(text) {
        PresetError::Toml(error) => error.to_string(),
        other => panic!("expected a TOML error, got {other:?}"),
    }
}

#[test]
fn minimal_preset_takes_the_defaults() {
    let preset = Preset::from_toml(MINIMAL).unwrap();
    let expected = CsvMapping::new(
        Currency::GBP,
        date_column(named("Date"), "%d/%m/%Y"),
        AmountColumns::Signed(named("Amount")),
        named("Description"),
    );
    assert_eq!(preset.mapping(), &expected);
    assert!(!preset.mapping().multiline_fields, "rows are one line each");
}

#[test]
fn multiline_fields_lets_quoted_fields_hold_line_breaks() {
    let text = "Date,Amount,Description\n01/11/2026,-1.00,\"two\nlines\"\n";
    let file = SourceFile::from_bytes("s.csv", text);

    let one_line = Preset::from_toml(MINIMAL).unwrap();
    let statement = one_line.importer().import(&file).unwrap();
    assert_eq!(
        statement
            .flagged()
            .map(|(_, problem)| problem)
            .collect::<Vec<_>>(),
        [&RowProblem::MultiLineRow { lines: 2 }]
    );

    let preset = Preset::from_toml(&format!("multiline_fields = true\n{MINIMAL}")).unwrap();
    assert!(preset.mapping().multiline_fields);
    let statement = preset.importer().import(&file).unwrap();
    assert!(!statement.has_flagged());
    assert_eq!(
        statement.lines().next().unwrap().1.description_raw,
        "two\nlines"
    );

    assert!(
        toml_error(&format!("multiline_fields = \"yes\"\n{MINIMAL}")).contains("multiline_fields")
    );
}

#[test]
fn unread_columns_ignore_a_trailing_empty_header_cell() {
    let preset = Preset::from_toml(MINIMAL).unwrap();
    let file = SourceFile::from_bytes("s.csv", "Date,Amount,Description,\n01/11/2026,-1.00,a\n");
    let parsed = preset.importer().parse(&file).unwrap();
    assert_eq!(parsed.width(), 3);
    assert_eq!(preset.unread_columns(&parsed), Vec::<&str>::new());
    assert_eq!(preset.importer().detect(&file), Detection::Source);
}

#[test]
fn unknown_and_missing_keys_are_refused() {
    let typo = toml_error(&MINIMAL.replace("description =", "descripton ="));
    assert!(typo.contains("descripton"), "{typo}");

    let inner_typo = toml_error(&MINIMAL.replace(
        r#"amount = { column = "Amount" }"#,
        r#"amount = { colum = "Amount" }"#,
    ));
    assert!(inner_typo.contains("colum"), "{inner_typo}");

    let missing = toml_error(&MINIMAL.replace(r#"currency = "GBP""#, ""));
    assert!(missing.contains("currency"), "{missing}");

    // An unknown table is refused too (more in `sub_account_transfer_*`).
    let table = toml_error(&format!("{MINIMAL}\n[sub_accounts]\nkind = \"pot\"\n"));
    assert!(table.contains("sub_accounts"), "{table}");
}

#[test]
fn field_values_are_checked_where_they_are_written() {
    for (from, to, expected) in [
        (r#""%d/%m/%Y""#, r#""%d/%m""#, "needs a year"),
        (r#""%d/%m/%Y""#, r#""%d/%m/%Y %z""#, "time zones"),
        (r#"currency = "GBP""#, r#"currency = "XYZ""#, "XYZ"),
        (r#"{ column = "Date""#, r#"{ column = """#, "can't be empty"),
        (r#"{ column = "Date""#, r#"{ column = 0"#, "count from 1"),
        (r#"{ column = "Date""#, r#"{ column = -2"#, "count from 1"),
        (
            r#"{ column = "Date""#,
            r#"{ column = 1.5"#,
            "column position",
        ),
    ] {
        let message = toml_error(&MINIMAL.replace(from, to));
        assert!(message.contains(expected), "{to}: {message}");
        assert!(message.contains("line"), "points at the line: {message}");
    }

    for (line, expected) in [
        (r#"encoding = "shift_jis""#, "unknown encoding"),
        (r#"delimiter = ",,""#, "delimiter must be"),
        (r#"delimiter = "tab""#, "delimiter must be"),
        (r#"decimal = "·""#, "decimal"),
        (r#"time = { column = "Time", format = "%d" }"#, "time"),
        ("skip_rows = -1", "skip_rows"),
    ] {
        let message = toml_error(&format!("{line}\n{MINIMAL}"));
        assert!(message.contains(expected), "{line}: {message}");
    }

    let category = toml_error(&format!(
        "{MINIMAL}source_category = {{ column = \"Category\" }}\n\
         [category_map]\n\"Groceries\" = \"Food.Groceries\"\n"
    ));
    assert!(category.contains("category key"), "{category}");
}

#[test]
fn region_id_and_name() {
    assert!(matches!(
        error(&MINIMAL.replace(r#"region = "uk""#, r#"region = "UK""#)),
        PresetError::Region(region) if region == "UK"
    ));
    assert!(matches!(
        error(&MINIMAL.replace(r#"region = "uk""#, r#"region = """#)),
        PresetError::Region(_)
    ));
    for id in [
        "test",
        "us-test",
        "uk-",
        "uk-Test",
        "uk-test--x",
        "uk-test-",
        "uk_test",
    ] {
        let text = MINIMAL.replace(r#"id = "uk-test""#, &format!("id = {id:?}"));
        assert!(
            matches!(error(&text), PresetError::Id { id: found, .. } if found == id),
            "{id}"
        );
    }
    for id in ["uk-test", "uk-test-2", "uk-a-b-c"] {
        let text = MINIMAL.replace(r#"id = "uk-test""#, &format!("id = {id:?}"));
        assert_eq!(Preset::from_toml(&text).unwrap().id(), id);
    }
    assert!(matches!(
        error(&MINIMAL.replace(r#"name = "Test""#, r#"name = " ""#)),
        PresetError::EmptyName
    ));
}

#[test]
fn amount_and_description_take_one_form() {
    for amount in [
        r#"{ column = "Amount", debit = "Out", credit = "In" }"#,
        r#"{ debit = "Out" }"#,
        r#"{ column = "Amount", signs = "signed" }"#,
        "{}",
    ] {
        let text = MINIMAL.replace(
            r#"amount = { column = "Amount" }"#,
            &format!("amount = {amount}"),
        );
        assert!(matches!(error(&text), PresetError::Amount), "{amount}");
    }
    for description in [r#"{ column = "A", columns = ["B"] }"#, "{}"] {
        let text = MINIMAL.replace(
            r#"description = { column = "Description" }"#,
            &format!("description = {description}"),
        );
        assert!(
            matches!(error(&text), PresetError::Description),
            "{description}"
        );
    }
}

#[test]
fn the_mapping_is_validated() {
    let no_description = MINIMAL.replace(
        r#"description = { column = "Description" }"#,
        "description = { columns = [] }",
    );
    assert!(matches!(
        error(&no_description),
        PresetError::Mapping(MappingError::NoDescription)
    ));

    let same = MINIMAL.replace(
        r#"amount = { column = "Amount" }"#,
        r#"amount = { debit = "Amount", credit = "Amount" }"#,
    );
    assert!(matches!(
        error(&same),
        PresetError::Mapping(MappingError::SameDebitAndCredit)
    ));

    let headerless = format!("header = false\n{MINIMAL}");
    assert!(matches!(
        error(&headerless),
        PresetError::Mapping(MappingError::NamedColumnWithoutHeader(_))
    ));
}

#[test]
fn ignore_lists_unread_header_columns_once() {
    let with = |ignore: &str| format!("ignore = {ignore}\n{MINIMAL}");
    assert!(matches!(
        error(&with(r#"["Emoji", " Amount "]"#)),
        PresetError::IgnoredColumnRead(name) if name == "Amount"
    ));
    assert!(matches!(
        error(&with(r#"["Emoji", "Emoji "]"#)),
        PresetError::IgnoredColumnRepeated(name) if name == "Emoji"
    ));
    assert!(matches!(
        error(&with(r#"[""]"#)),
        PresetError::IgnoredColumnRepeated(_)
    ));

    let headerless = MINIMAL
        .replace(r#""Date""#, "1")
        .replace(r#""Amount""#, "2")
        .replace(r#""Description""#, "3");
    assert!(Preset::from_toml(&headerless.replace("id =", "header = false\nid =")).is_ok());
    assert!(matches!(
        error(&format!(
            "header = false\nignore = [\"Emoji\"]\n{headerless}"
        )),
        PresetError::IgnoreWithoutHeader
    ));
}

#[test]
fn category_map_needs_source_categories_written_cleanly() {
    let map_only = format!("{MINIMAL}[category_map]\n\"Groceries\" = \"food.groceries\"\n");
    assert!(matches!(
        error(&map_only),
        PresetError::CategoryMapWithoutSourceCategory
    ));

    for source in [" Groceries", "Groceries ", ""] {
        let text = format!(
            "{MINIMAL}source_category = {{ column = \"Category\" }}\n\
             [category_map]\n{source:?} = \"food.groceries\"\n"
        );
        assert!(
            matches!(error(&text), PresetError::CategoryMapSource(found) if found == source),
            "{source:?}"
        );
    }
}

/// [`MINIMAL`] with another ID and region.
fn preset_text(id: &str, region: &str) -> String {
    MINIMAL
        .replace(r#"id = "uk-test""#, &format!("id = {id:?}"))
        .replace(r#"region = "uk""#, &format!("region = {region:?}"))
}

#[test]
fn sets_are_sorted_by_id_and_found_by_it() {
    let monzo = preset_text("uk-monzo-current", "uk");
    let starling = preset_text("uk-starling", "uk");
    let chase = preset_text("us-chase-checking", "us");
    let presets = Presets::from_files([
        ("us/chase.toml", chase.as_str()),
        ("uk/starling.toml", starling.as_str()),
        ("uk/monzo.toml", monzo.as_str()),
    ])
    .unwrap();

    let ids: Vec<_> = presets.iter().map(Preset::id).collect();
    assert_eq!(
        ids,
        ["uk-monzo-current", "uk-starling", "us-chase-checking"]
    );
    assert_eq!(presets.len(), 3);
    assert_eq!(presets.get("uk-starling").unwrap().id(), "uk-starling");
    assert!(presets.get("uk-starling-x").is_none());
    assert!(Presets::from_files([]).unwrap().is_empty());
}

#[test]
fn files_must_sit_where_their_id_and_region_say() {
    let text = preset_text("uk-monzo", "uk");
    for path in [
        "monzo.toml",
        "uk/monzo.txt",
        "uk/Monzo.toml",
        "uk/banks/monzo.toml",
        "UK/monzo.toml",
        "/monzo.toml",
        "uk/.toml",
    ] {
        assert!(
            matches!(
                Presets::from_files([(path, text.as_str())]),
                Err(LoadError::Location { path: found }) if found == path
            ),
            "{path}"
        );
    }

    assert!(matches!(
        Presets::from_files([("us/monzo.toml", text.as_str())]),
        Err(LoadError::Region { region, directory, .. }) if region == "uk" && directory == "us"
    ));

    for path in ["uk/starling.toml", "uk/mon.toml", "uk/monzo-joint.toml"] {
        assert!(
            matches!(
                Presets::from_files([(path, text.as_str())]),
                Err(LoadError::IdFileName { .. })
            ),
            "{path}"
        );
    }
    let longer = preset_text("uk-monzo-current", "uk");
    assert!(Presets::from_files([("uk/monzo.toml", longer.as_str())]).is_ok());

    let broken = Presets::from_files([("uk/monzo.toml", "id = ")]).unwrap_err();
    assert!(
        broken.to_string().starts_with("preset uk/monzo.toml: "),
        "{broken}"
    );
}

#[test]
fn ids_are_unique() {
    let first = preset_text("uk-monzo-current", "uk");
    let second = preset_text("uk-monzo-current", "uk");
    assert!(matches!(
        Presets::from_files([
            ("uk/monzo.toml", first.as_str()),
            ("uk/monzo-current.toml", second.as_str()),
        ]),
        Err(LoadError::DuplicateId { first, second, .. })
            if first == "uk/monzo.toml" && second == "uk/monzo-current.toml"
    ));
}

#[test]
fn detection_picks_the_preset_whose_columns_a_file_has() {
    let savings = r#"
        id = "uk-example-savings"
        name = "Example Bank (savings)"
        region = "uk"
        currency = "GBP"
        encoding = "windows-1252"
        skip_rows = 3
        date = { column = "Date", format = "%d/%m/%Y" }
        amount = { debit = "Paid out", credit = "Paid in" }
        description = { column = "Description" }
    "#;
    let presets = Presets::from_files([
        ("uk/example.toml", SIGNED),
        ("uk/example-savings.toml", savings),
    ])
    .unwrap();
    let importers = presets.importers();

    let candidates = detect(&importers, &fixture("signed-amount.csv"));
    assert_eq!(
        candidates[0].importer.name(),
        "Example Bank (current account)"
    );
    assert_eq!(candidates[0].detection, Detection::Source);
    assert!(
        candidates[1..]
            .iter()
            .all(|c| c.detection < Detection::Source)
    );
}

/// Every preset in `presets/` loads, and has a fixture at
/// `fixtures/presets/<region>/<bank>.<ext>` that it recognises and reads
/// (`AGENTS.md` → "Bank presets"). Each preset's own tests check the rows.
#[test]
fn bundled_presets_load_and_read_their_fixtures() {
    let presets = Presets::bundled().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(presets.len(), BUNDLED.len());

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/presets");
    for (path, text) in BUNDLED {
        let stem = path.strip_suffix(".toml").unwrap();
        let (region, bank) = stem.split_once('/').unwrap();
        let preset = Preset::from_toml(text).unwrap_or_else(|error| panic!("{path}: {error}"));
        let found: Vec<_> = std::fs::read_dir(fixtures.join(region))
            .into_iter()
            .flatten()
            .map(|entry| entry.unwrap().path())
            .filter(|fixture| fixture.file_stem().is_some_and(|s| s == bank))
            .collect();
        assert!(
            !found.is_empty(),
            "{path} needs a fixture in fixtures/presets/{region}/"
        );
        for fixture in found {
            let file = SourceFile::read(&fixture).unwrap();
            assert_eq!(
                preset.importer().detect(&file),
                Detection::Source,
                "{}",
                fixture.display()
            );
            let statement = preset
                .importer()
                .import(&file)
                .unwrap_or_else(|error| panic!("{}: {error}", fixture.display()));
            assert!(statement.lines().count() > 0, "{}", fixture.display());
        }
    }
}

/// A bundled preset maps only to default categories, which every database
/// starts with (`coffer_core::category::DEFAULT_CATEGORIES`). Presets the
/// user saves may name their own categories too, so loading doesn't check
/// this; the import resolves keys against the database instead.
#[test]
fn bundled_presets_map_to_default_categories() {
    let presets = Presets::bundled().unwrap_or_else(|error| panic!("{error}"));
    for preset in presets.iter() {
        for (source, key) in preset.category_map() {
            assert!(
                coffer_core::category::default_category(key.as_str()).is_some(),
                "{}: {source:?} maps to {key}, which isn't a default category",
                preset.id()
            );
        }
        if let Some(key) = preset.sub_account_category() {
            assert!(
                coffer_core::category::default_category(key.as_str()).is_some(),
                "{}: sub-account transfers get {key}, which isn't a default category",
                preset.id()
            );
        }
    }
}

// Sub-account transfers (M1.17) -------------------------------------------------

/// [`MINIMAL`] with the columns a pot transfer needs and the section.
const POTS: &str = r#"
id = "uk-test"
name = "Test"
region = "uk"
currency = "GBP"
date = { column = "Date", format = "%d/%m/%Y" }
amount = { column = "Amount" }
description = { column = "Description" }
merchant = { column = "Name" }
source_type = { column = "Type" }

[sub_account_transfer]
kind = "pot"
when = { column = "Type", is = "Pot transfer" }
key = { column = "Description" }
name = { column = "Name" }
"#;

#[test]
fn sub_account_transfer_is_read() {
    let preset = Preset::from_toml(POTS).unwrap();
    assert_eq!(
        preset.sub_account_transfer(),
        Some(&SubAccountTransfer {
            kind: SubAccountKind::Pot,
            match_column: named("Type"),
            match_value: "Pot transfer".into(),
            key: named("Description"),
            name: Some(named("Name")),
        })
    );
    assert_eq!(
        Preset::from_toml(MINIMAL).unwrap().sub_account_transfer(),
        None
    );

    // `name` is optional; positions count from 1 here too.
    let unnamed = POTS.replace("\nname = { column = \"Name\" }\n", "\n");
    let preset = Preset::from_toml(&unnamed).unwrap();
    assert_eq!(preset.sub_account_transfer().unwrap().name, None);
    let by_position = Preset::from_toml(
        &POTS.replace("key = { column = \"Description\" }", "key = { column = 3 }"),
    )
    .unwrap();
    assert_eq!(
        by_position.sub_account_transfer().unwrap().key,
        Column::Index(2)
    );
}

#[test]
fn sub_account_transfer_unknown_and_missing_keys_are_refused() {
    let typo = toml_error(&POTS.replace("key = {", "kee = {"));
    assert!(typo.contains("kee"), "{typo}");
    let inner = toml_error(&POTS.replace("is = \"Pot transfer\"", "equals = \"Pot transfer\""));
    assert!(inner.contains("equals"), "{inner}");
    let extra = toml_error(&format!("{POTS}balance = {{ column = \"Balance\" }}\n"));
    assert!(extra.contains("balance"), "{extra}");
    let no_key = toml_error(&POTS.replace("key = { column = \"Description\" }\n", ""));
    assert!(no_key.contains("key"), "{no_key}");
    let no_when =
        toml_error(&POTS.replace("when = { column = \"Type\", is = \"Pot transfer\" }\n", ""));
    assert!(no_when.contains("when"), "{no_when}");
    let no_kind = toml_error(&POTS.replace("kind = \"pot\"\n", ""));
    assert!(no_kind.contains("kind"), "{no_kind}");
}

#[test]
fn sub_account_transfer_values_are_checked_where_they_are_written() {
    let line_of = |text: &str, needle: &str| {
        text.lines()
            .position(|line| line.contains(needle))
            .map(|index| index + 1)
            .unwrap()
    };
    for (from, to, expected) in [
        ("kind = \"pot\"", "kind = \"space\"", "unknown variant"),
        ("kind = \"pot\"", "kind = \"Pot\"", "unknown variant"),
        ("is = \"Pot transfer\"", "is = \"\"", "can't be empty"),
        (
            "is = \"Pot transfer\"",
            "is = \" Pot transfer\"",
            "start or end with a space",
        ),
        ("is = \"Pot transfer\"", "is = 3", "a string"),
        (
            "key = { column = \"Description\" }",
            "key = { column = 0 }",
            "count from 1",
        ),
        (
            "key = { column = \"Description\" }",
            "key = { column = \" \" }",
            "can't be empty",
        ),
        (
            "name = { column = \"Name\" }",
            "name = { column = -1 }",
            "count from 1",
        ),
    ] {
        let text = POTS.replace(from, to);
        let message = toml_error(&text);
        assert!(message.contains(expected), "{to}: {message}");
        let line = line_of(&text, to);
        assert!(
            message.contains(&format!("line {line}")),
            "{to}: the error points at line {line}: {message}"
        );
    }
}

#[test]
fn sub_account_columns_count_as_read() {
    // A column only the section reads is read, not "unread", and can't be
    // ignored.
    let text = POTS.replace(
        "key = { column = \"Description\" }",
        "key = { column = \"Pot\" }",
    );
    let preset = Preset::from_toml(&text).unwrap();
    let file = SourceFile::from_bytes(
        "s.csv",
        "Date,Amount,Description,Name,Type,Pot\n01/11/2026,-1.00,a,b,c,\n",
    );
    let parsed = preset.importer().parse(&file).unwrap();
    assert_eq!(preset.unread_columns(&parsed), Vec::<&str>::new());
    assert_eq!(preset.importer().detect(&file), Detection::Source);
    let without_pot = SourceFile::from_bytes(
        "s.csv",
        "Date,Amount,Description,Name,Type\n01/11/2026,-1.00,a,b,c\n",
    );
    assert_eq!(preset.importer().detect(&without_pot), Detection::Format);

    assert!(matches!(
        error(&text.replace("[sub_account_transfer]", "ignore = [\"Pot\"]\n\n[sub_account_transfer]")),
        PresetError::IgnoredColumnRead(name) if name == "Pot"
    ));
}

#[test]
fn sub_account_transfer_rows_carry_their_pot() {
    let preset = Preset::from_toml(POTS).unwrap();
    let file = SourceFile::from_bytes(
        "s.csv",
        "Date,Amount,Description,Name,Type\n\
         01/11/2026,-20.00,pot_1,Rainy Day,Pot transfer\n\
         02/11/2026,5.00, pot_1 , Rainy  Day ,  Pot transfer \n\
         03/11/2026,-1.00,pot_2,,Pot transfer\n\
         04/11/2026,-3.40,BEAN THERE,Bean There,Card payment\n\
         05/11/2026,-1.00,pot_3,Holiday,pot transfer\n\
         06/11/2026,-1.00,  ,Holiday,Pot transfer\n",
    );
    let statement = preset.importer().import(&file).unwrap();
    let pot = |key: &str, name: Option<&str>| {
        Some(SubAccountRef {
            kind: SubAccountKind::Pot,
            key: key.into(),
            name: name.map(Into::into),
        })
    };
    let read: Vec<_> = statement
        .rows()
        .iter()
        .map(|row| row.outcome.as_ref().map(|line| line.sub_account.clone()))
        .collect();
    assert_eq!(
        read,
        [
            Ok(pot("pot_1", Some("Rainy Day"))),
            // Key and name trimmed; the type matches after trimming.
            Ok(pot("pot_1", Some("Rainy  Day"))),
            Ok(pot("pot_2", None)),
            Ok(None),
            // Matched exactly, case included.
            Ok(None),
            Err(&RowProblem::SubAccountKeyMissing {
                kind: SubAccountKind::Pot
            }),
        ]
    );
    // The rest of the row reads as before: the description is the pot ID as
    // written.
    let (_, second) = statement.lines().nth(1).unwrap();
    assert_eq!(second.description_raw, " pot_1 ");
    assert_eq!(
        RowProblem::SubAccountKeyMissing {
            kind: SubAccountKind::Pot
        }
        .to_string(),
        "the row is a pot transfer, but doesn't say which pot"
    );
}

#[test]
fn sub_account_category_is_read_and_checked() {
    assert_eq!(
        Preset::from_toml(POTS).unwrap().sub_account_category(),
        None
    );
    let with = POTS.replace(
        "name = { column = \"Name\" }\n",
        "name = { column = \"Name\" }\ncategory = \"transfer.savings\"\n",
    );
    let preset = Preset::from_toml(&with).unwrap();
    assert_eq!(
        preset.sub_account_category(),
        Some(&key("transfer.savings"))
    );
    let bad = toml_error(&with.replace("\"transfer.savings\"", "\"Transfer Savings\""));
    assert!(bad.contains("category"), "{bad}");
}

/// `[sub_account_transfer] category` goes on every sub-account transfer,
/// ahead of `category_map`, and on nothing else.
#[test]
fn sub_account_category_goes_on_pot_transfers_only() {
    let text = POTS
        .replace(
            "source_type = { column = \"Type\" }\n",
            "source_type = { column = \"Type\" }\nsource_category = { column = \"Category\" }\n",
        )
        .replace(
            "name = { column = \"Name\" }\n",
            "name = { column = \"Name\" }\ncategory = \"transfer.savings\"\n",
        )
        + "\n[category_map]\n\"Transfers\" = \"transfer\"\n\"Eating out\" = \"food.eating_out\"\n";
    let preset = Preset::from_toml(&text).unwrap();
    let file = SourceFile::from_bytes(
        "s.csv",
        "Date,Amount,Description,Name,Type,Category\n\
         01/11/2026,-20.00,pot_1,Rainy Day,Pot transfer,Transfers\n\
         02/11/2026,12.00,,A. Friend,Monzo-to-Monzo,Transfers\n\
         03/11/2026,-3.40,BEAN THERE,Bean There,Card payment,Eating out\n\
         04/11/2026,-1.00,pot_2,Holiday,Pot transfer,\n",
    );
    let statement = preset.importer().import(&file).unwrap();
    let categories: Vec<_> = statement
        .lines()
        .map(|(_, line)| line.category.as_ref().map(CategoryKey::as_str))
        .collect();
    assert_eq!(
        categories,
        [
            Some("transfer.savings"),
            Some("transfer"),
            Some("food.eating_out"),
            Some("transfer.savings"),
        ]
    );
}
