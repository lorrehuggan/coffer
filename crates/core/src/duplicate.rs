//! Possible duplicates (M1.26): the same transaction stored twice in one
//! account, which the exact dedupe at import couldn't match.
//!
//! The import refuses exact duplicates by source ID, then fingerprint. What
//! gets past it is the same payment told two ways: overlapping CSV and OFX
//! exports of one account (different source-ID schemes, different wording),
//! or a manual row entered ahead of the bank's export (a manual row has no
//! dedupe keys). Such pairs are recorded as `pending` candidates
//! (`migrations/V10__duplicate_candidate.sql`) for the user to merge or
//! dismiss.
//!
//! # Detection
//!
//! [`detect_for_batch`] runs inside the import's commit; [`scan`] looks at
//! rows already stored, in a DB transaction of its own. Two transactions
//! are a candidate pair when:
//!
//! - they are on the same account, in the same currency, with the same
//!   non-zero amount;
//! - their dates are at most the [`DuplicateWindow`] apart, both ends
//!   included (3 days by default: review queues decision point 4);
//! - neither is excluded (a merged duplicate is excluded, so it is never
//!   paired again);
//! - they didn't come from the same import: one file never holds one
//!   transaction twice, and identical rows in a file are already told apart
//!   by the dedupe's occurrence index;
//! - at least one was imported (`imported` or `mirrored`). Two manual rows
//!   were both typed by the user, who can see them;
//! - when both were imported, their `description_normalized` differs. Two
//!   imported rows with the same text are the exact dedupe's business, or a
//!   genuine repeat (the same coffee two days running). A manual row and an
//!   imported row pair whatever their text: nothing else ever matches a
//!   manual row against the bank's;
//! - the pair isn't recorded already, in any status: a dismissed pair stays
//!   dismissed (review queues decision point 5), and a merged one is done.
//!   A third matching row makes a new pair with one of the two.
//!
//! The merchant decides nothing on its own; it orders. Each transaction is
//! in at most one *new* pair per run (a row in an earlier, pending pair can
//! be in a new one). When a row has several possible counterparts, pairs are
//! taken closest first: the smallest gap in days, then pairs whose display
//! merchants agree (ignoring case) before those that don't, then the lower
//! and the higher transaction ID. The result depends only on row values and
//! IDs, so it is deterministic: two £3 coffees at two cafés on one day, in
//! both a CSV and an OFX export, pair café with café when the names agree,
//! and first with first when they don't. Transfers and split rows take part
//! like any row: an overlapping export repeats them too.
//!
//! # Merging
//!
//! A merge is one `duplicate_merge` change batch ([`crate::change`]), for
//! one candidate or many: [`merge_preview`] runs the plan in a DB
//! transaction that is rolled back, and [`merge`] runs it again and refuses
//! ([`Error::Stale`]) if anything differs from the preview.
//!
//! - **The duplicate is excluded, never deleted**, with reason `duplicate`,
//!   so it also leaves the account's balance. The candidate becomes
//!   `merged`, naming the batch and the side kept.
//! - **Which row stays** ([`default_keep`]): the decision record's order
//!   (review queues point 3), behind one rule the merge needs: a row in a
//!   suggested or confirmed transfer is kept over one that isn't, since
//!   excluding a transfer's side is refused ([`Error::InTransfer`]); then
//!   the row with a source ID; then an imported (or mirrored) row over a
//!   manual one; then the one imported first (lower import batch); then the
//!   lower ID. [`MergeRequest::keeping`] swaps.
//! - **What the user set on the duplicate moves to the kept row where the
//!   kept row has nothing**, as items in the same batch, so undo takes it
//!   back: its split (when the kept row isn't split, isn't in a transfer, has
//!   no user-set category and no share: the kept row's category is cleared
//!   first, as a split needs); its category when the kept row is
//!   uncategorised, or when the duplicate's was set by the user (or an
//!   approved suggestion) and the kept row's wasn't; its share when the kept
//!   row has none; a user-set merchant name over a derived or rule-set one;
//!   its note when the kept row has none; its tags (added); `reimbursable`.
//!   Nothing on the kept row is ever overwritten except a category or name
//!   a rule or preset set, by the user's own. The duplicate keeps its own
//!   values.
//! - **Locks.** Excluding a row as `duplicate` changes its account's
//!   balance, so a duplicate dated on or before `reconciled_through` is
//!   listed in [`MergePreview::locked`] and [`merge`] refuses
//!   ([`Error::Locked`]) until the user unlocks. Undo meets the same lock
//!   ([`change::Error::Locked`]).
//! - **Undo** is [`change::undo`]. It restores what still holds (rows
//!   changed since keep the later value) and puts every candidate the batch
//!   merged back to `pending`, whatever happened to its rows since: the pair
//!   is still a possible duplicate, and the user decides again (a pending
//!   candidate with an excluded row can't be merged; it can be dismissed).
//!
//! A merge refuses a candidate whose rows no longer match in account,
//! currency and amount (a manual row edited since), one with an excluded
//! row, and a set of candidates where one row would be both kept and
//! excluded ([`Error::Conflict`]).
//!
//! # Deleting transactions
//!
//! Deleting a transaction deletes the candidates that name it ([`clear`],
//! [`clear_batch`]), whatever their status. Nothing here logs transaction
//! text, and errors never quote it.

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;

use chrono::{Days, NaiveDate};
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};

use crate::account::{self, date_key};
use crate::change::{
    self, CategorySource, CategoryValue, Change, ChangeKind, ExclusionValue, Field, NameSource,
    Recorder, ShareValue, Value,
};
use crate::db::Database;
use crate::exclusion::ExclusionReason;
use crate::id::{AccountId, BatchId, ChangeBatchId, DuplicateCandidateId, TransactionId};
use crate::manual::Origin;
use crate::split;

/// Why finding, merging or reading duplicates failed. Messages never
/// contain transaction text.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("a duplicate window is at most {max} days, not {0}", max = DuplicateWindow::MAX_DAYS)]
    WindowTooLong(u32),

    #[error("there is no import batch {0}")]
    BatchNotFound(BatchId),

    #[error("there is no account {0}")]
    AccountNotFound(AccountId),

    #[error("there is no transaction {0}")]
    TransactionNotFound(TransactionId),

    #[error("there is no duplicate candidate {0}")]
    CandidateNotFound(DuplicateCandidateId),

    #[error("duplicate candidate {candidate} is {status}, not pending")]
    NotPending {
        candidate: DuplicateCandidateId,
        status: CandidateStatus,
    },

    #[error("duplicate candidate {0} isn't dismissed")]
    NotDismissed(DuplicateCandidateId),

    #[error("transaction {transaction} isn't one of duplicate candidate {candidate}")]
    NotInPair {
        candidate: DuplicateCandidateId,
        transaction: TransactionId,
    },

    #[error(
        "the rows of duplicate candidate {0} are no longer on one account with one amount; \
         dismiss it"
    )]
    NoLongerAMatch(DuplicateCandidateId),

    #[error(
        "transaction {transaction} of duplicate candidate {candidate} is excluded; include it \
         again or dismiss the candidate"
    )]
    Excluded {
        candidate: DuplicateCandidateId,
        transaction: TransactionId,
    },

    #[error(
        "transaction {transaction} of duplicate candidate {candidate} is in a transfer, so it \
         can't be the duplicate; keep it instead"
    )]
    InTransfer {
        candidate: DuplicateCandidateId,
        transaction: TransactionId,
    },

    #[error("transaction {0} would be both kept and excluded, or excluded twice, in one merge")]
    Conflict(TransactionId),

    #[error("duplicate candidate {0} is in the merge twice")]
    Repeated(DuplicateCandidateId),

    #[error("there is nothing to merge")]
    NothingToMerge,

    #[error(
        "transaction {transaction} is dated {date}, on or before {through}, which is locked, \
         and excluding it as a duplicate changes the balance there; unlock the account first"
    )]
    Locked {
        transaction: TransactionId,
        date: NaiveDate,
        through: NaiveDate,
    },

    #[error("the duplicates changed since the preview; preview the merge again")]
    Stale,

    #[error("change batch: {0}")]
    Change(#[from] change::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// How many whole days apart two copies of one transaction may be dated:
/// exports date a payment by when it was made or when it posted. Both ends
/// are included, so `0` means the same day.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DuplicateWindow(u32);

impl DuplicateWindow {
    /// The longest window: a month.
    pub const MAX_DAYS: u32 = 31;

    /// Three days (review queues decision point 4). A default, not an
    /// invariant.
    pub const DEFAULT: DuplicateWindow = DuplicateWindow(3);

    /// A window of `days`, at most [`DuplicateWindow::MAX_DAYS`].
    pub fn new(days: u32) -> Result<Self, Error> {
        if days > Self::MAX_DAYS {
            return Err(Error::WindowTooLong(days));
        }
        Ok(DuplicateWindow(days))
    }

    pub const fn days(self) -> u32 {
        self.0
    }
}

impl Default for DuplicateWindow {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Which kind of match a candidate is (`duplicate_candidate.reason`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CandidateReason {
    /// Two imported rows from different imports: overlapping exports.
    OverlappingImport,
    /// A manual row and an imported one: a row entered ahead of the
    /// bank's export.
    ManualEntry,
}

impl CandidateReason {
    pub const ALL: [CandidateReason; 2] = [
        CandidateReason::OverlappingImport,
        CandidateReason::ManualEntry,
    ];

    /// The key stored in `duplicate_candidate.reason`.
    pub const fn as_str(self) -> &'static str {
        match self {
            CandidateReason::OverlappingImport => "overlapping_import",
            CandidateReason::ManualEntry => "manual_entry",
        }
    }

    pub fn from_key(key: &str) -> Option<CandidateReason> {
        Self::ALL.into_iter().find(|reason| reason.as_str() == key)
    }
}

/// Where a candidate is in review (`duplicate_candidate.status`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CandidateStatus {
    /// Waiting for the user.
    Pending,
    /// One row was excluded as the other's duplicate.
    Merged,
    /// The user said the two are different transactions. Never raised
    /// again.
    Dismissed,
}

impl CandidateStatus {
    pub const ALL: [CandidateStatus; 3] = [
        CandidateStatus::Pending,
        CandidateStatus::Merged,
        CandidateStatus::Dismissed,
    ];

    /// The key stored in `duplicate_candidate.status`.
    pub const fn as_str(self) -> &'static str {
        match self {
            CandidateStatus::Pending => "pending",
            CandidateStatus::Merged => "merged",
            CandidateStatus::Dismissed => "dismissed",
        }
    }

    pub fn from_key(key: &str) -> Option<CandidateStatus> {
        Self::ALL.into_iter().find(|status| status.as_str() == key)
    }
}

impl fmt::Display for CandidateStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A stored candidate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub id: DuplicateCandidateId,
    /// The lower transaction ID of the pair.
    pub transaction_a: TransactionId,
    /// The higher one.
    pub transaction_b: TransactionId,
    pub reason: CandidateReason,
    pub status: CandidateStatus,
    /// The row kept, while merged.
    pub kept: Option<TransactionId>,
    /// The batch that merged it, while merged.
    pub change_batch: Option<ChangeBatchId>,
    /// UTC, `YYYY-MM-DDTHH:MM:SS.SSSZ`.
    pub created_at: String,
}

impl Candidate {
    /// The row excluded as the duplicate, while merged.
    pub fn duplicate(&self) -> Option<TransactionId> {
        self.kept.map(|kept| self.other(kept))
    }

    fn other(&self, transaction: TransactionId) -> TransactionId {
        if transaction == self.transaction_a {
            self.transaction_b
        } else {
            self.transaction_a
        }
    }
}

/// A candidate detection recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewCandidate {
    pub id: DuplicateCandidateId,
    pub transaction_a: TransactionId,
    pub transaction_b: TransactionId,
    pub reason: CandidateReason,
}

// Detection -------------------------------------------------------------------

/// Finds possible duplicates of the transactions of import `batch`, each
/// matched against every stored transaction (module docs, "Detection"), and
/// records them as `pending` candidates. Returns the new candidates in the
/// order they were recorded: by lower, then higher transaction ID.
///
/// Writes through `conn` without opening a transaction of its own, so it
/// runs inside the import's commit. An undone batch has no transactions
/// left and gets no candidates.
pub fn detect_for_batch(
    conn: &Connection,
    batch: BatchId,
    window: DuplicateWindow,
) -> Result<Vec<NewCandidate>, Error> {
    let exists = conn
        .query_row("SELECT 1 FROM import_batch WHERE id = ?1", [batch], |_| {
            Ok(())
        })
        .optional()?
        .is_some();
    if !exists {
        return Err(Error::BatchNotFound(batch));
    }
    let transactions = ids(
        conn,
        "SELECT id FROM \"transaction\" WHERE batch_id = ?1 ORDER BY id",
        [batch.get()],
    )?;
    let found = detect(conn, &transactions, window)?;
    tracing::debug!(
        batch = %batch,
        transactions = transactions.len(),
        candidates = found.len(),
        "found possible duplicates"
    );
    Ok(found)
}

/// Finds possible duplicates among the transactions already stored, on
/// `account` or on every account, and records them, in a DB transaction of
/// its own. For history imported before detection ran, or after a pair was
/// resolved.
pub fn scan(
    db: &mut Database,
    account: Option<AccountId>,
    window: DuplicateWindow,
) -> Result<Vec<NewCandidate>, Error> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let transactions = match account {
        Some(account) => {
            let exists = tx
                .query_row("SELECT 1 FROM account WHERE id = ?1", [account], |_| Ok(()))
                .optional()?
                .is_some();
            if !exists {
                return Err(Error::AccountNotFound(account));
            }
            ids(
                &tx,
                "SELECT id FROM \"transaction\" WHERE account_id = ?1 ORDER BY id",
                [account.get()],
            )?
        }
        None => ids(&tx, "SELECT id FROM \"transaction\" ORDER BY id", params![])?,
    };
    let found = detect(&tx, &transactions, window)?;
    tx.commit()?;
    tracing::info!(
        transactions = transactions.len(),
        candidates = found.len(),
        "scanned for possible duplicates"
    );
    Ok(found)
}

fn ids<P: rusqlite::Params>(
    conn: &Connection,
    sql: &str,
    params: P,
) -> rusqlite::Result<Vec<TransactionId>> {
    let mut statement = conn.prepare(sql)?;
    statement
        .query_map(params, |row| row.get(0))?
        .collect::<Result<_, _>>()
}

/// [`detect_for_batch`] for any set of transactions: records a candidate
/// for each of `transactions` that has a counterpart among every stored
/// transaction. One that can't pair (excluded, zero) is skipped; one that
/// doesn't exist is an error. The order of `transactions` and repeats in it
/// don't matter. Writes through `conn` without a transaction of its own.
pub fn detect(
    conn: &Connection,
    transactions: &[TransactionId],
    window: DuplicateWindow,
) -> Result<Vec<NewCandidate>, Error> {
    let sources: BTreeSet<TransactionId> = transactions.iter().copied().collect();
    let mut rows: BTreeMap<TransactionId, Twin> = BTreeMap::new();
    let mut edges: BTreeSet<Edge> = BTreeSet::new();

    let mut load = conn.prepare(&format!(
        "SELECT {COLUMNS}, t.excluded_reason IS NULL AS eligible \
         FROM \"transaction\" t WHERE t.id = ?1"
    ))?;
    // The `transaction_account_date` index.
    let mut counterparts = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM \"transaction\" t \
         WHERE t.account_id = ?1 AND t.date BETWEEN ?2 AND ?3 \
           AND t.amount_minor = ?4 AND t.currency = ?5 AND t.id <> ?6 \
           AND t.excluded_reason IS NULL"
    ))?;
    let mut recorded = conn.prepare(
        "SELECT EXISTS (SELECT 1 FROM duplicate_candidate \
                        WHERE transaction_a = ?1 AND transaction_b = ?2)",
    )?;

    for &id in &sources {
        let (source, eligible) = load
            .query_row([id], |row| {
                Ok((Twin::from_row(row)?, row.get::<_, bool>("eligible")?))
            })
            .optional()?
            .ok_or(Error::TransactionNotFound(id))?;
        if !eligible || source.amount == 0 {
            continue;
        }
        let days = Days::new(u64::from(window.days()));
        let from = source.date.checked_sub_days(days).unwrap_or(NaiveDate::MIN);
        let to = source.date.checked_add_days(days).unwrap_or(NaiveDate::MAX);
        let found = counterparts
            .query_map(
                params![
                    source.account,
                    date_key(from),
                    date_key(to),
                    source.amount,
                    source.currency,
                    source.id,
                ],
                Twin::from_row,
            )?
            .collect::<Result<Vec<_>, _>>()?;
        for candidate in found {
            if !source.may_pair(&candidate) {
                continue;
            }
            let edge = Edge::new(&source, &candidate);
            if recorded.query_row(params![edge.lower, edge.higher], |row| {
                row.get::<_, bool>(0)
            })? {
                continue;
            }
            edges.insert(edge);
            rows.insert(candidate.id, candidate);
        }
        rows.insert(source.id, source);
    }

    // Closest first; each row in at most one new pair.
    let mut taken: HashSet<TransactionId> = HashSet::new();
    let mut chosen: Vec<(TransactionId, TransactionId)> = Vec::new();
    for edge in edges {
        if taken.contains(&edge.lower) || taken.contains(&edge.higher) {
            continue;
        }
        taken.insert(edge.lower);
        taken.insert(edge.higher);
        chosen.push((edge.lower, edge.higher));
    }
    chosen.sort_unstable();

    let mut insert = conn.prepare(
        "INSERT INTO duplicate_candidate (transaction_a, transaction_b, reason) \
         VALUES (?1, ?2, ?3)",
    )?;
    let mut found = Vec::with_capacity(chosen.len());
    for (lower, higher) in chosen {
        let reason =
            if rows[&lower].origin == Origin::Manual || rows[&higher].origin == Origin::Manual {
                CandidateReason::ManualEntry
            } else {
                CandidateReason::OverlappingImport
            };
        insert.execute(params![lower, higher, reason.as_str()])?;
        found.push(NewCandidate {
            id: DuplicateCandidateId::new(conn.last_insert_rowid()),
            transaction_a: lower,
            transaction_b: higher,
            reason,
        });
    }
    Ok(found)
}

/// The columns [`Twin::from_row`] reads, from `"transaction" t`.
const COLUMNS: &str = "t.id, t.account_id, t.batch_id, t.date, t.amount_minor, t.currency, \
                       t.origin, t.description_normalized, t.merchant_display";

/// What detection needs of a transaction: one possible copy.
#[derive(Clone, Debug)]
struct Twin {
    id: TransactionId,
    account: AccountId,
    batch: Option<BatchId>,
    date: NaiveDate,
    amount: i64,
    currency: String,
    origin: Origin,
    description: String,
    merchant: Option<String>,
}

impl Twin {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Twin> {
        let origin: String = row.get("origin")?;
        Ok(Twin {
            id: row.get("id")?,
            account: row.get("account_id")?,
            batch: row.get("batch_id")?,
            date: account::date(row, "date")?,
            amount: row.get("amount_minor")?,
            currency: row.get("currency")?,
            origin: Origin::from_key(&origin).ok_or_else(|| account::invalid(row, "origin"))?,
            description: row.get("description_normalized")?,
            merchant: row.get("merchant_display")?,
        })
    }

    /// The rules beyond account, amount, date and exclusion, which the
    /// query checks (module docs, "Detection").
    fn may_pair(&self, other: &Twin) -> bool {
        let manual = (
            self.origin == Origin::Manual,
            other.origin == Origin::Manual,
        );
        match manual {
            (true, true) => false,
            (false, false) => self.batch != other.batch && self.description != other.description,
            _ => true,
        }
    }
}

/// A possible pair. The derived order is the order pairs are taken in:
/// closest first, then agreeing names, then by ID (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Edge {
    days: u64,
    names_differ: bool,
    lower: TransactionId,
    higher: TransactionId,
}

impl Edge {
    fn new(a: &Twin, b: &Twin) -> Edge {
        let names_agree = match (&a.merchant, &b.merchant) {
            (Some(x), Some(y)) => x.to_lowercase() == y.to_lowercase(),
            _ => false,
        };
        Edge {
            days: (a.date - b.date).num_days().unsigned_abs(),
            names_differ: !names_agree,
            lower: a.id.min(b.id),
            higher: a.id.max(b.id),
        }
    }
}

// Reading ---------------------------------------------------------------------

const CANDIDATE_COLUMNS: &str = "c.id, c.transaction_a, c.transaction_b, c.reason, c.status, \
                                 c.kept, c.change_batch_id, c.created_at";

fn candidate_from_row(row: &Row<'_>) -> rusqlite::Result<Candidate> {
    let reason: String = row.get("reason")?;
    let status: String = row.get("status")?;
    let kept: Option<String> = row.get("kept")?;
    let a: TransactionId = row.get("transaction_a")?;
    let b: TransactionId = row.get("transaction_b")?;
    Ok(Candidate {
        id: row.get("id")?,
        transaction_a: a,
        transaction_b: b,
        reason: CandidateReason::from_key(&reason)
            .ok_or_else(|| account::invalid(row, "reason"))?,
        status: CandidateStatus::from_key(&status)
            .ok_or_else(|| account::invalid(row, "status"))?,
        kept: match kept.as_deref() {
            None => None,
            Some("a") => Some(a),
            Some("b") => Some(b),
            Some(_) => return Err(account::invalid(row, "kept")),
        },
        change_batch: row.get("change_batch_id")?,
        created_at: row.get("created_at")?,
    })
}

fn load_candidate(conn: &Connection, id: DuplicateCandidateId) -> Result<Candidate, Error> {
    conn.query_row(
        &format!("SELECT {CANDIDATE_COLUMNS} FROM duplicate_candidate c WHERE c.id = ?1"),
        [id],
        candidate_from_row,
    )
    .optional()?
    .ok_or(Error::CandidateNotFound(id))
}

/// Candidate `id`.
pub fn get(db: &Database, id: DuplicateCandidateId) -> Result<Candidate, Error> {
    load_candidate(db.conn(), id)
}

/// The pending candidates, on `account` or on every account, oldest first.
pub fn pending(db: &Database, account: Option<AccountId>) -> Result<Vec<Candidate>, Error> {
    let mut statement = db.conn().prepare(&format!(
        "SELECT {CANDIDATE_COLUMNS} FROM duplicate_candidate c \
         JOIN \"transaction\" t ON t.id = c.transaction_a \
         WHERE c.status = 'pending' AND (?1 IS NULL OR t.account_id = ?1) ORDER BY c.id"
    ))?;
    let candidates = statement
        .query_map([account], candidate_from_row)?
        .collect::<Result<_, _>>()?;
    Ok(candidates)
}

/// The candidates naming `transaction`, in any status, oldest first.
pub fn of_transaction(db: &Database, transaction: TransactionId) -> Result<Vec<Candidate>, Error> {
    let mut statement = db.conn().prepare(&format!(
        "SELECT {CANDIDATE_COLUMNS} FROM duplicate_candidate c \
         WHERE c.transaction_a = ?1 OR c.transaction_b = ?1 ORDER BY c.id"
    ))?;
    let candidates = statement
        .query_map([transaction], candidate_from_row)?
        .collect::<Result<_, _>>()?;
    Ok(candidates)
}

/// The row a merge of candidate `id` keeps unless told otherwise (module
/// docs, "Merging").
pub fn default_keep(db: &Database, id: DuplicateCandidateId) -> Result<TransactionId, Error> {
    let candidate = load_candidate(db.conn(), id)?;
    let a = load_side(db.conn(), candidate.transaction_a)?;
    let b = load_side(db.conn(), candidate.transaction_b)?;
    Ok(best_sourced(&a, &b))
}

// Dismissing ------------------------------------------------------------------

/// Dismisses pending candidate `id`: the two are different transactions.
/// The pair is never raised again.
pub fn dismiss(db: &mut Database, id: DuplicateCandidateId) -> Result<(), Error> {
    let tx = db.conn_mut().transaction()?;
    let candidate = load_candidate(&tx, id)?;
    if candidate.status != CandidateStatus::Pending {
        return Err(Error::NotPending {
            candidate: id,
            status: candidate.status,
        });
    }
    tx.execute(
        "UPDATE duplicate_candidate SET status = 'dismissed' WHERE id = ?1",
        [id],
    )?;
    tx.commit()?;
    tracing::info!(candidate = %id, "dismissed a possible duplicate");
    Ok(())
}

/// Puts dismissed candidate `id` back to pending, for a dismissal made by
/// mistake.
pub fn reopen(db: &mut Database, id: DuplicateCandidateId) -> Result<(), Error> {
    let tx = db.conn_mut().transaction()?;
    let candidate = load_candidate(&tx, id)?;
    if candidate.status != CandidateStatus::Dismissed {
        return Err(Error::NotDismissed(id));
    }
    tx.execute(
        "UPDATE duplicate_candidate SET status = 'pending' WHERE id = ?1",
        [id],
    )?;
    tx.commit()?;
    tracing::info!(candidate = %id, "reopened a possible duplicate");
    Ok(())
}

// Merging ---------------------------------------------------------------------

/// One candidate to merge, and which row to keep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MergeRequest {
    pub candidate: DuplicateCandidateId,
    /// The row to keep; `None` keeps the best-sourced one ([`default_keep`]).
    pub keep: Option<TransactionId>,
}

impl MergeRequest {
    /// Merges `candidate`, keeping the best-sourced row.
    pub const fn new(candidate: DuplicateCandidateId) -> MergeRequest {
        MergeRequest {
            candidate,
            keep: None,
        }
    }

    /// Merges `candidate`, keeping `transaction` (one of its two rows).
    pub const fn keeping(
        candidate: DuplicateCandidateId,
        transaction: TransactionId,
    ) -> MergeRequest {
        MergeRequest {
            candidate,
            keep: Some(transaction),
        }
    }
}

/// What merging one candidate does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedMerge {
    pub candidate: DuplicateCandidateId,
    pub kept: TransactionId,
    /// Excluded as `duplicate`.
    pub duplicate: TransactionId,
    /// The fields copied from the duplicate to the kept row.
    pub carried: Vec<Field>,
}

/// A duplicate in its account's locked period: merging it needs an unlock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LockedRow {
    pub candidate: DuplicateCandidateId,
    pub transaction: TransactionId,
    pub date: NaiveDate,
    pub through: NaiveDate,
}

/// What [`merge`] would do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergePreview {
    /// The requests with the kept row filled in, by candidate ID.
    requests: Vec<MergeRequest>,
    /// One per candidate, by candidate ID.
    pub merges: Vec<PlannedMerge>,
    /// Every field change, in the order made. A locked duplicate's
    /// exclusion isn't among them.
    pub changes: Vec<Change>,
    /// Duplicates in a locked period. [`merge`] refuses while any is.
    pub locked: Vec<LockedRow>,
}

impl MergePreview {
    /// The requests the preview was made for, with the kept rows decided.
    pub fn requests(&self) -> &[MergeRequest] {
        &self.requests
    }

    /// Whether the merge needs an unlock first.
    pub fn is_locked(&self) -> bool {
        !self.locked.is_empty()
    }
}

/// What [`merge`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Merged {
    /// The `duplicate_merge` batch; [`change::undo`] undoes it.
    pub batch: ChangeBatchId,
    pub merges: Vec<PlannedMerge>,
}

/// Plans merging `requests` (one candidate or many) as one batch, in a DB
/// transaction that is rolled back. Fails as [`merge`] would, except that a
/// duplicate in a locked period is listed in [`MergePreview::locked`]
/// rather than refused.
pub fn merge_preview(db: &Database, requests: &[MergeRequest]) -> Result<MergePreview, Error> {
    change::dry_run(db, |conn| Ok(plan(conn, requests)?.0))
}

/// Merges what `preview` showed, as one `duplicate_merge` change batch, in
/// one DB transaction. Plans again and refuses ([`Error::Stale`]) if the
/// plan differs from the preview, and refuses a duplicate in a locked
/// period ([`Error::Locked`]).
pub fn merge(db: &mut Database, preview: &MergePreview) -> Result<Merged, Error> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (plan, batch) = plan(&tx, &preview.requests)?;
    if let Some(locked) = plan.locked.first() {
        return Err(Error::Locked {
            transaction: locked.transaction,
            date: locked.date,
            through: locked.through,
        });
    }
    if !plan.same_as(preview) {
        return Err(Error::Stale);
    }
    let batch = batch.expect("an unlocked merge excludes at least one row");
    tx.commit()?;
    tracing::info!(
        batch = %batch,
        candidates = plan.merges.len(),
        "merged possible duplicates"
    );
    Ok(Merged {
        batch,
        merges: plan.merges,
    })
}

impl MergePreview {
    /// Whether `self`, a fresh plan, does what `preview` showed. The time
    /// stamped on a new exclusion differs between the two and is ignored.
    fn same_as(&self, preview: &MergePreview) -> bool {
        fn normalized(changes: &[Change]) -> Vec<Change> {
            changes
                .iter()
                .cloned()
                .map(|mut change| {
                    if let Value::Exclusion(Some(exclusion)) = &mut change.new {
                        exclusion.at = None;
                    }
                    change
                })
                .collect()
        }
        self.requests == preview.requests
            && self.merges == preview.merges
            && self.locked == preview.locked
            && normalized(&self.changes) == normalized(&preview.changes)
    }
}

/// Runs the merge in `conn`'s transaction. With no locked row, also writes
/// the batch and marks the candidates merged, and returns the batch.
fn plan(
    conn: &Connection,
    requests: &[MergeRequest],
) -> Result<(MergePreview, Option<ChangeBatchId>), Error> {
    if requests.is_empty() {
        return Err(Error::NothingToMerge);
    }
    let mut sorted = requests.to_vec();
    sorted.sort_by_key(|request| request.candidate);
    for pair in sorted.windows(2) {
        if pair[0].candidate == pair[1].candidate {
            return Err(Error::Repeated(pair[0].candidate));
        }
    }

    let mut recorder = Recorder::new(ChangeKind::DuplicateMerge);
    let mut resolved = Vec::with_capacity(sorted.len());
    let mut merges = Vec::with_capacity(sorted.len());
    // `duplicate_candidate.kept` for each merge.
    let mut kept_sides = Vec::with_capacity(sorted.len());
    let mut locked = Vec::new();
    let mut kept_rows: HashSet<TransactionId> = HashSet::new();
    let mut duplicates: HashSet<TransactionId> = HashSet::new();

    for request in sorted {
        let id = request.candidate;
        let candidate = load_candidate(conn, id)?;
        if candidate.status != CandidateStatus::Pending {
            return Err(Error::NotPending {
                candidate: id,
                status: candidate.status,
            });
        }
        let a = load_side(conn, candidate.transaction_a)?;
        let b = load_side(conn, candidate.transaction_b)?;
        if a.account != b.account || a.currency != b.currency || a.amount != b.amount {
            return Err(Error::NoLongerAMatch(id));
        }
        let kept = match request.keep {
            None => best_sourced(&a, &b),
            Some(keep) if keep == a.id || keep == b.id => keep,
            Some(keep) => {
                return Err(Error::NotInPair {
                    candidate: id,
                    transaction: keep,
                });
            }
        };
        let (kept, duplicate) = if kept == a.id { (a, b) } else { (b, a) };
        if duplicates.contains(&kept.id)
            || kept_rows.contains(&duplicate.id)
            || duplicates.contains(&duplicate.id)
        {
            return Err(Error::Conflict(if duplicates.contains(&kept.id) {
                kept.id
            } else {
                duplicate.id
            }));
        }
        for side in [&kept, &duplicate] {
            if side.excluded {
                return Err(Error::Excluded {
                    candidate: id,
                    transaction: side.id,
                });
            }
        }
        if duplicate.in_transfer {
            return Err(Error::InTransfer {
                candidate: id,
                transaction: duplicate.id,
            });
        }
        kept_rows.insert(kept.id);
        duplicates.insert(duplicate.id);

        match change::locked(conn, duplicate.id, Field::Exclusion)? {
            Some(through) => locked.push(LockedRow {
                candidate: id,
                transaction: duplicate.id,
                date: duplicate.date,
                through,
            }),
            None => {
                recorder.set(
                    conn,
                    duplicate.id,
                    Value::Exclusion(Some(ExclusionValue::now(ExclusionReason::Duplicate))),
                )?;
            }
        }
        let carried = carry_over(conn, &mut recorder, &kept, duplicate.id)?;
        resolved.push(MergeRequest::keeping(id, kept.id));
        kept_sides.push(if kept.id == candidate.transaction_a {
            "a"
        } else {
            "b"
        });
        merges.push(PlannedMerge {
            candidate: id,
            kept: kept.id,
            duplicate: duplicate.id,
            carried,
        });
    }

    let changes: Vec<Change> = recorder.changes().cloned().collect();
    let batch = if locked.is_empty() {
        let batch = recorder.finish(conn)?;
        if let Some(batch) = batch {
            let mut mark = conn.prepare(
                "UPDATE duplicate_candidate SET status = 'merged', kept = ?1, \
                 change_batch_id = ?2 WHERE id = ?3",
            )?;
            for (merge, side) in merges.iter().zip(&kept_sides) {
                mark.execute(params![side, batch, merge.candidate])?;
            }
        }
        batch
    } else {
        None
    };
    Ok((
        MergePreview {
            requests: resolved,
            merges,
            changes,
            locked,
        },
        batch,
    ))
}

/// What a merge needs to know of one row of a pair.
#[derive(Clone, Debug)]
struct Side {
    id: TransactionId,
    account: AccountId,
    date: NaiveDate,
    amount: i64,
    currency: String,
    origin: Origin,
    has_source_id: bool,
    batch: Option<BatchId>,
    excluded: bool,
    /// In a suggested or confirmed transfer group.
    in_transfer: bool,
}

fn load_side(conn: &Connection, id: TransactionId) -> Result<Side, Error> {
    conn.query_row(
        "SELECT t.id, t.account_id, t.date, t.amount_minor, t.currency, t.origin, \
         t.external_id IS NOT NULL AS has_source_id, t.batch_id, \
         t.excluded_reason IS NOT NULL AS excluded, \
         EXISTS (SELECT 1 FROM transfer_group g \
                 WHERE g.id = t.transfer_group_id AND g.status <> 'rejected') AS in_transfer \
         FROM \"transaction\" t WHERE t.id = ?1",
        [id],
        |row| {
            let origin: String = row.get("origin")?;
            Ok(Side {
                id: row.get("id")?,
                account: row.get("account_id")?,
                date: account::date(row, "date")?,
                amount: row.get("amount_minor")?,
                currency: row.get("currency")?,
                origin: Origin::from_key(&origin).ok_or_else(|| account::invalid(row, "origin"))?,
                has_source_id: row.get("has_source_id")?,
                batch: row.get("batch_id")?,
                excluded: row.get("excluded")?,
                in_transfer: row.get("in_transfer")?,
            })
        },
    )
    .optional()?
    .ok_or(Error::TransactionNotFound(id))
}

/// The row to keep: in a transfer, then with a source ID, then imported
/// over manual, then imported first, then the lower ID.
fn best_sourced(a: &Side, b: &Side) -> TransactionId {
    let rank = |side: &Side| {
        (
            !side.in_transfer,
            !side.has_source_id,
            side.origin == Origin::Manual,
            side.batch.map_or(i64::MAX, BatchId::get),
            side.id,
        )
    };
    if rank(a) <= rank(b) { a.id } else { b.id }
}

/// Copies what the user set on `duplicate` to `kept` where `kept` has
/// nothing (module docs, "Merging"), recording each change. Returns the
/// fields copied.
fn carry_over(
    conn: &Connection,
    recorder: &mut Recorder,
    kept: &Side,
    duplicate: TransactionId,
) -> Result<Vec<Field>, Error> {
    let read = |id: TransactionId, field: Field| change::read(conn, id, field);
    let users = |source: Option<CategorySource>| {
        matches!(
            source,
            Some(CategorySource::User | CategorySource::AiApproved)
        )
    };
    let mut carried = Vec::new();

    // The split first: it needs the kept row's category cleared.
    if let Value::Split(lines) = read(duplicate, Field::Split)?
        && !lines.is_empty()
        && !kept.in_transfer
        && !split::is_split(conn, kept.id)?
        && let Value::Category(category) = read(kept.id, Field::Category)?
        && !users(category.source)
        && read(kept.id, Field::Share)? == Value::Share(ShareValue::WHOLE)
    {
        recorder.set(conn, kept.id, Value::Category(CategoryValue::NONE))?;
        if recorder.set(conn, kept.id, Value::Split(lines))? {
            carried.push(Field::Split);
        }
    }

    if let Value::Category(theirs) = read(duplicate, Field::Category)?
        && theirs.category_id.is_some()
        && !split::is_split(conn, kept.id)?
        && let Value::Category(ours) = read(kept.id, Field::Category)?
        && (ours.category_id.is_none() || (users(theirs.source) && !users(ours.source)))
        && recorder.set(conn, kept.id, Value::Category(theirs))?
    {
        carried.push(Field::Category);
    }

    if let Value::Share(theirs) = read(duplicate, Field::Share)?
        && theirs != ShareValue::WHOLE
        && !split::is_split(conn, kept.id)?
        && read(kept.id, Field::Share)? == Value::Share(ShareValue::WHOLE)
        && recorder.set(conn, kept.id, Value::Share(theirs))?
    {
        carried.push(Field::Share);
    }

    if let Value::Merchant(theirs) = read(duplicate, Field::Merchant)?
        && theirs.source == Some(NameSource::User)
        && let Value::Merchant(ours) = read(kept.id, Field::Merchant)?
        && ours.source != Some(NameSource::User)
        && recorder.set(conn, kept.id, Value::Merchant(theirs))?
    {
        carried.push(Field::Merchant);
    }

    if let Value::UserNote(Some(theirs)) = read(duplicate, Field::UserNote)?
        && read(kept.id, Field::UserNote)? == Value::UserNote(None)
        && recorder.set(conn, kept.id, Value::UserNote(Some(theirs)))?
    {
        carried.push(Field::UserNote);
    }

    if let Value::Tags(theirs) = read(duplicate, Field::Tags)?
        && let Value::Tags(ours) = read(kept.id, Field::Tags)?
    {
        let all: BTreeSet<_> = ours.iter().chain(&theirs).copied().collect();
        if all.len() > ours.len()
            && recorder.set(conn, kept.id, Value::Tags(all.into_iter().collect()))?
        {
            carried.push(Field::Tags);
        }
    }

    if read(duplicate, Field::Reimbursable)? == Value::Reimbursable(true)
        && recorder.set(conn, kept.id, Value::Reimbursable(true))?
    {
        carried.push(Field::Reimbursable);
    }

    Ok(carried)
}

// Undo and deletes ------------------------------------------------------------

/// Puts the candidates change batch `batch` merged back to `pending`, for
/// [`change::undo`] in the same DB transaction. Whatever happened to their
/// rows since, the pair is still a possible duplicate (module docs,
/// "Merging").
pub(crate) fn after_undo(conn: &Connection, batch: ChangeBatchId) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE duplicate_candidate SET status = 'pending', kept = NULL, \
         change_batch_id = NULL WHERE change_batch_id = ?1",
        [batch],
    )
}

/// Deletes the candidates naming `transaction`, whatever their status, for
/// whoever deletes the row (a manual row): the foreign keys have no
/// `ON DELETE`. Opens no transaction of its own.
pub fn clear(conn: &Connection, transaction: TransactionId) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM duplicate_candidate WHERE transaction_a = ?1 OR transaction_b = ?1",
        [transaction],
    )
}

/// Deletes the candidates naming any transaction of import `batch`
/// (mirrored rows included), for undoing the import. Opens no transaction
/// of its own.
pub fn clear_batch(conn: &Connection, batch: BatchId) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM duplicate_candidate WHERE \
         transaction_a IN (SELECT id FROM \"transaction\" WHERE batch_id = ?1) \
         OR transaction_b IN (SELECT id FROM \"transaction\" WHERE batch_id = ?1)",
        [batch],
    )
}
