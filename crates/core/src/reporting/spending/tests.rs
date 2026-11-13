use crate::exclusion::ExclusionReason;
use crate::money::Currency;

use super::super::test_ledger::TestLedger;
use super::*;

/// `(currency, minor)` per currency.
type Pairs = Vec<(&'static str, i64)>;

fn pairs(totals: &Totals) -> Pairs {
    totals
        .iter()
        .map(|money| {
            let code = match money.currency() {
                Currency::GBP => "GBP",
                Currency::EUR => "EUR",
                other => panic!("unexpected {other}"),
            };
            (code, money.minor())
        })
        .collect()
}

/// `(group key, own, [(subcategory key, spent)], total)`.
type Group<'a> = (&'a str, Pairs, Vec<(&'a str, Pairs)>, Pairs);

#[test]
fn nests_spending_under_groups_in_tree_order() {
    let mut ledger = TestLedger::new();
    let current = ledger.account("Current", Currency::GBP);
    let savings = ledger.account("Savings", Currency::GBP);
    let euros = ledger.account("Euros", Currency::EUR);

    // A split shop: a subcategory, the group itself, and uncategorised.
    let shop = ledger.row(current, "2026-11-03", -5_000, None);
    ledger.split(
        shop,
        &[
            (-3_000, Some("food.groceries")),
            (-1_500, Some("food")),
            (-500, None),
        ],
    );
    // October's shoes, refunded in November: negative spending in
    // November, nothing of it in October's category this month.
    let shoes = ledger.row(current, "2026-10-28", -6_000, Some("shopping.clothing"));
    let back = ledger.row(current, "2026-11-04", 6_000, None);
    ledger.refund(back, shoes, 6_000);
    // Half of a shared dinner.
    let dinner = ledger.row(current, "2026-11-06", -4_000, Some("food.eating_out"));
    ledger.share(dinner, -2_000);
    // A claimed-back takeaway counts 0 and doesn't appear.
    let takeaway = ledger.row(current, "2026-11-07", -3_000, Some("food.takeaway"));
    ledger.claim_back(takeaway);
    // Excluded, a confirmed transfer, income and money in: none of it.
    let disputed = ledger.row(current, "2026-11-08", -1_000, Some("bills.energy"));
    ledger.exclude(disputed, ExclusionReason::Disputed);
    let out = ledger.row(current, "2026-11-09", -10_000, Some("home.rent"));
    let into = ledger.row(savings, "2026-11-09", 10_000, None);
    ledger.transfer(out, into);
    ledger.row(current, "2026-11-10", 150_000, Some("income.salary"));
    ledger.row(current, "2026-11-11", 700, None);
    ledger.row(current, "2026-11-12", -900, Some("transfer.savings"));
    // Euros in groceries, kept apart.
    ledger.row(euros, "2026-11-12", -1_234, Some("food.groceries"));
    // December is another month.
    ledger.row(current, "2026-12-01", -800, Some("health"));

    let spending = spending_by_category(&ledger.reader(), "2026-11".parse().unwrap()).unwrap();
    assert_eq!(spending.period.key(), "2026-11");
    let groups: Vec<Group<'_>> = spending
        .groups
        .iter()
        .map(|group| {
            (
                group.key.as_str(),
                pairs(&group.own),
                group
                    .children
                    .iter()
                    .map(|child| (child.key.as_str(), pairs(&child.spent)))
                    .collect(),
                pairs(&group.total),
            )
        })
        .collect();
    assert_eq!(
        groups,
        [
            (
                "food",
                vec![("GBP", 1_500)],
                vec![
                    ("food.groceries", vec![("EUR", 1_234), ("GBP", 3_000)]),
                    ("food.eating_out", vec![("GBP", 2_000)]),
                ],
                vec![("EUR", 1_234), ("GBP", 6_500)],
            ),
            (
                "shopping",
                vec![],
                vec![("shopping.clothing", vec![("GBP", -6_000)])],
                vec![("GBP", -6_000)],
            ),
        ]
    );
    let tree = ledger.db.categories().unwrap();
    let food = tree.get(&CategoryKey::new("food").unwrap()).unwrap();
    assert_eq!(spending.groups[0].label, food.label());
    assert_eq!(pairs(&spending.uncategorized), [("GBP", 500)]);
    assert_eq!(pairs(&spending.total), [("EUR", 1_234), ("GBP", 1_000)]);
}

#[test]
fn an_empty_month_has_no_groups() {
    let ledger = TestLedger::new();
    let spending = spending_by_category(&ledger.reader(), "2026-11".parse().unwrap()).unwrap();
    assert!(spending.groups.is_empty());
    assert!(spending.uncategorized.is_empty());
    assert!(spending.total.is_empty());
}
