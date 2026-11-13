use crate::budget::NewBudget;
use crate::money::Currency;

use super::super::test_ledger::TestLedger;
use super::*;

fn key(text: &str) -> CategoryKey {
    CategoryKey::new(text).unwrap()
}

fn set(
    ledger: &mut TestLedger,
    category: &str,
    month: Option<&str>,
    amount: Money,
    group: BudgetGroup,
) {
    budget::set(
        &mut ledger.db,
        &NewBudget {
            category: key(category),
            month: month.map(|m| m.parse().unwrap()),
            amount,
            group,
        },
    )
    .unwrap();
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn eur(minor: i64) -> Money {
    Money::new(minor, Currency::EUR)
}

#[test]
fn compares_each_budget_with_the_users_spending() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let euros = ledger.account("Euros", Currency::EUR);
    let shop = ledger.row(current, "2026-11-03", -5_000, None);
    ledger.split(
        shop,
        &[(-3_000, Some("food.groceries")), (-2_000, Some("home"))],
    );
    let dinner = ledger.row(current, "2026-11-06", -4_000, Some("food.eating_out"));
    ledger.share(dinner, -2_000);
    let takeaway = ledger.row(current, "2026-11-07", -3_000, Some("food.takeaway"));
    ledger.claim_back(takeaway);
    ledger.row(euros, "2026-11-12", -1_234, Some("food.groceries"));
    let shoes = ledger.row(current, "2026-10-28", -6_000, Some("shopping.clothing"));
    let back = ledger.row(current, "2026-11-04", 6_000, None);
    ledger.refund(back, shoes, 6_000);
    // Another month's spending doesn't count.
    ledger.row(current, "2026-10-15", -9_999, Some("food.groceries"));

    set(&mut ledger, "food", None, gbp(20_000), BudgetGroup::Flex);
    // November's own budget overrides the standing one.
    set(
        &mut ledger,
        "food",
        Some("2026-11"),
        gbp(25_000),
        BudgetGroup::Fixed,
    );
    set(
        &mut ledger,
        "food.groceries",
        None,
        gbp(2_000),
        BudgetGroup::Flex,
    );
    set(
        &mut ledger,
        "shopping",
        None,
        gbp(5_000),
        BudgetGroup::NonMonthly,
    );
    set(
        &mut ledger,
        "food.eating_out",
        None,
        eur(1_000),
        BudgetGroup::Flex,
    );
    // December's budget isn't November's.
    set(
        &mut ledger,
        "health",
        Some("2026-12"),
        gbp(100),
        BudgetGroup::Flex,
    );

    let status = budget_status(&ledger.reader(), "2026-11".parse().unwrap()).unwrap();
    assert_eq!(status.period.key(), "2026-11");
    let lines: Vec<_> = status
        .budgets
        .iter()
        .map(|line| {
            (
                line.category.as_str(),
                line.group,
                line.rollover,
                line.for_month,
                line.budget,
                line.spent,
                line.remaining,
                line.spent_other_currencies.iter().collect::<Vec<_>>(),
            )
        })
        .collect();
    assert_eq!(
        lines,
        [
            // The group's budget covers its subcategories: groceries 30.00
            // and the dinner's 20.00 share; the claim counts 0, the euros
            // apart.
            (
                "food",
                BudgetGroup::Fixed,
                false,
                true,
                gbp(25_000),
                gbp(5_000),
                gbp(20_000),
                vec![eur(1_234)],
            ),
            // Over budget.
            (
                "food.groceries",
                BudgetGroup::Flex,
                false,
                false,
                gbp(2_000),
                gbp(3_000),
                gbp(-1_000),
                vec![eur(1_234)],
            ),
            // A euro budget: the pounds are reported, not counted.
            (
                "food.eating_out",
                BudgetGroup::Flex,
                false,
                false,
                eur(1_000),
                eur(0),
                eur(1_000),
                vec![gbp(2_000)],
            ),
            // The refund of October's shoes, on its own date.
            (
                "shopping",
                BudgetGroup::NonMonthly,
                true,
                false,
                gbp(5_000),
                gbp(-6_000),
                gbp(11_000),
                vec![],
            ),
        ]
    );
    let tree = ledger.db.categories().unwrap();
    assert_eq!(
        status.budgets[0].label,
        tree.get(&key("food")).unwrap().label()
    );
}

#[test]
fn no_budgets_no_lines() {
    let ledger = TestLedger::new();
    let status = budget_status(&ledger.reader(), "2026-11".parse().unwrap()).unwrap();
    assert!(status.budgets.is_empty());
}
