//! Accounts management (ROADMAP M3.2; `docs/DESIGN.md` → "Accounts panel";
//! `docs/ARCHITECTURE.md` → "Decision record: accounts management").
//!
//! - [`panel`]: the left panel of the money views: net worth, the accounts
//!   with their pots nested under them, and the categories.
//! - [`inspector`]: the account inspector, shown on Transactions while an
//!   account is selected and no transaction is: its details (name, bank,
//!   kind, currency, preset), entered balances, pots and imports.
//! - [`new_account`] and [`add_balance`]: the New account and Add balance
//!   dialogs.
//!
//! [`Accounts`] holds what they show and which account is selected, and
//! makes every change through [`store::AccountStore`], which opens the
//! database for each call; core decides what's allowed. Screens drawn
//! without a database (snapshots, tests) use [`fixture`] data, which can't
//! change.

pub mod add_balance;
pub mod data;
pub mod fixture;
pub mod inspector;
pub mod new_account;
pub mod panel;
pub mod store;

/// Binds the accounts panel's keys. Call once, before the first panel.
pub fn init(cx: &mut gpui_kit::App) {
    use gpui_kit::KeyBinding;
    use gpui_kit::base::actions::{Cancel, SelectDown, SelectUp};
    cx.bind_keys([
        KeyBinding::new("up", SelectUp, Some(panel::CONTEXT)),
        KeyBinding::new("down", SelectDown, Some(panel::CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(panel::CONTEXT)),
    ]);
}

use coffer_core::account::{AccountEdit, EnteredBalance, NewAccount};
use coffer_core::id::AccountId;
use coffer_core::reporting::account_tree::AccountNode;
use gpui_kit::{Context, EventEmitter, SharedString};

use data::AccountsData;
use store::AccountStore;

/// Today, where the computer's clock can be read.
fn today() -> chrono::NaiveDate {
    coffer_core::schedule::local_today().unwrap_or_else(|_| fixture::today())
}

/// Where the accounts come from.
enum Source {
    /// The database: what `coffer` opens.
    Database(AccountStore),
    /// Sample data that can't change.
    Fixture,
}

/// Something the panel or inspector asked for that the shell carries out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountsEvent {
    /// The selected account changed (`None`: all accounts).
    Selected(Option<AccountId>),
    /// The `+` on the Accounts section: open New account.
    NewAccountRequested,
    /// "Add balance…" in the inspector: open Add balance for the account.
    AddBalanceRequested(AccountId),
}

/// A dialog is done: the change was saved, or the user cancelled. The
/// shell closes it and puts focus back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DialogClosed;

/// The accounts the app shows, and which one is selected.
pub struct Accounts {
    source: Source,
    data: AccountsData,
    selected: Option<AccountId>,
    /// Why the accounts couldn't be read, shown in the panel.
    load_error: Option<SharedString>,
}

impl EventEmitter<AccountsEvent> for Accounts {}

impl Accounts {
    /// The accounts in `store`, read now.
    pub fn from_store(store: AccountStore) -> Self {
        let mut accounts = Self {
            data: AccountsData::empty(Vec::new(), today()),
            source: Source::Database(store),
            selected: None,
            load_error: None,
        };
        accounts.load();
        accounts
    }

    /// No accounts, because they can't be read: `reason` shows in the
    /// panel.
    pub fn unavailable(reason: impl Into<SharedString>) -> Self {
        Self {
            source: Source::Fixture,
            data: AccountsData::empty(Vec::new(), today()),
            selected: None,
            load_error: Some(reason.into()),
        }
    }

    /// Sample data, which can't change.
    pub fn fixture(data: AccountsData) -> Self {
        Self {
            source: Source::Fixture,
            data,
            selected: None,
            load_error: None,
        }
    }

    pub fn data(&self) -> &AccountsData {
        &self.data
    }

    pub fn selected(&self) -> Option<AccountId> {
        self.selected
    }

    /// The selected account, if it's still open.
    pub fn selected_node(&self) -> Option<&AccountNode> {
        self.selected.and_then(|id| self.data.node(id))
    }

    pub fn load_error(&self) -> Option<&SharedString> {
        self.load_error.as_ref()
    }

    /// Selects `account` (`None`: all accounts).
    pub fn select(&mut self, account: Option<AccountId>, cx: &mut Context<Self>) {
        if self.selected == account {
            return;
        }
        self.selected = account;
        cx.emit(AccountsEvent::Selected(account));
        cx.notify();
    }

    /// Selects `account`, or all accounts if it's selected already: what a
    /// click on a panel row does.
    pub fn toggle(&mut self, account: AccountId, cx: &mut Context<Self>) {
        let next = (self.selected != Some(account)).then_some(account);
        self.select(next, cx);
    }

    /// Moves the selection `step` rows down the panel (up when negative),
    /// stopping at the ends; with nothing selected, down selects the first
    /// row and up the last. The arrow keys in the panel.
    pub fn select_next(&mut self, step: isize, cx: &mut Context<Self>) {
        let rows: Vec<AccountId> = panel::order(&self.data);
        if rows.is_empty() {
            return;
        }
        let next = match self
            .selected
            .and_then(|id| rows.iter().position(|row| *row == id))
        {
            Some(at) => at.saturating_add_signed(step).min(rows.len() - 1),
            None if step < 0 => rows.len() - 1,
            None => 0,
        };
        self.select(Some(rows[next]), cx);
    }

    /// Asks the shell to open New account.
    pub fn request_new_account(&mut self, cx: &mut Context<Self>) {
        cx.emit(AccountsEvent::NewAccountRequested);
    }

    /// Asks the shell to open Add balance for `account`.
    pub fn request_add_balance(&mut self, account: AccountId, cx: &mut Context<Self>) {
        cx.emit(AccountsEvent::AddBalanceRequested(account));
    }

    /// Reads the accounts again.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.load();
        cx.notify();
    }

    fn load(&mut self) {
        let Source::Database(store) = &self.source else {
            return;
        };
        match store.load(store::bundled_presets()) {
            Ok(data) => {
                self.data = data;
                self.load_error = None;
            }
            Err(error) => {
                tracing::warn!(%error, "couldn't read the accounts");
                self.load_error = Some(error.to_string().into());
            }
        }
        if self.selected.is_some_and(|id| !self.data.contains(id)) {
            self.selected = None;
        }
    }

    fn store(&self) -> Result<&AccountStore, SharedString> {
        match &self.source {
            Source::Database(store) => Ok(store),
            Source::Fixture => Err("This is sample data, which can't be changed.".into()),
        }
    }

    /// Adds an account (and its first balance), then selects it. The error
    /// says why it wasn't added.
    pub fn create(
        &mut self,
        account: &NewAccount,
        balance: Option<&EnteredBalance>,
        cx: &mut Context<Self>,
    ) -> Result<AccountId, SharedString> {
        let id = self
            .store()?
            .create(account, balance)
            .map_err(|error| SharedString::from(error.to_string()))?;
        self.reload(cx);
        self.select(Some(id), cx);
        Ok(id)
    }

    /// Changes an account's details. The error says why they weren't.
    pub fn update(
        &mut self,
        account: AccountId,
        edit: &AccountEdit,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        self.store()?
            .update(account, edit)
            .map_err(|error| SharedString::from(error.to_string()))?;
        self.reload(cx);
        Ok(())
    }

    /// Records a balance for `account`. The error says why it wasn't.
    pub fn add_balance(
        &mut self,
        account: AccountId,
        balance: &EnteredBalance,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        self.store()?
            .add_balance(account, balance)
            .map_err(|error| SharedString::from(error.to_string()))?;
        self.reload(cx);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
