//! Transfer-pair detection (M1.16): money that left one of the user's
//! accounts and arrived in another is a transfer, neither spending nor
//! income, once the user confirms it.
//!
//! [`detect_for_batch`] runs after an import, inside the import's own DB
//! transaction: for each of the batch's transactions it looks for a
//! counterpart among all stored transactions (the batch's own included), and
//! records each pair it finds as a `transfer_group` with status `suggested`.
//! Confirming and rejecting a suggestion is the user's, elsewhere.
//!
//! Two transactions pair when:
//!
//! - they are on different accounts. Every account is the user's own, so a
//!   pot and its parent pair like any two accounts; `asset` and `liability`
//!   accounts hold no transactions and never take part;
//! - they are in the same currency and their amounts are exactly opposite
//!   and not zero;
//! - their dates are at most the [`TransferWindow`] apart, both ends
//!   included;
//! - neither is in a transfer group already, whatever its status (a
//!   `rejected` group keeps its rows, so a rejected pair isn't suggested
//!   again), neither is excluded, and neither is split (a transfer can't
//!   be split: [`crate::split`]);
//! - neither is dated after its account's `closed_on`. A row on a closed
//!   account dated after the closing is suspect, not a transfer.
//!
//! Each transaction is in at most one pair. When a transaction has several
//! possible counterparts, the candidate pairs are taken closest first: the
//! smallest gap in days, then, when both rows have a time, the smallest gap
//! in time (a pair with both times sorts before one without), then the
//! lowest transaction IDs (the lower of the two, then the higher). Taking a
//! pair removes its two rows from every other candidate. The order depends
//! only on the rows' values and IDs, never on the order SQLite returns them
//! in, so the same database always gets the same pairs: two identical £50
//! transfers on one day pair one-to-one, the first out with the first in.
//!
//! Detection doesn't change categories, and logs counts only, never
//! transaction text.

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet, HashSet};

use chrono::{Days, NaiveDate, NaiveDateTime, NaiveTime};
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};

use crate::account::{self, date_key};
use crate::db::Database;
use crate::id::{AccountId, BatchId, TransactionId, TransferGroupId};
use crate::money::{Currency, Money};

/// Why detecting or reading transfers failed. Messages never contain
/// transaction text.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("a transfer window is at most {max} days, not {0}", max = TransferWindow::MAX_DAYS)]
    WindowTooLong(u32),

    #[error("there is no import batch {0}")]
    BatchNotFound(BatchId),

    #[error("there is no transaction {0}")]
    TransactionNotFound(TransactionId),

    #[error("there is no transfer group {0}")]
    GroupNotFound(TransferGroupId),

    #[error(
        "transactions {0} and {1} aren't two sides of one transfer: they must be on different \
         accounts, in one currency, with opposite amounts"
    )]
    NotAPair(TransactionId, TransactionId),

    #[error("transaction {0} is already in a transfer group")]
    AlreadyGrouped(TransactionId),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// How many whole days apart the two sides of a transfer may be dated. A
/// transfer between banks can take a few working days to arrive; the
/// window includes both ends, so `0` means the same day.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TransferWindow(u32);

impl TransferWindow {
    /// The longest window: a month. Wider, unrelated payments of a round
    /// amount start to pair.
    pub const MAX_DAYS: u32 = 31;

    /// Three days, the user's choice: a weekend plus a working day.
    pub const DEFAULT: TransferWindow = TransferWindow(3);

    /// A window of `days`, at most [`TransferWindow::MAX_DAYS`].
    pub fn new(days: u32) -> Result<Self, Error> {
        if days > Self::MAX_DAYS {
            return Err(Error::WindowTooLong(days));
        }
        Ok(TransferWindow(days))
    }

    pub const fn days(self) -> u32 {
        self.0
    }
}

impl Default for TransferWindow {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A pair detection suggested, recorded as a `suggested` transfer group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SuggestedPair {
    pub group: TransferGroupId,
    /// The side money left (negative amount).
    pub outflow: TransactionId,
    /// The side money arrived on (positive amount).
    pub inflow: TransactionId,
    /// What moved: the inflow's amount, always positive.
    pub amount: Money,
}

/// A transfer group's status (`transfer_group.status`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransferStatus {
    /// Found by detection, waiting for the user.
    Suggested,
    /// The user confirmed it: left out of income and spending.
    Confirmed,
    /// The user said it isn't a transfer. The group keeps its rows so the
    /// pair isn't suggested again.
    Rejected,
}

impl TransferStatus {
    pub const ALL: [TransferStatus; 3] = [
        TransferStatus::Suggested,
        TransferStatus::Confirmed,
        TransferStatus::Rejected,
    ];

    /// The key stored in `transfer_group.status`.
    pub const fn as_str(self) -> &'static str {
        match self {
            TransferStatus::Suggested => "suggested",
            TransferStatus::Confirmed => "confirmed",
            TransferStatus::Rejected => "rejected",
        }
    }

    pub fn from_key(key: &str) -> Option<TransferStatus> {
        Self::ALL.into_iter().find(|status| status.as_str() == key)
    }
}

/// A stored transfer group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferGroup {
    pub id: TransferGroupId,
    pub status: TransferStatus,
    /// Its transactions, by ID.
    pub transactions: Vec<TransactionId>,
}

/// Suggests transfer pairs for the transactions of import `batch`, each
/// matched against every stored transaction (see the module docs for the
/// rules), and records them. Returns the new pairs, ordered by their lower
/// transaction ID; their groups were created in that order.
///
/// Writes through `conn` without opening a transaction of its own, so it
/// can run inside the import's commit (a [`rusqlite::Transaction`] derefs
/// to a [`Connection`]). Callers run it inside a transaction, so a failure
/// leaves no pair half-recorded. An undone batch has no transactions left
/// and gets no pairs.
pub fn detect_for_batch(
    conn: &Connection,
    batch: BatchId,
    window: TransferWindow,
) -> Result<Vec<SuggestedPair>, Error> {
    let exists = conn
        .query_row("SELECT 1 FROM import_batch WHERE id = ?1", [batch], |_| {
            Ok(())
        })
        .optional()?
        .is_some();
    if !exists {
        return Err(Error::BatchNotFound(batch));
    }
    let transactions: Vec<TransactionId> = {
        let mut statement =
            conn.prepare("SELECT id FROM \"transaction\" WHERE batch_id = ?1 ORDER BY id")?;
        statement
            .query_map([batch], |row| row.get(0))?
            .collect::<Result<_, _>>()?
    };
    let pairs = detect(conn, &transactions, window)?;
    tracing::debug!(
        batch = %batch,
        transactions = transactions.len(),
        pairs = pairs.len(),
        "suggested transfer pairs"
    );
    Ok(pairs)
}

/// [`detect_for_batch`] for any set of transactions: suggests a pair for
/// each of `transactions` that has a counterpart among every stored
/// transaction, and records them. A transaction that can't pair (already
/// in a group, excluded, zero) is skipped; one that doesn't exist is an
/// error. The order of `transactions` and repeats in it don't matter.
///
/// Like [`detect_for_batch`], it writes through `conn` without a
/// transaction of its own.
pub fn detect(
    conn: &Connection,
    transactions: &[TransactionId],
    window: TransferWindow,
) -> Result<Vec<SuggestedPair>, Error> {
    let sources: BTreeSet<TransactionId> = transactions.iter().copied().collect();
    let mut rows: BTreeMap<TransactionId, Candidate> = BTreeMap::new();
    let mut edges: BTreeSet<Edge> = BTreeSet::new();

    let mut load = conn.prepare(&format!(
        "SELECT {COLUMNS}, {ELIGIBLE} AS eligible \
         FROM \"transaction\" t JOIN account a ON a.id = t.account_id \
         WHERE t.id = ?1"
    ))?;
    // Per account, then by date: the `transaction_account_date` index.
    // CROSS JOIN keeps accounts as the outer loop, and the unary `+` in
    // ELIGIBLE keeps SQLite from walking every unpaired row through the
    // `transfer_group_id` index instead.
    let mut counterparts = conn.prepare(&format!(
        "SELECT {COLUMNS} \
         FROM account a CROSS JOIN \"transaction\" t ON t.account_id = a.id \
         WHERE a.id <> ?1 AND a.currency = ?2 \
           AND t.date BETWEEN ?3 AND ?4 \
           AND t.amount_minor = ?5 AND t.currency = ?2 \
           AND {ELIGIBLE}"
    ))?;

    for &id in &sources {
        let (source, eligible) = load
            .query_row([id], |row| {
                Ok((Candidate::from_row(row)?, row.get::<_, bool>("eligible")?))
            })
            .optional()?
            .ok_or(Error::TransactionNotFound(id))?;
        // `i64::MIN` has no opposite to pair with.
        let Some(opposite) = source.amount.checked_neg().filter(|_| eligible) else {
            continue;
        };
        let days = Days::new(u64::from(window.days()));
        let from = source.date.checked_sub_days(days).unwrap_or(NaiveDate::MIN);
        let to = source.date.checked_add_days(days).unwrap_or(NaiveDate::MAX);
        let found = counterparts
            .query_map(
                params![
                    source.account,
                    source.currency,
                    date_key(from),
                    date_key(to),
                    opposite,
                ],
                Candidate::from_row,
            )?
            .collect::<Result<Vec<_>, _>>()?;
        for candidate in found {
            edges.insert(Edge::new(&source, &candidate));
            rows.insert(candidate.id, candidate);
        }
        rows.insert(source.id, source);
    }

    // Closest first; each row in at most one pair.
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

    let mut pairs = Vec::with_capacity(chosen.len());
    let mut group_insert =
        conn.prepare("INSERT INTO transfer_group (status) VALUES ('suggested')")?;
    let mut assign = conn.prepare(
        "UPDATE \"transaction\" SET transfer_group_id = ?1 \
         WHERE id IN (?2, ?3) AND transfer_group_id IS NULL",
    )?;
    for (lower, higher) in chosen {
        let (lower, higher) = (&rows[&lower], &rows[&higher]);
        let (outflow, inflow) = if lower.amount < 0 {
            (lower, higher)
        } else {
            (higher, lower)
        };
        group_insert.execute([])?;
        let group = TransferGroupId::new(conn.last_insert_rowid());
        let assigned = assign.execute(params![group, outflow.id, inflow.id])?;
        debug_assert_eq!(assigned, 2, "both sides were free when read");
        pairs.push(SuggestedPair {
            group,
            outflow: outflow.id,
            inflow: inflow.id,
            amount: Money::new(inflow.amount, inflow.currency),
        });
    }
    Ok(pairs)
}

/// Records transactions `a` and `b` as one `confirmed` transfer group and
/// returns it: for a pair that is known to be a transfer rather than found
/// by detection, such as an imported row and its mirrored pot-side row
/// ([`crate::sub_account::mirror`]). [`detect_for_batch`] then leaves both
/// alone, since they're in a group.
///
/// The two must be on different accounts, in one currency, with opposite
/// amounts ([`Error::NotAPair`]; either may be the outflow), and neither
/// may be in a transfer group already ([`Error::AlreadyGrouped`]). Unlike
/// detection, it doesn't look at dates, exclusions or closed accounts: the
/// caller knows the pair. Two zero amounts are opposite.
///
/// Like [`detect_for_batch`], it writes through `conn` without a
/// transaction of its own.
pub fn record_confirmed_pair(
    conn: &Connection,
    a: TransactionId,
    b: TransactionId,
) -> Result<TransferGroupId, Error> {
    let mut load = conn.prepare_cached(
        "SELECT account_id, amount_minor, currency, transfer_group_id \
         FROM \"transaction\" WHERE id = ?1",
    )?;
    let mut side = |id: TransactionId| {
        load.query_row([id], |row| {
            Ok(Side {
                account: row.get(0)?,
                amount: row.get(1)?,
                currency: row.get(2)?,
                group: row.get(3)?,
            })
        })
        .optional()?
        .ok_or(Error::TransactionNotFound(id))
    };
    let (first, second) = (side(a)?, side(b)?);
    let opposite = first.amount.checked_neg() == Some(second.amount);
    if first.account == second.account || first.currency != second.currency || !opposite {
        return Err(Error::NotAPair(a, b));
    }
    for (id, side) in [(a, &first), (b, &second)] {
        if side.group.is_some() {
            return Err(Error::AlreadyGrouped(id));
        }
    }
    conn.execute(
        "INSERT INTO transfer_group (status) VALUES (?1)",
        [TransferStatus::Confirmed.as_str()],
    )?;
    let group = TransferGroupId::new(conn.last_insert_rowid());
    let assigned = conn.execute(
        "UPDATE \"transaction\" SET transfer_group_id = ?1 \
         WHERE id IN (?2, ?3) AND transfer_group_id IS NULL",
        params![group, a, b],
    )?;
    debug_assert_eq!(assigned, 2, "both sides were free when read");
    Ok(group)
}

/// What [`record_confirmed_pair`] checks of each side.
struct Side {
    account: AccountId,
    amount: i64,
    currency: Currency,
    group: Option<TransferGroupId>,
}

/// Detects pairs for import `batch` in a DB transaction of its own, for a
/// batch committed before detection ran (the CLI, or until the import
/// commit calls [`detect_for_batch`] itself).
pub fn suggest_for_batch(
    db: &mut Database,
    batch: BatchId,
    window: TransferWindow,
) -> Result<Vec<SuggestedPair>, Error> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let pairs = detect_for_batch(&tx, batch, window)?;
    tx.commit()?;
    Ok(pairs)
}

/// The transfer group with this ID.
pub fn group(db: &Database, id: TransferGroupId) -> Result<TransferGroup, Error> {
    load_group(db.conn(), id)?.ok_or(Error::GroupNotFound(id))
}

/// The transfer group `transaction` is in, if any.
pub fn group_of(db: &Database, transaction: TransactionId) -> Result<Option<TransferGroup>, Error> {
    let conn = db.conn();
    let group: Option<TransferGroupId> = conn
        .query_row(
            "SELECT transfer_group_id FROM \"transaction\" WHERE id = ?1",
            [transaction],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(Error::TransactionNotFound(transaction))?;
    match group {
        Some(id) => Ok(load_group(conn, id)?),
        None => Ok(None),
    }
}

fn load_group(conn: &Connection, id: TransferGroupId) -> Result<Option<TransferGroup>, Error> {
    let status: Option<String> = conn
        .query_row(
            "SELECT status FROM transfer_group WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(status) = status else {
        return Ok(None);
    };
    let status = TransferStatus::from_key(&status).ok_or_else(|| {
        rusqlite::Error::InvalidColumnType(0, "status".to_owned(), rusqlite::types::Type::Text)
    })?;
    let mut statement =
        conn.prepare("SELECT id FROM \"transaction\" WHERE transfer_group_id = ?1 ORDER BY id")?;
    let transactions = statement
        .query_map([id], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(Some(TransferGroup {
        id,
        status,
        transactions,
    }))
}

/// The columns [`Candidate::from_row`] reads, from `"transaction" t`.
const COLUMNS: &str = "t.id, t.account_id, t.date, t.time, t.amount_minor, t.currency";

/// A transaction that can be one side of a new pair, on account `a`.
/// A split row is spending or income by category, not a transfer, and the
/// schema refuses it in a suggested group.
const ELIGIBLE: &str = "(+t.transfer_group_id IS NULL AND t.excluded_reason IS NULL \
                        AND t.amount_minor <> 0 \
                        AND NOT EXISTS (SELECT 1 FROM transaction_split s \
                                        WHERE s.transaction_id = t.id) \
                        AND a.kind NOT IN ('asset', 'liability') \
                        AND (a.closed_on IS NULL OR a.closed_on >= t.date))";

/// What pairing needs of a transaction.
#[derive(Clone, Debug)]
struct Candidate {
    id: TransactionId,
    account: AccountId,
    date: NaiveDate,
    time: Option<NaiveTime>,
    amount: i64,
    currency: Currency,
}

impl Candidate {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Candidate> {
        let time: Option<String> = row.get("time")?;
        let time = time
            .map(|text| {
                NaiveTime::parse_from_str(&text, "%H:%M:%S")
                    .map_err(|_| account::invalid(row, "time"))
            })
            .transpose()?;
        Ok(Candidate {
            id: row.get("id")?,
            account: row.get("account_id")?,
            date: account::date(row, "date")?,
            time,
            amount: row.get("amount_minor")?,
            currency: row.get("currency")?,
        })
    }
}

/// A possible pair. The derived order is the order pairs are taken in:
/// closest first, then by ID (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Edge {
    days: u64,
    /// `(0, seconds apart)` when both rows have a time, `(1, 0)` otherwise.
    time: (u8, u64),
    lower: TransactionId,
    higher: TransactionId,
}

impl Edge {
    fn new(a: &Candidate, b: &Candidate) -> Edge {
        let days = (a.date - b.date).num_days().unsigned_abs();
        let time = match (a.time, b.time) {
            (Some(at), Some(bt)) => {
                let gap = NaiveDateTime::new(a.date, at) - NaiveDateTime::new(b.date, bt);
                (0, gap.num_seconds().unsigned_abs())
            }
            _ => (1, 0),
        };
        Edge {
            days,
            time,
            lower: a.id.min(b.id),
            higher: a.id.max(b.id),
        }
    }
}
