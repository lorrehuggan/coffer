//! What the accounts panel and inspector show: the account tree from core
//! ([`coffer_core::reporting::account_tree`]) and, per account, what the
//! inspector lists. Plain data, read in one go by [`super::store`] or built
//! by [`super::fixture`].

use std::collections::BTreeMap;

use chrono::NaiveDate;
use coffer_core::account::{StoredBalance, Usage};
use coffer_core::id::AccountId;
use coffer_core::import::BatchInfo;
use coffer_core::reporting::account_tree::{AccountNode, AccountTree};

/// A CSV preset the Preset select offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresetChoice {
    /// Its stable ID, stored on the account (`uk-monzo`).
    pub id: String,
    /// What the select shows ("Monzo").
    pub name: String,
}

/// The inspector's lists for one account.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccountDetail {
    /// What it holds, which decides what about it can change.
    pub usage: Usage,
    /// Balances the user entered, and the opening balance, newest first.
    pub balances: Vec<StoredBalance>,
    /// Its committed imports, newest first.
    pub imports: Vec<BatchInfo>,
}

/// Everything the accounts panel and inspector show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountsData {
    pub tree: AccountTree,
    pub details: BTreeMap<AccountId, AccountDetail>,
    pub presets: Vec<PresetChoice>,
    /// The day the data was read, which decides how dates are written.
    pub today: NaiveDate,
}

impl AccountsData {
    /// No accounts.
    pub fn empty(presets: Vec<PresetChoice>, today: NaiveDate) -> Self {
        Self {
            tree: AccountTree {
                accounts: Vec::new(),
                net_worth: Default::default(),
            },
            details: BTreeMap::new(),
            presets,
            today,
        }
    }

    /// The account with this ID, a pot included, if it's open.
    pub fn node(&self, id: AccountId) -> Option<&AccountNode> {
        self.tree.rows().find(|node| node.account.id == id)
    }

    /// The account a pot belongs to.
    pub fn parent_of(&self, pot: AccountId) -> Option<&AccountNode> {
        self.tree
            .accounts
            .iter()
            .find(|node| node.pots.iter().any(|child| child.account.id == pot))
    }

    pub fn detail(&self, id: AccountId) -> Option<&AccountDetail> {
        self.details.get(&id)
    }

    /// The name of the preset with ID `id`; the ID itself for one this build
    /// doesn't have.
    pub fn preset_name<'a>(&'a self, id: &'a str) -> &'a str {
        self.presets
            .iter()
            .find(|preset| preset.id == id)
            .map_or(id, |preset| preset.name.as_str())
    }

    /// Whether an account selected before a reload is still there.
    pub fn contains(&self, id: AccountId) -> bool {
        self.node(id).is_some()
    }
}
