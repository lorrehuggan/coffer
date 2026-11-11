//! Preset TOML → [`CsvMapping`], the checks on it, and loading sets of
//! presets, including the bundled ones and their fixtures.

use std::path::Path;

use coffer_core::money::{Currency, DecimalMark};
use coffer_core::statement::RowProblem;

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

    // Sub-account transfers come with M1.17; until then they're refused,
    // not ignored.
    let later = toml_error(&format!(
        "{MINIMAL}\n[sub_account_transfer]\nkind = \"pot\"\n"
    ));
    assert!(later.contains("sub_account_transfer"), "{later}");
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
