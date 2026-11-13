//! Tests for recurring-series detection: [`detect`] on synthetic rows, and
//! [`find`], [`dismiss`] and [`undismiss`] against a real database.

use chrono::{Datelike, NaiveDate, Weekday};
use tempfile::TempDir;

use super::*;
use crate::account::{self, AccountKind, NewAccount};
use crate::db::Database;
use crate::manual::{self, ManualTransaction};
use crate::split::{self, SplitLine};

const ACCOUNT: AccountId = AccountId::new(1);

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

/// Builds rows with increasing transaction IDs.
#[derive(Default)]
struct Rows {
    rows: Vec<LedgerRow>,
}

impl Rows {
    fn add(&mut self, on: NaiveDate, minor: i64, merchant: &str) -> TransactionId {
        let id = TransactionId::new(self.rows.len() as i64 + 1);
        self.rows.push(LedgerRow {
            transaction: id,
            account: ACCOUNT,
            date: on,
            amount: gbp(minor),
            merchant_display: Some(merchant.to_owned()),
            category: None,
            excluded: false,
            transfer: false,
        });
        id
    }

    fn last_mut(&mut self) -> &mut LedgerRow {
        self.rows.last_mut().unwrap()
    }

    /// `count` occurrences of `cadence` from `start`, on the nominal dates.
    fn series(
        &mut self,
        cadence: Cadence,
        start: NaiveDate,
        count: u32,
        minor: i64,
        merchant: &str,
    ) -> Vec<TransactionId> {
        (0..count)
            .map(|k| self.add(cadence.nth(start, k).unwrap(), minor, merchant))
            .collect()
    }
}

/// A small deterministic generator for noise (a linear congruential
/// generator: no dependency, the same sequence everywhere).
struct Noise(u64);

impl Noise {
    fn next(&mut self, below: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % below
    }
}

fn only(found: &[RecurringSeries]) -> &RecurringSeries {
    assert_eq!(found.len(), 1, "expected one series, found {found:#?}");
    &found[0]
}

#[test]
fn a_monthly_subscription_is_detected() {
    let mut rows = Rows::default();
    let ids = rows.series(Cadence::Monthly, date(2025, 1, 10), 6, -999, "Netflix.com");
    let found = detect(&rows.rows);
    let series = only(&found);
    assert_eq!(series.cadence, Cadence::Monthly);
    assert_eq!(series.direction, Direction::Out);
    assert_eq!(series.amount_kind, AmountKind::Fixed);
    assert_eq!(series.typical, gbp(-999));
    assert_eq!(series.amount_spread_minor, 0);
    assert_eq!(series.anchor, date(2025, 1, 10));
    assert_eq!(series.first_date, date(2025, 1, 10));
    assert_eq!(series.last_date, date(2025, 6, 10));
    assert_eq!(series.next_expected, date(2025, 7, 10));
    assert_eq!(series.occurrences, 6);
    assert_eq!(series.transactions, ids);
    assert_eq!(series.merchant_display, "Netflix.com");
    assert!(!series.lapsed);
    assert_eq!(
        series.key,
        SeriesKey::derive(
            ACCOUNT,
            "netflix.com",
            Direction::Out,
            Currency::GBP,
            Cadence::Monthly,
            0
        )
    );
}

#[test]
fn month_end_days_fall_on_the_last_day() {
    let mut rows = Rows::default();
    for (m, d) in [
        (1, 31),
        (2, 28),
        (3, 31),
        (4, 30),
        (5, 31),
        (6, 30),
        (7, 31),
    ] {
        rows.add(date(2025, m, d), -2500, "Gym");
    }
    let found = detect(&rows.rows);
    let series = only(&found);
    assert_eq!(series.cadence, Cadence::Monthly);
    assert_eq!(series.anchor, date(2025, 1, 31));
    assert_eq!(series.occurrences, 7);
    assert_eq!(series.next_expected, date(2025, 8, 31));
}

#[test]
fn a_month_end_series_starting_in_a_short_month_still_fits() {
    let mut rows = Rows::default();
    for (m, d) in [(4, 30), (5, 31), (6, 30), (7, 31), (8, 31), (9, 30)] {
        rows.add(date(2025, m, d), -1500, "Phone");
    }
    let found = detect(&rows.rows);
    let series = only(&found);
    assert_eq!(series.occurrences, 6);
    let tolerance = i64::from(Cadence::Monthly.default_tolerance_days());
    for (k, id) in series.transactions.iter().enumerate() {
        let row = rows.rows.iter().find(|row| row.transaction == *id).unwrap();
        let nominal = Cadence::Monthly.nth(series.anchor, k as u32).unwrap();
        assert!((row.date - nominal).num_days().abs() <= tolerance);
    }
}

/// The 15th of each month of 2025, moved off weekends by `shift`.
fn weekend_shifted(shift: impl Fn(Weekday) -> i64) -> Rows {
    let mut rows = Rows::default();
    for m in 1..=12 {
        let nominal = date(2025, m, 15);
        let on = nominal + TimeDelta::days(shift(nominal.weekday()));
        rows.add(on, -4000, "Council Tax");
    }
    rows
}

#[test]
fn payments_moved_off_weekends_stay_one_series() {
    // Saturday to Friday, Sunday to Monday.
    let rows = weekend_shifted(|day| match day {
        Weekday::Sat => -1,
        Weekday::Sun => 1,
        _ => 0,
    });
    let found = detect(&rows.rows);
    let series = only(&found);
    assert_eq!(series.cadence, Cadence::Monthly);
    assert_eq!(series.occurrences, 12);
    assert_eq!(series.anchor, date(2025, 1, 15));
    assert_eq!(series.next_expected, date(2026, 1, 15));

    // Both to the next working day.
    let rows = weekend_shifted(|day| match day {
        Weekday::Sat => 2,
        Weekday::Sun => 1,
        _ => 0,
    });
    let found = detect(&rows.rows);
    let series = only(&found);
    assert_eq!(series.occurrences, 12);
    assert_eq!(series.anchor, date(2025, 1, 15));
}

#[test]
fn a_price_rise_stays_one_fixed_series() {
    let mut rows = Rows::default();
    let mut ids = rows.series(Cadence::Monthly, date(2024, 9, 3), 6, -999, "Spotify");
    ids.extend(rows.series(Cadence::Monthly, date(2025, 3, 3), 4, -1099, "Spotify"));
    let found = detect(&rows.rows);
    let series = only(&found);
    assert_eq!(series.amount_kind, AmountKind::Fixed);
    assert_eq!(series.typical, gbp(-1099));
    assert_eq!(series.amount_spread_minor, 0);
    assert_eq!(series.occurrences, 10);
    assert_eq!(series.transactions, ids);
}

#[test]
fn a_stopped_subscription_is_lapsed_and_still_detected() {
    let mut rows = Rows::default();
    rows.series(Cadence::Monthly, date(2025, 1, 5), 6, -799, "Disney Plus");
    // The account's imports go on to December.
    rows.series(Cadence::Monthly, date(2025, 1, 20), 12, -60000, "Landlord");
    let found = detect(&rows.rows);
    assert_eq!(found.len(), 2);
    let stopped = found
        .iter()
        .find(|series| series.merchant_display == "Disney Plus")
        .unwrap();
    assert!(stopped.lapsed);
    assert_eq!(stopped.next_expected, date(2025, 7, 5));
    let rent = found
        .iter()
        .find(|series| series.merchant_display == "Landlord")
        .unwrap();
    assert!(!rent.lapsed);
}

#[test]
fn lapsed_waits_for_the_tolerance() {
    let mut rows = Rows::default();
    rows.series(Cadence::Monthly, date(2025, 1, 5), 4, -799, "Disney Plus");
    // The account's latest row is 8 May: within the tolerance of 5 May.
    rows.add(date(2025, 5, 8), -300, "Cafe");
    assert!(!only(&detect(&rows.rows)).lapsed);
    rows.add(date(2025, 5, 9), -300, "Bakery");
    assert!(only(&detect(&rows.rows)).lapsed);
}

#[test]
fn lapsed_counts_every_row_of_the_account() {
    let mut rows = Rows::default();
    rows.series(Cadence::Monthly, date(2025, 1, 5), 4, -799, "Disney Plus");
    // Excluded and unnamed rows still show the account's imports go on.
    rows.add(date(2025, 9, 1), -300, "Cafe");
    rows.last_mut().excluded = true;
    assert!(only(&detect(&rows.rows)).lapsed);
}

#[test]
fn two_series_from_one_merchant_at_different_amounts() {
    let mut rows = Rows::default();
    // Same day, and a third on another day.
    let music = rows.series(Cadence::Monthly, date(2025, 1, 3), 6, -799, "Apple");
    let storage = rows.series(Cadence::Monthly, date(2025, 1, 3), 6, -99, "Apple");
    let tv = rows.series(Cadence::Monthly, date(2025, 1, 19), 6, -899, "Apple");
    let found = detect(&rows.rows);
    assert_eq!(found.len(), 3, "{found:#?}");
    let key = |ordinal| {
        SeriesKey::derive(
            ACCOUNT,
            "Apple",
            Direction::Out,
            Currency::GBP,
            Cadence::Monthly,
            ordinal,
        )
    };
    // Smallest typical amount first.
    assert_eq!(found[0].transactions, storage);
    assert_eq!(found[0].key, key(0));
    assert_eq!(found[1].transactions, music);
    assert_eq!(found[1].key, key(1));
    assert_eq!(found[2].transactions, tv);
    assert_eq!(found[2].key, key(2));
    assert!(
        found
            .iter()
            .all(|series| series.amount_kind == AmountKind::Fixed)
    );
}

#[test]
fn a_one_off_at_another_price_is_a_gap_not_a_price_change() {
    let mut rows = Rows::default();
    let mut ids = rows.series(Cadence::Monthly, date(2025, 1, 3), 4, -99, "Apple");
    // In May only a 1.49 purchase, then 0.99 again.
    rows.add(date(2025, 5, 3), -149, "Apple");
    ids.extend(rows.series(Cadence::Monthly, date(2025, 6, 3), 4, -99, "Apple"));
    let found = detect(&rows.rows);
    let series = only(&found);
    assert_eq!(series.transactions, ids);
    assert_eq!(series.typical, gbp(-99));
    assert_eq!(series.amount_spread_minor, 0);
}

#[test]
fn variable_amounts_use_the_median() {
    let mut rows = Rows::default();
    let amounts = [-5210, -4890, -6130, -3920, -4475, -5500, -6020, -4100];
    for (k, minor) in amounts.into_iter().enumerate() {
        let on = Cadence::Monthly.nth(date(2025, 1, 12), k as u32).unwrap();
        rows.add(on, minor, "Octopus Energy");
    }
    let found = detect(&rows.rows);
    let series = only(&found);
    assert_eq!(series.amount_kind, AmountKind::Variable);
    // The latest 6: 6130, 3920, 4475, 5500, 6020, 4100; the middle two
    // are 4475 and 5500: (4475 + 5500) / 2 = 4987.5, rounded up.
    assert_eq!(series.typical, gbp(-4988));
    assert_eq!(series.amount_spread_minor, 6130 - 3920);
    assert_eq!(series.occurrences, 8);
}

#[test]
fn median_rule() {
    assert_eq!(median(&[5]), 5);
    assert_eq!(median(&[3, 1, 2]), 2);
    assert_eq!(median(&[1, 2]), 2);
    assert_eq!(median(&[10, 20, 30, 41]), 25);
    assert_eq!(median(&[10, 20, 31, 40]), 26);
}

#[test]
fn fixed_or_variable() {
    let fixed = |amounts: &[i128]| classify(amounts).map(|fit| fit.kind);
    assert_eq!(fixed(&[999, 999, 999]), Some(AmountKind::Fixed));
    // A rise just taken, an introductory price, both.
    assert_eq!(fixed(&[999, 999, 1099]), Some(AmountKind::Fixed));
    assert_eq!(fixed(&[499, 999, 999]), Some(AmountKind::Fixed));
    // Converted from another currency: within 2%.
    assert_eq!(fixed(&[631, 629, 635, 630]), Some(AmountKind::Fixed));
    // A one-off surcharge in the middle: variable.
    assert_eq!(
        fixed(&[5000, 5000, 6300, 5000, 5000]),
        Some(AmountKind::Variable)
    );
    assert_eq!(fixed(&[5210, 4890, 6130, 3920]), Some(AmountKind::Variable));
    // Too far apart to be one series.
    assert_eq!(fixed(&[1000, 1900, 400, 1200]), None);
}

#[test]
fn irregular_cafe_purchases_are_not_a_series() {
    for seed in 1..=20 {
        let mut noise = Noise(seed);
        let mut rows = Rows::default();
        let mut on = date(2024, 1, 1);
        while on < date(2025, 12, 31) {
            on += TimeDelta::days(1 + noise.next(4) as i64);
            rows.add(on, -(250 + 10 * noise.next(20) as i64), "Corner Cafe");
        }
        let found = detect(&rows.rows);
        assert!(found.is_empty(), "seed {seed}: {found:#?}");
    }
}

#[test]
fn the_same_coffee_on_irregular_days_is_not_a_series() {
    for seed in 1..=20 {
        let mut noise = Noise(seed);
        let mut rows = Rows::default();
        let mut on = date(2024, 1, 1);
        while on < date(2025, 12, 31) {
            on += TimeDelta::days(1 + noise.next(6) as i64);
            let price = [-280, -280, -320, -350][noise.next(4) as usize];
            rows.add(on, price, "Corner Cafe");
        }
        let found = detect(&rows.rows);
        assert!(found.is_empty(), "seed {seed}: {found:#?}");
    }
}

#[test]
fn random_shopping_is_not_a_series() {
    for seed in 1..=20 {
        let mut noise = Noise(seed);
        let mut rows = Rows::default();
        let mut on = date(2024, 1, 1);
        while on < date(2025, 12, 31) {
            on += TimeDelta::days(1 + noise.next(14) as i64);
            rows.add(on, -(500 + noise.next(15000) as i64), "Big Shop");
        }
        let found = detect(&rows.rows);
        assert!(found.is_empty(), "seed {seed}: {found:#?}");
    }
}

#[test]
fn two_payments_are_not_a_series() {
    let mut rows = Rows::default();
    rows.series(Cadence::Monthly, date(2025, 3, 1), 2, -4500, "Plumber");
    assert!(detect(&rows.rows).is_empty());
}

#[test]
fn a_regular_variable_amount_needs_one_more_occurrence() {
    let mut rows = Rows::default();
    for (k, minor) in [-5210, -4890, -6130].into_iter().enumerate() {
        rows.add(
            Cadence::Monthly.nth(date(2025, 1, 12), k as u32).unwrap(),
            minor,
            "Water",
        );
    }
    assert!(detect(&rows.rows).is_empty());
    rows.add(date(2025, 4, 12), -3920, "Water");
    assert_eq!(only(&detect(&rows.rows)).occurrences, 4);
}

#[test]
fn yearly_needs_two_occurrences() {
    let mut rows = Rows::default();
    rows.add(date(2024, 3, 14), -9500, "Amazon Prime");
    assert!(detect(&rows.rows).is_empty());
    // A year later, moved by a weekend.
    rows.add(date(2025, 3, 17), -9500, "Amazon Prime");
    let found = detect(&rows.rows);
    let series = only(&found);
    assert_eq!(series.cadence, Cadence::Yearly);
    assert_eq!(series.occurrences, 2);
    assert_eq!(series.amount_kind, AmountKind::Fixed);
    let tolerance = i64::from(Cadence::Yearly.default_tolerance_days());
    assert!((series.next_expected - date(2026, 3, 15)).num_days().abs() <= tolerance);
}

#[test]
fn each_cadence_is_detected_as_itself() {
    for cadence in Cadence::ALL {
        let count = if cadence == Cadence::Yearly { 3 } else { 8 };
        let mut rows = Rows::default();
        let ids = rows.series(cadence, date(2023, 2, 6), count, -1200, "Payee");
        let found = detect(&rows.rows);
        let series = only(&found);
        assert_eq!(series.cadence, cadence);
        assert_eq!(series.transactions, ids, "{cadence}");
        assert_eq!(series.occurrences, count as usize);
    }
}

#[test]
fn income_is_its_own_direction() {
    let mut rows = Rows::default();
    let salary = rows.series(
        Cadence::FourWeekly,
        date(2025, 1, 2),
        6,
        250000,
        "Employer Ltd",
    );
    let found = detect(&rows.rows);
    let series = only(&found);
    assert_eq!(series.direction, Direction::In);
    assert_eq!(series.typical, gbp(250000));
    assert_eq!(series.transactions, salary);
}

#[test]
fn a_single_missed_payment_is_a_gap() {
    let mut rows = Rows::default();
    let ids: Vec<_> = (0..10)
        .filter(|&k| k != 4)
        .map(|k| {
            rows.add(
                Cadence::Monthly.nth(date(2025, 1, 8), k).unwrap(),
                -1000,
                "Gym",
            )
        })
        .collect();
    let found = detect(&rows.rows);
    let series = only(&found);
    assert_eq!(series.transactions, ids);
    assert_eq!(series.next_expected, date(2025, 11, 8));
}

#[test]
fn two_missed_payments_end_a_run_and_the_new_one_is_reported() {
    let mut rows = Rows::default();
    rows.series(Cadence::Monthly, date(2024, 1, 8), 6, -1000, "Gym");
    let resumed = rows.series(Cadence::Monthly, date(2024, 9, 20), 6, -1000, "Gym");
    let found = detect(&rows.rows);
    let series = only(&found);
    assert_eq!(series.transactions, resumed);
    assert_eq!(series.anchor, date(2024, 9, 20));
}

#[test]
fn excluded_transfer_zero_and_unnamed_rows_are_left_out() {
    for change in 0..4 {
        let mut rows = Rows::default();
        rows.series(Cadence::Monthly, date(2025, 1, 8), 3, -1000, "Gym");
        let row = &mut rows.rows[1];
        match change {
            0 => row.excluded = true,
            1 => row.transfer = true,
            2 => row.amount = gbp(0),
            _ => row.merchant_display = None,
        }
        assert!(detect(&rows.rows).is_empty(), "change {change}");
    }
}

#[test]
fn transfers_never_form_a_series() {
    let mut rows = Rows::default();
    rows.series(Cadence::Monthly, date(2025, 1, 1), 6, -50000, "Savings");
    for row in &mut rows.rows {
        row.transfer = true;
    }
    assert!(detect(&rows.rows).is_empty());
}

#[test]
fn merchants_are_compared_normalised() {
    let mut rows = Rows::default();
    rows.add(date(2025, 1, 10), -999, "NETFLIX.COM");
    rows.add(date(2025, 2, 10), -999, "Netflix.com ");
    rows.add(date(2025, 3, 10), -999, "netflix.com");
    let found = detect(&rows.rows);
    let series = only(&found);
    assert_eq!(series.occurrences, 3);
    // The latest's display text, as given.
    assert_eq!(series.merchant_display, "netflix.com");
}

#[test]
fn accounts_and_currencies_are_separate() {
    let mut rows = Rows::default();
    rows.series(Cadence::Monthly, date(2025, 1, 10), 2, -999, "Netflix");
    rows.add(date(2025, 3, 10), -999, "Netflix");
    rows.last_mut().account = AccountId::new(2);
    assert!(detect(&rows.rows).is_empty());
    rows.last_mut().account = ACCOUNT;
    rows.last_mut().amount = Money::new(-999, Currency::EUR);
    assert!(detect(&rows.rows).is_empty());
}

#[test]
fn category_is_the_most_common_latest_on_a_tie() {
    let key = |text| Some(CategoryKey::new(text).unwrap());
    let mut rows = Rows::default();
    rows.series(Cadence::Monthly, date(2025, 1, 10), 5, -999, "Netflix");
    rows.rows[0].category = key("bills.subscriptions");
    rows.rows[1].category = key("entertainment");
    rows.rows[2].category = key("bills.subscriptions");
    rows.rows[3].category = key("entertainment");
    assert_eq!(only(&detect(&rows.rows)).category, key("entertainment"));
    rows.rows[4].category = key("bills.subscriptions");
    assert_eq!(
        only(&detect(&rows.rows)).category,
        key("bills.subscriptions")
    );
    for row in &mut rows.rows {
        row.category = None;
    }
    assert_eq!(only(&detect(&rows.rows)).category, None);
}

/// A mixed ledger: several series and noise.
fn mixed() -> Rows {
    let mut rows = Rows::default();
    rows.series(Cadence::Monthly, date(2024, 1, 31), 18, -999, "Netflix");
    rows.series(Cadence::Monthly, date(2024, 1, 3), 18, -99, "Apple");
    rows.series(Cadence::Monthly, date(2024, 1, 3), 18, -799, "Apple");
    rows.series(
        Cadence::Weekly,
        date(2024, 1, 6),
        70,
        -1500,
        "Window Cleaner",
    );
    rows.series(
        Cadence::FourWeekly,
        date(2024, 1, 25),
        19,
        210000,
        "Employer Ltd",
    );
    rows.series(Cadence::Yearly, date(2023, 6, 1), 3, -12000, "Insurer");
    let mut noise = Noise(7);
    let mut on = date(2024, 1, 1);
    while on < date(2025, 6, 30) {
        on += TimeDelta::days(1 + noise.next(3) as i64);
        rows.add(on, -(200 + noise.next(300) as i64), "Corner Cafe");
    }
    rows
}

#[test]
fn input_order_does_not_matter() {
    let rows = mixed().rows;
    let found = detect(&rows);
    assert_eq!(found.len(), 6, "{found:#?}");
    let mut reversed = rows.clone();
    reversed.reverse();
    assert_eq!(detect(&reversed), found);
    let mut shuffled = rows.clone();
    let mut noise = Noise(3);
    for index in (1..shuffled.len()).rev() {
        let other = noise.next(index as u64 + 1) as usize;
        shuffled.swap(index, other);
    }
    assert_eq!(detect(&shuffled), found);
}

#[test]
fn keys_are_stable_when_a_new_occurrence_arrives() {
    let mut rows = mixed();
    let before: Vec<SeriesKey> = detect(&rows.rows).into_iter().map(|s| s.key).collect();
    rows.add(date(2025, 7, 31), -999, "Netflix");
    rows.add(date(2025, 7, 3), -99, "Apple");
    rows.add(date(2025, 7, 3), -799, "Apple");
    let after = detect(&rows.rows);
    let keys: Vec<SeriesKey> = after.iter().map(|s| s.key.clone()).collect();
    assert_eq!(keys, before);
    let netflix = after
        .iter()
        .find(|series| series.merchant_display == "Netflix")
        .unwrap();
    assert_eq!(netflix.occurrences, 19);
}

#[test]
fn keys_are_distinct() {
    let found = detect(&mixed().rows);
    let keys: BTreeSet<&SeriesKey> = found.iter().map(|series| &series.key).collect();
    assert_eq!(keys.len(), found.len());
}

// The database ----------------------------------------------------------------

struct Fixture {
    db: Database,
    account: AccountId,
    _dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
        let account = account::create(
            &mut db,
            &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        Fixture {
            db,
            account,
            _dir: dir,
        }
    }

    fn manual(&mut self, on: NaiveDate, minor: i64, merchant: &str) -> TransactionId {
        let mut entry = ManualTransaction::new(self.account, on, gbp(minor), "Card payment");
        entry.merchant = Some(merchant.to_owned());
        manual::create(&mut self.db, &entry).unwrap()
    }

    fn monthly(
        &mut self,
        start: NaiveDate,
        count: u32,
        minor: i64,
        merchant: &str,
    ) -> Vec<TransactionId> {
        (0..count)
            .map(|k| self.manual(Cadence::Monthly.nth(start, k).unwrap(), minor, merchant))
            .collect()
    }
}

#[test]
fn find_reads_the_ledger() {
    let mut fixture = Fixture::new();
    let ids = fixture.monthly(date(2025, 1, 10), 4, -999, "Netflix");
    fixture.manual(date(2025, 2, 2), -450, "Bakery");
    let found = find(&fixture.db).unwrap();
    let series = only(&found);
    assert_eq!(series.account, fixture.account);
    assert_eq!(series.transactions, ids);
    assert_eq!(series.typical, gbp(-999));
    assert_eq!(series.cadence, Cadence::Monthly);

    let rows = load_rows(fixture.db.conn()).unwrap();
    assert_eq!(rows.len(), 5);
    assert!(rows.iter().all(|row| row.merchant_display.is_some()));
    assert!(rows.iter().all(|row| !row.excluded && !row.transfer));

    let reader = fixture.db.reader().unwrap();
    assert_eq!(find_with_reader(&reader).unwrap(), found);
}

#[test]
fn a_split_transaction_counts_once_with_its_whole_amount() {
    let mut fixture = Fixture::new();
    let ids = fixture.monthly(date(2025, 1, 10), 3, -3000, "Phone Shop");
    let groceries = CategoryKey::new("food.groceries").unwrap();
    let key = |text| CategoryKey::new(text).unwrap();
    split::set(
        &mut fixture.db,
        ids[1],
        &[
            SplitLine::new(gbp(-1000), Some(key("bills.phone"))),
            SplitLine::new(gbp(-2000), Some(groceries.clone())),
        ],
    )
    .unwrap();
    split::set(
        &mut fixture.db,
        ids[2],
        &[
            SplitLine::new(gbp(-1000), Some(groceries.clone())),
            SplitLine::new(gbp(-2000), Some(groceries.clone())),
        ],
    )
    .unwrap();

    let rows = load_rows(fixture.db.conn()).unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row.amount == gbp(-3000)));
    // Lines in two categories: none; in one: that one.
    assert_eq!(rows[1].category, None);
    assert_eq!(rows[2].category, Some(groceries));

    let found = find(&fixture.db).unwrap();
    let series = only(&found);
    assert_eq!(series.transactions, ids);
    assert_eq!(series.typical, gbp(-3000));
    assert_eq!(series.amount_kind, AmountKind::Fixed);
}

#[test]
fn excluded_rows_are_read_as_excluded() {
    let mut fixture = Fixture::new();
    let ids = fixture.monthly(date(2025, 1, 10), 3, -999, "Netflix");
    fixture
        .db
        .conn()
        .execute(
            "UPDATE \"transaction\" SET excluded_reason = 'disputed', excluded_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
            [ids[1]],
        )
        .unwrap();
    let rows = load_rows(fixture.db.conn()).unwrap();
    assert!(rows[1].excluded);
    assert!(find(&fixture.db).unwrap().is_empty());
}

#[test]
fn dismiss_hides_a_series_and_undismiss_brings_it_back() {
    let mut fixture = Fixture::new();
    fixture.monthly(date(2025, 1, 10), 4, -999, "Netflix");
    fixture.monthly(date(2025, 1, 20), 4, -1599, "Gym");
    let found = find(&fixture.db).unwrap();
    assert_eq!(found.len(), 2);
    let netflix = found
        .iter()
        .find(|series| series.merchant_display == "Netflix")
        .unwrap()
        .key
        .clone();

    assert!(dismiss(&mut fixture.db, &netflix).unwrap());
    assert!(!dismiss(&mut fixture.db, &netflix).unwrap());
    let left = find(&fixture.db).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].merchant_display, "Gym");

    assert!(undismiss(&mut fixture.db, &netflix).unwrap());
    assert!(!undismiss(&mut fixture.db, &netflix).unwrap());
    assert_eq!(find(&fixture.db).unwrap(), found);

    // A key no series has can be dismissed too: keys are opaque.
    let unknown: SeriesKey = "0123456789ab".parse().unwrap();
    assert!(dismiss(&mut fixture.db, &unknown).unwrap());
    assert_eq!(find(&fixture.db).unwrap(), found);
}

#[test]
fn a_scheduled_series_is_hidden() {
    let mut fixture = Fixture::new();
    let ids = fixture.monthly(date(2025, 1, 10), 4, -999, "Netflix");
    fixture.monthly(date(2025, 1, 20), 4, -1599, "Gym");
    let conn = fixture.db.conn();
    conn.execute(
        "INSERT INTO schedule (account_id, merchant_display, cadence, start_date, \
         amount_minor, currency, tolerance_days) \
         VALUES (?1, 'Netflix', 'monthly', '2025-01-10', -999, 'GBP', 3)",
        [fixture.account],
    )
    .unwrap();
    let schedule = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO schedule_match (schedule_id, occurrence_date, transaction_id, matched_by) \
         VALUES (?1, '2025-02-10', ?2, 'auto')",
        rusqlite::params![schedule, ids[1]],
    )
    .unwrap();
    let left = find(&fixture.db).unwrap();
    let series = only(&left);
    assert_eq!(series.merchant_display, "Gym");
}

/// Measures how often noise yields a spurious series, for tuning
/// `CHANCE_DIVISOR`: `cargo test --release -p coffer-core noise_rate --
/// --ignored --nocapture`. About 1 in 300 at the time of writing.
#[test]
#[ignore = "a measurement, not a check"]
fn noise_rate() {
    let mut spurious = 0;
    for seed in 1..=300u64 {
        let mut noise = Noise(seed * 7919);
        let mut rows = Rows::default();
        let mut on = date(2024, 1, 1);
        let kind = seed % 3;
        while on < date(2025, 12, 31) {
            match kind {
                0 => {
                    on += TimeDelta::days(1 + noise.next(14) as i64);
                    rows.add(on, -(500 + noise.next(15000) as i64), "Big Shop");
                }
                1 => {
                    on += TimeDelta::days(1 + noise.next(4) as i64);
                    rows.add(on, -(250 + 10 * noise.next(20) as i64), "Cafe");
                }
                _ => {
                    on += TimeDelta::days(1 + noise.next(6) as i64);
                    let price = [-280, -280, -320, -350][noise.next(4) as usize];
                    rows.add(on, price, "Cafe");
                }
            }
        }
        let found = detect(&rows.rows);
        if !found.is_empty() {
            spurious += 1;
            let shapes: Vec<_> = found
                .iter()
                .map(|series| (series.cadence, series.amount_kind, series.occurrences))
                .collect();
            eprintln!("seed {seed}, kind {kind}: {shapes:?}");
        }
    }
    eprintln!("spurious: {spurious}/300");
}
