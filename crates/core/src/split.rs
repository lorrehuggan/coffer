//! Split transactions (M1.20): one transaction divided into lines, each
//! with its own category and share, that sum exactly to its amount.
//!
//! A split is stored as `transaction_split` rows
//! (`migrations/V5__transaction_split.sql`). Reports, budgets and MCP read
//! the `ledger_line` view, which has one row per line of a split
//! transaction and one row for each transaction that isn't split, so a split
//! purchase counts in each of its categories.
//!
//! The rules, checked by [`validate`] and [`replace`] in the same DB
//! transaction as the edit:
//!
//! - a split has at least two lines;
//! - each line is in the transaction's currency, in integer minor units,
//!   not zero, and has the transaction's sign (a purchase splits into
//!   amounts spent, a refund into amounts back), so a zero transaction
//!   can't be split;
//! - the lines sum exactly to the transaction's amount;
//! - a line's share is part of the line, with its sign, and its share label
//!   isn't blank;
//! - the categories the lines name exist. A line may have no category.
//!
//! A split transaction has no category or share of its own: its lines do.
//! Splitting clears the transaction's `category_id`, `category_source`,
//! `rule_id`, `my_share_minor` and `share_label`, and removing the split
//! leaves it uncategorised with no share. Its `reimbursable` flag, notes and
//! exclusion stay on the transaction.
//!
//! Decisions:
//!
//! - **An excluded transaction can be split.** Exclusion is about whether
//!   the row counts at all; its lines carry its `excluded_reason` in
//!   `ledger_line`, and reports leave them out with it. Splitting doesn't
//!   change the exclusion.
//! - **A transfer can't be split** ([`Error::InTransfer`]). A transaction in
//!   a `suggested` or `confirmed` transfer group is money moving between the
//!   user's accounts, neither spending nor income, so there is nothing to
//!   divide between categories; a suggested pair is resolved first. A row
//!   in a `rejected` group is an ordinary transaction and can be split. The
//!   schema holds this both ways: a split row can't join a suggested or
//!   confirmed group, and transfer detection doesn't suggest one.
//! - **Replacing a split replaces its lines**: the new lines get new IDs.
//!
//! Errors never quote transaction text, notes, share labels or category
//! keys: they name lines by their position, from 1.

#[cfg(test)]
mod tests;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::category::CategoryKey;
use crate::db::Database;
use crate::id::{BatchId, SplitId, TransactionId};
use crate::money::{Currency, Money};

/// Why a split couldn't be set, removed or read. Lines are numbered from
/// 1, in the order given. Messages never contain transaction text, notes,
/// share labels or category keys.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("there is no transaction {0}")]
    TransactionNotFound(TransactionId),

    #[error("a split has at least two lines, not {0}")]
    TooFewLines(usize),

    #[error("a transaction of zero can't be split")]
    ZeroAmount,

    #[error("split line {line} is in {found}, but the transaction is in {expected}")]
    CurrencyMismatch {
        line: usize,
        expected: Currency,
        found: Currency,
    },

    #[error("split line {0} is zero")]
    ZeroLine(usize),

    #[error("split line {0} has the opposite sign to the transaction")]
    WrongSign(usize),

    #[error(
        "split line {0}'s share isn't part of the line: it has the line's sign and is at most the line's amount"
    )]
    ShareOutsideLine(usize),

    #[error("split line {0}'s share label is blank")]
    BlankShareLabel(usize),

    #[error("the split lines add up to {found}, not the transaction's {expected}")]
    SumMismatch { expected: Money, found: Money },

    #[error("the split lines add up to more than can be represented")]
    Overflow,

    #[error("split line {0} names a category that doesn't exist")]
    UnknownCategory(usize),

    #[error("transaction {0} is in a transfer, which can't be split; reject the pair first")]
    InTransfer(TransactionId),

    #[error("transaction {0} isn't split")]
    NotSplit(TransactionId),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// One line of a split, as the user enters it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitLine {
    /// In the transaction's currency, with its sign; not zero.
    pub amount: Money,
    /// `None` leaves the line uncategorised.
    pub category: Option<CategoryKey>,
    /// The user's part of the line, with its sign; `None` means all of it.
    pub my_share: Option<Money>,
    /// A local label for who shares the cost; never leaves the app.
    pub share_label: Option<String>,
    /// The user's note on this line; not sent over MCP.
    pub user_note: Option<String>,
}

impl SplitLine {
    /// A line of `amount` in `category`, all of it the user's, with no note.
    pub fn new(amount: Money, category: Option<CategoryKey>) -> Self {
        SplitLine {
            amount,
            category,
            my_share: None,
            share_label: None,
            user_note: None,
        }
    }
}

/// A stored split line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredSplitLine {
    pub id: SplitId,
    pub transaction: TransactionId,
    /// The line's place in the split, from 1.
    pub line_no: u32,
    pub amount: Money,
    pub category: Option<CategoryKey>,
    pub my_share: Option<Money>,
    pub share_label: Option<String>,
    pub user_note: Option<String>,
}

/// Checks `lines` as a split of a transaction of `parent` (module docs),
/// without the database: everything but whether the categories exist. For
/// an editor to check as the user types; [`set`] checks again.
pub fn validate(parent: Money, lines: &[SplitLine]) -> Result<(), Error> {
    if parent.is_zero() {
        return Err(Error::ZeroAmount);
    }
    if lines.len() < 2 {
        return Err(Error::TooFewLines(lines.len()));
    }
    let currency = parent.currency();
    for (index, line) in lines.iter().enumerate() {
        let number = index + 1;
        let mismatch = |found: Currency| Error::CurrencyMismatch {
            line: number,
            expected: currency,
            found,
        };
        if line.amount.currency() != currency {
            return Err(mismatch(line.amount.currency()));
        }
        if line.amount.is_zero() {
            return Err(Error::ZeroLine(number));
        }
        if line.amount.is_negative() != parent.is_negative() {
            return Err(Error::WrongSign(number));
        }
        if let Some(share) = line.my_share {
            if share.currency() != currency {
                return Err(mismatch(share.currency()));
            }
            let (amount, share) = (line.amount.minor(), share.minor());
            let within = if amount > 0 {
                (0..=amount).contains(&share)
            } else {
                (amount..=0).contains(&share)
            };
            if !within {
                return Err(Error::ShareOutsideLine(number));
            }
        }
        if line
            .share_label
            .as_deref()
            .is_some_and(|label| label.trim().is_empty())
        {
            return Err(Error::BlankShareLabel(number));
        }
    }
    let total =
        Money::sum(currency, lines.iter().map(|line| line.amount)).map_err(|_| Error::Overflow)?;
    if total != parent {
        return Err(Error::SumMismatch {
            expected: parent,
            found: total,
        });
    }
    Ok(())
}

/// Splits `transaction` into `lines`, replacing any split it has, in one DB
/// transaction; see [`replace`]. Returns the new lines' IDs, in order.
pub fn set(
    db: &mut Database,
    transaction: TransactionId,
    lines: &[SplitLine],
) -> Result<Vec<SplitId>, Error> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let ids = replace(&tx, transaction, lines)?;
    tx.commit()?;
    Ok(ids)
}

/// Splits `transaction` into `lines`, replacing any split it has: checks
/// the lines against the stored transaction ([`validate`], the categories
/// exist, not a transfer), deletes its old lines, clears its own category,
/// rule and share, and stores the new lines numbered from 1. Returns their
/// IDs, in order.
///
/// Writes through `conn` without opening a transaction of its own, so it
/// can run inside a larger change (a rule's split template, a change
/// batch). Callers run it inside a transaction, so a failure leaves no
/// split half-written.
pub fn replace(
    conn: &Connection,
    transaction: TransactionId,
    lines: &[SplitLine],
) -> Result<Vec<SplitId>, Error> {
    let parent = load_parent(conn, transaction)?;
    if parent.in_transfer {
        return Err(Error::InTransfer(transaction));
    }
    validate(parent.amount, lines)?;

    let mut find_category = conn.prepare_cached("SELECT id FROM category WHERE key = ?1")?;
    let mut categories = Vec::with_capacity(lines.len());
    for (index, line) in lines.iter().enumerate() {
        let id: Option<i64> = match &line.category {
            Some(key) => Some(
                find_category
                    .query_row([key], |row| row.get(0))
                    .optional()?
                    .ok_or(Error::UnknownCategory(index + 1))?,
            ),
            None => None,
        };
        categories.push(id);
    }

    clear(conn, transaction)?;
    conn.execute(
        "UPDATE \"transaction\" SET category_id = NULL, category_source = NULL, rule_id = NULL, \
         my_share_minor = NULL, share_label = NULL WHERE id = ?1",
        [transaction],
    )?;
    let mut insert = conn.prepare_cached(
        "INSERT INTO transaction_split (transaction_id, line_no, amount_minor, category_id, \
         my_share_minor, share_label, user_note) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    let mut ids = Vec::with_capacity(lines.len());
    for ((index, line), category) in lines.iter().enumerate().zip(categories) {
        insert.execute(params![
            transaction,
            // A slice never holds `i64::MAX` lines.
            index as i64 + 1,
            line.amount.minor(),
            category,
            line.my_share.map(|share| share.minor()),
            line.share_label,
            line.user_note,
        ])?;
        ids.push(SplitId::new(conn.last_insert_rowid()));
    }
    tracing::debug!(
        transaction = %transaction,
        lines = lines.len(),
        "split a transaction"
    );
    Ok(ids)
}

/// Removes `transaction`'s split in one DB transaction and returns how many
/// lines it had. The transaction is left uncategorised with no share: its
/// own were cleared when it was split. [`Error::NotSplit`] when it has no
/// lines.
pub fn remove(db: &mut Database, transaction: TransactionId) -> Result<usize, Error> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    load_parent(&tx, transaction)?;
    let removed = clear(&tx, transaction)?;
    if removed == 0 {
        return Err(Error::NotSplit(transaction));
    }
    tx.commit()?;
    Ok(removed)
}

/// `transaction`'s split lines in order; empty when it isn't split.
pub fn lines(db: &Database, transaction: TransactionId) -> Result<Vec<StoredSplitLine>, Error> {
    let conn = db.conn();
    let parent = load_parent(conn, transaction)?;
    let currency = parent.amount.currency();
    let mut statement = conn.prepare(
        "SELECT s.id, s.line_no, s.amount_minor, c.key, s.my_share_minor, s.share_label, \
         s.user_note \
         FROM transaction_split s LEFT JOIN category c ON c.id = s.category_id \
         WHERE s.transaction_id = ?1 ORDER BY s.line_no",
    )?;
    let lines = statement
        .query_map([transaction], |row| {
            Ok(StoredSplitLine {
                id: row.get(0)?,
                transaction,
                line_no: row.get(1)?,
                amount: Money::new(row.get(2)?, currency),
                category: row.get(3)?,
                my_share: row
                    .get::<_, Option<i64>>(4)?
                    .map(|minor| Money::new(minor, currency)),
                share_label: row.get(5)?,
                user_note: row.get(6)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(lines)
}

/// Whether `transaction` has split lines. Opens no transaction of its own.
pub fn is_split(conn: &Connection, transaction: TransactionId) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM transaction_split WHERE transaction_id = ?1)",
        [transaction],
        |row| row.get(0),
    )
}

/// Deletes `transaction`'s split lines, if any, and returns how many there
/// were. Leaves the transaction itself alone. For whoever deletes a
/// transaction (a manual row): its lines go first, or the foreign key
/// refuses the delete. Opens no transaction of its own.
pub fn clear(conn: &Connection, transaction: TransactionId) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM transaction_split WHERE transaction_id = ?1",
        [transaction],
    )
}

/// Deletes the split lines of every transaction of import `batch` (mirrored
/// rows included) and returns how many there were. For undo, before it
/// deletes the batch's transactions. Opens no transaction of its own.
pub fn clear_batch(conn: &Connection, batch: BatchId) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM transaction_split WHERE transaction_id IN \
         (SELECT id FROM \"transaction\" WHERE batch_id = ?1)",
        [batch],
    )
}

/// What splitting needs of the transaction.
struct Parent {
    amount: Money,
    /// In a `suggested` or `confirmed` transfer group.
    in_transfer: bool,
}

fn load_parent(conn: &Connection, transaction: TransactionId) -> Result<Parent, Error> {
    conn.query_row(
        "SELECT t.amount_minor, t.currency, \
         coalesce(g.status IN ('suggested', 'confirmed'), 0) \
         FROM \"transaction\" t LEFT JOIN transfer_group g ON g.id = t.transfer_group_id \
         WHERE t.id = ?1",
        [transaction],
        |row| {
            Ok(Parent {
                amount: Money::new(row.get(0)?, row.get(1)?),
                in_transfer: row.get(2)?,
            })
        },
    )
    .optional()?
    .ok_or(Error::TransactionNotFound(transaction))
}
