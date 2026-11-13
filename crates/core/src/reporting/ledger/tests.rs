use crate::exclusion::ExclusionReason;
use crate::money::Currency;

use super::super::test_ledger::{TestLedger, date};
use super::super::{DateRange, Limit};
use super::*;

/// `(transaction, split line, link, category key, amount, personal)` of
/// each line.
type Summary = (
    TransactionId,
    Option<u32>,
    Option<LinkKind>,
    Option<String>,
    i64,
    i64,
);

fn summarize(lines: &[LedgerLine]) -> Vec<Summary> {
    lines
        .iter()
        .map(|line| {
            (
                line.transaction_id,
                line.line_no,
                line.link_kind,
                line.category.as_ref().map(|c| c.key.as_str().to_owned()),
                line.amount.minor(),
                line.personal.minor(),
            )
        })
        .collect()
}

fn key(text: &str) -> Option<String> {
    Some(text.to_owned())
}

#[test]
fn a_split_purchase_counts_in_each_lines_category() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let shop = ledger.row(current, "2026-11-03", -5_000, None);
    ledger.split(
        shop,
        &[
            (-3_000, Some("food.groceries")),
            (-1_500, Some("home")),
            (-500, None),
        ],
    );

    let lines = Ledger::new().lines(&ledger.reader()).unwrap();
    assert_eq!(
        summarize(&lines),
        [
            (shop, Some(1), None, key("food.groceries"), -3_000, -3_000),
            (shop, Some(2), None, key("home"), -1_500, -1_500),
            (shop, Some(3), None, None, -500, -500),
        ]
    );
    assert!(lines.iter().all(|line| line.split_id.is_some()));
    let groceries = Ledger::new()
        .categories([CategoryKey::new("food").unwrap()])
        .lines(&ledger.reader())
        .unwrap();
    assert_eq!(
        summarize(&groceries).len(),
        1,
        "a group includes its subcategories"
    );
}

#[test]
fn a_refund_nets_against_its_purchases_category_on_its_own_date() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let shoes = ledger.row(current, "2026-10-28", -6_000, Some("shopping.clothing"));
    let back = ledger.row(current, "2026-11-04", 6_500, Some("income.gifts"));
    ledger.refund(back, shoes, 6_000);

    let lines = Ledger::new().lines(&ledger.reader()).unwrap();
    assert_eq!(
        summarize(&lines),
        [
            (shoes, None, None, key("shopping.clothing"), -6_000, -6_000),
            // The unlinked rest keeps the inflow's own category.
            (back, None, None, key("income.gifts"), 500, 500),
            (
                back,
                None,
                Some(LinkKind::Refund),
                key("shopping.clothing"),
                6_000,
                6_000
            ),
        ]
    );
    let refund = &lines[2];
    assert!(refund.is_refund());
    assert_eq!(refund.target_id, Some(shoes));
    assert_eq!(refund.date, date("2026-11-04"));
    let november = Ledger::new()
        .within(DateRange::parse("2026-11-01", "2026-11-30").unwrap())
        .categories([CategoryKey::new("shopping").unwrap()])
        .lines(&ledger.reader())
        .unwrap();
    assert_eq!(summarize(&november).len(), 1);
}

#[test]
fn a_fully_linked_inflow_leaves_no_zero_row() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let shoes = ledger.row(current, "2026-10-28", -6_000, Some("shopping.clothing"));
    let back = ledger.row(current, "2026-11-04", 6_000, None);
    ledger.refund(back, shoes, 6_000);
    let lines = Ledger::new()
        .uncategorized()
        .lines(&ledger.reader())
        .unwrap();
    assert!(lines.is_empty(), "{lines:#?}");
}

#[test]
fn a_shared_cost_counts_as_your_share_and_a_claim_as_nothing() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let dinner = ledger.row(current, "2026-11-05", -9_000, Some("food.eating_out"));
    ledger.share(dinner, -3_000);
    let hotel = ledger.row(current, "2026-11-06", -20_000, Some("travel"));
    ledger.claim_back(hotel);

    let lines = Ledger::new().lines(&ledger.reader()).unwrap();
    assert_eq!(
        summarize(&lines),
        [
            (dinner, None, None, key("food.eating_out"), -9_000, -3_000),
            (hotel, None, None, key("travel"), -20_000, 0),
        ]
    );
    assert_eq!(lines[0].counted(Basis::Personal).minor(), -3_000);
    assert_eq!(lines[0].counted(Basis::Gross).minor(), -9_000);
}

#[test]
fn excluded_rows_and_confirmed_transfers_are_left_out() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let savings = ledger.account("Savings", Currency::GBP);
    let kept = ledger.row(current, "2026-11-01", -1_000, Some("food.groceries"));
    let disputed = ledger.row(current, "2026-11-02", -2_000, Some("food.groceries"));
    ledger.exclude(disputed, ExclusionReason::Disputed);
    let out = ledger.row(current, "2026-11-03", -5_000, None);
    let into = ledger.row(savings, "2026-11-03", 5_000, None);
    ledger.transfer(out, into);
    // A refund of an excluded double charge leaves reports with the charge.
    let double = ledger.row(current, "2026-11-04", -700, Some("shopping"));
    let back = ledger.row(current, "2026-11-06", 700, None);
    ledger.refund(back, double, 700);
    ledger.exclude(double, ExclusionReason::BankError);

    let lines = Ledger::new().lines(&ledger.reader()).unwrap();
    assert_eq!(
        summarize(&lines),
        [(kept, None, None, key("food.groceries"), -1_000, -1_000)]
    );
}

#[test]
fn every_line_keeps_its_own_currency() {
    let mut ledger = TestLedger::new();
    let gbp = ledger.account("Current", Currency::GBP);
    let eur = ledger.account("Euro", Currency::EUR);
    ledger.row(gbp, "2026-11-01", -1_000, Some("food.groceries"));
    ledger.row(eur, "2026-11-01", -1_000, Some("food.groceries"));

    let lines = Ledger::new().lines(&ledger.reader()).unwrap();
    let currencies: Vec<String> = lines
        .iter()
        .map(|l| l.amount.currency().code().to_owned())
        .collect();
    assert_eq!(currencies, ["GBP", "EUR"]);
    let mut totals = super::super::Totals::new();
    for line in &lines {
        totals.add(line.counted(Basis::Personal)).unwrap();
    }
    assert_eq!(totals.len(), 2);
    assert_eq!(totals.get(Currency::GBP).unwrap().minor(), -1_000);
    assert_eq!(totals.get(Currency::EUR).unwrap().minor(), -1_000);
}

#[test]
fn filters_and_together() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let other = ledger.account("Other", Currency::GBP);
    let coffee = ledger.row_at(
        current,
        "2026-11-01",
        -350,
        Some("food.eating_out"),
        Some("BEAN THERE 100%"),
        "CARD PAYMENT BEAN THERE",
    );
    let lunch = ledger.row_at(
        current,
        "2026-11-02",
        -1_200,
        None,
        Some("Noodle Bar"),
        "NOODLE_BAR LONDON",
    );
    let elsewhere = ledger.row_at(other, "2026-11-03", -900, None, None, "BEAN THERE KIOSK");
    ledger.tag(coffee, "Work", false);
    ledger.tag(lunch, "secret-fixture", true);
    let reader = ledger.reader();
    let ids = |query: Ledger| -> Vec<TransactionId> {
        query
            .lines(&reader)
            .unwrap()
            .iter()
            .map(|l| l.transaction_id)
            .collect()
    };

    assert_eq!(
        ids(Ledger::new().matching("bean there")),
        [coffee, elsewhere]
    );
    assert_eq!(
        ids(Ledger::new().matching("  ")),
        [coffee, lunch, elsewhere]
    );
    // `%` and `_` are literal.
    assert_eq!(ids(Ledger::new().matching("100%")), [coffee]);
    assert_eq!(
        ids(Ledger::new().matching("n_t")),
        Vec::<TransactionId>::new()
    );
    assert_eq!(ids(Ledger::new().matching("noodle_bar")), [lunch]);
    assert_eq!(ids(Ledger::new().merchant(" noodle bar ")), [lunch]);
    assert_eq!(
        ids(Ledger::new().merchant("noodle")),
        Vec::<TransactionId>::new()
    );
    assert_eq!(ids(Ledger::new().accounts([other])), [elsewhere]);
    assert_eq!(ids(Ledger::new().uncategorized()), [lunch, elsewhere]);
    assert_eq!(
        ids(Ledger::new().amount_between(Some(-1_000), Some(-500))),
        [elsewhere]
    );
    assert_eq!(
        ids(Ledger::new().tags([TagName::new("work").unwrap()])),
        [coffee]
    );
    assert_eq!(
        ids(Ledger::new().newest_first()),
        [elsewhere, lunch, coffee]
    );
    assert_eq!(
        ids(Ledger::new()
            .matching("bean")
            .within(DateRange::parse("2026-11-02", "2026-11-30").unwrap())),
        [elsewhere]
    );
    assert!(ledger_lines_have_no_bank_text(&reader));
    let with_text = Ledger::new().with_bank_text().lines(&reader).unwrap();
    assert_eq!(
        with_text[1].bank_text.as_ref().unwrap().description,
        "NOODLE_BAR LONDON"
    );
}

fn ledger_lines_have_no_bank_text(reader: &Reader) -> bool {
    Ledger::new()
        .lines(reader)
        .unwrap()
        .iter()
        .all(|line| line.bank_text.is_none())
}

#[test]
fn unknown_and_private_filters_are_refused() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let row = ledger.row(current, "2026-11-01", -100, None);
    ledger.tag(row, "secret-fixture", true);
    let reader = ledger.reader();

    let error = Ledger::new()
        .tags([TagName::new("Secret-Fixture").unwrap()])
        .lines(&reader)
        .unwrap_err();
    assert!(matches!(error, Error::UnknownTag));
    assert!(!error.to_string().to_lowercase().contains("secret"));
    assert!(matches!(
        Ledger::new()
            .tags([TagName::new("nope").unwrap()])
            .lines(&reader),
        Err(Error::UnknownTag)
    ));
    assert!(matches!(
        Ledger::new()
            .categories([CategoryKey::new("no.such").unwrap()])
            .lines(&reader),
        Err(Error::UnknownCategory(_))
    ));
    assert!(matches!(
        Ledger::new().accounts([AccountId::new(999)]).lines(&reader),
        Err(Error::UnknownAccount(_))
    ));
    let error = Ledger::new()
        .amount_between(Some(10), Some(-10))
        .lines(&reader)
        .unwrap_err();
    assert!(matches!(error, Error::InvalidAmountRange));
    assert!(error.is_invalid_input());
}

#[test]
fn capped_lines_say_when_more_matched() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    for day in 1..=5 {
        ledger.row(current, &format!("2026-11-0{day}"), -100 * day, None);
    }
    let reader = ledger.reader();
    let cut = Ledger::new()
        .newest_first()
        .lines_capped(&reader, Limit::new(Some(2)))
        .unwrap();
    assert!(cut.truncated);
    let amounts: Vec<i64> = cut.items.iter().map(|l| l.amount.minor()).collect();
    assert_eq!(amounts, [-500, -400]);
    let all = Ledger::new()
        .lines_capped(&reader, Limit::new(Some(5)))
        .unwrap();
    assert!(!all.truncated);
    assert_eq!(all.len(), 5);
}
