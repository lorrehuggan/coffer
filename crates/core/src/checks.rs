//! Coffer's own checks, as the review queues show them (ROADMAP M3.12;
//! `docs/DESIGN.md` → "Review queues"; `docs/ARCHITECTURE.md` → "Decision
//! record: review queues" and "Decision record: review queues (built)").
//!
//! A read model over [`crate::duplicate`] and [`crate::reconciliation`]:
//! nothing here changes the database. Merging, dismissing, locking and
//! unlocking stay those modules' own calls.
//!
//! - **Possible duplicates** ([`Review::pairs`]): each pending candidate
//!   with both rows as the user would compare them, why it was flagged
//!   ([`Why`]), the row a merge keeps by default, and what merging would
//!   do ([`MergeEffect`]).
//! - **What a merge would do** ([`merge_effect`]) is computed by the
//!   merge's own plan ([`crate::duplicate`]) in a DB transaction that is
//!   rolled back, so it can't drift from what merging does: the row it
//!   excludes, the values that move to the kept row, the spending each
//!   category gains or loses, Coffer's count on the date of the next
//!   balance check, and every balance check whose result would change.
//!   A duplicate in a locked period is planned as if unlocked (the lock is
//!   lifted inside the rolled-back transaction), so the user sees what an
//!   unlock would let the merge do; [`MergeEffect::locked`] says it's
//!   locked, and [`crate::duplicate::merge`] still refuses.
//! - **Balance checks** ([`Review::accounts`]): every account with a
//!   balance to check, failing accounts first, each check with the balance
//!   it was counted from, the transactions in its window, and the pending
//!   pair whose merge would make a failing check pass; the dates the
//!   account can be locked through; and the accounts not checked
//!   ([`NotChecked`]).
//!
//! Transaction text is returned as stored. It is untrusted: whoever shows
//! it strips it ([`crate::envelope::shown`]) and acts on none of it.
//! Nothing here logs it, and errors never quote it.

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use chrono::{Days, NaiveDate};
use rusqlite::{Connection, OptionalExtension, params};

use crate::account::{self, Account, AccountKind, date_key};
use crate::category::{CategoryKey, CategoryKind};
use crate::change::{self, Field};
use crate::db::Database;
use crate::duplicate::{
    self, Candidate, CandidateReason, DuplicateWindow, LockedRow, MergeRequest,
};
use crate::id::{AccountId, DuplicateCandidateId, TransactionId};
use crate::inbox::preview::{CategoryRef, SpendingChange};
use crate::manual::Origin;
use crate::money::{Currency, Money};
use crate::reconciliation::{
    self, AccountChecks, AssertionCheck, BalanceRef, BalanceSource, Outcome,
};

/// Why reading the checks failed. Messages never contain transaction text.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Duplicate(#[from] duplicate::Error),

    #[error(transparent)]
    Reconciliation(#[from] reconciliation::Error),

    #[error(transparent)]
    Account(#[from] account::Error),

    #[error(transparent)]
    Change(#[from] change::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

// Possible duplicates ---------------------------------------------------------

/// A row's category as the pair shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowCategory {
    Uncategorised,
    Category(CategoryRef),
    /// Split into lines, each with its own category.
    Split,
}

/// Where a row of a pair came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Added {
    /// From an import of `file`, on `on` (local date).
    Imported { file: String, on: NaiveDate },
    /// The pot side of a pot transfer, made by an import.
    Mirrored,
    /// Typed in by the user on `on` (local date).
    Manual { on: NaiveDate },
}

/// One row of a pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairRow {
    pub transaction: TransactionId,
    pub date: NaiveDate,
    /// `merchant_display`, when there is one. Untrusted text.
    pub merchant: Option<String>,
    pub amount: Money,
    pub category: RowCategory,
    /// `description_raw`: the bank's text, or what the user typed for a
    /// manual row. Untrusted text.
    pub bank_text: String,
    /// The file's own ID for the row (`external_id`).
    pub bank_id: Option<String>,
    pub added: Added,
}

impl PairRow {
    /// The name to show: the display merchant, else the bank text.
    pub fn name(&self) -> &str {
        self.merchant.as_deref().unwrap_or(&self.bank_text)
    }

    pub fn is_manual(&self) -> bool {
        matches!(self.added, Added::Manual { .. })
    }
}

/// How the two rows' bank IDs compare.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BankIds {
    Neither,
    OneOnly,
    /// Both have one, and they differ (different files' ID schemes).
    Different,
    /// Both have the same one (only possible across a manual edit).
    Same,
}

/// Why a pair was flagged: what matched and what kept the exact dedupe
/// from matching it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Why {
    /// Whole days between the two dates.
    pub days_apart: u32,
    /// The window detection used (decision record "review queues", point
    /// 4).
    pub window: DuplicateWindow,
    /// Whether the normalized bank text is the same.
    pub same_text: bool,
    pub bank_ids: BankIds,
    /// Whether one row was typed in by the user: a manual row has no
    /// dedupe keys at all.
    pub manual: bool,
}

/// A pending possible duplicate, as the queue lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pair {
    pub candidate: DuplicateCandidateId,
    pub reason: CandidateReason,
    pub account: AccountId,
    pub account_name: String,
    /// Kept by default first, then the other.
    pub rows: [PairRow; 2],
    pub why: Why,
    /// What merging with the default keep would do, or core's reason it
    /// can't (IDs only, never text).
    pub effect: Result<MergeEffect, String>,
}

impl Pair {
    /// The row a merge keeps unless told otherwise.
    pub fn default_keep(&self) -> TransactionId {
        self.rows[0].transaction
    }

    pub fn row(&self, transaction: TransactionId) -> Option<&PairRow> {
        self.rows.iter().find(|row| row.transaction == transaction)
    }

    /// The other row of the pair.
    pub fn other(&self, transaction: TransactionId) -> &PairRow {
        if self.rows[0].transaction == transaction {
            &self.rows[1]
        } else {
            &self.rows[0]
        }
    }
}

/// How a balance check's result would change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckChange {
    pub balance: BalanceRef,
    pub source: BalanceSource,
    pub date: NaiveDate,
    pub stated: Money,
    pub before: Outcome,
    pub after: Outcome,
}

impl CheckChange {
    /// A failing check that would pass.
    pub fn fixes(&self) -> bool {
        self.before.is_failed() && self.after.is_passed()
    }

    /// A passing check that would fail.
    pub fn breaks(&self) -> bool {
        self.before.is_passed() && self.after.is_failed()
    }
}

/// Coffer's count on one day, before and after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CountChange {
    pub date: NaiveDate,
    pub before: Money,
    pub after: Money,
}

/// What merging a pair one way would do (module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeEffect {
    pub kept: TransactionId,
    /// Excluded as `duplicate`.
    pub excluded: TransactionId,
    /// The values that move from the excluded row to the kept one.
    pub carried: Vec<Field>,
    /// What each category gains (positive) or loses (negative) in
    /// spending, losses first.
    pub spending: Vec<SpendingChange>,
    /// Coffer's count on the date of the first balance check on or after
    /// the excluded row, when there is one.
    pub count: Option<CountChange>,
    /// The balance checks whose result would change, by date.
    pub checks: Vec<CheckChange>,
    /// Set when the excluded row is in its account's locked period:
    /// merging needs an unlock first.
    pub locked: Option<LockedRow>,
}

/// What merging `request` would do (module docs), planned by the merge's
/// own code in a rolled-back DB transaction. Fails as
/// [`duplicate::merge_preview`] would, except for a lock, which is
/// reported in [`MergeEffect::locked`].
pub fn merge_effect(db: &Database, request: MergeRequest) -> Result<MergeEffect, Error> {
    change::dry_run(db, |conn| effect_in(conn, request))
}

fn effect_in(conn: &Connection, request: MergeRequest) -> Result<MergeEffect, Error> {
    let candidate = duplicate::load_candidate(conn, request.candidate)?;
    let account_id: AccountId = conn.query_row(
        "SELECT account_id FROM \"transaction\" WHERE id = ?1",
        [candidate.transaction_a],
        |row| row.get(0),
    )?;
    let stored = account::find(conn, account_id)?.ok_or(Error::Reconciliation(
        reconciliation::Error::AccountNotFound(account_id),
    ))?;

    let keep = match request.keep {
        Some(keep) => keep,
        None => duplicate::default_keep_in(conn, candidate.id)?,
    };
    let other = if keep == candidate.transaction_a {
        candidate.transaction_b
    } else {
        candidate.transaction_a
    };
    // What the merge meets: a lock on the row it would exclude.
    let locked = match change::locked(conn, other, Field::Exclusion)? {
        Some(through) => Some(LockedRow {
            candidate: candidate.id,
            transaction: other,
            date: conn.query_row(
                "SELECT date FROM \"transaction\" WHERE id = ?1",
                [other],
                |row| account::date(row, "date"),
            )?,
            through,
        }),
        None => None,
    };
    let before_checks = reconciliation::load(conn, &stored)?;
    let rows = [candidate.transaction_a, candidate.transaction_b];
    let before_spending = spending_of(conn, &rows)?;

    // Then what an unlock would let it do: the lock is lifted in this
    // rolled-back transaction only.
    if stored.reconciled_through.is_some() {
        conn.execute(
            "UPDATE account SET reconciled_through = NULL WHERE id = ?1",
            [account_id],
        )?;
    }
    let (plan, _) = duplicate::plan(conn, &[MergeRequest::keeping(candidate.id, keep)])?;
    let merge = plan
        .merges
        .first()
        .cloned()
        .expect("a plan of one request has one merge");
    let after_checks = reconciliation::load(conn, &stored)?;
    let after_spending = spending_of(conn, &rows)?;

    let excluded_date: NaiveDate = conn.query_row(
        "SELECT date FROM \"transaction\" WHERE id = ?1",
        [merge.duplicate],
        |row| account::date(row, "date"),
    )?;
    Ok(MergeEffect {
        kept: merge.kept,
        excluded: merge.duplicate,
        carried: merge.carried,
        spending: spending_change(before_spending, after_spending, stored.currency),
        count: count_change(&before_checks, &after_checks, excluded_date),
        checks: check_changes(&before_checks, &after_checks),
        locked,
    })
}

/// Spent per category: the category (`None`: uncategorised money out) and
/// minor units.
type Spent = BTreeMap<Option<CategoryKey>, (Option<CategoryRef>, i64)>;

/// What each category counts as spent over `rows`' ledger lines: money out
/// in a spending category, or uncategorised, in reports, as the user's own.
fn spending_of(conn: &Connection, rows: &[TransactionId]) -> rusqlite::Result<Spent> {
    let mut statement = conn.prepare_cached(
        "SELECT c.key, c.label, c.kind, l.personal_minor, l.amount_minor \
         FROM ledger_line l LEFT JOIN category c ON c.id = l.category_id \
         WHERE l.transaction_id = ?1 AND l.in_reports",
    )?;
    let mut spent = BTreeMap::new();
    for row in rows {
        let lines = statement.query_map([row], |row| {
            let key: Option<String> = row.get(0)?;
            let category = match key {
                Some(key) => Some(CategoryRef {
                    key: CategoryKey::new(&key).map_err(|_| account::invalid(row, "key"))?,
                    label: row.get(1)?,
                    kind: row.get(2)?,
                }),
                None => None,
            };
            Ok((category, row.get::<_, i64>(3)?, row.get::<_, i64>(4)?))
        })?;
        for line in lines {
            let (category, personal, amount) = line?;
            let spending = match &category {
                Some(category) => category.kind == CategoryKind::Spending,
                None => amount < 0,
            };
            if !spending {
                continue;
            }
            let entry = spent
                .entry(category.as_ref().map(|c| c.key.clone()))
                .or_insert((category, 0_i64));
            entry.1 = entry.1.saturating_sub(personal);
        }
    }
    Ok(spent)
}

fn spending_change(before: Spent, after: Spent, currency: Currency) -> Vec<SpendingChange> {
    let mut changes: Spent = BTreeMap::new();
    for (key, (category, spent)) in before {
        changes.insert(key, (category, spent.saturating_neg()));
    }
    for (key, (category, spent)) in after {
        let entry = changes.entry(key).or_insert((category, 0));
        entry.1 = entry.1.saturating_add(spent);
    }
    let mut changes: Vec<SpendingChange> = changes
        .into_values()
        .filter(|(_, change)| *change != 0)
        .map(|(category, change)| SpendingChange {
            category,
            change: Money::new(change, currency),
        })
        .collect();
    // Losses first, then gains, each biggest first.
    changes.sort_by_key(|change| {
        let minor = change.change.minor();
        (minor > 0, std::cmp::Reverse(minor.unsigned_abs()))
    });
    changes
}

/// What Coffer counts on a check's day: the stated balance when it passed,
/// else what the transactions give.
fn counted(check: &AssertionCheck) -> Option<Money> {
    match check.outcome {
        Outcome::Passed { .. } => Some(check.stated),
        Outcome::Failed { .. } => check.computed(),
        _ => None,
    }
}

fn count_change(
    before: &AccountChecks,
    after: &AccountChecks,
    from: NaiveDate,
) -> Option<CountChange> {
    let (index, check) = before
        .checks
        .iter()
        .enumerate()
        .find(|(_, check)| check.date >= from && check.outcome.is_check())?;
    let after = after.checks.get(index)?;
    Some(CountChange {
        date: check.date,
        before: counted(check)?,
        after: counted(after)?,
    })
}

fn check_changes(before: &AccountChecks, after: &AccountChecks) -> Vec<CheckChange> {
    before
        .checks
        .iter()
        .zip(&after.checks)
        .filter(|(old, new)| {
            old.outcome.is_check() && old.outcome.is_failed() != new.outcome.is_failed()
        })
        .map(|(old, new)| CheckChange {
            balance: old.balance,
            source: old.source,
            date: old.date,
            stated: old.stated,
            before: old.outcome,
            after: new.outcome,
        })
        .collect()
}

/// The pending pair `candidate`, with its rows ordered for a merge keeping
/// `keep` (the default when `None`) and that merge's effect.
pub fn pair(
    db: &Database,
    candidate: DuplicateCandidateId,
    keep: Option<TransactionId>,
) -> Result<Pair, Error> {
    let found = duplicate::get(db, candidate)?;
    load_pair(db, &found, keep)
}

fn load_pair(
    db: &Database,
    candidate: &Candidate,
    keep: Option<TransactionId>,
) -> Result<Pair, Error> {
    let conn = db.conn();
    let a = load_row(conn, candidate.transaction_a)?;
    let b = load_row(conn, candidate.transaction_b)?;
    let keep = match keep {
        Some(keep) => keep,
        None => duplicate::default_keep(db, candidate.id)?,
    };
    let request = MergeRequest::keeping(candidate.id, keep);
    let effect = merge_effect(db, request).map_err(|error| error.to_string());
    let (account, account_name) = conn.query_row(
        "SELECT a.id, a.name FROM \"transaction\" t JOIN account a ON a.id = t.account_id \
         WHERE t.id = ?1",
        [candidate.transaction_a],
        |row| Ok((row.get::<_, AccountId>(0)?, row.get::<_, String>(1)?)),
    )?;
    let why = why(&a, &b);
    let rows = if a.row.transaction == keep {
        [a.row, b.row]
    } else {
        [b.row, a.row]
    };
    Ok(Pair {
        candidate: candidate.id,
        reason: candidate.reason,
        account,
        account_name,
        rows,
        why,
        effect,
    })
}

struct Loaded {
    row: PairRow,
    normalized: String,
}

fn load_row(conn: &Connection, id: TransactionId) -> Result<Loaded, Error> {
    let loaded = conn
        .query_row(
            "SELECT t.id, t.date, t.merchant_display, t.amount_minor, t.currency, \
             t.description_raw, t.description_normalized, t.external_id, t.origin, \
             date(t.created_at, 'localtime') AS created_on, \
             b.source_file, date(b.imported_at, 'localtime') AS imported_on, \
             c.key, c.label, c.kind, \
             EXISTS (SELECT 1 FROM transaction_split s WHERE s.transaction_id = t.id) AS split \
             FROM \"transaction\" t \
             LEFT JOIN import_batch b ON b.id = t.batch_id \
             LEFT JOIN category c ON c.id = t.category_id \
             WHERE t.id = ?1",
            [id],
            |row| {
                let currency: String = row.get("currency")?;
                let currency =
                    Currency::new(&currency).map_err(|_| account::invalid(row, "currency"))?;
                let origin: String = row.get("origin")?;
                let origin =
                    Origin::from_key(&origin).ok_or_else(|| account::invalid(row, "origin"))?;
                let added = match origin {
                    Origin::Manual => Added::Manual {
                        on: account::date(row, "created_on")?,
                    },
                    Origin::Mirrored => Added::Mirrored,
                    Origin::Imported => Added::Imported {
                        file: row
                            .get::<_, Option<String>>("source_file")?
                            .unwrap_or_default(),
                        on: account::date(row, "imported_on")?,
                    },
                };
                let key: Option<String> = row.get("key")?;
                let category = if row.get::<_, bool>("split")? {
                    RowCategory::Split
                } else {
                    match key {
                        Some(key) => RowCategory::Category(CategoryRef {
                            key: CategoryKey::new(&key)
                                .map_err(|_| account::invalid(row, "key"))?,
                            label: row.get("label")?,
                            kind: row.get("kind")?,
                        }),
                        None => RowCategory::Uncategorised,
                    }
                };
                Ok(Loaded {
                    row: PairRow {
                        transaction: row.get("id")?,
                        date: account::date(row, "date")?,
                        merchant: row.get("merchant_display")?,
                        amount: Money::new(row.get("amount_minor")?, currency),
                        category,
                        bank_text: row.get("description_raw")?,
                        bank_id: row.get("external_id")?,
                        added,
                    },
                    normalized: row.get("description_normalized")?,
                })
            },
        )
        .optional()?;
    loaded.ok_or(Error::Duplicate(duplicate::Error::TransactionNotFound(id)))
}

fn why(a: &Loaded, b: &Loaded) -> Why {
    let bank_ids = match (&a.row.bank_id, &b.row.bank_id) {
        (None, None) => BankIds::Neither,
        (Some(_), None) | (None, Some(_)) => BankIds::OneOnly,
        (Some(x), Some(y)) if x == y => BankIds::Same,
        (Some(_), Some(_)) => BankIds::Different,
    };
    Why {
        days_apart: u32::try_from((a.row.date - b.row.date).num_days().unsigned_abs())
            .unwrap_or(u32::MAX),
        window: DuplicateWindow::DEFAULT,
        same_text: a.normalized == b.normalized,
        bank_ids,
        manual: a.row.is_manual() || b.row.is_manual(),
    }
}

// Balance checks --------------------------------------------------------------

/// One balance of an account, checked, with what the queue says about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckRow {
    pub check: AssertionCheck,
    /// The balance it was counted from (the last that passed, or the
    /// starting point): what was "matched" on [`Self::since`].
    pub base: Option<Money>,
    /// Transactions in the window it was counted over (after the base's
    /// day, up to and including its own), not counting duplicates.
    pub transactions: usize,
    /// For a failing check: the pending pair whose merge (keeping the
    /// default row) would make it pass.
    pub explained_by: Option<DuplicateCandidateId>,
}

impl CheckRow {
    /// The day the count started from, for a check.
    pub fn since(&self) -> Option<NaiveDate> {
        match self.check.outcome {
            Outcome::Passed { since } | Outcome::Failed { since, .. } => Some(since),
            _ => None,
        }
    }

    /// The days counted, first and last included, for a check: from the
    /// day after the base to the check's own.
    pub fn days(&self) -> Option<(NaiveDate, NaiveDate)> {
        let since = self.since()?;
        Some((
            since.checked_add_days(Days::new(1)).unwrap_or(since),
            self.check.date,
        ))
    }
}

/// An account's balance checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountReview {
    pub account: AccountId,
    pub name: String,
    pub kind: AccountKind,
    pub currency: Currency,
    /// The account's preset ID (`uk-monzo`): its exports' quirks, such as
    /// carrying no balances.
    pub preset: Option<String>,
    pub locked_through: Option<NaiveDate>,
    /// By date, oldest first (the queue lists them newest first).
    pub rows: Vec<CheckRow>,
    /// The dates the account can be locked through, latest first: passing
    /// checks with no failure on or before them, after the current lock.
    pub lockable: Vec<NaiveDate>,
}

impl AccountReview {
    pub fn failures(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| row.check.outcome.is_failed())
            .count()
    }

    /// Whether there is anything to check yet: at least one balance after
    /// the starting point.
    pub fn has_checks(&self) -> bool {
        self.rows.iter().any(|row| row.check.outcome.is_check())
    }
}

/// The accounts the balance checks leave out, by name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotChecked {
    /// Accounts with no balance at all.
    pub no_balance: Vec<String>,
    /// Asset and liability accounts: a valuation sets the balance.
    pub valued: Vec<String>,
    /// Pots with nothing to check: they follow their transfers.
    pub pots: usize,
}

/// Both review queues.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Review {
    /// The pending possible duplicates, oldest first.
    pub pairs: Vec<Pair>,
    /// Accounts with a balance, failing ones first, then in the order they
    /// were made.
    pub accounts: Vec<AccountReview>,
    pub not_checked: NotChecked,
}

impl Review {
    /// Failing balance checks, over every account.
    pub fn failing(&self) -> usize {
        self.accounts.iter().map(AccountReview::failures).sum()
    }

    pub fn pair(&self, candidate: DuplicateCandidateId) -> Option<&Pair> {
        self.pairs.iter().find(|pair| pair.candidate == candidate)
    }

    pub fn account(&self, account: AccountId) -> Option<&AccountReview> {
        self.accounts
            .iter()
            .find(|review| review.account == account)
    }
}

/// Both queues, read now (module docs).
pub fn review(db: &Database) -> Result<Review, Error> {
    let mut pairs = Vec::new();
    for candidate in duplicate::pending(db, None)? {
        pairs.push(load_pair(db, &candidate, None)?);
    }
    let (accounts, not_checked) = balance_review(db, &pairs)?;
    Ok(Review {
        pairs,
        accounts,
        not_checked,
    })
}

/// The number of failing balance checks, over every account: the status
/// bar's count, without reading the pairs.
pub fn failing(db: &Database) -> Result<usize, Error> {
    Ok(reconciliation::all_checks(db)?
        .iter()
        .map(|checks| checks.failures().count())
        .sum())
}

fn balance_review(
    db: &Database,
    pairs: &[Pair],
) -> Result<(Vec<AccountReview>, NotChecked), Error> {
    let conn = db.conn();
    let mut accounts = Vec::new();
    let mut not_checked = NotChecked::default();
    for stored in account::list(db)? {
        let checks = reconciliation::load(conn, &stored)?;
        if matches!(stored.kind, AccountKind::Asset | AccountKind::Liability) {
            not_checked.valued.push(stored.name.clone());
            continue;
        }
        if checks.checks.is_empty() {
            if stored.parent.is_some() {
                not_checked.pots += 1;
            } else {
                not_checked.no_balance.push(stored.name.clone());
            }
            continue;
        }
        let has_checks = checks.checks.iter().any(|check| check.outcome.is_check());
        if stored.parent.is_some() && !has_checks {
            not_checked.pots += 1;
            continue;
        }
        accounts.push(account_review(conn, &stored, &checks, pairs)?);
    }
    // Failing accounts first; the sort is stable, so creation order stays.
    accounts.sort_by_key(|review| review.failures() == 0);
    Ok((accounts, not_checked))
}

fn account_review(
    conn: &Connection,
    stored: &Account,
    checks: &AccountChecks,
    pairs: &[Pair],
) -> Result<AccountReview, Error> {
    let mut count = conn.prepare_cached(
        "SELECT COUNT(*) FROM \"transaction\" WHERE account_id = ?1 AND date > ?2 \
         AND date <= ?3 AND excluded_reason IS NOT 'duplicate'",
    )?;
    let mut rows = Vec::with_capacity(checks.checks.len());
    for check in &checks.checks {
        let since = match check.outcome {
            Outcome::Passed { since } | Outcome::Failed { since, .. } => Some(since),
            _ => None,
        };
        let base = since.and_then(|since| base_on(checks, since));
        let transactions = match since {
            Some(since) => count.query_row(
                params![stored.id, date_key(since), date_key(check.date)],
                |row| row.get::<_, i64>(0),
            )?,
            None => 0,
        };
        let explained_by = if check.outcome.is_failed() {
            pairs
                .iter()
                .filter(|pair| pair.account == stored.id)
                .find(|pair| {
                    pair.effect.as_ref().is_ok_and(|effect| {
                        effect
                            .checks
                            .iter()
                            .any(|change| change.balance == check.balance && change.fixes())
                    })
                })
                .map(|pair| pair.candidate)
        } else {
            None
        };
        rows.push(CheckRow {
            check: *check,
            base,
            transactions: usize::try_from(transactions).unwrap_or(0),
            explained_by,
        });
    }
    Ok(AccountReview {
        account: stored.id,
        name: stored.name.clone(),
        kind: stored.kind,
        currency: stored.currency,
        preset: stored.preset.clone(),
        locked_through: stored.reconciled_through,
        rows,
        lockable: lockable(checks),
    })
}

/// The balance counting restarted from on `since`: the last one on that
/// day that passed, or is a starting point or valuation.
fn base_on(checks: &AccountChecks, since: NaiveDate) -> Option<Money> {
    checks
        .checks
        .iter()
        .rev()
        .find(|check| check.date == since && !check.outcome.is_failed())
        .map(|check| check.stated)
}

/// The dates [`reconciliation::lock`] would accept, after the current
/// lock, latest first.
fn lockable(checks: &AccountChecks) -> Vec<NaiveDate> {
    let Some(latest) = checks.lockable_through() else {
        return Vec::new();
    };
    let mut dates: Vec<NaiveDate> = checks
        .checks
        .iter()
        .filter(|check| check.outcome.is_passed())
        .map(|check| check.date)
        .filter(|date| *date <= latest)
        .filter(|date| checks.locked_through.is_none_or(|locked| *date > locked))
        .collect();
    dates.sort_unstable_by(|a, b| b.cmp(a));
    dates.dedup();
    dates
}
