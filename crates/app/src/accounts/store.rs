//! Reading and changing accounts in the database (`docs/ARCHITECTURE.md` →
//! "Decision record: accounts management").
//!
//! Like the window state ([`crate::shell::window_state`]) and the MCP
//! server, the app holds no connection between reads and writes: each call
//! opens the database at the path it was given, creating and migrating it
//! as the CLI does, and checks the restore lock first, so a restore can swap
//! the file. Reads and writes are small (a person's accounts) and run on the
//! main thread.
//!
//! All the business rules are core's ([`coffer_core::account`]): this only
//! opens the database and reads what the panel and inspector show.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;

use coffer_core::account::{self, AccountEdit, EnteredBalance, NewAccount};
use coffer_core::backup;
use coffer_core::db::Database;
use coffer_core::id::AccountId;
use coffer_core::import::{self, BatchStatus};
use coffer_core::reporting::account_tree::account_tree;
use coffer_core::schedule;
use coffer_import::preset::Presets;

use super::data::{AccountDetail, AccountsData, PresetChoice};

/// Why a read or write didn't happen. Its message is shown to the user, and
/// never holds transaction text: core's errors don't.
pub type StoreError = Box<dyn Error + Send + Sync>;

/// A restore is replacing the database.
#[derive(Debug)]
struct Restoring;

impl fmt::Display for Restoring {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("A backup is being restored; try again when it's done.")
    }
}

impl Error for Restoring {}

/// The bundled CSV presets, as the Preset select offers them. Empty (with a
/// warning) if they don't load, which a test rules out.
pub fn bundled_presets() -> Vec<PresetChoice> {
    match Presets::bundled() {
        Ok(presets) => presets
            .iter()
            .map(|preset| PresetChoice {
                id: preset.id().to_owned(),
                name: preset.name().to_owned(),
            })
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "the bundled presets don't load");
            Vec::new()
        }
    }
}

/// The accounts in the database at `database`.
#[derive(Clone, Debug)]
pub struct AccountStore {
    database: PathBuf,
}

impl AccountStore {
    pub fn new(database: PathBuf) -> Self {
        Self { database }
    }

    fn open(&self) -> Result<Database, StoreError> {
        if backup::restoring(&self.database)? {
            return Err(Box::new(Restoring));
        }
        Ok(Database::open(&self.database)?)
    }

    /// Every open account with what the panel and inspector show.
    pub fn load(&self, presets: Vec<PresetChoice>) -> Result<AccountsData, StoreError> {
        let db = self.open()?;
        let tree = account_tree(&db.reader()?)?;
        let mut details = BTreeMap::new();
        for node in tree.rows() {
            let id = node.account.id;
            let mut imports: Vec<_> = import::batches(&db, id)?
                .into_iter()
                .filter(|batch| batch.status == BatchStatus::Committed)
                .collect();
            imports.reverse();
            details.insert(
                id,
                AccountDetail {
                    usage: account::usage(&db, id)?,
                    balances: account::entered_balances(&db, id)?,
                    imports,
                },
            );
        }
        Ok(AccountsData {
            tree,
            details,
            presets,
            today: schedule::local_today()?,
        })
    }

    /// Adds an account, with its first balance when given.
    pub fn create(
        &self,
        account: &NewAccount,
        balance: Option<&EnteredBalance>,
    ) -> Result<AccountId, StoreError> {
        let mut db = self.open()?;
        Ok(account::create_with_balance(&mut db, account, balance)?)
    }

    /// Changes an account's name, bank, kind, currency or preset.
    pub fn update(&self, id: AccountId, edit: &AccountEdit) -> Result<(), StoreError> {
        let mut db = self.open()?;
        Ok(account::update(&mut db, id, edit)?)
    }

    /// Records a balance the user entered.
    pub fn add_balance(&self, id: AccountId, balance: &EnteredBalance) -> Result<(), StoreError> {
        let mut db = self.open()?;
        account::add_balance(&mut db, id, balance)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use coffer_core::account::{AccountKind, EnteredSource};
    use coffer_core::money::{Currency, Money};
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn the_bundled_presets_load() {
        let presets = bundled_presets();
        assert!(presets.iter().any(|preset| preset.id == "uk-monzo"));
    }

    #[test]
    fn creates_edits_and_reads_back_accounts() {
        let dir = TempDir::new().unwrap();
        let store = AccountStore::new(dir.path().join("coffer.db"));
        assert!(store.load(Vec::new()).unwrap().tree.accounts.is_empty());

        let mut new = NewAccount::new("Current account", AccountKind::Current, Currency::GBP);
        new.preset = Some("uk-monzo".into());
        let balance = EnteredBalance {
            date: NaiveDate::from_ymd_opt(2026, 11, 9).unwrap(),
            balance: Money::new(128_410, Currency::GBP),
            source: EnteredSource::Manual,
            note: None,
        };
        let id = store.create(&new, Some(&balance)).unwrap();

        let data = store.load(bundled_presets()).unwrap();
        let node = data.node(id).unwrap();
        assert_eq!(node.latest.unwrap().balance.minor(), 128_410);
        assert_eq!(data.preset_name("uk-monzo"), "Monzo");
        assert_eq!(data.detail(id).unwrap().balances.len(), 1);

        let mut edit = AccountEdit::of(&node.account);
        edit.name = "Monzo".into();
        store.update(id, &edit).unwrap();
        assert_eq!(
            store
                .load(Vec::new())
                .unwrap()
                .node(id)
                .unwrap()
                .account
                .name,
            "Monzo"
        );

        // A refused write says why.
        edit.name = " ".into();
        let error = store.update(id, &edit).unwrap_err();
        assert_eq!(error.to_string(), "an account needs a name");
    }
}
