use crate::exclusion::ExclusionReason;
use crate::money::Currency;

use super::super::test_ledger::TestLedger;
use super::*;

fn period(text: &str) -> Period {
    text.parse().unwrap()
}

/// `(month, income, spending, net)`, each as `(currency, minor)` pairs.
type Row = (
    String,
    Vec<(String, i64)>,
    Vec<(String, i64)>,
    Vec<(String, i64)>,
);

fn pairs(totals: &Totals) -> Vec<(String, i64)> {
    totals
        .iter()
        .map(|money| (money.currency().code().to_owned(), money.minor()))
        .collect()
}

fn rows(summary: &Capped<MonthSummary>) -> Vec<Row> {
    summary
        .items
        .iter()
        .map(|month| {
            (
                month.period.key(),
                pairs(&month.income),
                pairs(&month.spending),
                pairs(&month.net),
            )
        })
        .collect()
}

fn gbp(minor: i64) -> Vec<(String, i64)> {
    vec![("GBP".into(), minor)]
}

/// October and November of a household: every kind of line the summary
/// treats differently (module docs).
fn household() -> TestLedger {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let savings = ledger.account("Savings", Currency::GBP);
    let euros = ledger.account("Euros", Currency::EUR);

    // October: pay, a split shop (one line uncategorised), shoes.
    ledger.row(current, "2026-10-01", 200_000, Some("income.salary"));
    let shop = ledger.row(current, "2026-10-03", -5_000, None);
    ledger.split(shop, &[(-3_000, Some("food.groceries")), (-2_000, None)]);
    let shoes = ledger.row(current, "2026-10-28", -6_000, Some("shopping.clothing"));

    // November: the shoes refunded with a 5.00 gift on top.
    let back = ledger.row(current, "2026-11-04", 6_500, Some("income.gifts"));
    ledger.refund(back, shoes, 6_000);
    // A dinner shared half and half.
    let dinner = ledger.row(current, "2026-11-06", -4_000, Some("food.eating_out"));
    ledger.share(dinner, -2_000);
    // A work purchase being claimed back.
    let laptop = ledger.row(current, "2026-11-07", -3_000, Some("shopping.electronics"));
    ledger.claim_back(laptop);
    // A disputed charge, excluded.
    let disputed = ledger.row(current, "2026-11-08", -1_000, Some("food.groceries"));
    ledger.exclude(disputed, ExclusionReason::Disputed);
    // A confirmed transfer to savings.
    let out = ledger.row(current, "2026-11-09", -10_000, None);
    let into = ledger.row(savings, "2026-11-09", 10_000, None);
    ledger.transfer(out, into);
    // An unconfirmed move to an account Coffer doesn't see.
    ledger.row(current, "2026-11-10", -500, Some("transfer.savings"));
    // Money in without a category.
    ledger.row(current, "2026-11-11", 700, None);
    // Spending in euros.
    ledger.row(euros, "2026-11-12", -1_234, Some("food"));
    ledger
}

#[test]
fn classifies_lines_by_kind_and_counts_the_users_share() {
    let ledger = household();
    let summary = monthly_summary(&ledger.reader(), period("2026-10"), period("2026-12")).unwrap();
    assert!(!summary.truncated);
    assert_eq!(
        rows(&summary),
        [
            // Pay; the split shop (both lines) and the shoes.
            (
                "2026-10".to_owned(),
                gbp(200_000),
                gbp(11_000),
                gbp(189_000)
            ),
            // Income: the gift's unlinked 5.00 and the uncategorised 7.00.
            // Spending: the refund (−60.00) nets against the dinner's
            // 20.00 share; the claim, the excluded charge, the transfer
            // and the unconfirmed move count for nothing. Euros apart.
            (
                "2026-11".to_owned(),
                gbp(1_200),
                vec![("EUR".into(), 1_234), ("GBP".into(), -4_000)],
                vec![("EUR".into(), -1_234), ("GBP".into(), 5_200)],
            ),
            // An empty month is still there.
            ("2026-12".to_owned(), vec![], vec![], vec![]),
        ]
    );
}

#[test]
fn a_single_month_and_an_empty_database() {
    let ledger = TestLedger::new();
    let summary = monthly_summary(&ledger.reader(), period("2026-11"), period("2026-11")).unwrap();
    assert_eq!(
        rows(&summary),
        [("2026-11".to_owned(), vec![], vec![], vec![])]
    );
}

#[test]
fn refuses_a_backwards_range_and_caps_long_ones() {
    let ledger = household();
    let reader = ledger.reader();
    assert!(matches!(
        monthly_summary(&reader, period("2026-11"), period("2026-10")),
        Err(Error::EmptyRange { .. })
    ));
    let long = monthly_summary(&reader, period("2010-01"), period("2026-12")).unwrap();
    assert!(long.truncated);
    assert_eq!(long.len(), Limit::MAX.get());
    assert_eq!(long.items[0].period, period("2010-01"));
    assert_eq!(long.items.last().unwrap().period, period("2026-08"));
}
