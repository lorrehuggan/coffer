use crate::account::AccountKind;
use crate::money::Currency;

use super::super::test_ledger::TestLedger;
use super::*;

/// `(name, balance or None, flagged, overdrawn)` per row, in panel order.
fn rows(tree: &AccountTree) -> Vec<(String, Option<i64>, bool, bool)> {
    tree.rows()
        .map(|node| {
            (
                node.account.name.clone(),
                node.latest.map(|latest| latest.balance.minor()),
                node.status == Status::Mismatch,
                node.is_overdrawn(),
            )
        })
        .collect()
}

fn row(
    name: &str,
    balance: Option<i64>,
    flagged: bool,
    overdrawn: bool,
) -> (String, Option<i64>, bool, bool) {
    (name.to_owned(), balance, flagged, overdrawn)
}

fn totals(tree: &AccountTree) -> Vec<(String, i64)> {
    tree.net_worth
        .iter()
        .map(|money| (money.currency().code().to_owned(), money.minor()))
        .collect()
}

#[test]
fn lists_pots_under_their_parent_with_balances_and_net_worth() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current account", Currency::GBP);
    ledger.balance(current, "2026-11-01", 100_000);
    let joint = ledger.account("Joint account", Currency::GBP);
    // Created after Joint, but listed under its parent.
    let holiday = ledger.pot(current, "Holiday");
    let out = ledger.row(current, "2026-11-05", -12_000, None);
    let into = ledger.row(holiday, "2026-11-05", 12_000, None);
    ledger.transfer(out, into);
    // Overdrawn, from its entered balance.
    ledger.balance(joint, "2026-11-02", -2_410);
    // A credit card with no balance: no balance, left out of net worth,
    // though it has a transaction.
    let amex = ledger.account("Amex", Currency::GBP);
    ledger.row(amex, "2026-11-03", -4_000, None);
    ledger
        .db
        .conn()
        .execute(
            "UPDATE account SET kind = 'credit_card' WHERE id = ?1",
            [amex],
        )
        .unwrap();
    // A liability is negative by nature: not overdrawn, and it subtracts.
    let mortgage = ledger.valuation_account("Mortgage", AccountKind::Liability, Currency::GBP);
    ledger.valuation(mortgage, "2026-10-01", -50_000);
    // Euros count in their own currency.
    let euros = ledger.account("Euros", Currency::EUR);
    ledger.balance(euros, "2026-11-01", 700);
    // A closed account and its pot are left out.
    let old = ledger.account("Old", Currency::GBP);
    ledger.balance(old, "2026-10-01", 9_900);
    let old_pot = ledger.pot(old, "Old pot");
    ledger
        .db
        .conn()
        .execute(
            "UPDATE account SET closed_on = '2026-10-20' WHERE id = ?1",
            [old],
        )
        .unwrap();
    let _ = old_pot;

    let tree = account_tree(&ledger.reader()).unwrap();
    assert_eq!(
        rows(&tree),
        [
            row("Current account", Some(88_000), false, false),
            row("Holiday", Some(12_000), false, false),
            row("Joint account", Some(-2_410), false, true),
            row("Amex", None, false, false),
            row("Mortgage", Some(-50_000), false, false),
            row("Euros", Some(700), false, false),
        ]
    );
    // 88,000 + 12,000 − 2,410 − 50,000.
    assert_eq!(
        totals(&tree),
        [("EUR".to_owned(), 700), ("GBP".to_owned(), 47_590)]
    );
    let current_node = &tree.accounts[0];
    assert_eq!(
        current_node
            .pots_total()
            .unwrap()
            .map(|total| total.minor()),
        Some(12_000)
    );
    assert_eq!(tree.accounts[1].pots_total().unwrap(), None);
    // The latest balance is at the end of the last active day.
    assert_eq!(
        current_node.latest.unwrap().date,
        crate::reporting::test_ledger::date("2026-11-05")
    );
}

#[test]
fn a_failed_balance_check_flags_the_account() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current account", Currency::GBP);
    ledger.balance(current, "2026-11-01", 10_000);
    ledger.row(current, "2026-11-02", -1_000, None);
    // Should be 9,000: £5 off.
    ledger.balance(current, "2026-11-03", 8_500);

    let tree = account_tree(&ledger.reader()).unwrap();
    assert_eq!(
        rows(&tree),
        [row("Current account", Some(8_500), true, false)]
    );
}

#[test]
fn no_accounts_is_an_empty_tree() {
    let ledger = TestLedger::new();
    let tree = account_tree(&ledger.reader()).unwrap();
    assert!(tree.accounts.is_empty());
    assert!(tree.net_worth.is_empty());
}
