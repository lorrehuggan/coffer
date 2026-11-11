//! Committing a statement to an account: dedupe (M1.11), reconciliation
//! (M1.12) and undoable import batches (M1.13).
//!
//! An import is two steps, so the CLI's `--dry-run` and the app's Review
//! screen show exactly what a commit would do:
//!
//! 1. [`preview`] reads the database and decides, for every row of the
//!    [`Statement`], whether it is new, already imported, or flagged, which
//!    balances the file adds, and how they reconcile. It writes nothing.
//! 2. [`commit`] does it, in one DB transaction: an `import_batch`, its
//!    transactions, the file's balances and a record of its flagged rows.
//!    In the same transaction it categorises the new rows (the preset's
//!    category map, then the rules, which override it) and suggests
//!    transfer pairs ([`crate::transfer`]).
//!    It first plans again inside that transaction and refuses with
//!    [`Error::Stale`] if anything differs from the preview, so what the
//!    user saw is what is stored. Flagged rows and a failed reconciliation
//!    need the user's [`Confirmations`].
//!
//! [`undo`] removes a batch again (after [`undo_preview`] has said what goes
//! with it).
//!
//! Decisions (`docs/ARCHITECTURE.md` → "Decision record: import commit,
//! dedupe and undo"):
//!
//! - **Dedupe**: a row with a source ID matches the account's row with that
//!   ID, and never by fingerprint; one without matches the account's row
//!   without an ID that has its fingerprint ([`fingerprint`]). A source ID
//!   already stored for a different date or amount is flagged
//!   ([`Flag::SourceIdConflict`]), not skipped as a duplicate. Excluded rows
//!   still count as imported.
//! - **Nothing is dropped silently**: a row that isn't committed is either
//!   already in the account (it is shown as such) or flagged, and the batch
//!   keeps a record of each flag (`import_flag`).
//! - **Locked periods**: a new row dated on or before the account's
//!   `reconciled_through` is flagged ([`Flag::Locked`]) and stays out until
//!   the user unlocks.
//!
//! Nothing here logs transaction contents; logs carry counts and IDs only.

mod dedupe;
mod reconcile;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::account::{self, Account, date_key};
use crate::backup;
use crate::balance::{Assertion, DailyMovements};
use crate::category::CategoryKey;
use crate::db::Database;
use crate::id::{AccountId, BalanceId, BatchId, TransactionId};
use crate::money::{Currency, Money};
use crate::rule;
use crate::statement::{
    RowProblem, SourceFormat, SourceLocation, Statement, StatementLine, SubAccountRef,
};
use crate::sub_account::{self, MirrorPlan};
use crate::transfer::{self, TransferWindow};

pub use dedupe::{FINGERPRINT_VERSION, fingerprint};
pub use reconcile::{
    BalanceCheck, CheckedBalance, FileOrder, Reconciliation, ReconciliationStatus, RunningBalances,
    RunningMismatch,
};

/// Why an import, or undoing one, failed. Messages never contain
/// transaction text.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("there is no account {0}")]
    AccountNotFound(AccountId),

    #[error("{0} accounts hold valuations, not imported transactions")]
    HoldsValuations(&'static str),

    #[error("the file is in {statement}, but the account is in {account}")]
    CurrencyMismatch {
        account: Currency,
        statement: Currency,
    },

    #[error("the import needs the file's name")]
    NoFileName,

    #[error("balances from {0} files can't be recorded yet")]
    BalancesNotSupported(SourceFormat),

    #[error("the account changed since the preview; preview the import again")]
    Stale,

    #[error("{}", not_confirmed(.0))]
    NotConfirmed(Confirmations),

    #[error("there is no import batch {0}")]
    BatchNotFound(BatchId),

    #[error("import batch {0} is already undone")]
    AlreadyUndone(BatchId),

    #[error(
        "the import has transactions or balances on or before {through}, which is locked; \
         unlock the account first"
    )]
    Locked { through: NaiveDate },

    #[error("an amount is too large to represent")]
    Overflow,

    #[error("rules: {0}")]
    Rules(#[from] rule::StoreError),

    #[error("transfer detection: {0}")]
    Transfers(#[from] transfer::Error),

    #[error("pots: {0}")]
    SubAccounts(#[from] sub_account::Error),

    /// The backup taken before the commit failed, so nothing was imported
    /// (M1.28).
    #[error("couldn't back up the database before importing, so nothing was imported: {0}")]
    Backup(#[source] Box<backup::Error>),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

fn not_confirmed(needed: &Confirmations) -> &'static str {
    match (needed.flagged_rows, needed.balance_mismatch) {
        (true, true) => "the import has flagged rows and a balance mismatch to confirm",
        (true, false) => "the import has flagged rows to confirm",
        _ => "the import has a balance mismatch to confirm",
    }
}

fn load_account(conn: &Connection, id: AccountId) -> Result<Account, Error> {
    account::find(conn, id)?.ok_or(Error::AccountNotFound(id))
}

/// How a row was found to be in the account already.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchedBy {
    SourceId,
    Fingerprint,
}

/// Why a row isn't committed as a transaction. The user sees each one, and
/// the batch records it.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Flag {
    /// The importer couldn't read the row.
    #[error("{0}")]
    Unreadable(RowProblem),

    /// The account already has this source ID for a transaction with a
    /// different date or amount.
    #[error(
        "the account already has transaction {existing} with this ID, on another date or for another amount"
    )]
    SourceIdConflict { existing: TransactionId },

    /// An earlier row in the file has this source ID with a different date
    /// or amount.
    #[error("{first} has the same ID, on another date or for another amount")]
    SourceIdRepeated { first: SourceLocation },

    /// A new row in a locked period.
    #[error("it's dated on or before {through}, which is locked")]
    Locked { through: NaiveDate },
}

impl Flag {
    pub fn kind(&self) -> FlagKind {
        match self {
            Flag::Unreadable(_) => FlagKind::Unreadable,
            Flag::SourceIdConflict { .. } | Flag::SourceIdRepeated { .. } => {
                FlagKind::SourceIdConflict
            }
            Flag::Locked { .. } => FlagKind::Locked,
        }
    }
}

/// The kind of a recorded flag (`import_flag.kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FlagKind {
    Unreadable,
    SourceIdConflict,
    Locked,
    /// A running balance that doesn't follow from the rows before it. The
    /// row itself is imported; its balance is in doubt.
    RunningBalance,
}

impl FlagKind {
    pub const ALL: [FlagKind; 4] = [
        FlagKind::Unreadable,
        FlagKind::SourceIdConflict,
        FlagKind::Locked,
        FlagKind::RunningBalance,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            FlagKind::Unreadable => "unreadable",
            FlagKind::SourceIdConflict => "source_id_conflict",
            FlagKind::Locked => "locked",
            FlagKind::RunningBalance => "running_balance",
        }
    }

    pub fn from_key(key: &str) -> Option<FlagKind> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == key)
    }
}

/// What the import does with one row of the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowOutcome {
    /// Committed as a new transaction with this fingerprint.
    New { fingerprint: String },
    /// The account already has it.
    AlreadyImported {
        transaction: TransactionId,
        matched_by: MatchedBy,
    },
    /// An earlier row in the file has the same source ID, date and amount:
    /// the file repeats it.
    RepeatedInFile { first: SourceLocation },
    /// Not committed; shown to the user and recorded with the batch.
    Flagged(Flag),
}

/// One row of a preview.
#[derive(Clone, Copy, Debug)]
pub struct PreviewRow<'a> {
    pub location: SourceLocation,
    /// The row as read; `None` when it couldn't be.
    pub line: Option<&'a StatementLine>,
    pub outcome: &'a RowOutcome,
}

/// Where a balance the file gives comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileBalanceKind {
    /// Stated by the file (OFX `LEDGERBAL`).
    Stated,
    /// Worked out from the running balances: the end of the day before the
    /// file's first row.
    Opening,
}

/// What the import does with a balance from the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileBalanceStatus {
    /// Recorded as a new `balance_snapshot` of the batch.
    New,
    /// An earlier import recorded the same balance for the same day.
    AlreadyRecorded(BalanceId),
    /// The file gives the same balance for the same day twice.
    Repeated,
    /// Dated on or before the account's `reconciled_through`: left out, like
    /// a row in the locked period, so the import changes nothing there and
    /// stays undoable without unlocking. Not checked either.
    Locked,
}

/// A balance the file gives: the account's balance at the end of `date`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileBalance {
    pub date: NaiveDate,
    pub balance: Money,
    pub kind: FileBalanceKind,
    pub status: FileBalanceStatus,
}

/// What a commit needs the user to have seen and accepted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Confirmations {
    /// Rows that won't be committed as transactions ([`RowOutcome::Flagged`]).
    pub flagged_rows: bool,
    /// [`ReconciliationStatus::Mismatch`]: the batch is committed and the
    /// failure stays visible ("Import and flag the mismatch").
    pub balance_mismatch: bool,
}

/// The Review screen's figures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PreviewCounts {
    pub new: usize,
    /// Already in the account, or repeated in the file.
    pub already_imported: usize,
    pub flagged: usize,
}

/// What importing a statement into an account would do. Made by
/// [`preview`], consumed by [`commit`].
#[derive(Clone, Debug)]
pub struct ImportPreview {
    account: Account,
    source_file: String,
    statement: Statement,
    plan: Plan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Plan {
    outcomes: Vec<RowOutcome>,
    balances: Vec<FileBalance>,
    reconciliation: Reconciliation,
    /// The pots the new rows' sub-account transfers go to (M1.17).
    sub_accounts: MirrorPlan,
}

impl ImportPreview {
    /// The account as it was when previewed.
    pub fn account(&self) -> &Account {
        &self.account
    }

    pub fn source_file(&self) -> &str {
        &self.source_file
    }

    pub fn statement(&self) -> &Statement {
        &self.statement
    }

    /// Every row of the file, in file order, with what the import does with
    /// it.
    pub fn rows(&self) -> impl ExactSizeIterator<Item = PreviewRow<'_>> {
        self.statement
            .rows()
            .iter()
            .zip(&self.plan.outcomes)
            .map(|(row, outcome)| PreviewRow {
                location: row.location,
                line: row.outcome.as_ref().ok(),
                outcome,
            })
    }

    pub fn counts(&self) -> PreviewCounts {
        let mut counts = PreviewCounts::default();
        for outcome in &self.plan.outcomes {
            match outcome {
                RowOutcome::New { .. } => counts.new += 1,
                RowOutcome::AlreadyImported { .. } | RowOutcome::RepeatedInFile { .. } => {
                    counts.already_imported += 1;
                }
                RowOutcome::Flagged(_) => counts.flagged += 1,
            }
        }
        counts
    }

    /// The balances the file gives: statement balances in file order, after
    /// the opening worked out from running balances.
    pub fn balances(&self) -> &[FileBalance] {
        &self.plan.balances
    }

    pub fn reconciliation(&self) -> &Reconciliation {
        &self.plan.reconciliation
    }

    /// What the new rows' sub-account transfers do: the pots the commit
    /// creates, the existing (and closed) pots that get rows, and how many
    /// rows are mirrored onto them ([`sub_account::plan`]).
    pub fn sub_accounts(&self) -> &MirrorPlan {
        &self.plan.sub_accounts
    }

    /// What the user has to confirm before [`commit`].
    pub fn needs_confirmation(&self) -> Confirmations {
        Confirmations {
            flagged_rows: self
                .plan
                .outcomes
                .iter()
                .any(|outcome| matches!(outcome, RowOutcome::Flagged(_))),
            balance_mismatch: self.plan.reconciliation.status() == ReconciliationStatus::Mismatch,
        }
    }

    /// Whether a commit would store nothing: no new transaction and no new
    /// balance. Re-importing a file is then a no-op.
    pub fn is_nothing_new(&self) -> bool {
        !self
            .plan
            .outcomes
            .iter()
            .any(|outcome| matches!(outcome, RowOutcome::New { .. }))
            && !self
                .plan
                .balances
                .iter()
                .any(|balance| balance.status == FileBalanceStatus::New)
    }
}

/// What [`commit`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Committed {
    /// A new batch.
    Batch(CommitSummary),
    /// Nothing was new, so no batch was recorded.
    NothingNew,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommitSummary {
    pub batch: BatchId,
    pub transactions: usize,
    pub balances: usize,
    pub flags: usize,
    /// New rows the preset's category map categorised and no rule
    /// overrode (`category_source = 'preset_map'`).
    pub categorized_by_preset: usize,
    /// New rows a rule categorised (`category_source = 'rule'`).
    pub categorized_by_rules: usize,
    /// Pot-side rows mirrored from the new rows' sub-account transfers
    /// (`origin = 'mirrored'`), each paired with its row as a `confirmed`
    /// transfer. Not counted in `transactions` or the category counts.
    pub mirrored: usize,
    /// Pots the commit created for sub-account keys the account had no pot
    /// for.
    pub pots_created: usize,
    /// Transfer pairs suggested for the new rows, each a `suggested`
    /// `transfer_group`.
    pub transfer_pairs: usize,
}

/// Plans importing `statement` into `account`. Reads only.
///
/// `source_file` is the file's name; anything up to the last `/` or `\` is
/// dropped, since a directory can contain the user's name.
pub fn preview(
    db: &Database,
    account: AccountId,
    source_file: &str,
    statement: Statement,
) -> Result<ImportPreview, Error> {
    let source_file = source_file
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .ok_or(Error::NoFileName)?
        .to_owned();
    // One read transaction, so every query sees the same database.
    let tx = Transaction::new_unchecked(db.conn(), TransactionBehavior::Deferred)?;
    let account = load_account(&tx, account)?;
    check_target(&account, &statement)?;
    let plan = plan(&tx, &account, &statement)?;
    drop(tx);
    Ok(ImportPreview {
        account,
        source_file,
        statement,
        plan,
    })
}

/// Commits `preview` in one DB transaction and records it as an import
/// batch. Fails with [`Error::Stale`] when the account changed since the
/// preview, and with [`Error::NotConfirmed`] when `confirmed` doesn't cover
/// [`ImportPreview::needs_confirmation`].
///
/// After storing the rows, and in the same transaction:
///
/// 1. each new row gets the category the preset's map gives it
///    ([`StatementLine::category`]; `category_source = 'preset_map'`). A
///    key with no category (the user deleted it) leaves the row
///    uncategorised;
/// 2. each new row the preset recognised as a sub-account transfer is
///    mirrored onto its pot, which is created if the account has none with
///    its key, and the two are paired as a `confirmed` transfer
///    ([`sub_account::mirror`]; the preview lists it as
///    [`ImportPreview::sub_accounts`]). The mirrored rows belong to the
///    batch, so undo removes them;
/// 3. the rules run on the batch ([`rule::apply_to_batch`]), mirrored rows
///    included; a matching `set_category` rule overrides the preset's
///    category;
/// 4. transfer pairs are suggested for the batch's rows
///    ([`transfer::detect_for_batch`], [`TransferWindow::DEFAULT`]).
///
/// A failure in any step rolls the whole import back. Merchant names
/// (M1.23) will run in the same transaction.
///
/// Before its DB transaction (`VACUUM INTO` can't run inside one), the
/// commit takes a backup of the database (reason `import`,
/// [`backup::create`]), unless the preview has nothing new or lacks a
/// confirmation, which commit refuses without writing anything. If the
/// backup fails, nothing is imported ([`Error::Backup`]).
pub fn commit(
    db: &mut Database,
    preview: &ImportPreview,
    confirmed: Confirmations,
) -> Result<Committed, Error> {
    let needed = preview.needs_confirmation();
    let is_confirmed = (!needed.flagged_rows || confirmed.flagged_rows)
        && (!needed.balance_mismatch || confirmed.balance_mismatch);
    if !preview.is_nothing_new() && is_confirmed {
        backup::create(db, backup::Reason::Import)
            .map_err(|error| Error::Backup(Box::new(error)))?;
    }

    let tx = db.conn_mut().transaction()?;
    let account = load_account(&tx, preview.account.id)?;
    check_target(&account, &preview.statement)?;
    let plan = plan(&tx, &account, &preview.statement)?;
    if account != preview.account || plan != preview.plan {
        return Err(Error::Stale);
    }
    if preview.is_nothing_new() {
        return Ok(Committed::NothingNew);
    }
    if !is_confirmed {
        return Err(Error::NotConfirmed(needed));
    }

    let statement = &preview.statement;
    tx.execute(
        "INSERT INTO import_batch (account_id, source_file, format, preset_id) \
         VALUES (?1, ?2, ?3, ?4)",
        params![
            account.id,
            preview.source_file,
            statement.format().as_str(),
            statement.preset_id(),
        ],
    )?;
    let batch = BatchId::new(tx.last_insert_rowid());
    let imported_at: String = tx.query_row(
        "SELECT imported_at FROM import_batch WHERE id = ?1",
        [batch],
        |row| row.get(0),
    )?;

    let mut summary = CommitSummary {
        batch,
        transactions: 0,
        balances: 0,
        flags: 0,
        categorized_by_preset: 0,
        categorized_by_rules: 0,
        mirrored: 0,
        pots_created: 0,
        transfer_pairs: 0,
    };
    // The new rows that are sub-account transfers, for the mirror below.
    let mut pot_rows: Vec<(TransactionId, SubAccountRef)> = Vec::new();
    {
        let categories = category_ids(&tx)?;
        let mut insert_row = tx.prepare(
            "INSERT INTO \"transaction\" (account_id, batch_id, date, time, amount_minor, \
             currency, merchant, description_raw, description_normalized, external_id, \
             fingerprint, source_category, source_type, original_amount_minor, \
             original_currency, note, origin, created_at, category_id, category_source) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, \
             'imported', ?17, ?18, ?19)",
        )?;
        let mut insert_flag = tx.prepare(
            "INSERT INTO import_flag (batch_id, location_kind, location, kind, detail) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        let mut flag = |location: SourceLocation, kind: FlagKind, detail: String| {
            let (location_kind, number) = location_parts(location);
            insert_flag.execute(params![batch, location_kind, number, kind.as_str(), detail])
        };

        for row in preview.rows() {
            match (row.outcome, row.line) {
                (RowOutcome::New { fingerprint }, Some(line)) => {
                    let category = line.category.as_ref().and_then(|key| categories.get(key));
                    insert_row.execute(params![
                        account.id,
                        batch,
                        date_key(line.date),
                        line.time.map(|time| time.format("%H:%M:%S").to_string()),
                        line.amount.minor(),
                        line.amount.currency(),
                        line.merchant,
                        line.description_raw,
                        line.description_normalized(),
                        line.external_id,
                        fingerprint,
                        line.source_category,
                        line.source_type,
                        line.original_amount.map(|amount| amount.minor()),
                        line.original_amount.map(|amount| amount.currency()),
                        line.note,
                        imported_at,
                        category,
                        category.map(|_| "preset_map"),
                    ])?;
                    if let Some(sub_account) = &line.sub_account {
                        pot_rows.push((
                            TransactionId::new(tx.last_insert_rowid()),
                            sub_account.clone(),
                        ));
                    }
                    summary.transactions += 1;
                }
                (RowOutcome::Flagged(problem), _) => {
                    flag(row.location, problem.kind(), problem.to_string())?;
                    summary.flags += 1;
                }
                _ => {}
            }
        }
        if let Some(running) = &plan.reconciliation.running {
            for mismatch in &running.mismatches {
                flag(
                    mismatch.location,
                    FlagKind::RunningBalance,
                    format!(
                        "the running balance is {}, but the rows before it give {}",
                        mismatch.stated, mismatch.computed
                    ),
                )?;
                summary.flags += 1;
            }
        }
    }

    let new_balances: Vec<&FileBalance> = plan
        .balances
        .iter()
        .filter(|balance| balance.status == FileBalanceStatus::New)
        .collect();
    if !new_balances.is_empty() {
        let source = balance_source(statement.format())?;
        let mut insert = tx.prepare(
            "INSERT INTO balance_snapshot (account_id, batch_id, date, balance_minor, source) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for balance in new_balances {
            insert.execute(params![
                account.id,
                batch,
                date_key(balance.date),
                balance.balance.minor(),
                source,
            ])?;
            summary.balances += 1;
        }
    }

    // The pot side of sub-account transfers, before rules and transfer
    // detection so both see the mirrored rows (which are then already in a
    // confirmed pair, so detection leaves them alone).
    let mirrored = sub_account::mirror(&tx, &account, batch, &pot_rows)?;
    summary.mirrored = mirrored.mirrored_rows();
    summary.pots_created = mirrored.created_pots.len();

    // Rules override the preset's categories, so they run after the rows
    // are stored with them. The counts are of the imported rows: a mirrored
    // row is the same transfer seen from the pot.
    rule::apply_to_batch(&tx, batch)?;
    let categorized = |source: &str| -> Result<usize, Error> {
        let n: i64 = tx.query_row(
            "SELECT COUNT(*) FROM \"transaction\" \
             WHERE batch_id = ?1 AND origin = 'imported' AND category_source = ?2",
            params![batch, source],
            |row| row.get(0),
        )?;
        Ok(usize::try_from(n).unwrap_or(0))
    };
    summary.categorized_by_preset = categorized("preset_map")?;
    summary.categorized_by_rules = categorized("rule")?;
    summary.transfer_pairs = transfer::detect_for_batch(&tx, batch, TransferWindow::DEFAULT)?.len();

    tx.commit()?;
    tracing::info!(
        batch = %batch,
        account = %account.id,
        transactions = summary.transactions,
        balances = summary.balances,
        flags = summary.flags,
        categorized_by_preset = summary.categorized_by_preset,
        categorized_by_rules = summary.categorized_by_rules,
        mirrored = summary.mirrored,
        pots_created = summary.pots_created,
        transfer_pairs = summary.transfer_pairs,
        "committed an import"
    );
    Ok(Committed::Batch(summary))
}

/// Every category's ID, by key.
fn category_ids(conn: &Connection) -> Result<HashMap<CategoryKey, i64>, Error> {
    let mut statement = conn.prepare("SELECT key, id FROM category")?;
    let ids = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    Ok(ids)
}

/// The account must take transactions in the statement's currency.
fn check_target(account: &Account, statement: &Statement) -> Result<(), Error> {
    if !account.kind.holds_transactions() {
        return Err(Error::HoldsValuations(account.kind.as_str()));
    }
    if account.currency != statement.currency() {
        return Err(Error::CurrencyMismatch {
            account: account.currency,
            statement: statement.currency(),
        });
    }
    Ok(())
}

/// `balance_snapshot.source` for a file's balances.
fn balance_source(format: SourceFormat) -> Result<&'static str, Error> {
    match format {
        SourceFormat::Csv => Ok("csv"),
        SourceFormat::Ofx => Ok("ofx"),
        other => Err(Error::BalancesNotSupported(other)),
    }
}

fn plan(conn: &Connection, account: &Account, statement: &Statement) -> Result<Plan, Error> {
    let outcomes = classify_rows(conn, account, statement)?;

    // The file's balances: the opening from running balances, then the
    // stated ones.
    let running = reconcile::running(statement)?;
    let mut given: Vec<(NaiveDate, Money, FileBalanceKind)> = Vec::new();
    if let Some((_, Some((date, balance)))) = &running {
        given.push((*date, *balance, FileBalanceKind::Opening));
    }
    given.extend(
        statement
            .balances()
            .iter()
            .map(|stated| (stated.date, stated.balance, FileBalanceKind::Stated)),
    );
    if !given.is_empty() {
        balance_source(statement.format())?;
    }

    let history = history(conn, account)?;
    let mut balances: Vec<FileBalance> = Vec::with_capacity(given.len());
    for (date, balance, kind) in given {
        let status = if balances
            .iter()
            .any(|earlier| earlier.date == date && earlier.balance == balance)
        {
            FileBalanceStatus::Repeated
        } else if let Some(id) = recorded_balance(conn, account.id, date, balance)? {
            FileBalanceStatus::AlreadyRecorded(id)
        } else if account
            .reconciled_through
            .is_some_and(|through| date <= through)
        {
            FileBalanceStatus::Locked
        } else {
            FileBalanceStatus::New
        };
        balances.push(FileBalance {
            date,
            balance,
            kind,
            status,
        });
    }

    let mut new_movements = DailyMovements::new();
    for (row, outcome) in statement.rows().iter().zip(&outcomes) {
        if let (Ok(line), RowOutcome::New { .. }) = (&row.outcome, outcome) {
            *new_movements.entry(line.date).or_default() += i128::from(line.amount.minor());
        }
    }
    let added: Vec<(usize, Assertion)> = balances
        .iter()
        .enumerate()
        .filter(|(_, balance)| balance.status == FileBalanceStatus::New)
        .map(|(index, balance)| {
            (
                index,
                Assertion {
                    date: balance.date,
                    balance_minor: balance.balance.minor(),
                },
            )
        })
        .collect();
    let restated: HashSet<BalanceId> = balances
        .iter()
        .filter_map(|balance| match balance.status {
            FileBalanceStatus::AlreadyRecorded(id) => Some(id),
            _ => None,
        })
        .collect();
    let checks = reconcile::checks(
        &history,
        &added,
        &new_movements,
        &restated,
        account.currency,
    );

    // Only new rows are mirrored: a row already imported has its pot side.
    let sub_accounts = sub_account::plan(
        conn,
        account,
        statement
            .rows()
            .iter()
            .zip(&outcomes)
            .filter(|(_, outcome)| matches!(outcome, RowOutcome::New { .. }))
            .filter_map(|(row, _)| row.outcome.as_ref().ok()),
    )?;

    Ok(Plan {
        outcomes,
        balances,
        reconciliation: Reconciliation {
            running: running.map(|(running, _)| running),
            checks,
        },
        sub_accounts,
    })
}

/// Dedupe: what happens to each row.
fn classify_rows(
    conn: &Connection,
    account: &Account,
    statement: &Statement,
) -> Result<Vec<RowOutcome>, Error> {
    let mut by_source_id = conn.prepare_cached(
        "SELECT id, date, amount_minor FROM \"transaction\" \
         WHERE account_id = ?1 AND external_id = ?2",
    )?;
    let mut by_fingerprint = conn.prepare_cached(
        "SELECT id FROM \"transaction\" \
         WHERE account_id = ?1 AND external_id IS NULL AND fingerprint = ?2",
    )?;

    let mut occurrences = dedupe::Occurrences::default();
    let mut first_with_id: HashMap<&str, (SourceLocation, &StatementLine)> = HashMap::new();
    let mut outcomes = Vec::with_capacity(statement.rows().len());
    for row in statement.rows() {
        let line = match &row.outcome {
            Ok(line) => line,
            Err(problem) => {
                outcomes.push(RowOutcome::Flagged(Flag::Unreadable(problem.clone())));
                continue;
            }
        };
        let fingerprint = fingerprint(line, occurrences.next(line));
        let same_movement =
            |date: NaiveDate, minor: i64| date == line.date && minor == line.amount.minor();

        let found = if let Some(source_id) = line.external_id.as_deref() {
            if let Some(&(first, earlier)) = first_with_id.get(source_id) {
                outcomes.push(if same_movement(earlier.date, earlier.amount.minor()) {
                    RowOutcome::RepeatedInFile { first }
                } else {
                    RowOutcome::Flagged(Flag::SourceIdRepeated { first })
                });
                continue;
            }
            first_with_id.insert(source_id, (row.location, line));
            by_source_id
                .query_row(params![account.id, source_id], |row| {
                    Ok((
                        row.get::<_, TransactionId>("id")?,
                        account::date(row, "date")?,
                        row.get::<_, i64>("amount_minor")?,
                    ))
                })
                .optional()?
                .map(|(existing, date, minor)| {
                    if same_movement(date, minor) {
                        RowOutcome::AlreadyImported {
                            transaction: existing,
                            matched_by: MatchedBy::SourceId,
                        }
                    } else {
                        RowOutcome::Flagged(Flag::SourceIdConflict { existing })
                    }
                })
        } else {
            by_fingerprint
                .query_row(params![account.id, fingerprint], |row| row.get(0))
                .optional()?
                .map(|existing| RowOutcome::AlreadyImported {
                    transaction: existing,
                    matched_by: MatchedBy::Fingerprint,
                })
        };

        let outcome = match (found, account.reconciled_through) {
            (Some(found), _) => found,
            (None, Some(through)) if line.date <= through => {
                RowOutcome::Flagged(Flag::Locked { through })
            }
            (None, _) => RowOutcome::New { fingerprint },
        };
        outcomes.push(outcome);
    }
    Ok(outcomes)
}

/// The account's balances and transactions before the import. Rows
/// excluded as `duplicate` never happened at the bank, so they leave the
/// balance; every other row counts (decision record "Transactions
/// additions", point 1).
fn history(conn: &Connection, account: &Account) -> Result<reconcile::History, Error> {
    let mut history = reconcile::History::default();
    if let Some((date, balance)) = account.opening_balance {
        history.assertions.push((
            CheckedBalance::Opening,
            Assertion {
                date,
                balance_minor: balance.minor(),
            },
        ));
    }
    let mut snapshots = conn.prepare_cached(
        "SELECT id, date, balance_minor FROM balance_snapshot \
         WHERE account_id = ?1 ORDER BY date, id",
    )?;
    let stored = snapshots.query_map([account.id], |row| {
        Ok((
            CheckedBalance::Stored(row.get("id")?),
            Assertion {
                date: account::date(row, "date")?,
                balance_minor: row.get("balance_minor")?,
            },
        ))
    })?;
    for snapshot in stored {
        history.assertions.push(snapshot?);
    }
    // Stable: the opening balance stays ahead of a snapshot on its day.
    history
        .assertions
        .sort_by_key(|(_, assertion)| assertion.date);

    let mut movements = conn.prepare_cached(
        "SELECT date, SUM(amount_minor) AS moved FROM \"transaction\" \
         WHERE account_id = ?1 AND excluded_reason IS NOT 'duplicate' GROUP BY date",
    )?;
    let days = movements.query_map([account.id], |row| {
        Ok((account::date(row, "date")?, row.get::<_, i64>("moved")?))
    })?;
    for day in days {
        let (date, moved) = day?;
        history.movements.insert(date, i128::from(moved));
    }
    Ok(history)
}

/// A balance from an earlier import of this account, on `date`, equal to
/// `balance`.
fn recorded_balance(
    conn: &Connection,
    account: AccountId,
    date: NaiveDate,
    balance: Money,
) -> Result<Option<BalanceId>, Error> {
    let mut statement = conn.prepare_cached(
        "SELECT id FROM balance_snapshot \
         WHERE account_id = ?1 AND date = ?2 AND balance_minor = ?3 AND batch_id IS NOT NULL \
         ORDER BY id LIMIT 1",
    )?;
    Ok(statement
        .query_row(params![account, date_key(date), balance.minor()], |row| {
            row.get(0)
        })
        .optional()?)
}

fn location_parts(location: SourceLocation) -> (&'static str, i64) {
    let (kind, number) = match location {
        SourceLocation::Line(n) => ("line", n),
        SourceLocation::Entry(n) => ("entry", n),
    };
    (kind, i64::try_from(number).unwrap_or(i64::MAX))
}

// Batches ---------------------------------------------------------------------

/// Whether a batch is still in the account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchStatus {
    Committed,
    Undone,
}

/// One import, for the account's import history and `coffer undo-import`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchInfo {
    pub id: BatchId,
    pub account: AccountId,
    /// The file name, without its directory.
    pub source_file: String,
    pub format: SourceFormat,
    pub preset_id: Option<String>,
    /// UTC, `YYYY-MM-DDTHH:MM:SS.SSSZ`.
    pub imported_at: String,
    pub status: BatchStatus,
    /// Transactions it holds, mirrored pot rows included. 0 once undone.
    pub transactions: usize,
}

/// A flag recorded with a batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedFlag {
    pub location: SourceLocation,
    pub kind: FlagKind,
    /// Core's message; never the row's text.
    pub detail: String,
}

const BATCH_COLUMNS: &str = "b.id, b.account_id, b.source_file, b.format, b.preset_id, \
     b.imported_at, b.status, \
     (SELECT COUNT(*) FROM \"transaction\" t WHERE t.batch_id = b.id) AS transactions";

/// The account's imports, oldest first, undone ones included.
pub fn batches(db: &Database, account: AccountId) -> Result<Vec<BatchInfo>, Error> {
    let mut statement = db.conn().prepare(&format!(
        "SELECT {BATCH_COLUMNS} FROM import_batch b WHERE b.account_id = ?1 ORDER BY b.id"
    ))?;
    let batches: Vec<BatchInfo> = statement
        .query_map([account], batch_from_row)?
        .collect::<Result<_, _>>()?;
    Ok(batches)
}

/// One import.
pub fn batch(db: &Database, batch: BatchId) -> Result<BatchInfo, Error> {
    load_batch(db.conn(), batch)
}

/// The flags recorded with a batch, in file order. Empty once undone.
pub fn batch_flags(db: &Database, batch: BatchId) -> Result<Vec<RecordedFlag>, Error> {
    let mut statement = db.conn().prepare(
        "SELECT location_kind, location, kind, detail FROM import_flag \
         WHERE batch_id = ?1 ORDER BY location_kind, location, id",
    )?;
    let flags: Vec<RecordedFlag> = statement
        .query_map([batch], |row| {
            let location_kind: String = row.get("location_kind")?;
            let number = u64::try_from(row.get::<_, i64>("location")?)
                .map_err(|_| account::invalid(row, "location"))?;
            let location = match location_kind.as_str() {
                "line" => SourceLocation::Line(number),
                "entry" => SourceLocation::Entry(number),
                _ => return Err(account::invalid(row, "location_kind")),
            };
            let kind: String = row.get("kind")?;
            Ok(RecordedFlag {
                location,
                kind: FlagKind::from_key(&kind).ok_or_else(|| account::invalid(row, "kind"))?,
                detail: row.get("detail")?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(flags)
}

fn load_batch(conn: &Connection, batch: BatchId) -> Result<BatchInfo, Error> {
    conn.query_row(
        &format!("SELECT {BATCH_COLUMNS} FROM import_batch b WHERE b.id = ?1"),
        [batch],
        batch_from_row,
    )
    .optional()?
    .ok_or(Error::BatchNotFound(batch))
}

fn batch_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<BatchInfo> {
    let format: String = row.get("format")?;
    let format = [
        SourceFormat::Csv,
        SourceFormat::Ofx,
        SourceFormat::Qif,
        SourceFormat::Camt053,
        SourceFormat::Mt940,
    ]
    .into_iter()
    .find(|candidate| candidate.as_str() == format)
    .ok_or_else(|| account::invalid(row, "format"))?;
    let status: String = row.get("status")?;
    let status = match status.as_str() {
        "committed" => BatchStatus::Committed,
        "undone" => BatchStatus::Undone,
        _ => return Err(account::invalid(row, "status")),
    };
    Ok(BatchInfo {
        id: row.get("id")?,
        account: row.get("account_id")?,
        source_file: row.get("source_file")?,
        format,
        preset_id: row.get("preset_id")?,
        imported_at: row.get("imported_at")?,
        status,
        transactions: row.get::<_, i64>("transactions")?.try_into().unwrap_or(0),
    })
}

// Undo ------------------------------------------------------------------------

/// What undoing a batch removes, for the confirmation. [`undo`] returns
/// the same counts for what it removed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoPreview {
    pub batch: BatchInfo,
    /// Imported rows on the batch's account.
    pub transactions: usize,
    /// Pot-side rows the import created (M1.17).
    pub mirrored: usize,
    pub balances: usize,
    pub flags: usize,
    /// Rows with the user's own edits (category set by the user or an
    /// approved suggestion, note, share, reimbursable, exclusion). The
    /// edits go with the rows; importing the file again brings the rows
    /// back without them.
    pub edited: usize,
    /// Transfer pairs that include one of the rows and a row of another
    /// batch (or a manual one). They are dissolved: the other side stays as
    /// an ordinary transaction, to be paired again when the file is imported
    /// again. A pot transfer and its mirrored row are both the batch's, so
    /// they go together and aren't counted here.
    pub transfers: usize,
    /// Later imports into the same account still committed. Rows they
    /// skipped as already imported came from this batch and go with it;
    /// importing those files again brings them back.
    pub later_batches: Vec<BatchId>,
}

/// What [`undo`] would remove. Fails like `undo` would: the batch must
/// exist, not be undone, and have nothing in a locked period.
pub fn undo_preview(db: &Database, batch: BatchId) -> Result<UndoPreview, Error> {
    let tx = Transaction::new_unchecked(db.conn(), TransactionBehavior::Deferred)?;
    plan_undo(&tx, batch)
}

/// Undoes an import in one DB transaction: deletes its transactions
/// (mirrored pot rows included), its balances and its flags, dissolves
/// transfer pairs that include its rows, and marks it `undone`. The batch
/// row stays, so the account's import history still lists it.
pub fn undo(db: &mut Database, batch: BatchId) -> Result<UndoPreview, Error> {
    let tx = db.conn_mut().transaction()?;
    let undone = plan_undo(&tx, batch)?;

    let groups: Vec<i64> = {
        let mut statement = tx.prepare(
            "SELECT DISTINCT transfer_group_id FROM \"transaction\" \
             WHERE batch_id = ?1 AND transfer_group_id IS NOT NULL",
        )?;
        statement
            .query_map([batch], |row| row.get(0))?
            .collect::<Result<_, _>>()?
    };
    for group in groups {
        tx.execute(
            "UPDATE \"transaction\" SET transfer_group_id = NULL WHERE transfer_group_id = ?1",
            [group],
        )?;
        tx.execute("DELETE FROM transfer_group WHERE id = ?1", [group])?;
    }
    // Tables that reference a transaction (splits, tags, links, duplicate
    // candidates, change items, schedule matches) are cleared here when
    // they arrive. Until then a reference makes the delete fail, and the
    // whole undo with it.
    tx.execute("DELETE FROM \"transaction\" WHERE batch_id = ?1", [batch])?;
    tx.execute("DELETE FROM balance_snapshot WHERE batch_id = ?1", [batch])?;
    tx.execute("DELETE FROM import_flag WHERE batch_id = ?1", [batch])?;
    tx.execute(
        "UPDATE import_batch SET status = 'undone' WHERE id = ?1",
        [batch],
    )?;
    tx.commit()?;
    tracing::info!(
        batch = %batch,
        transactions = undone.transactions,
        mirrored = undone.mirrored,
        balances = undone.balances,
        "undid an import"
    );
    Ok(undone)
}

fn plan_undo(conn: &Connection, batch: BatchId) -> Result<UndoPreview, Error> {
    let info = load_batch(conn, batch)?;
    if info.status == BatchStatus::Undone {
        return Err(Error::AlreadyUndone(batch));
    }

    // Anything on or before its account's lock date (the pot's own lock
    // for a mirrored row) stays until the user unlocks.
    let locked = conn
        .query_row(
            "SELECT a.reconciled_through FROM \"transaction\" t \
             JOIN account a ON a.id = t.account_id \
             WHERE t.batch_id = ?1 AND t.date <= a.reconciled_through \
             UNION ALL \
             SELECT a.reconciled_through FROM balance_snapshot s \
             JOIN account a ON a.id = s.account_id \
             WHERE s.batch_id = ?1 AND s.date <= a.reconciled_through \
             LIMIT 1",
            [batch],
            |row| account::date(row, "reconciled_through"),
        )
        .optional()?;
    if let Some(through) = locked {
        return Err(Error::Locked { through });
    }

    let count = |sql: &str| -> Result<usize, Error> {
        let n: i64 = conn.query_row(sql, [batch], |row| row.get(0))?;
        Ok(usize::try_from(n).unwrap_or(0))
    };
    let later_batches = {
        let mut statement = conn.prepare(
            "SELECT id FROM import_batch \
             WHERE account_id = ?1 AND id > ?2 AND status = 'committed' ORDER BY id",
        )?;
        statement
            .query_map(params![info.account, batch], |row| row.get(0))?
            .collect::<Result<_, _>>()?
    };
    Ok(UndoPreview {
        transactions: count(
            "SELECT COUNT(*) FROM \"transaction\" WHERE batch_id = ?1 AND origin = 'imported'",
        )?,
        mirrored: count(
            "SELECT COUNT(*) FROM \"transaction\" WHERE batch_id = ?1 AND origin = 'mirrored'",
        )?,
        balances: count("SELECT COUNT(*) FROM balance_snapshot WHERE batch_id = ?1")?,
        flags: count("SELECT COUNT(*) FROM import_flag WHERE batch_id = ?1")?,
        edited: count(
            "SELECT COUNT(*) FROM \"transaction\" WHERE batch_id = ?1 AND ( \
             category_source IN ('user', 'ai_approved') OR user_note IS NOT NULL \
             OR my_share_minor IS NOT NULL OR share_label IS NOT NULL OR reimbursable = 1 \
             OR excluded_reason IS NOT NULL)",
        )?,
        transfers: count(
            "SELECT COUNT(DISTINCT t.transfer_group_id) FROM \"transaction\" t \
             WHERE t.batch_id = ?1 AND t.transfer_group_id IS NOT NULL \
             AND EXISTS (SELECT 1 FROM \"transaction\" o \
                         WHERE o.transfer_group_id = t.transfer_group_id \
                         AND o.batch_id IS NOT ?1)",
        )?,
        later_batches,
        batch: info,
    })
}
