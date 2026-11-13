use crate::account::AccountKind;
use crate::money::Currency;

use super::super::test_ledger::TestLedger;
use super::*;

fn period(text: &str) -> Period {
    text.parse().unwrap()
}

/// `(name, balance, counted, unanchored)` per account, and the totals as
/// `(currency, minor)`.
type Month = (Vec<(String, i64, i64, bool)>, Vec<(String, i64)>);

fn months(history: &Capped<NetWorthMonth>) -> Vec<Month> {
    history
        .items
        .iter()
        .map(|month| {
            (
                month
                    .accounts
                    .iter()
                    .map(|a| {
                        (
                            a.name.clone(),
                            a.balance.minor(),
                            a.counted.minor(),
                            a.unanchored,
                        )
                    })
                    .collect(),
                month
                    .total
                    .iter()
                    .map(|m| (m.currency().code().to_owned(), m.minor()))
                    .collect(),
            )
        })
        .collect()
}

fn account(name: &str, balance: i64, counted: i64, unanchored: bool) -> (String, i64, i64, bool) {
    (name.to_owned(), balance, counted, unanchored)
}

#[test]
fn sums_month_end_balances_per_currency() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    ledger.balance(current, "2026-09-30", 100_000);
    // A move into a pot, paired: net worth doesn't change.
    let pot = ledger.pot(current, "Holiday");
    let out = ledger.row(current, "2026-10-05", -10_000, None);
    let into = ledger.row(pot, "2026-10-05", 10_000, None);
    ledger.transfer(out, into);
    // A shared purchase counts in full: balances are gross.
    let dinner = ledger.row(current, "2026-11-06", -4_000, Some("food.eating_out"));
    ledger.share(dinner, -2_000);
    // Valuation accounts.
    let house = ledger.valuation_account("House", AccountKind::Asset, Currency::GBP);
    ledger.valuation(house, "2026-10-15", 30_000_000);
    let mortgage = ledger.valuation_account("Mortgage", AccountKind::Liability, Currency::GBP);
    ledger.valuation(mortgage, "2026-09-01", -20_000_000);
    // A liability entered positive still subtracts.
    let loan = ledger.valuation_account("Loan", AccountKind::Liability, Currency::GBP);
    ledger.valuation(loan, "2026-09-01", 5_000);
    // Euros without a stated balance: unanchored.
    let euros = ledger.account("Euros", Currency::EUR);
    ledger.row(euros, "2026-11-02", -500, None);
    // Closed on 20 October: in September, gone after.
    let old = ledger.account("Old savings", Currency::GBP);
    ledger.balance(old, "2026-09-01", 3_000);
    ledger
        .db
        .conn()
        .execute(
            "UPDATE account SET closed_on = '2026-10-20' WHERE id = ?1",
            [old],
        )
        .unwrap();

    let history =
        net_worth_history(&ledger.reader(), period("2026-09"), period("2026-11")).unwrap();
    assert!(!history.truncated);
    let dates: Vec<String> = history.items.iter().map(|m| m.date.to_string()).collect();
    assert_eq!(dates, ["2026-09-30", "2026-10-31", "2026-11-30"]);
    let gbp = |minor| ("GBP".to_owned(), minor);
    let eur = |minor| ("EUR".to_owned(), minor);
    assert_eq!(
        months(&history),
        [
            (
                // The house has no valuation yet.
                vec![
                    account("Current", 100_000, 100_000, false),
                    account("Holiday", 0, 0, true),
                    account("Mortgage", -20_000_000, -20_000_000, false),
                    account("Loan", 5_000, -5_000, false),
                    account("Euros", 0, 0, true),
                    account("Old savings", 3_000, 3_000, false),
                ],
                vec![eur(0), gbp(-19_902_000)],
            ),
            (
                vec![
                    account("Current", 90_000, 90_000, false),
                    account("Holiday", 10_000, 10_000, true),
                    account("House", 30_000_000, 30_000_000, false),
                    account("Mortgage", -20_000_000, -20_000_000, false),
                    account("Loan", 5_000, -5_000, false),
                    account("Euros", 0, 0, true),
                ],
                vec![eur(0), gbp(10_095_000)],
            ),
            (
                vec![
                    account("Current", 86_000, 86_000, false),
                    account("Holiday", 10_000, 10_000, true),
                    account("House", 30_000_000, 30_000_000, false),
                    account("Mortgage", -20_000_000, -20_000_000, false),
                    account("Loan", 5_000, -5_000, false),
                    account("Euros", -500, -500, true),
                ],
                vec![eur(-500), gbp(10_091_000)],
            ),
        ]
    );
}

#[test]
fn refuses_a_backwards_range_and_caps_long_ones() {
    let mut ledger = TestLedger::new();
    ledger.account("Current", Currency::GBP);
    let reader = ledger.reader();
    assert!(matches!(
        net_worth_history(&reader, period("2026-11"), period("2026-10")),
        Err(Error::EmptyRange { .. })
    ));
    let long = net_worth_history(&reader, period("2000-01"), period("2026-12")).unwrap();
    assert!(long.truncated);
    assert_eq!(long.len(), Limit::MAX.get());
}
