//! Reading and changing transactions in the database (`docs/ARCHITECTURE.md`
//! → "Decision record: the Transactions view").
//!
//! Like [`crate::accounts::store::AccountStore`], the store holds no
//! connection: each call opens the database at its path (creating and
//! migrating it as the CLI does) after checking the restore lock. Reading
//! the register runs on a background thread; a category change is one small
//! write on the main thread.
//!
//! Every change is core's: [`own::recategorize`] sets the categories as the
//! user's, in one DB transaction recorded as one undoable change batch (the
//! CLI's `coffer categorize` takes the same path), and [`change::undo`]
//! reverts it. Nothing here touches an imported source field.

use std::path::PathBuf;

use chrono::NaiveDate;
use coffer_core::backup;
use coffer_core::category::CategoryKey;
use coffer_core::change::{self, UndoSummary};
use coffer_core::db::Database;
use coffer_core::id::{ChangeBatchId, TransactionId};
use coffer_core::inbox::own;
use coffer_core::reporting::register::Register;

use crate::accounts::store::StoreError;

/// A restore is replacing the database.
#[derive(Debug)]
struct Restoring;

impl std::fmt::Display for Restoring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("A backup is being restored; try again when it's done.")
    }
}

impl std::error::Error for Restoring {}

/// The transactions in the database at `database`.
#[derive(Clone, Debug)]
pub struct TransactionStore {
    database: PathBuf,
}

impl TransactionStore {
    pub fn new(database: PathBuf) -> Self {
        Self { database }
    }

    fn open(&self) -> Result<Database, StoreError> {
        if backup::restoring(&self.database)? {
            return Err(Box::new(Restoring));
        }
        Ok(Database::open(&self.database)?)
    }

    /// Every transaction, with the colours ranked over the 12 months to
    /// `today`.
    pub fn load(&self, today: NaiveDate) -> Result<Register, StoreError> {
        let db = self.open()?;
        Ok(Register::load(&db.reader()?, today)?)
    }

    /// Sets the category of each transaction as the user's: one undoable
    /// change batch, `None` when nothing changed.
    pub fn recategorize(
        &self,
        items: &[(TransactionId, CategoryKey)],
    ) -> Result<Option<ChangeBatchId>, StoreError> {
        let mut db = self.open()?;
        Ok(own::recategorize(&mut db, items)?)
    }

    /// Reverts a change batch where its values still hold.
    pub fn undo(&self, batch: ChangeBatchId) -> Result<UndoSummary, StoreError> {
        let mut db = self.open()?;
        Ok(change::undo(&mut db, batch)?)
    }
}

#[cfg(test)]
mod tests {
    use coffer_core::account::{self, AccountKind, NewAccount};
    use coffer_core::manual::{self, ManualTransaction};
    use coffer_core::money::{Currency, Money};
    use coffer_core::reporting::register::RowCategory;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn categorises_reads_back_and_undoes() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("coffer.db");
        let id = {
            let mut db = Database::open(&path).unwrap();
            let current = account::create(
                &mut db,
                &NewAccount::new("Current account", AccountKind::Current, Currency::GBP),
            )
            .unwrap();
            let day = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
            manual::create(
                &mut db,
                &ManualTransaction::new(current, day, Money::new(-420, Currency::GBP), "COFFEE"),
            )
            .unwrap()
        };
        let store = TransactionStore::new(path);
        let today = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
        assert_eq!(
            store.load(today).unwrap().row(id).unwrap().category,
            RowCategory::Uncategorised
        );

        let key = CategoryKey::new("food.eating_out").unwrap();
        let batch = store.recategorize(&[(id, key.clone())]).unwrap().unwrap();
        let register = store.load(today).unwrap();
        let RowCategory::Category(category) = register.row(id).unwrap().category else {
            panic!("categorised");
        };
        assert_eq!(register.category(category).unwrap().key, key);
        // The same category again changes nothing.
        assert_eq!(store.recategorize(&[(id, key)]).unwrap(), None);

        store.undo(batch).unwrap();
        assert_eq!(
            store.load(today).unwrap().row(id).unwrap().category,
            RowCategory::Uncategorised
        );
    }
}
