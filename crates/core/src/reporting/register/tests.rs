use chrono::NaiveDate;

use crate::exclusion::ExclusionReason;
use crate::money::{Currency, Money};
use crate::reporting::DateRange;
use crate::reporting::test_ledger::{TestLedger, date};

use super::*;

const TODAY: &str = "2026-11-14";

fn load(ledger: &TestLedger) -> Register {
    Register::load(&ledger.reader(), date(TODAY)).unwrap()
}

fn category_id(register: &Register, key: &str) -> i64 {
    register
        .categories()
        .iter()
        .find(|category| category.key.as_str() == key)
        .unwrap()
        .id
}

/// The listed transactions' IDs, in order.
fn ids(register: &Register, listing: &Listing) -> Vec<TransactionId> {
    listing
        .rows
        .iter()
        .map(|&index| register.rows()[index].id)
        .collect()
}

#[test]
fn reads_one_row_per_transaction_newest_first() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current account", Currency::GBP);
    let pot = ledger.pot(current, "Rainy day");
    let older = ledger.row_at(
        current,
        "2026-11-08",
        -1862,
        Some("food.groceries"),
        Some("Sainsbury's"),
        "SAINSBURYS S/MKT",
    );
    let split = ledger.row(current, "2026-11-09", -5216, None);
    ledger.split(
        split,
        &[(-4000, Some("food.groceries")), (-1216, Some("shopping"))],
    );
    let out = ledger.row(current, "2026-11-09", -25_000, None);
    let into = ledger.row(pot, "2026-11-09", 25_000, None);
    ledger.transfer(out, into);
    let bare = ledger.row_at(current, "2026-11-09", -420, None, None, "SQ *BLUE BOTTLE");

    let register = load(&ledger);
    let order: Vec<_> = register.rows().iter().map(|row| row.id).collect();
    assert_eq!(order, [bare, into, out, split, older]);

    let groceries = category_id(&register, "food.groceries");
    let row = register.row(older).unwrap();
    assert_eq!(row.merchant, "Sainsbury's");
    assert_eq!(row.description, "SAINSBURYS S/MKT");
    assert_eq!(row.category, RowCategory::Category(groceries));
    assert_eq!(row.amount, Money::new(-1862, Currency::GBP));

    // With no merchant, the cleaned description names the row.
    let bare_row = register.row(bare).unwrap();
    assert_eq!(bare_row.merchant, "BLUE BOTTLE");
    assert_eq!(bare_row.description, "SQ *BLUE BOTTLE");

    let RowCategory::Split(parts) = &register.row(split).unwrap().category else {
        panic!("a split row");
    };
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].amount.minor(), -4000);
    assert_eq!(parts[0].category, Some(groceries));
    let labels: Vec<_> = register
        .row_categories(register.row(split).unwrap())
        .iter()
        .map(|category| category.label.clone())
        .collect();
    assert_eq!(labels, ["Groceries", "Shopping"]);

    // A confirmed transfer names the other side's account.
    assert_eq!(register.row(out).unwrap().transfer, Some(vec![pot]));
    assert_eq!(register.row(into).unwrap().transfer, Some(vec![current]));
    assert!(!register.row(out).unwrap().is_uncategorised());
    assert_eq!(register.account(pot).unwrap().kind, AccountKind::Pot);
}

#[test]
fn days_net_what_counts_and_hide_excluded_rows() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current account", Currency::GBP);
    let savings = ledger.account("Savings", Currency::GBP);
    let coffee = ledger.row(current, "2026-11-09", -420, Some("food.eating_out"));
    let lunch = ledger.row(current, "2026-11-09", -645, Some("food.eating_out"));
    let out = ledger.row(current, "2026-11-09", -10_000, None);
    let into = ledger.row(savings, "2026-11-09", 10_000, None);
    ledger.transfer(out, into);
    let twice = ledger.row(current, "2026-11-09", -645, Some("food.eating_out"));
    ledger.exclude(twice, ExclusionReason::Duplicate);
    let earlier = ledger.row(current, "2026-11-07", 3400, Some("shopping"));

    let register = load(&ledger);
    let listing = register.select(&Filter::default(), Sort::Default);
    assert_eq!(
        ids(&register, &listing),
        [into, out, lunch, coffee, earlier]
    );
    assert_eq!(listing.days.len(), 2);
    let day = &listing.days[0];
    assert_eq!(day.date, date("2026-11-09"));
    assert_eq!(day.rows, 0..4);
    assert_eq!(day.net, [Money::new(-1065, Currency::GBP)]);
    assert_eq!(listing.days[1].net, [Money::new(3400, Currency::GBP)]);

    // The Excluded filter shows the row, still left out of the net.
    let filter = Filter {
        include_excluded: true,
        ..Filter::default()
    };
    let listing = register.select(&filter, Sort::Default);
    assert_eq!(listing.len(), 6);
    assert_eq!(listing.days[0].net, [Money::new(-1065, Currency::GBP)]);
}

#[test]
fn a_day_in_two_currencies_nets_each_apart() {
    let mut ledger = TestLedger::new();
    let pounds = ledger.account("Current account", Currency::GBP);
    let euros = ledger.account("Euro account", Currency::EUR);
    ledger.row(pounds, "2026-11-09", -500, None);
    ledger.row(euros, "2026-11-09", -1200, None);
    let register = load(&ledger);
    let listing = register.select(&Filter::default(), Sort::Default);
    assert_eq!(
        listing.days[0].net,
        [
            Money::new(-1200, Currency::EUR),
            Money::new(-500, Currency::GBP)
        ]
    );
}

#[test]
fn filters_by_account_category_dates_and_text() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current account", Currency::GBP);
    let joint = ledger.account("Joint account", Currency::GBP);
    let groceries = ledger.row_at(
        current,
        "2026-11-09",
        -1862,
        Some("food.groceries"),
        Some("Sainsbury's"),
        "SAINSBURYS S/MKT CLAPHAM",
    );
    let eating_out = ledger.row_at(
        joint,
        "2026-11-08",
        -2745,
        Some("food.eating_out"),
        Some("Deliveroo"),
        "DELIVEROO.CO.UK",
    );
    let split = ledger.row(joint, "2026-10-30", -5216, None);
    ledger.split(split, &[(-4000, Some("shopping")), (-1216, Some("food"))]);
    let bills = ledger.row(current, "2026-10-01", -8640, Some("bills.energy"));

    let register = load(&ledger);
    let select = |filter: Filter| ids(&register, &register.select(&filter, Sort::Default));

    assert_eq!(
        select(Filter {
            account: Some(joint),
            ..Filter::default()
        }),
        [eating_out, split]
    );
    // A group includes its subcategories and the split's line in it.
    let food = category_id(&register, "food");
    assert_eq!(
        select(Filter {
            category: Some(food),
            ..Filter::default()
        }),
        [groceries, eating_out, split]
    );
    let shopping = category_id(&register, "shopping");
    assert_eq!(
        select(Filter {
            category: Some(shopping),
            ..Filter::default()
        }),
        [split]
    );
    assert_eq!(
        select(Filter {
            dates: Some(DateRange::new(date("2026-11-01"), date("2026-11-30")).unwrap()),
            ..Filter::default()
        }),
        [groceries, eating_out]
    );
    // Text matches the merchant and the raw description, ignoring case.
    assert_eq!(
        select(Filter {
            text: "  clapham ".into(),
            ..Filter::default()
        }),
        [groceries]
    );
    assert_eq!(
        select(Filter {
            text: "DELIVEROO".into(),
            account: Some(current),
            ..Filter::default()
        }),
        Vec::<TransactionId>::new()
    );
    assert_eq!(
        select(Filter::default()),
        [groceries, eating_out, split, bills]
    );
}

#[test]
fn uncategorised_rows_are_counted_and_filtered() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current account", Currency::GBP);
    let savings = ledger.account("Savings", Currency::GBP);
    let bare = ledger.row(current, "2026-11-09", -420, None);
    ledger.row(current, "2026-11-09", -645, Some("food.eating_out"));
    let half = ledger.row(current, "2026-11-08", -1000, None);
    ledger.split(half, &[(-600, Some("shopping")), (-400, None)]);
    let out = ledger.row(current, "2026-11-08", -5000, None);
    let into = ledger.row(savings, "2026-11-08", 5000, None);
    ledger.transfer(out, into);
    let other_account = ledger.row(savings, "2026-11-07", -100, None);

    let register = load(&ledger);
    let listing = register.select(&Filter::default(), Sort::Default);
    assert_eq!(listing.uncategorised, 3, "transfers don't need a category");

    let filter = Filter {
        uncategorised: true,
        ..Filter::default()
    };
    let listing = register.select(&filter, Sort::Default);
    assert_eq!(ids(&register, &listing), [bare, half, other_account]);
    assert_eq!(listing.uncategorised, 3);

    // The count follows the other filters.
    let filter = Filter {
        account: Some(current),
        ..Filter::default()
    };
    assert_eq!(register.select(&filter, Sort::Default).uncategorised, 2);
}

#[test]
fn sorting_orders_rows_within_each_day() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current account", Currency::GBP);
    let joint = ledger.account("Joint account", Currency::GBP);
    let pret = ledger.row_at(
        current,
        "2026-11-09",
        -645,
        None,
        Some("Pret A Manger"),
        "PRET",
    );
    let amazon = ledger.row_at(
        joint,
        "2026-11-09",
        -2399,
        Some("shopping"),
        Some("amazon"),
        "AMZN",
    );
    let tfl = ledger.row_at(
        current,
        "2026-11-09",
        -310,
        Some("transport"),
        Some("Transport for London"),
        "TFL",
    );
    let refund = ledger.row_at(joint, "2026-11-08", 3400, None, Some("ASOS"), "ASOS");
    let bills = ledger.row_at(
        current,
        "2026-11-08",
        -8640,
        Some("bills"),
        Some("Octopus"),
        "OCTOPUS",
    );

    let register = load(&ledger);
    let sorted = |sort: Sort| ids(&register, &register.select(&Filter::default(), sort));

    assert_eq!(sorted(Sort::Default), [tfl, amazon, pret, bills, refund]);
    assert_eq!(
        sorted(Sort::Ascending(SortColumn::Merchant)),
        [amazon, pret, tfl, refund, bills],
        "ignoring case, and never across days"
    );
    assert_eq!(
        sorted(Sort::Descending(SortColumn::Merchant)),
        [tfl, pret, amazon, bills, refund]
    );
    assert_eq!(
        sorted(Sort::Ascending(SortColumn::Amount)),
        [amazon, pret, tfl, bills, refund]
    );
    assert_eq!(
        sorted(Sort::Ascending(SortColumn::Account)),
        [tfl, pret, amazon, bills, refund]
    );
    // Uncategorised rows sort first.
    assert_eq!(
        sorted(Sort::Ascending(SortColumn::Category)),
        [pret, amazon, tfl, refund, bills]
    );
    let listing = register.select(&Filter::default(), Sort::Ascending(SortColumn::Amount));
    assert_eq!(listing.days.len(), 2);
    assert_eq!(listing.days[1].rows, 3..5);
}

#[test]
fn a_header_click_sorts_reverses_then_resets() {
    let column = SortColumn::Amount;
    let first = Sort::Default.next(column);
    assert_eq!(first, Sort::Ascending(column));
    assert_eq!(first.next(column), Sort::Descending(column));
    assert_eq!(first.next(column).next(column), Sort::Default);
    assert_eq!(
        Sort::Descending(SortColumn::Merchant).next(column),
        Sort::Ascending(column)
    );
    assert_eq!(first.column(), Some(column));
    assert_eq!(Sort::Default.column(), None);
}

#[test]
fn colours_go_to_the_six_groups_with_most_spending() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current account", Currency::GBP);
    // Seven spending groups, most first; plus income, `other`, and spending
    // older than 12 months.
    let groups = [
        ("food.groceries", -9000),
        ("bills", -8000),
        ("transport.public", -7000),
        ("shopping", -6000),
        ("entertainment", -5000),
        ("health", -4000),
        ("travel", -3000),
    ];
    for (key, minor) in groups {
        ledger.row(current, "2026-10-01", minor, Some(key));
    }
    ledger.row(current, "2026-10-01", -100_000, Some("other"));
    ledger.row(current, "2026-10-01", 500_000, Some("income.salary"));
    ledger.row(current, "2025-11-14", -100_000, Some("giving"));
    ledger.row(current, "2025-11-15", -1000, Some("home"));

    let register = load(&ledger);
    let colour = |key: &str| {
        register
            .categories()
            .iter()
            .find(|category| category.key.as_str() == key)
            .unwrap()
            .colour
    };
    assert_eq!(colour("food"), Colour::Chart(1));
    assert_eq!(
        colour("food.eating_out"),
        Colour::Chart(1),
        "shared by the group"
    );
    assert_eq!(colour("bills.energy"), Colour::Chart(2));
    assert_eq!(colour("transport"), Colour::Chart(3));
    assert_eq!(colour("shopping"), Colour::Chart(4));
    assert_eq!(colour("entertainment"), Colour::Chart(5));
    assert_eq!(colour("health"), Colour::Chart(6));
    assert_eq!(colour("travel"), Colour::Other, "only six colours");
    assert_eq!(colour("other"), Colour::Other);
    assert_eq!(colour("income"), Colour::Other);
    assert_eq!(colour("giving"), Colour::Other, "more than 12 months ago");
    assert_eq!(colour("home"), Colour::Other, "beaten by the six");

    // The menu order: each group, then its subcategories.
    let food = register
        .categories()
        .iter()
        .position(|category| category.key.as_str() == "food")
        .unwrap();
    assert_eq!(
        register.categories()[food + 1].key.as_str(),
        "food.groceries"
    );
    assert_eq!(
        register.categories()[food + 1].group,
        Some(register.categories()[food].id)
    );
}

#[test]
fn with_no_spending_every_category_is_other() {
    let ledger = TestLedger::new();
    let register = load(&ledger);
    assert!(!register.categories().is_empty());
    assert!(
        register
            .categories()
            .iter()
            .all(|category| category.colour == Colour::Other)
    );
    assert!(
        register
            .select(&Filter::default(), Sort::Default)
            .is_empty()
    );
}

#[test]
fn date_presets_cover_whole_periods() {
    let today = date(TODAY);
    let range = |preset: DatePreset| preset.range(today).map(|range| (range.from(), range.to()));
    let day = |text: &str| NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap();
    assert_eq!(
        range(DatePreset::ThisMonth),
        Some((day("2026-11-01"), day("2026-11-30")))
    );
    assert_eq!(
        range(DatePreset::LastMonth),
        Some((day("2026-10-01"), day("2026-10-31")))
    );
    assert_eq!(
        range(DatePreset::LastThreeMonths),
        Some((day("2026-09-01"), day("2026-11-30")))
    );
    assert_eq!(
        range(DatePreset::ThisYear),
        Some((day("2026-01-01"), day("2026-12-31")))
    );
    assert_eq!(range(DatePreset::AllTime), None);
    // January's last month is the previous December.
    assert_eq!(
        DatePreset::LastMonth
            .range(day("2027-01-15"))
            .map(|range| (range.from(), range.to())),
        Some((day("2026-12-01"), day("2026-12-31")))
    );
}

#[test]
fn rows_carry_tags_notes_the_users_name_and_whether_they_can_be_split() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current account", Currency::GBP);
    let savings = ledger.account("Savings", Currency::GBP);
    let shop = ledger.row(current, "2026-11-07", -5216, None);
    ledger.tag(shop, "weekly shop", false);
    ledger.tag(shop, "Mum's birthday", true);
    ledger
        .db
        .conn()
        .execute(
            "UPDATE \"transaction\" SET note = 'Flowers + weekly shop', user_note = 'Card', \
             merchant_display = 'Waitrose', merchant_display_source = 'user' WHERE id = ?1",
            [shop],
        )
        .unwrap();
    let out = ledger.row(current, "2026-11-08", -25_000, None);
    let into = ledger.row(savings, "2026-11-08", 25_000, None);
    ledger.transfer(out, into);
    let purchase = ledger.row(current, "2026-11-01", -3400, Some("shopping"));
    let refund = ledger.row(current, "2026-11-06", 3400, None);
    ledger.refund(refund, purchase, 3400);

    let register = load(&ledger);
    let names: Vec<_> = register
        .tags()
        .iter()
        .map(|tag| tag.name.as_str())
        .collect();
    assert_eq!(names, ["Mum's birthday", "weekly shop"]);
    assert!(register.tags()[0].private);

    let row = register.row(shop).unwrap();
    let on_row: Vec<_> = row
        .tags
        .iter()
        .map(|id| register.tag(*id).unwrap().name.as_str())
        .collect();
    assert_eq!(on_row, ["Mum's birthday", "weekly shop"]);
    assert_eq!(row.bank_note.as_deref(), Some("Flowers + weekly shop"));
    assert_eq!(row.note.as_deref(), Some("Card"));
    assert!(row.merchant_set);
    assert_eq!(row.merchant, "Waitrose");
    assert!(row.splittable);

    // A transfer and the money in of a confirmed refund can't be split.
    assert!(!register.row(out).unwrap().splittable);
    assert!(!register.row(refund).unwrap().splittable);
    let purchase_row = register.row(purchase).unwrap();
    assert!(purchase_row.splittable);
    assert!(purchase_row.tags.is_empty());
    assert!(!purchase_row.merchant_set);
}
