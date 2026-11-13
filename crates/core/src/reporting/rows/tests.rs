use crate::exclusion::ExclusionReason;
use crate::link::LinkKind;
use crate::money::{Currency, Money};

use super::super::test_ledger::{TestLedger, date};
use super::*;

fn limit(n: u32) -> Limit {
    Limit::new(Some(n))
}

fn ids(lines: &[LedgerLine]) -> Vec<TransactionId> {
    lines.iter().map(|line| line.transaction_id).collect()
}

fn key(text: &str) -> CategoryKey {
    CategoryKey::new(text).unwrap()
}

#[test]
fn split_lines_are_rows_with_the_parent_id() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let shop = ledger.row(current, "2026-11-03", -5_000, None);
    ledger.split(shop, &[(-3_000, Some("food.groceries")), (-2_000, None)]);

    let found = search(&ledger.reader(), &Search::default(), limit(10)).unwrap();
    assert!(!found.truncated);
    let lines: Vec<(TransactionId, Option<u32>, i64)> = found
        .items
        .iter()
        .map(|line| (line.transaction_id, line.line_no, line.amount.minor()))
        .collect();
    assert_eq!(lines, [(shop, Some(1), -3_000), (shop, Some(2), -2_000)]);
    assert!(found.items.iter().all(|line| line.bank_text.is_some()));

    // Only the uncategorised line is uncategorised.
    let open = uncategorized(&ledger.reader(), limit(10)).unwrap();
    assert_eq!(open.items.len(), 1);
    assert_eq!(open.items[0].line_no, Some(2));
}

#[test]
fn a_confirmed_refund_is_a_row_with_its_purchase() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let shoes = ledger.row(current, "2026-10-28", -6_000, Some("shopping.clothing"));
    let back = ledger.row(current, "2026-11-04", 6_000, None);
    ledger.refund(back, shoes, 6_000);

    let found = search(&ledger.reader(), &Search::default(), limit(10)).unwrap();
    assert_eq!(ids(&found.items), [back, shoes], "newest first");
    let refund = &found.items[0];
    assert_eq!(refund.link_kind, Some(LinkKind::Refund));
    assert_eq!(refund.target_id, Some(shoes));
    assert_eq!(
        refund.category.as_ref().map(|c| c.key.as_str()),
        Some("shopping.clothing")
    );
    // Fully linked: no uncategorised rest.
    assert!(
        uncategorized(&ledger.reader(), limit(10))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_shared_cost_shows_the_whole_amount_and_my_share() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let dinner = ledger.row(current, "2026-11-05", -4_000, Some("food"));
    ledger.share(dinner, -1_500);

    let found = search(&ledger.reader(), &Search::default(), limit(10)).unwrap();
    assert_eq!(found.items.len(), 1);
    assert_eq!(found.items[0].amount, Money::new(-4_000, Currency::GBP));
    assert_eq!(found.items[0].personal, Money::new(-1_500, Currency::GBP));
}

#[test]
fn excluded_rows_and_confirmed_transfers_are_left_out() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let savings = ledger.account("Savings", Currency::GBP);
    let kept = ledger.row(current, "2026-11-01", -100, None);
    let duplicate = ledger.row(current, "2026-11-01", -100, None);
    ledger.exclude(duplicate, ExclusionReason::Duplicate);
    let out = ledger.row(current, "2026-11-02", -5_000, None);
    let into = ledger.row(savings, "2026-11-02", 5_000, None);
    ledger.transfer(out, into);

    let found = search(&ledger.reader(), &Search::default(), limit(10)).unwrap();
    assert_eq!(ids(&found.items), [kept]);
    assert_eq!(
        ids(&uncategorized(&ledger.reader(), limit(10)).unwrap().items),
        [kept]
    );
}

#[test]
fn rows_keep_their_own_currency() {
    let mut ledger = TestLedger::new();
    let pounds = ledger.account("Pounds", Currency::GBP);
    let euros = ledger.account("Euros", Currency::EUR);
    ledger.row(pounds, "2026-11-01", -100, None);
    ledger.row(euros, "2026-11-02", -200, None);

    let found = search(&ledger.reader(), &Search::default(), limit(10)).unwrap();
    let amounts: Vec<Money> = found.items.iter().map(|line| line.amount).collect();
    assert_eq!(
        amounts,
        [
            Money::new(-200, Currency::EUR),
            Money::new(-100, Currency::GBP)
        ]
    );
}

#[test]
fn public_tags_never_include_private_ones() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let a = ledger.row(current, "2026-11-01", -100, None);
    let b = ledger.row(current, "2026-11-02", -100, None);
    let c = ledger.row(current, "2026-11-03", -100, None);
    ledger.tag(a, "holiday", false);
    ledger.tag(a, "Business", false);
    ledger.tag(a, "therapy", true);
    ledger.tag(b, "therapy", true);

    let tags = public_tags(&ledger.reader(), &[a, b, c, a]).unwrap();
    assert_eq!(tags.len(), 1, "{tags:?}");
    assert_eq!(tags[&a], ["Business", "holiday"]);
    assert!(public_tags(&ledger.reader(), &[]).unwrap().is_empty());

    // Filtering by a private tag is refused as if it didn't exist.
    let private = Search {
        tags: vec![TagName::new("therapy").unwrap()],
        ..Search::default()
    };
    assert!(matches!(
        search(&ledger.reader(), &private, limit(10)),
        Err(Error::UnknownTag)
    ));
}

#[test]
fn filters_combine() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let other = ledger.account("Other", Currency::GBP);
    let coffee = ledger.row_at(
        current,
        "2026-11-03",
        -350,
        Some("food.eating_out"),
        Some("Bean There"),
        "CARD BEAN THERE",
    );
    // Wrong account, too early, too large, wrong category, untagged.
    let elsewhere = ledger.row_at(
        other,
        "2026-11-03",
        -350,
        Some("food.eating_out"),
        Some("Bean There"),
        "CARD BEAN THERE",
    );
    let early = ledger.row_at(
        current,
        "2026-10-03",
        -350,
        Some("food.eating_out"),
        Some("Bean There"),
        "CARD BEAN THERE",
    );
    let big = ledger.row_at(
        current,
        "2026-11-04",
        -9_000,
        Some("food.eating_out"),
        Some("Bean There"),
        "CARD BEAN THERE",
    );
    let groceries = ledger.row_at(
        current,
        "2026-11-05",
        -350,
        Some("home"),
        Some("Bean There"),
        "CARD BEAN THERE",
    );
    let untagged = ledger.row_at(
        current,
        "2026-11-06",
        -350,
        Some("food.eating_out"),
        Some("Bean There"),
        "CARD BEAN THERE",
    );
    ledger.row_at(
        current,
        "2026-11-07",
        -350,
        Some("food"),
        None,
        "OTHER SHOP",
    );
    for id in [coffee, elsewhere, early, big, groceries] {
        ledger.tag(id, "treats", false);
    }
    let _ = untagged;

    let query = Search {
        text: Some("bean".to_owned()),
        from: Some(date("2026-11-01")),
        to: None,
        accounts: vec![current],
        categories: vec![key("food")],
        tags: vec![TagName::new("TREATS").unwrap()],
        amount_min: Some(-1_000),
        amount_max: Some(0),
    };
    let found = search(&ledger.reader(), &query, limit(10)).unwrap();
    assert_eq!(ids(&found.items), [coffee]);

    // Only an end date: every earlier day.
    let until = Search {
        to: Some(date("2026-10-31")),
        ..Search::default()
    };
    assert_eq!(
        ids(&search(&ledger.reader(), &until, limit(10)).unwrap().items),
        [early]
    );
}

#[test]
fn bad_input_is_refused() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    ledger.row(current, "2026-11-01", -100, None);
    let reader = ledger.reader();
    let refused = |query: Search| search(&reader, &query, limit(10)).unwrap_err();

    assert!(matches!(
        refused(Search {
            from: Some(date("2026-11-02")),
            to: Some(date("2026-11-01")),
            ..Search::default()
        }),
        Error::EmptyRange { .. }
    ));
    assert!(matches!(
        refused(Search {
            accounts: vec![AccountId::new(999)],
            ..Search::default()
        }),
        Error::UnknownAccount(_)
    ));
    assert!(matches!(
        refused(Search {
            categories: vec![key("nope")],
            ..Search::default()
        }),
        Error::UnknownCategory(_)
    ));
    assert!(matches!(
        refused(Search {
            amount_min: Some(10),
            amount_max: Some(-10),
            ..Search::default()
        }),
        Error::InvalidAmountRange
    ));
}

#[test]
fn search_is_capped_newest_first() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let rows: Vec<TransactionId> = (1..=5)
        .map(|day| ledger.row(current, &format!("2026-11-0{day}"), -100, None))
        .collect();
    let found = search(&ledger.reader(), &Search::default(), limit(2)).unwrap();
    assert!(found.truncated);
    assert_eq!(ids(&found.items), [rows[4], rows[3]]);
    let open = uncategorized(&ledger.reader(), limit(5)).unwrap();
    assert!(!open.truncated);
    assert_eq!(open.len(), 5);
}

#[test]
fn merchant_history_sums_every_row_even_when_capped() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let euros = ledger.account("Euros", Currency::EUR);
    let first = ledger.row_at(current, "2026-09-01", -300, None, Some("Bean There"), "X");
    let shared = ledger.row_at(current, "2026-10-01", -1_000, None, Some("Bean There"), "X");
    ledger.share(shared, -400);
    let split = ledger.row_at(current, "2026-10-15", -500, None, Some("Bean There"), "X");
    ledger.split(split, &[(-200, Some("food")), (-300, None)]);
    let last = ledger.row_at(euros, "2026-11-01", -700, None, Some("BEAN THERE"), "X");
    ledger.row_at(current, "2026-11-02", -999, None, Some("Elsewhere"), "X");

    let history = merchant_history(&ledger.reader(), "  bean there ", limit(2)).unwrap();
    assert!(history.lines.truncated);
    assert_eq!(ids(&history.lines.items), [last, split]);
    let summary = &history.summary;
    assert_eq!(summary.lines, 5);
    assert_eq!(summary.transactions, 4);
    assert_eq!(
        summary.personal.get(Currency::GBP),
        Some(Money::new(-300 - 400 - 500, Currency::GBP))
    );
    assert_eq!(
        summary.personal.get(Currency::EUR),
        Some(Money::new(-700, Currency::EUR))
    );
    assert_eq!(summary.first, Some(date("2026-09-01")));
    assert_eq!(summary.last, Some(date("2026-11-01")));
    let _ = first;

    let blank = merchant_history(&ledger.reader(), "  ", limit(2)).unwrap();
    assert!(blank.lines.is_empty() && !blank.lines.truncated);
    assert_eq!(blank.summary, MerchantSummary::default());
}

#[test]
fn account_identifiers_are_found() {
    for text in [
        "TRANSFER TO ROBIN FIXTURE-HOLDER 77-41-29 58316402 IBAN GB94 BARC 1020 1530 0934 59",
        "SORT 77-41-29",
        "SORT 77 41 29 REF",
        "77-41-29",
        "ACC 58316402",
        "ACC58316402",
        "CARD 4111111111111111",
        "IBAN GB94BARC10201530093459",
        "iban gb94barc10201530093459.",
        "GB94 BARC 1020 1530 0934 59",
        "DE89 3704 0044 0532 0130 00",
        "NL91ABNA0417164300",
    ] {
        assert!(has_account_identifier(text), "{text:?}");
    }
}

#[test]
fn ordinary_bank_text_is_not_an_account_identifier() {
    for text in [
        "",
        "CARD PAYMENT TO BEAN THERE ON 2026-11-20",
        "2026-11-20",
        "20-11-2026",
        "CARD *1234",
        "AMAZON MKTPLACE 1234567",
        "£1,234.56 REFUND",
        "-12345.67",
        "12:34:56",
        "0800 123 4567",
        "DD 12-34 REF",
        "GB94 CARD PAYMENT TO SHOP",
        "AB12 CARD PAYM ENTS ONLY",
        "UK12 1234",
        "TFL TRAVEL CH 12 34 5",
        "Pot transfer",
    ] {
        assert!(!has_account_identifier(text), "{text:?}");
    }
}
