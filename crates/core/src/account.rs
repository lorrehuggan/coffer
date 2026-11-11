//! Accounts, and the balances the user enters for them.
//!
//! Just enough for imports (M1.11–M1.13): creating, reading and listing
//! accounts, and recording an entered balance, the anchor an account whose
//! exports have no balances is checked against (`docs/ARCHITECTURE.md` →
//! "Decision record: import commit, dedupe and undo"). Editing, closing and
//! pots come with M1.17 and M3.2.

use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::db::Database;
use crate::id::{AccountId, BalanceId};
use crate::money::{Currency, Money};

/// What an account is (`account.kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AccountKind {
    Current,
    Savings,
    CreditCard,
    /// A Monzo Pot, Starling Space or Revolut Vault: a child of a current or
    /// savings account.
    Pot,
    Cash,
    /// A manually valued asset (a house, a pension): valuations only.
    Asset,
    /// A manually valued liability (a mortgage, a loan): valuations only.
    Liability,
}

impl AccountKind {
    pub const ALL: [AccountKind; 7] = [
        AccountKind::Current,
        AccountKind::Savings,
        AccountKind::CreditCard,
        AccountKind::Pot,
        AccountKind::Cash,
        AccountKind::Asset,
        AccountKind::Liability,
    ];

    /// The key stored in `account.kind`.
    pub const fn as_str(self) -> &'static str {
        match self {
            AccountKind::Current => "current",
            AccountKind::Savings => "savings",
            AccountKind::CreditCard => "credit_card",
            AccountKind::Pot => "pot",
            AccountKind::Cash => "cash",
            AccountKind::Asset => "asset",
            AccountKind::Liability => "liability",
        }
    }

    pub fn from_key(key: &str) -> Option<AccountKind> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == key)
    }

    /// Whether the account holds transactions. `asset` and `liability`
    /// accounts hold valuations instead.
    pub const fn holds_transactions(self) -> bool {
        !matches!(self, AccountKind::Asset | AccountKind::Liability)
    }
}

/// An account to create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewAccount {
    pub name: String,
    pub institution: Option<String>,
    pub kind: AccountKind,
    pub currency: Currency,
    /// The parent of a pot; `None` for every other kind.
    pub parent: Option<AccountId>,
}

impl NewAccount {
    /// An account with no institution and no parent.
    pub fn new(name: impl Into<String>, kind: AccountKind, currency: Currency) -> Self {
        NewAccount {
            name: name.into(),
            institution: None,
            kind,
            currency,
            parent: None,
        }
    }
}

/// A stored account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub id: AccountId,
    pub parent: Option<AccountId>,
    pub name: String,
    pub institution: Option<String>,
    pub kind: AccountKind,
    pub currency: Currency,
    /// `account.opening_balance_minor` on `opening_date`. Checked like any
    /// other balance, as the end of that day.
    pub opening_balance: Option<(NaiveDate, Money)>,
    /// History up to and including this date is locked: imports and undos
    /// leave it alone until the user unlocks (M1.27).
    pub reconciled_through: Option<NaiveDate>,
    pub closed_on: Option<NaiveDate>,
}

/// Where an entered balance comes from (`balance_snapshot.source`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EnteredSource {
    /// Typed in by the user, e.g. read off the bank's app or a cash count.
    Manual,
    /// Read off a paper or PDF statement.
    Statement,
}

impl EnteredSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            EnteredSource::Manual => "manual",
            EnteredSource::Statement => "statement",
        }
    }
}

/// A balance the user enters: the account's balance at the end of `date`.
///
/// It is an assertion like any other (`docs/ARCHITECTURE.md` → "Decision
/// record: review queues"): the account's earliest balance is the starting
/// point, and every later one is checked against it plus the transactions
/// between them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnteredBalance {
    pub date: NaiveDate,
    pub balance: Money,
    pub source: EnteredSource,
    /// The user's note; not sent over MCP by default.
    pub note: Option<String>,
}

/// Why an account operation failed. Messages never contain transaction
/// text.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("there is no account {0}")]
    NotFound(AccountId),

    #[error("the balance is in {found}, but the account is in {expected}")]
    CurrencyMismatch { expected: Currency, found: Currency },

    #[error(
        "{0} accounts hold valuations, not balances checked against transactions; \
         record a valuation instead"
    )]
    HoldsValuations(&'static str),

    #[error("the account already has an entered balance on {0}; edit that one instead")]
    BalanceExists(NaiveDate),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// Creates an account. The schema checks the rest: a non-blank name, and a
/// pot under a current or savings account in its currency.
pub fn create(db: &mut Database, account: &NewAccount) -> Result<AccountId, Error> {
    let conn = db.conn();
    conn.execute(
        "INSERT INTO account (parent_id, name, institution, kind, currency) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            account.parent,
            account.name,
            account.institution,
            account.kind.as_str(),
            account.currency,
        ],
    )?;
    Ok(AccountId::new(conn.last_insert_rowid()))
}

/// The account with this ID.
pub fn get(db: &Database, id: AccountId) -> Result<Account, Error> {
    load(db.conn(), id)
}

/// Every account, in the order they were created.
pub fn list(db: &Database) -> Result<Vec<Account>, Error> {
    let mut statement = db
        .conn()
        .prepare(&format!("SELECT {COLUMNS} FROM account ORDER BY id"))?;
    let accounts = statement
        .query_map([], from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(accounts)
}

/// Records a balance the user entered: the anchor for an account whose
/// exports have no balances, or one more check for one whose exports do.
/// One entered balance per account and day.
pub fn add_balance(
    db: &mut Database,
    account: AccountId,
    balance: &EnteredBalance,
) -> Result<BalanceId, Error> {
    let tx = db.conn_mut().transaction()?;
    let stored = load(&tx, account)?;
    if !stored.kind.holds_transactions() {
        return Err(Error::HoldsValuations(stored.kind.as_str()));
    }
    if balance.balance.currency() != stored.currency {
        return Err(Error::CurrencyMismatch {
            expected: stored.currency,
            found: balance.balance.currency(),
        });
    }
    let date = date_key(balance.date);
    let exists = tx
        .query_row(
            "SELECT 1 FROM balance_snapshot \
             WHERE account_id = ?1 AND date = ?2 AND batch_id IS NULL",
            params![account, date],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if exists {
        return Err(Error::BalanceExists(balance.date));
    }
    tx.execute(
        "INSERT INTO balance_snapshot (account_id, date, balance_minor, source, note) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            account,
            date,
            balance.balance.minor(),
            balance.source.as_str(),
            balance.note,
        ],
    )?;
    let id = BalanceId::new(tx.last_insert_rowid());
    tx.commit()?;
    Ok(id)
}

const COLUMNS: &str = "id, parent_id, name, institution, kind, currency, \
                       opening_balance_minor, opening_date, reconciled_through, closed_on";

/// The account with this ID, `None` when there is none.
pub(crate) fn find(conn: &Connection, id: AccountId) -> rusqlite::Result<Option<Account>> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM account WHERE id = ?1"),
        [id],
        from_row,
    )
    .optional()
}

fn load(conn: &Connection, id: AccountId) -> Result<Account, Error> {
    find(conn, id)?.ok_or(Error::NotFound(id))
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<Account> {
    let kind: String = row.get("kind")?;
    let kind = AccountKind::from_key(&kind).ok_or_else(|| invalid(row, "kind"))?;
    let currency: Currency = row.get("currency")?;
    let opening_minor: Option<i64> = row.get("opening_balance_minor")?;
    let opening_date = optional_date(row, "opening_date")?;
    Ok(Account {
        id: row.get("id")?,
        parent: row.get("parent_id")?,
        name: row.get("name")?,
        institution: row.get("institution")?,
        kind,
        currency,
        opening_balance: opening_date
            .zip(opening_minor)
            .map(|(date, minor)| (date, Money::new(minor, currency))),
        reconciled_through: optional_date(row, "reconciled_through")?,
        closed_on: optional_date(row, "closed_on")?,
    })
}

/// The `YYYY-MM-DD` form every date column holds.
pub(crate) fn date_key(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Reads a `YYYY-MM-DD` column. The schema checks the form, so a failure
/// means the file was changed outside Coffer.
pub(crate) fn date(row: &Row<'_>, column: &str) -> rusqlite::Result<NaiveDate> {
    let text: String = row.get(column)?;
    NaiveDate::parse_from_str(&text, "%Y-%m-%d").map_err(|_| invalid(row, column))
}

pub(crate) fn optional_date(row: &Row<'_>, column: &str) -> rusqlite::Result<Option<NaiveDate>> {
    let text: Option<String> = row.get(column)?;
    text.map(|text| NaiveDate::parse_from_str(&text, "%Y-%m-%d").map_err(|_| invalid(row, column)))
        .transpose()
}

/// A stored value outside the schema's own CHECKs.
pub(crate) fn invalid(row: &Row<'_>, column: &str) -> rusqlite::Error {
    let index = row.as_ref().column_index(column).unwrap_or_default();
    rusqlite::Error::InvalidColumnType(index, column.to_owned(), rusqlite::types::Type::Text)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn open() -> (Database, TempDir) {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("coffer.db")).unwrap();
        (db, dir)
    }

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    #[test]
    fn kind_keys_match_the_schema() {
        let keys = AccountKind::ALL.map(AccountKind::as_str);
        assert_eq!(
            keys,
            [
                "current",
                "savings",
                "credit_card",
                "pot",
                "cash",
                "asset",
                "liability"
            ]
        );
        for kind in AccountKind::ALL {
            assert_eq!(AccountKind::from_key(kind.as_str()), Some(kind));
        }
        assert_eq!(AccountKind::from_key("loan"), None);
    }

    #[test]
    fn creates_reads_and_lists() {
        let (mut db, _dir) = open();
        let mut new = NewAccount::new("Joint", AccountKind::Current, Currency::GBP);
        new.institution = Some("Example Bank".into());
        let current = create(&mut db, &new).unwrap();
        let mut pot = NewAccount::new("Holiday", AccountKind::Pot, Currency::GBP);
        pot.parent = Some(current);
        let pot = create(&mut db, &pot).unwrap();

        let account = get(&db, current).unwrap();
        assert_eq!(account.name, "Joint");
        assert_eq!(account.institution.as_deref(), Some("Example Bank"));
        assert_eq!(account.kind, AccountKind::Current);
        assert_eq!(account.currency, Currency::GBP);
        assert_eq!(account.opening_balance, None);
        assert_eq!(account.reconciled_through, None);
        assert_eq!(get(&db, pot).unwrap().parent, Some(current));

        let ids: Vec<_> = list(&db).unwrap().into_iter().map(|a| a.id).collect();
        assert_eq!(ids, [current, pot]);
        assert!(matches!(
            get(&db, AccountId::new(99)),
            Err(Error::NotFound(id)) if id == AccountId::new(99)
        ));
    }

    #[test]
    fn the_schema_refuses_a_bad_account() {
        let (mut db, _dir) = open();
        let blank = NewAccount::new("  ", AccountKind::Current, Currency::GBP);
        assert!(matches!(create(&mut db, &blank), Err(Error::Database(_))));
        let orphan_pot = NewAccount::new("Pot", AccountKind::Pot, Currency::GBP);
        assert!(matches!(
            create(&mut db, &orphan_pot),
            Err(Error::Database(_))
        ));
    }

    #[test]
    fn entered_balances_are_one_per_day_in_the_account_currency() {
        let (mut db, _dir) = open();
        let account = create(
            &mut db,
            &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        let balance = |date, minor, currency| EnteredBalance {
            date,
            balance: Money::new(minor, currency),
            source: EnteredSource::Manual,
            note: None,
        };

        add_balance(&mut db, account, &balance(day(1), 10_000, Currency::GBP)).unwrap();
        assert!(matches!(
            add_balance(&mut db, account, &balance(day(1), 12_000, Currency::GBP)),
            Err(Error::BalanceExists(date)) if date == day(1)
        ));
        assert!(matches!(
            add_balance(&mut db, account, &balance(day(2), 1, Currency::EUR)),
            Err(Error::CurrencyMismatch { .. })
        ));
        let mut statement = balance(day(2), 9_000, Currency::GBP);
        statement.source = EnteredSource::Statement;
        add_balance(&mut db, account, &statement).unwrap();

        let house = create(
            &mut db,
            &NewAccount::new("House", AccountKind::Asset, Currency::GBP),
        )
        .unwrap();
        assert!(matches!(
            add_balance(&mut db, house, &balance(day(1), 1, Currency::GBP)),
            Err(Error::HoldsValuations("asset"))
        ));
        assert!(matches!(
            add_balance(
                &mut db,
                AccountId::new(99),
                &balance(day(1), 1, Currency::GBP)
            ),
            Err(Error::NotFound(_))
        ));
    }
}
