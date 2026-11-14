//! Accounts, and the balances the user enters for them.
//!
//! Creating, reading, listing and editing accounts, and recording an entered
//! balance, the anchor an account whose exports have no balances is checked
//! against (`docs/ARCHITECTURE.md` → "Decision record: import commit, dedupe
//! and undo"). Pots are created by imports ([`crate::sub_account`]);
//! closing comes with M3.17.
//!
//! Editing (M3.2, "Decision record: accounts management") changes the
//! account's own fields only: its name, bank, kind, currency and preset.
//! What the account already holds decides what can change ([`Usage`]): the
//! currency is fixed once it has transactions, balances, pots or a rule
//! with amounts, and the kind can't move where its rows couldn't follow.
//! The schema refuses the same changes; checking first gives an error that
//! says why.

use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::db::Database;
use crate::id::{AccountId, BalanceId};
use crate::money::{Currency, Money};
use crate::reconciliation::BalanceSource;

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

    /// Whether its transactions come from a bank's exports, so it can have
    /// a preset: current, savings and credit card accounts. Cash, asset and
    /// liability accounts are kept by the user, and a pot's rows arrive
    /// with its parent's imports.
    pub const fn is_imported(self) -> bool {
        matches!(
            self,
            AccountKind::Current | AccountKind::Savings | AccountKind::CreditCard
        )
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
    /// The preset its imports use ([`Account::preset`]); only for an
    /// [imported](AccountKind::is_imported) kind.
    pub preset: Option<String>,
}

impl NewAccount {
    /// An account with no institution, parent or preset.
    pub fn new(name: impl Into<String>, kind: AccountKind, currency: Currency) -> Self {
        NewAccount {
            name: name.into(),
            institution: None,
            kind,
            currency,
            parent: None,
            preset: None,
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
    /// History up to and including this date is locked: imports, undos,
    /// manual rows, balance-changing exclusions and entered balances leave
    /// it alone until the user unlocks ([`crate::reconciliation`]).
    pub reconciled_through: Option<NaiveDate>,
    pub closed_on: Option<NaiveDate>,
    /// The ID of the CSV preset the account's next import uses
    /// (`account.preset_id`, V15), e.g. `uk-monzo`. Core doesn't check it
    /// against the presets, which `coffer-import` holds.
    pub preset: Option<String>,
}

/// What the user can change about an account: the account inspector's
/// details. Start from [`AccountEdit::of`] and change the fields the user
/// changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountEdit {
    pub name: String,
    pub institution: Option<String>,
    pub kind: AccountKind,
    pub currency: Currency,
    /// Cleared when `kind` isn't [imported](AccountKind::is_imported).
    pub preset: Option<String>,
}

impl AccountEdit {
    /// The account's fields as they are.
    pub fn of(account: &Account) -> Self {
        AccountEdit {
            name: account.name.clone(),
            institution: account.institution.clone(),
            kind: account.kind,
            currency: account.currency,
            preset: account.preset.clone(),
        }
    }
}

/// What an account holds, which decides what about it can change
/// ([`usage`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub transactions: usize,
    /// Every balance snapshot: entered, from a file, or a valuation; and
    /// the opening balance.
    pub balances: usize,
    pub pots: usize,
    /// Rules on this account with amount bounds, which are in its minor
    /// units.
    pub rules_with_amounts: usize,
    /// It is a pot, which is in its parent's currency.
    pub is_pot: bool,
}

impl Usage {
    /// Whether its currency is fixed: it has transactions, balances, pots
    /// or rules with amounts, or it is a pot.
    pub const fn currency_locked(&self) -> bool {
        self.is_pot
            || self.transactions > 0
            || self.balances > 0
            || self.pots > 0
            || self.rules_with_amounts > 0
    }

    /// Why an account of kind `from` holding this can't become `to`;
    /// `None` when it can.
    pub fn kind_change_refused(&self, from: AccountKind, to: AccountKind) -> Option<KindLock> {
        let valuations = |kind: AccountKind| !kind.holds_transactions();
        if from == to {
            None
        } else if from == AccountKind::Pot || to == AccountKind::Pot {
            Some(KindLock::Pot)
        } else if valuations(to) && self.transactions > 0 {
            Some(KindLock::Transactions)
        } else if valuations(from) != valuations(to) && self.balances > 0 {
            Some(KindLock::Balances)
        } else if !matches!(to, AccountKind::Current | AccountKind::Savings) && self.pots > 0 {
            Some(KindLock::Pots)
        } else {
            None
        }
    }
}

/// Why an account's kind can't change ([`Usage::kind_change_refused`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KindLock {
    /// A pot stays a pot, and only an import makes one.
    Pot,
    /// Asset and liability accounts hold no transactions.
    Transactions,
    /// Its balances are checked balances or valuations, and a valuation
    /// account's are the other.
    Balances,
    /// Only a current or savings account has pots.
    Pots,
}

impl KindLock {
    /// Why, in a few words: "has transactions".
    pub const fn reason(self) -> &'static str {
        match self {
            KindLock::Pot => "pots come from imports",
            KindLock::Transactions => "has transactions",
            KindLock::Balances => "has balances",
            KindLock::Pots => "has pots",
        }
    }
}

/// A balance the user entered, or the account's opening balance, as the
/// account inspector lists them ([`entered_balances`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredBalance {
    /// `None` for the opening balance, which lives on the account row.
    pub id: Option<BalanceId>,
    pub date: NaiveDate,
    pub balance: Money,
    /// [`BalanceSource::Opening`], `Manual`, `Statement` or `Valuation`:
    /// never a file's.
    pub source: BalanceSource,
    /// The user's note; not sent over MCP by default.
    pub note: Option<String>,
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

    /// A balance on or before the lock date would add a check inside the
    /// locked period (or a new starting point before it).
    #[error("{date} is on or before {through}, which is locked; unlock the account first")]
    Locked { date: NaiveDate, through: NaiveDate },

    #[error("an account needs a name")]
    BlankName,

    #[error("{0} accounts aren't imported, so they have no preset")]
    PresetNotImported(&'static str),

    #[error(
        "the account's currency can't change: it already has transactions, balances, \
         pots or rules with amounts, or it is a pot"
    )]
    CurrencyLocked,

    #[error("the account can't change from {from} to {to}: it {}", why.reason())]
    KindLocked {
        from: &'static str,
        to: &'static str,
        why: KindLock,
    },

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// Creates an account. The name is trimmed and must not be blank; a blank
/// bank or preset is none. The schema checks the rest: a pot under a
/// current or savings account in its currency.
pub fn create(db: &mut Database, account: &NewAccount) -> Result<AccountId, Error> {
    create_with_balance(db, account, None)
}

/// Creates an account and, when given, records its first entered balance
/// ([`add_balance`]), in one DB transaction: the New account dialog's
/// optional Balance. Nothing is stored if either is refused.
pub fn create_with_balance(
    db: &mut Database,
    account: &NewAccount,
    balance: Option<&EnteredBalance>,
) -> Result<AccountId, Error> {
    let name = clean_name(&account.name)?;
    let preset = clean_preset(account.kind, account.preset.as_deref())?;
    let tx = db.conn_mut().transaction()?;
    tx.execute(
        "INSERT INTO account (parent_id, name, institution, kind, currency, preset_id) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            account.parent,
            name,
            clean_text(account.institution.as_deref()),
            account.kind.as_str(),
            account.currency,
            preset,
        ],
    )?;
    let id = AccountId::new(tx.last_insert_rowid());
    if let Some(balance) = balance {
        let stored = load(&tx, id)?;
        insert_balance(&tx, &stored, balance)?;
    }
    tx.commit()?;
    Ok(id)
}

/// Changes an account's name, bank, kind, currency and preset to `edit`
/// (module docs). The name is trimmed and must not be blank; a blank bank or
/// preset is none, and a kind that isn't imported keeps no preset. Changes
/// nothing when `edit` matches the account.
pub fn update(db: &mut Database, id: AccountId, edit: &AccountEdit) -> Result<(), Error> {
    let name = clean_name(&edit.name)?;
    let preset = if edit.kind.is_imported() {
        clean_text(edit.preset.as_deref())
    } else {
        None
    };
    let tx = db.conn_mut().transaction()?;
    let stored = load(&tx, id)?;
    let usage = load_usage(&tx, &stored)?;
    if edit.currency != stored.currency && usage.currency_locked() {
        return Err(Error::CurrencyLocked);
    }
    if let Some(why) = usage.kind_change_refused(stored.kind, edit.kind) {
        return Err(Error::KindLocked {
            from: stored.kind.as_str(),
            to: edit.kind.as_str(),
            why,
        });
    }
    tx.execute(
        "UPDATE account SET name = ?1, institution = ?2, kind = ?3, currency = ?4, \
         preset_id = ?5 WHERE id = ?6",
        params![
            name,
            clean_text(edit.institution.as_deref()),
            edit.kind.as_str(),
            edit.currency,
            preset,
            id,
        ],
    )?;
    tx.commit()?;
    Ok(())
}

/// What the account holds ([`Usage`]), for showing which of its fields can
/// change.
pub fn usage(db: &Database, id: AccountId) -> Result<Usage, Error> {
    let stored = load(db.conn(), id)?;
    Ok(load_usage(db.conn(), &stored)?)
}

/// The balances the user entered for the account (manual, statement and
/// valuation snapshots, not a file's) and its opening balance, newest
/// first; on one day, the latest entered first.
pub fn entered_balances(db: &Database, id: AccountId) -> Result<Vec<StoredBalance>, Error> {
    let conn = db.conn();
    let stored = load(conn, id)?;
    let mut statement = conn.prepare(
        "SELECT id, date, balance_minor, source, note FROM balance_snapshot \
         WHERE account_id = ?1 AND batch_id IS NULL ORDER BY date DESC, id DESC",
    )?;
    let mut balances: Vec<StoredBalance> = statement
        .query_map([id], |row| {
            let source: String = row.get("source")?;
            Ok(StoredBalance {
                id: Some(row.get("id")?),
                date: date(row, "date")?,
                balance: Money::new(row.get("balance_minor")?, stored.currency),
                source: BalanceSource::from_snapshot_key(&source)
                    .ok_or_else(|| invalid(row, "source"))?,
                note: row.get("note")?,
            })
        })?
        .collect::<Result<_, _>>()?;
    if let Some((date, balance)) = stored.opening_balance {
        // The opening balance counts first on its day, so it lists last.
        let at = balances
            .iter()
            .position(|entered| entered.date < date)
            .unwrap_or(balances.len());
        balances.insert(
            at,
            StoredBalance {
                id: None,
                date,
                balance,
                source: BalanceSource::Opening,
                note: None,
            },
        );
    }
    Ok(balances)
}

fn load_usage(conn: &Connection, account: &Account) -> rusqlite::Result<Usage> {
    let count = |sql: &str| -> rusqlite::Result<usize> {
        conn.query_row(sql, [account.id], |row| row.get::<_, i64>(0))
            .map(|n| usize::try_from(n).unwrap_or_default())
    };
    Ok(Usage {
        transactions: count("SELECT COUNT(*) FROM \"transaction\" WHERE account_id = ?1")?,
        balances: count("SELECT COUNT(*) FROM balance_snapshot WHERE account_id = ?1")?
            + usize::from(account.opening_balance.is_some()),
        pots: count("SELECT COUNT(*) FROM account WHERE parent_id = ?1")?,
        rules_with_amounts: count(
            "SELECT COUNT(*) FROM rule WHERE account_id = ?1 \
             AND (amount_min IS NOT NULL OR amount_max IS NOT NULL)",
        )?,
        is_pot: account.kind == AccountKind::Pot,
    })
}

/// The trimmed name, which must not be blank.
fn clean_name(name: &str) -> Result<&str, Error> {
    let name = name.trim();
    if name.is_empty() {
        Err(Error::BlankName)
    } else {
        Ok(name)
    }
}

/// Trimmed optional text; blank is none.
fn clean_text(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|text| !text.is_empty())
}

fn clean_preset(kind: AccountKind, preset: Option<&str>) -> Result<Option<&str>, Error> {
    match clean_text(preset) {
        Some(_) if !kind.is_imported() => Err(Error::PresetNotImported(kind.as_str())),
        preset => Ok(preset),
    }
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
/// One entered balance per account and day, after the account's lock date
/// (`reconciled_through`): one inside the locked period would change the
/// checks the lock rests on ([`crate::reconciliation`]).
pub fn add_balance(
    db: &mut Database,
    account: AccountId,
    balance: &EnteredBalance,
) -> Result<BalanceId, Error> {
    let tx = db.conn_mut().transaction()?;
    let stored = load(&tx, account)?;
    let id = insert_balance(&tx, &stored, balance)?;
    tx.commit()?;
    Ok(id)
}

/// [`add_balance`]'s checks and insert, inside the caller's transaction.
fn insert_balance(
    conn: &Connection,
    stored: &Account,
    balance: &EnteredBalance,
) -> Result<BalanceId, Error> {
    if !stored.kind.holds_transactions() {
        return Err(Error::HoldsValuations(stored.kind.as_str()));
    }
    if balance.balance.currency() != stored.currency {
        return Err(Error::CurrencyMismatch {
            expected: stored.currency,
            found: balance.balance.currency(),
        });
    }
    if let Some(through) = stored.reconciled_through
        && balance.date <= through
    {
        return Err(Error::Locked {
            date: balance.date,
            through,
        });
    }
    let date = date_key(balance.date);
    let exists = conn
        .query_row(
            "SELECT 1 FROM balance_snapshot \
             WHERE account_id = ?1 AND date = ?2 AND batch_id IS NULL",
            params![stored.id, date],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if exists {
        return Err(Error::BalanceExists(balance.date));
    }
    conn.execute(
        "INSERT INTO balance_snapshot (account_id, date, balance_minor, source, note) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            stored.id,
            date,
            balance.balance.minor(),
            balance.source.as_str(),
            balance.note,
        ],
    )?;
    Ok(BalanceId::new(conn.last_insert_rowid()))
}

const COLUMNS: &str = "id, parent_id, name, institution, kind, currency, \
                       opening_balance_minor, opening_date, reconciled_through, closed_on, \
                       preset_id";

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
        preset: row.get("preset_id")?,
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
        assert!(matches!(create(&mut db, &blank), Err(Error::BlankName)));
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

        // Inside the lock (on or before its date) is refused; after it is fine.
        db.conn()
            .execute(
                "UPDATE account SET reconciled_through = ?1 WHERE id = ?2",
                params![date_key(day(2)), account],
            )
            .unwrap();
        for d in [1, 2] {
            assert!(matches!(
                add_balance(&mut db, account, &balance(day(d), 5, Currency::GBP)),
                Err(Error::Locked { date, through }) if date == day(d) && through == day(2)
            ));
        }
        add_balance(&mut db, account, &balance(day(3), 9_000, Currency::GBP)).unwrap();

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

    fn gbp(minor: i64) -> Money {
        Money::new(minor, Currency::GBP)
    }

    fn entered(date: NaiveDate, minor: i64) -> EnteredBalance {
        EnteredBalance {
            date,
            balance: gbp(minor),
            source: EnteredSource::Manual,
            note: None,
        }
    }

    fn count(db: &Database, sql: &str) -> i64 {
        db.conn().query_row(sql, [], |row| row.get(0)).unwrap()
    }

    #[test]
    fn creates_with_a_trimmed_name_a_preset_and_a_first_balance() {
        let (mut db, _dir) = open();
        let mut new = NewAccount::new("  Current account ", AccountKind::Current, Currency::GBP);
        new.institution = Some("  ".into());
        new.preset = Some("uk-monzo".into());
        let mut balance = entered(day(9), 128_410);
        balance.note = Some("From the Monzo app".into());
        let id = create_with_balance(&mut db, &new, Some(&balance)).unwrap();

        let account = get(&db, id).unwrap();
        assert_eq!(account.name, "Current account");
        assert_eq!(account.institution, None);
        assert_eq!(account.preset.as_deref(), Some("uk-monzo"));
        assert_eq!(
            entered_balances(&db, id).unwrap(),
            [StoredBalance {
                id: entered_balances(&db, id).unwrap()[0].id,
                date: day(9),
                balance: gbp(128_410),
                source: BalanceSource::Manual,
                note: Some("From the Monzo app".into()),
            }]
        );
    }

    #[test]
    fn a_refused_first_balance_stores_nothing() {
        let (mut db, _dir) = open();
        let new = NewAccount::new("Euro account", AccountKind::Current, Currency::EUR);
        assert!(matches!(
            create_with_balance(&mut db, &new, Some(&entered(day(1), 100))),
            Err(Error::CurrencyMismatch { .. })
        ));
        assert_eq!(count(&db, "SELECT COUNT(*) FROM account"), 0);
        assert_eq!(count(&db, "SELECT COUNT(*) FROM balance_snapshot"), 0);
    }

    #[test]
    fn only_imported_kinds_have_a_preset() {
        let (mut db, _dir) = open();
        let mut cash = NewAccount::new("Cash", AccountKind::Cash, Currency::GBP);
        cash.preset = Some("uk-monzo".into());
        assert!(matches!(
            create(&mut db, &cash),
            Err(Error::PresetNotImported("cash"))
        ));
        // A blank preset is none, so it isn't refused.
        cash.preset = Some(" ".into());
        let cash = create(&mut db, &cash).unwrap();
        assert_eq!(get(&db, cash).unwrap().preset, None);

        // Becoming a kind that isn't imported clears it.
        let mut card = NewAccount::new("Card", AccountKind::CreditCard, Currency::GBP);
        card.preset = Some("uk-monzo".into());
        let card = create(&mut db, &card).unwrap();
        let mut edit = AccountEdit::of(&get(&db, card).unwrap());
        edit.kind = AccountKind::Cash;
        update(&mut db, card, &edit).unwrap();
        let card = get(&db, card).unwrap();
        assert_eq!((card.kind, card.preset), (AccountKind::Cash, None));
    }

    #[test]
    fn the_schema_keeps_presets_on_imported_kinds() {
        let (mut db, _dir) = open();
        let cash = create(
            &mut db,
            &NewAccount::new("Cash", AccountKind::Cash, Currency::GBP),
        )
        .unwrap();
        for preset in ["uk-monzo", ""] {
            assert!(
                db.conn()
                    .execute(
                        "UPDATE account SET preset_id = ?1 WHERE id = ?2",
                        params![preset, cash],
                    )
                    .is_err()
            );
        }
    }

    #[test]
    fn edits_the_name_bank_kind_currency_and_preset() {
        let (mut db, _dir) = open();
        let id = create(
            &mut db,
            &NewAccount::new("Amex", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        let before = get(&db, id).unwrap();
        let edit = AccountEdit {
            name: " Amex Gold ".into(),
            institution: Some("American Express".into()),
            kind: AccountKind::CreditCard,
            currency: Currency::USD,
            preset: Some("uk-monzo".into()),
        };
        update(&mut db, id, &edit).unwrap();
        let after = get(&db, id).unwrap();
        assert_eq!(after.name, "Amex Gold");
        assert_eq!(after.institution.as_deref(), Some("American Express"));
        assert_eq!(after.kind, AccountKind::CreditCard);
        assert_eq!(after.currency, Currency::USD);
        assert_eq!(after.preset.as_deref(), Some("uk-monzo"));
        assert_eq!(AccountEdit::of(&after).name, "Amex Gold");
        assert_ne!(before, after);

        let mut blank = AccountEdit::of(&after);
        blank.name = "\t".into();
        assert!(matches!(update(&mut db, id, &blank), Err(Error::BlankName)));
        assert!(matches!(
            update(&mut db, AccountId::new(99), &AccountEdit::of(&after)),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn what_an_account_holds_fixes_its_currency_and_kind() {
        let (mut db, _dir) = open();
        let current = create(
            &mut db,
            &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        let fresh = usage(&db, current).unwrap();
        assert_eq!(fresh, Usage::default());
        assert!(!fresh.currency_locked());

        // A balance fixes the currency and keeps it out of valuations.
        add_balance(&mut db, current, &entered(day(1), 10_000)).unwrap();
        let used = usage(&db, current).unwrap();
        assert_eq!(used.balances, 1);
        assert!(used.currency_locked());
        let mut edit = AccountEdit::of(&get(&db, current).unwrap());
        edit.currency = Currency::EUR;
        assert!(matches!(
            update(&mut db, current, &edit),
            Err(Error::CurrencyLocked)
        ));
        let mut edit = AccountEdit::of(&get(&db, current).unwrap());
        edit.kind = AccountKind::Asset;
        assert!(matches!(
            update(&mut db, current, &edit),
            Err(Error::KindLocked {
                why: KindLock::Balances,
                ..
            })
        ));

        // Pots keep it a current or savings account, and a pot stays a pot.
        let mut pot = NewAccount::new("Holiday", AccountKind::Pot, Currency::GBP);
        pot.parent = Some(current);
        let pot = create(&mut db, &pot).unwrap();
        assert_eq!(usage(&db, current).unwrap().pots, 1);
        let mut edit = AccountEdit::of(&get(&db, current).unwrap());
        edit.kind = AccountKind::CreditCard;
        assert!(matches!(
            update(&mut db, current, &edit),
            Err(Error::KindLocked {
                why: KindLock::Pots,
                ..
            })
        ));
        edit.kind = AccountKind::Savings;
        update(&mut db, current, &edit).unwrap();
        let pot_usage = usage(&db, pot).unwrap();
        assert!(pot_usage.is_pot && pot_usage.currency_locked());
        let mut edit = AccountEdit::of(&get(&db, pot).unwrap());
        edit.kind = AccountKind::Savings;
        assert!(matches!(
            update(&mut db, pot, &edit),
            Err(Error::KindLocked {
                why: KindLock::Pot,
                ..
            })
        ));
        // Renaming a pot is fine.
        let mut edit = AccountEdit::of(&get(&db, pot).unwrap());
        edit.name = "Summer".into();
        update(&mut db, pot, &edit).unwrap();

        // Transactions keep it out of asset and liability.
        let cash = create(
            &mut db,
            &NewAccount::new("Cash", AccountKind::Cash, Currency::GBP),
        )
        .unwrap();
        crate::manual::create(
            &mut db,
            &crate::manual::ManualTransaction::new(cash, day(2), gbp(-500), "Test row"),
        )
        .unwrap();
        assert_eq!(usage(&db, cash).unwrap().transactions, 1);
        let mut edit = AccountEdit::of(&get(&db, cash).unwrap());
        edit.kind = AccountKind::Liability;
        assert!(matches!(
            update(&mut db, cash, &edit),
            Err(Error::KindLocked {
                why: KindLock::Transactions,
                ..
            })
        ));
        edit.kind = AccountKind::Current;
        update(&mut db, cash, &edit).unwrap();
    }

    #[test]
    fn lists_entered_balances_newest_first_and_none_from_files() {
        let (mut db, _dir) = open();
        let id = create(
            &mut db,
            &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        add_balance(&mut db, id, &entered(day(1), 100)).unwrap();
        let mut statement = entered(day(5), 500);
        statement.source = EnteredSource::Statement;
        add_balance(&mut db, id, &statement).unwrap();
        // The opening balance, on the same day as an entered one, and a
        // file's balance, which isn't listed.
        db.conn()
            .execute(
                "UPDATE account SET opening_balance_minor = 50, opening_date = '2026-11-01' \
                 WHERE id = ?1",
                [id],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO import_batch (account_id, source_file, format) \
                 VALUES (?1, 'x.csv', 'csv')",
                [id],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO balance_snapshot (account_id, batch_id, date, balance_minor, source) \
                 VALUES (?1, 1, '2026-11-03', 300, 'csv')",
                [id],
            )
            .unwrap();

        let listed: Vec<_> = entered_balances(&db, id)
            .unwrap()
            .into_iter()
            .map(|balance| (balance.date, balance.balance.minor(), balance.source))
            .collect();
        assert_eq!(
            listed,
            [
                (day(5), 500, BalanceSource::Statement),
                (day(1), 100, BalanceSource::Manual),
                (day(1), 50, BalanceSource::Opening),
            ]
        );
        assert_eq!(usage(&db, id).unwrap().balances, 4);
    }
}
