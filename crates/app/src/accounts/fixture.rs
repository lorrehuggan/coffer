//! Sample accounts for the screenshots and tests that run without a
//! database: the accounts on the D6.1 artboards (Transactions `18E-0`,
//! Accounts panel · states `8D8-0`). Names, banks, files and amounts are
//! made up. The real app never shows them: `coffer` reads the database.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use coffer_core::account::{Account, AccountKind, StoredBalance, Usage};
use coffer_core::id::{AccountId, BalanceId, BatchId};
use coffer_core::import::{BatchInfo, BatchStatus};
use coffer_core::money::{Currency, Money};
use coffer_core::reconciliation::{BalanceSource, Status};
use coffer_core::reporting::Totals;
use coffer_core::reporting::account_tree::{AccountNode, AccountTree};
use coffer_core::reporting::balance::{AccountBalance, Anchor};
use coffer_core::statement::SourceFormat;

use super::data::{AccountDetail, AccountsData, PresetChoice};

/// The Current account's ID, the account the inspector artboard selects.
pub const CURRENT: AccountId = AccountId::new(1);
/// The Joint account, overdrawn (and flagged on the states board).
pub const JOINT: AccountId = AccountId::new(4);
/// Amex, with no balance (states board only).
pub const AMEX: AccountId = AccountId::new(7);

/// The day the artboards are drawn on.
pub fn today() -> NaiveDate {
    date(14)
}

fn date(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, day).expect("a November date")
}

fn october_31() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 31).expect("a date")
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn account(id: i64, name: &str, kind: AccountKind, parent: Option<AccountId>) -> Account {
    Account {
        id: AccountId::new(id),
        parent,
        name: name.to_owned(),
        institution: None,
        kind,
        currency: Currency::GBP,
        opening_balance: None,
        reconciled_through: None,
        closed_on: None,
        preset: None,
    }
}

fn node(account: Account, balance: Option<(i64, NaiveDate)>) -> AccountNode {
    let id = account.id;
    AccountNode {
        account,
        latest: balance.map(|(minor, day)| AccountBalance {
            account: id,
            date: day,
            balance: gbp(minor),
            anchor: Anchor::Stated { date: date(1) },
        }),
        status: Status::Reconciled,
        pots: Vec::new(),
    }
}

/// The accounts in the left panel of the Transactions artboards (`18E-0`):
/// Current account with its two pots, Joint account overdrawn, Savings and
/// Stocks ISA. Net worth £18,240.55. Current account has the inspector's
/// details (`8D8-0`): Monzo, preset, an entered balance, two imports.
pub fn accounts() -> AccountsData {
    let mut current = account(1, "Current account", AccountKind::Current, None);
    current.institution = Some("Monzo".to_owned());
    current.preset = Some("uk-monzo".to_owned());
    let mut current = node(current, Some((128_410, date(9))));
    current.pots = vec![
        node(
            account(2, "Holiday", AccountKind::Pot, Some(CURRENT)),
            Some((120_000, date(9))),
        ),
        node(
            account(3, "Rainy day", AccountKind::Pot, Some(CURRENT)),
            Some((300_000, date(9))),
        ),
    ];
    let mut joint = account(4, "Joint account", AccountKind::Current, None);
    joint.institution = Some("Monzo".to_owned());
    let joint = node(joint, Some((-2_410, date(9))));
    let savings = node(
        account(5, "Savings", AccountKind::Savings, None),
        Some((575_000, october_31())),
    );
    let isa = node(
        account(6, "Stocks ISA", AccountKind::Savings, None),
        Some((703_055, october_31())),
    );
    let accounts = vec![current, joint, savings, isa];

    let mut net_worth = Totals::new();
    let mut details = BTreeMap::new();
    for node in accounts
        .iter()
        .flat_map(|node| std::iter::once(node).chain(&node.pots))
    {
        if let Some(latest) = node.latest {
            net_worth
                .add(latest.balance)
                .expect("the fixture's sums fit");
        }
        details.insert(
            node.account.id,
            AccountDetail {
                usage: Usage {
                    transactions: 1,
                    is_pot: node.account.kind == AccountKind::Pot,
                    pots: node.pots.len(),
                    ..Usage::default()
                },
                ..AccountDetail::default()
            },
        );
    }
    let current = details.get_mut(&CURRENT).expect("the current account");
    current.usage = Usage {
        transactions: 577,
        balances: 1,
        pots: 2,
        ..Usage::default()
    };
    current.balances = vec![StoredBalance {
        id: Some(BalanceId::new(1)),
        date: NaiveDate::from_ymd_opt(2026, 9, 1).expect("a date"),
        balance: gbp(107_170),
        source: BalanceSource::Manual,
        note: None,
    }];
    current.imports = vec![
        batch(2, "monzo-november.csv", 296, "2026-11-09T08:12:00.000Z"),
        batch(1, "monzo-october.csv", 281, "2026-11-02T19:40:00.000Z"),
    ];

    AccountsData {
        tree: AccountTree {
            accounts,
            net_worth,
        },
        details,
        presets: vec![PresetChoice {
            id: "uk-monzo".to_owned(),
            name: "Monzo".to_owned(),
        }],
        today: today(),
    }
}

/// [`accounts`] with the rest of the states board (`8D8-0`): Joint
/// account's balance check failing, and Amex, a credit card with no
/// balance.
pub fn with_states() -> AccountsData {
    let mut data = accounts();
    let joint = data
        .tree
        .accounts
        .iter_mut()
        .find(|node| node.account.id == JOINT)
        .expect("the joint account");
    joint.status = Status::Mismatch;
    let mut amex = account(7, "Amex", AccountKind::CreditCard, None);
    amex.institution = Some("American Express".to_owned());
    data.tree.accounts.push(node(amex, None));
    data.details.insert(AMEX, AccountDetail::default());
    data
}

fn batch(id: i64, file: &str, transactions: usize, at: &str) -> BatchInfo {
    BatchInfo {
        id: BatchId::new(id),
        account: CURRENT,
        source_file: file.to_owned(),
        format: SourceFormat::Csv,
        preset_id: Some("uk-monzo".to_owned()),
        imported_at: at.to_owned(),
        status: BatchStatus::Committed,
        transactions,
    }
}

#[cfg(test)]
mod tests {
    use crate::format::{Sign, money};

    use super::*;

    #[test]
    fn net_worth_matches_the_artboards() {
        let data = accounts();
        let total = data.tree.net_worth.get(Currency::GBP).unwrap();
        assert_eq!(money(total, Sign::Negative), "£18,240.55");
        let names: Vec<_> = data
            .tree
            .rows()
            .map(|node| node.account.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "Current account",
                "Holiday",
                "Rainy day",
                "Joint account",
                "Savings",
                "Stocks ISA"
            ]
        );
        assert!(data.node(JOINT).unwrap().is_overdrawn());
        assert!(with_states().node(AMEX).unwrap().latest.is_none());
    }
}
