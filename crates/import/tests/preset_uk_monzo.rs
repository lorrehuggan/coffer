//! `presets/uk/monzo.toml` against its fixture, `fixtures/presets/uk/monzo.csv`:
//! a synthetic file in the shape of Monzo's CSV export, with one row per quirk
//! the preset has to get right.

use std::path::Path;

use chrono::{NaiveDate, NaiveTime};
use coffer_core::category::CategoryKey;
use coffer_core::money::{Currency, Money};
use coffer_core::statement::{RowProblem, SourceLocation, Statement, StatementLine};
use coffer_import::preset::{Preset, Presets};
use coffer_import::{AnyImporter, Detection, Importer, SourceFile, detect};

const PRESET: &str = include_str!("../../../presets/uk/monzo.toml");

fn fixture() -> SourceFile {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/presets/uk/monzo.csv");
    SourceFile::read(&path).unwrap()
}

fn fixture_text() -> String {
    String::from_utf8(fixture().bytes().to_vec()).unwrap()
}

fn monzo() -> Preset {
    let presets = Presets::bundled().unwrap();
    presets.get("uk-monzo").expect("bundled").clone()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn date(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
}

fn time(text: &str) -> NaiveTime {
    NaiveTime::parse_from_str(text, "%H:%M:%S").unwrap()
}

fn statement() -> Statement {
    monzo().importer().import(&fixture()).unwrap()
}

/// The fixture's lines by transaction ID number (`tx_…01` is `line(1)`).
fn line(statement: &Statement, n: u32) -> &StatementLine {
    let id = format!("tx_0000FAKE{n:015}");
    statement
        .lines()
        .map(|(_, line)| line)
        .find(|line| line.external_id.as_deref() == Some(id.as_str()))
        .unwrap_or_else(|| panic!("no line {id}"))
}

#[test]
fn the_bundled_preset_is_the_file_in_presets() {
    let bundled = monzo();
    let from_text = Preset::from_toml(PRESET).unwrap();
    assert_eq!(bundled.id(), "uk-monzo");
    assert_eq!(bundled.name(), "Monzo");
    assert_eq!(bundled.region(), "uk");
    assert_eq!(bundled.mapping(), from_text.mapping());
    assert!(bundled.mapping().multiline_fields);
}

#[test]
fn detection_picks_monzo() {
    let presets = Presets::bundled().unwrap();
    let candidates = detect(&presets.importers(), &fixture());
    assert_eq!(candidates[0].importer.name(), "Monzo");
    assert_eq!(candidates[0].detection, Detection::Source);
    assert!(
        candidates[1..]
            .iter()
            .all(|candidate| candidate.detection < Detection::Source),
        "no other preset claims a Monzo export"
    );
}

#[test]
fn every_column_is_read_or_ignored() {
    let preset = monzo();
    let parsed = preset.importer().parse(&fixture()).unwrap();
    assert_eq!(preset.unread_columns(&parsed), Vec::<&str>::new());
}

#[test]
fn every_row_imports_in_file_order() {
    let statement = statement();
    assert_eq!(statement.preset_id(), Some("uk-monzo"));
    assert_eq!(statement.currency(), Currency::GBP);
    assert!(
        !statement.has_flagged(),
        "{:?}",
        statement.flagged().collect::<Vec<_>>()
    );
    assert!(
        statement.balances().is_empty(),
        "Monzo exports have no balances"
    );

    // Physical lines: the multi-line note takes lines 7 and 8.
    let rows: Vec<_> = statement
        .lines()
        .map(|(location, line)| (location, line.external_id.clone().unwrap()))
        .collect();
    let expected: Vec<_> = [2, 3, 4, 5, 6, 7, 9, 10, 11, 12, 13, 14]
        .into_iter()
        .zip(1..)
        .map(|(at, n)| (SourceLocation::Line(at), format!("tx_0000FAKE{n:015}")))
        .collect();
    assert_eq!(rows, expected);
}

#[test]
fn amounts_dates_and_times() {
    let statement = statement();
    let expected = [
        (1, 1, "07:58:12", 215_000),
        (2, 1, "08:30:00", -20_000),
        (3, 2, "08:14:51", -340),
        (4, 2, "08:16:03", -340),
        (5, 3, "12:41:09", -862),
        (6, 4, "19:02:44", -2_400),
        (7, 6, "10:15:00", 340),
        (8, 7, "09:00:01", 0),
        (9, 8, "05:12:30", -6_125),
        (10, 9, "18:00:00", 5_000),
        (11, 10, "16:20:37", -415),
        (12, 11, "21:30:00", 1_200),
    ];
    for (n, day, at, minor) in expected {
        let line = line(&statement, n);
        assert_eq!(
            (line.date, line.time, line.amount),
            (date(day), Some(time(at)), gbp(minor)),
            "tx {n}"
        );
    }
    let total = Money::sum(
        Currency::GBP,
        statement.lines().map(|(_, line)| line.amount),
    );
    assert_eq!(
        total,
        Ok(gbp(215_000 - 20_000 - 340 - 340 - 862 - 2_400 + 340
            - 6_125
            + 5_000
            - 415
            + 1_200))
    );
}

#[test]
fn a_card_payment() {
    let statement = statement();
    let coffee = line(&statement, 3);
    assert_eq!(coffee.merchant.as_deref(), Some("Bean There"));
    assert_eq!(coffee.description_raw, "BEAN THERE TESTVILLE GBR");
    assert_eq!(coffee.source_category.as_deref(), Some("Eating out"));
    assert_eq!(coffee.source_type.as_deref(), Some("Card payment"));
    assert_eq!(
        coffee.original_amount, None,
        "the local amount repeats the amount"
    );
    assert_eq!(coffee.note, None);
    assert_eq!(coffee.balance_after, None);
}

/// Two identical coffees on the same day: different IDs, so dedupe keeps both.
#[test]
fn identical_purchases_keep_their_own_ids() {
    let statement = statement();
    let (first, second) = (line(&statement, 3), line(&statement, 4));
    assert_eq!(
        (first.date, first.amount, first.description_normalized()),
        (second.date, second.amount, second.description_normalized())
    );
    assert_ne!(first.external_id, second.external_id);
}

/// Monzo lists this refund under "Money Out" as a positive value; the signed
/// `Amount` column reads it as money in.
#[test]
fn a_refund_in_money_out_is_money_in() {
    let fixture = fixture_text();
    let refund_row = fixture
        .lines()
        .find(|row| row.starts_with("tx_0000FAKE000000000000007,"))
        .unwrap();
    assert!(
        refund_row.ends_with(",3.40,"),
        "the quirk is in the fixture"
    );

    let statement = statement();
    let refund = line(&statement, 7);
    assert_eq!(refund.amount, gbp(340));
    assert_eq!(refund.note.as_deref(), Some("refund #coffee"));
}

#[test]
fn a_card_payment_abroad_keeps_the_local_amount() {
    let statement = statement();
    let lunch = line(&statement, 5);
    assert_eq!(lunch.amount, gbp(-862));
    assert_eq!(
        lunch.original_amount,
        Some(Money::new(-1_000, Currency::EUR))
    );
    assert_eq!(lunch.merchant.as_deref(), Some("Café Lumière"));
    assert_eq!(lunch.description_raw, "CAFE LUMIERE PARIS FRA");
}

/// Pot transfers are ordinary rows until M1.17 recognises pots: the pot's
/// name as the merchant and its ID as the description.
#[test]
fn pot_transfers_import_as_ordinary_rows() {
    let statement = statement();
    for (n, minor) in [(2, -20_000), (10, 5_000)] {
        let pot = line(&statement, n);
        assert_eq!(pot.amount, gbp(minor));
        assert_eq!(pot.source_type.as_deref(), Some("Pot transfer"));
        assert_eq!(pot.source_category.as_deref(), Some("Savings"));
        assert_eq!(pot.merchant.as_deref(), Some("Rainy Day"));
        assert_eq!(pot.description_raw, "pot_0000FAKEPOT00000000001");
    }
}

/// A zero-amount card check is kept, not dropped.
#[test]
fn a_zero_amount_row_is_kept() {
    let statement = statement();
    let check = line(&statement, 8);
    assert!(check.amount.is_zero());
    assert_eq!(check.original_amount, None);
    assert_eq!(check.description_raw, "EXAMPLE STREAMING CARD CHECK");
}

#[test]
fn a_note_over_two_lines_is_read_whole() {
    let statement = statement();
    let dinner = line(&statement, 6);
    assert_eq!(
        dinner.note.as_deref(),
        Some("Team dinner, \"the usual\"\n#work #shared")
    );
    assert_eq!(dinner.description_raw, "FAKE PIZZA CO TESTVILLE GBR");
}

/// Why the preset sets `multiline_fields`: without it, the row with the
/// two-line note is flagged as a possible stray quote.
#[test]
fn multiline_fields_is_needed_for_notes() {
    let one_line =
        Preset::from_toml(&PRESET.replace("multiline_fields = true", "multiline_fields = false"))
            .unwrap();
    let statement = one_line.importer().import(&fixture()).unwrap();
    assert_eq!(
        statement.flagged().collect::<Vec<_>>(),
        [(
            SourceLocation::Line(7),
            &RowProblem::MultiLineRow { lines: 2 }
        )]
    );
}

#[test]
fn non_ascii_text_is_kept_as_written() {
    let statement = statement();
    let bakery = line(&statement, 11);
    assert_eq!(bakery.merchant.as_deref(), Some("Bäckerei Groß 🥨"));
    assert_eq!(bakery.description_raw, "BACKEREI GROSS TESTVILLE GBR");
}

#[test]
fn a_payment_from_a_person() {
    let statement = statement();
    let payback = line(&statement, 12);
    assert_eq!(payback.amount, gbp(1_200));
    assert_eq!(payback.source_type.as_deref(), Some("Monzo-to-Monzo"));
    assert_eq!(payback.source_category.as_deref(), Some("Transfers"));
    assert_eq!(payback.description_raw, "Pizza money");
}

/// The `Currency` column is read, so a row Monzo writes in another currency
/// is flagged instead of being taken as pounds.
#[test]
fn a_row_in_another_currency_is_flagged() {
    let text = fixture_text().replace("Bills,-61.25,GBP,-61.25,GBP", "Bills,-61.25,EUR,-61.25,EUR");
    let file = SourceFile::from_bytes("monzo.csv", text);
    let statement = monzo().importer().import(&file).unwrap();
    let flagged: Vec<_> = statement.flagged().collect();
    assert_eq!(flagged.len(), 1);
    assert_eq!(flagged[0].0, SourceLocation::Line(11));
    assert!(matches!(flagged[0].1, RowProblem::CurrencyMismatch { .. }));
}

#[test]
fn categories_map_to_coffer_keys() {
    let preset = monzo();
    let statement = statement();
    let key = |key: &str| CategoryKey::new(key).unwrap();
    let mapped = |n| {
        line(&statement, n)
            .source_category
            .as_deref()
            .and_then(|category| preset.category_for(category))
            .cloned()
    };
    assert_eq!(mapped(1), Some(key("income")));
    assert_eq!(mapped(2), Some(key("transfer.savings")), "pot transfers");
    assert_eq!(mapped(3), Some(key("food.eating_out")));
    assert_eq!(mapped(9), Some(key("bills")));
    assert_eq!(mapped(11), Some(key("food.groceries")));
    assert_eq!(mapped(8), None, "General says nothing");
    assert_eq!(mapped(12), None, "Transfers includes payments from people");
}
