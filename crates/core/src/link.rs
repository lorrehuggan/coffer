//! Refunds, reimbursements and shared costs (M1.25).
//!
//! A [`Link`] (`transaction_link`, `migrations/V9__transaction_link.sql`)
//! ties money in (the inflow) to what it pays back:
//!
//! - a [`LinkKind::Refund`] to a purchase. The linked part counts in the
//!   purchase's category on the refund's own date (`ledger_line`), so it
//!   nets against the purchase instead of counting as income, and a late
//!   refund leaves the purchase's month as it was;
//! - a [`LinkKind::Reimbursement`] to a purchase marked to claim back
//!   ([`Sharing::ClaimBack`]); it clears the claim;
//! - a [`LinkKind::Settlement`] to a share label: a person who owes the user
//!   for shared costs.
//!
//! Links are suggested by Coffer ([`suggest_for_batch`], [`suggest_all`]),
//! confirmed or made by the user ([`confirm`], [`create`]), and rejected
//! ([`reject`]). Only confirmed links count. A rejected pair keeps its row
//! and is never suggested again; unlinking a confirmed link rejects it too,
//! so "Undo" is [`confirm`] and the pair doesn't come back as a suggestion.
//!
//! Decisions (`docs/ARCHITECTURE.md` → "Decision record: refunds,
//! reimbursements and shared costs"):
//!
//! - **One currency** on both sides; a settlement is in its inflow's
//!   currency, and what a person owes is kept per currency.
//! - **Capped.** A link is at most what the inflow has left and what the
//!   purchase can still take ([`inflow_left`], [`purchase_left`]); a
//!   settlement is also at most what the person owes ([`owed_to`]). The rest
//!   of the money in stays unlinked and counts by its own category.
//! - **Spending is the user's share.** `ledger_line.personal_minor` is what
//!   a row counts as the user's own: their share (`my_share_minor`, else the
//!   whole amount), 0 for a purchase being claimed back and for link parts
//!   that repay one, and 0 for rows outside reports. A claim counts as 0
//!   while it is outstanding and once it is repaid; set back to "just me"
//!   ([`Sharing::JustMe`]), it counts in full and its repayments net against
//!   it like refunds. Balances and the transactions table stay gross.
//! - **Shared, claimed back or just me**, one per transaction
//!   ([`set_sharing`]), for money out only. A split purchase shares per line
//!   ([`set_line_share`]); claiming one back covers the whole purchase.
//! - **Owed to you** ([`owed_by_label`], [`outstanding_claims`],
//!   [`owed_total`]): per share label and currency, the others' part of
//!   shared costs (less what refunds of them returned) minus confirmed
//!   settlements; plus each claim's outstanding amount.
//! - **Locks don't apply.** A link doesn't change a balance
//!   (`reconciled_through` guards balances only).
//!
//! Share labels are local names that never leave the app (no MCP, no
//! logs). Errors never quote them or any transaction text.

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fmt;

use chrono::{Days, NaiveDate};
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};

use crate::account::{date_key, invalid};
use crate::db::Database;
use crate::id::{AccountId, BatchId, LinkId, SplitId, TransactionId};
use crate::money::{Currency, Money};

/// How far back a refund looks for its purchase, in days (decision point 2).
pub const REFUND_WINDOW_DAYS: u64 = 90;

/// How long after the newest claim of a set a reimbursement is suggested,
/// in days (decision point 2).
pub const REIMBURSEMENT_WINDOW_DAYS: u64 = 60;

/// How many outstanding claims a reimbursement suggestion considers: the
/// newest this many on or before the payment. Every non-empty set of them
/// is tried (at most 2^12 − 1 = 4,095), so the search stays instant
/// whatever the history.
pub const REIMBURSEMENT_CANDIDATES: usize = 12;

/// The largest purchase a link can target, in minor units: `ledger_line`
/// multiplies two amounts no larger than the purchase, and this keeps the
/// product inside an `i64` (`floor(sqrt(i64::MAX))`).
pub const MAX_LINKED_PURCHASE_MINOR: i64 = 3_037_000_499;

/// A share label's longest length, in characters.
pub const MAX_LABEL_CHARS: usize = 64;

/// What a link does (`transaction_link.kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LinkKind {
    /// Money back for a purchase; nets against its category.
    Refund,
    /// Repays a purchase marked to claim back.
    Reimbursement,
    /// Repays a person (a share label) for shared costs.
    Settlement,
}

impl LinkKind {
    pub const ALL: [LinkKind; 3] = [
        LinkKind::Refund,
        LinkKind::Reimbursement,
        LinkKind::Settlement,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            LinkKind::Refund => "refund",
            LinkKind::Reimbursement => "reimbursement",
            LinkKind::Settlement => "settlement",
        }
    }

    pub fn from_key(key: &str) -> Option<LinkKind> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == key)
    }
}

/// Where a link stands (`transaction_link.status`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LinkStatus {
    /// Coffer's guess, for the user to confirm or reject.
    Suggested,
    /// The user's; the only status that counts.
    Confirmed,
    /// Not this pair: never suggested again.
    Rejected,
}

impl LinkStatus {
    pub const ALL: [LinkStatus; 3] = [
        LinkStatus::Suggested,
        LinkStatus::Confirmed,
        LinkStatus::Rejected,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            LinkStatus::Suggested => "suggested",
            LinkStatus::Confirmed => "confirmed",
            LinkStatus::Rejected => "rejected",
        }
    }

    pub fn from_key(key: &str) -> Option<LinkStatus> {
        Self::ALL.into_iter().find(|status| status.as_str() == key)
    }
}

/// A local name for who shares a cost: trimmed, not blank, at most
/// [`MAX_LABEL_CHARS`] characters, no control characters. Compared exactly
/// (case included). It never leaves the app, and its `Debug` doesn't show
/// it.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShareLabel(String);

impl ShareLabel {
    pub fn new(label: &str) -> Result<ShareLabel, Error> {
        let label = label.trim();
        if label.is_empty() {
            return Err(Error::BlankLabel);
        }
        if label.chars().count() > MAX_LABEL_CHARS || label.chars().any(char::is_control) {
            return Err(Error::InvalidLabel);
        }
        Ok(ShareLabel(label.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ShareLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ShareLabel(..)")
    }
}

/// What a link pays back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
    /// A purchase (money out): for refunds and reimbursements.
    Purchase(TransactionId),
    /// A person: for settlements.
    Person(ShareLabel),
}

/// A link the user makes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewLink {
    pub kind: LinkKind,
    /// The money in.
    pub inflow: TransactionId,
    /// A purchase for a refund or reimbursement, a person for a settlement.
    pub target: LinkTarget,
    /// Positive, in the inflow's currency.
    pub amount: Money,
}

/// A stored link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub id: LinkId,
    pub kind: LinkKind,
    pub inflow: TransactionId,
    pub target: LinkTarget,
    /// Positive, in the inflow's currency.
    pub amount: Money,
    pub status: LinkStatus,
    /// A UTC timestamp, `YYYY-MM-DDTHH:MM:SS.SSSZ`.
    pub created_at: String,
}

/// How a purchase is shared: the inspector's Share control.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sharing {
    /// All of it is the user's (the default).
    JustMe,
    /// The user's part is `my_share`; the rest is owed by `label`.
    Shared { my_share: Money, label: ShareLabel },
    /// Someone else (an employer) pays all of it back: counts as nothing
    /// while claimed.
    ClaimBack,
}

/// A split line's share: the user's part, the rest owed by `label`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineShare {
    pub my_share: Money,
    pub label: ShareLabel,
}

/// A purchase being claimed back that isn't fully repaid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claim {
    pub transaction: TransactionId,
    pub account: AccountId,
    pub date: NaiveDate,
    /// What was spent: the purchase's amount, negative.
    pub spent: Money,
    /// What is still to come back: positive.
    pub owed: Money,
}

/// What one person owes the user, in one currency.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersonOwed {
    pub label: ShareLabel,
    /// Positive when they owe the user, negative when settlements (or a
    /// share changed since) put the user in their debt.
    pub owed: Money,
    /// Shared costs with them: purchases (or lines) where they have a part.
    pub costs: u32,
    /// The oldest of those costs.
    pub first_cost: Option<NaiveDate>,
    /// The newest confirmed settlement from them.
    pub last_paid: Option<NaiveDate>,
}

/// Why a link or a share couldn't be made, read or changed. Messages never
/// contain share labels or transaction text.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("there is no transaction {0}")]
    TransactionNotFound(TransactionId),

    #[error("there is no split line {0}")]
    SplitNotFound(SplitId),

    #[error("there is no link {0}")]
    LinkNotFound(LinkId),

    #[error("a {kind} pays back {expected}")]
    WrongTarget {
        kind: &'static str,
        expected: &'static str,
    },

    #[error("transaction {0} isn't money in")]
    NotMoneyIn(TransactionId),

    #[error("transaction {0} isn't money out")]
    NotMoneyOut(TransactionId),

    #[error("the link is in {found}, but the transactions are in {expected}")]
    CurrencyMismatch { expected: Currency, found: Currency },

    #[error("a link's amount is more than zero")]
    NotPositive,

    #[error("transaction {0} is split; a linked inflow can't be")]
    InflowSplit(TransactionId),

    #[error("transaction {0} has a share of its own; a linked inflow can't")]
    InflowShared(TransactionId),

    #[error("transaction {0} is in a transfer, which isn't spending or income")]
    InTransfer(TransactionId),

    #[error("transaction {0} is more than a link can take (at most 3037000499 minor units)")]
    TooLarge(TransactionId),

    #[error("transaction {0} isn't marked to claim back")]
    NotClaim(TransactionId),

    #[error("the money in has {left} left to link")]
    ExceedsInflow { left: Money },

    #[error("the purchase can take {left} more")]
    ExceedsPurchase { left: Money },

    #[error("they owe {owed}")]
    ExceedsOwed { owed: Money },

    #[error("these are already linked (link {0})")]
    AlreadyLinked(LinkId),

    #[error("a share label can't be blank")]
    BlankLabel,

    #[error("a share label is at most 64 characters, with no control characters")]
    InvalidLabel,

    #[error("the share is in {found}, but the transaction is in {expected}")]
    ShareCurrency { expected: Currency, found: Currency },

    #[error(
        "the share isn't part of the amount: it has the amount's sign and is at most the amount"
    )]
    ShareOutsideAmount,

    #[error("transaction {0} is split: share its lines instead")]
    Split(TransactionId),

    #[error("transaction {0} is claimed back as a whole; set it to just me first")]
    ClaimedBack(TransactionId),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

// Links -----------------------------------------------------------------------

/// Makes a confirmed link, in one DB transaction, and returns its ID. A
/// suggested or rejected link for the same pair becomes this one. Checks
/// both sides and the caps (module docs) before writing, and drops
/// suggestions that no longer fit.
pub fn create(db: &mut Database, link: &NewLink) -> Result<LinkId, Error> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let id = create_in(&tx, link)?;
    tx.commit()?;
    tracing::debug!(link = %id, kind = link.kind.as_str(), "linked a transaction");
    Ok(id)
}

/// [`create`] inside the caller's DB transaction.
pub(crate) fn create_in(conn: &Connection, link: &NewLink) -> Result<LinkId, Error> {
    let inflow = load_row(conn, link.inflow)?;
    check_inflow(conn, &inflow)?;
    if link.amount.currency() != inflow.currency {
        return Err(Error::CurrencyMismatch {
            expected: inflow.currency,
            found: link.amount.currency(),
        });
    }
    if !link.amount.is_positive() {
        return Err(Error::NotPositive);
    }
    let amount = link.amount.minor();
    let left = inflow_left_in(conn, &inflow)?;
    if amount > left {
        return Err(Error::ExceedsInflow {
            left: Money::new(left, inflow.currency),
        });
    }

    let existing: Option<(LinkId, String, String)> = match (&link.target, link.kind) {
        (LinkTarget::Purchase(target), LinkKind::Refund | LinkKind::Reimbursement) => {
            let purchase = load_row(conn, *target)?;
            check_purchase(&inflow, &purchase, link.kind)?;
            let left = purchase_left_in(conn, &purchase)?;
            if amount > left {
                return Err(Error::ExceedsPurchase {
                    left: Money::new(left, purchase.currency),
                });
            }
            conn.query_row(
                "SELECT id, kind, status FROM transaction_link \
                 WHERE target_id = ?1 AND inflow_id = ?2",
                params![target, link.inflow],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
        }
        (LinkTarget::Person(label), LinkKind::Settlement) => {
            let owed = owed_to_in(conn, label, inflow.currency)?;
            if amount > owed {
                return Err(Error::ExceedsOwed {
                    owed: Money::new(owed.max(0), inflow.currency),
                });
            }
            conn.query_row(
                "SELECT id, kind, status FROM transaction_link \
                 WHERE kind = 'settlement' AND inflow_id = ?1 AND share_label = ?2",
                params![link.inflow, label.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
        }
        (_, LinkKind::Settlement) => {
            return Err(Error::WrongTarget {
                kind: "settlement",
                expected: "a person",
            });
        }
        (_, kind) => {
            return Err(Error::WrongTarget {
                kind: kind.as_str(),
                expected: "a purchase",
            });
        }
    };

    let id = match existing {
        Some((id, _, status)) if status == LinkStatus::Confirmed.as_str() => {
            return Err(Error::AlreadyLinked(id));
        }
        Some((id, kind, _)) if kind == link.kind.as_str() => {
            conn.execute(
                "UPDATE transaction_link SET amount_minor = ?1, status = 'confirmed' WHERE id = ?2",
                params![amount, id],
            )?;
            id
        }
        other => {
            // A suggestion or rejection of another kind for the same pair
            // gives way to what the user chose.
            if let Some((id, _, _)) = other {
                conn.execute("DELETE FROM transaction_link WHERE id = ?1", [id])?;
            }
            let (target, label) = match &link.target {
                LinkTarget::Purchase(target) => (Some(*target), None),
                LinkTarget::Person(label) => (None, Some(label.as_str())),
            };
            conn.execute(
                "INSERT INTO transaction_link \
                 (kind, inflow_id, target_id, share_label, amount_minor, status) \
                 VALUES (?1, ?2, ?3, ?4, ?5, 'confirmed')",
                params![link.kind.as_str(), link.inflow, target, label, amount],
            )?;
            LinkId::new(conn.last_insert_rowid())
        }
    };
    prune_suggestions(conn, link.inflow, target_of(&link.target))?;
    Ok(id)
}

/// Confirms link `id` (a suggestion, or a rejected link the user links
/// after all), in one DB transaction, and drops suggestions that no longer
/// fit. Confirming a confirmed link does nothing.
pub fn confirm(db: &mut Database, id: LinkId) -> Result<(), Error> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let link = load_link(&tx, id)?;
    if link.status == LinkStatus::Confirmed {
        return Ok(());
    }
    let inflow = load_row(&tx, link.inflow)?;
    check_inflow(&tx, &inflow)?;
    let amount = link.amount.minor();
    let left = inflow_left_in(&tx, &inflow)?;
    if amount > left {
        return Err(Error::ExceedsInflow {
            left: Money::new(left, inflow.currency),
        });
    }
    match &link.target {
        LinkTarget::Purchase(target) => {
            let purchase = load_row(&tx, *target)?;
            check_purchase(&inflow, &purchase, link.kind)?;
            let left = purchase_left_in(&tx, &purchase)?;
            if amount > left {
                return Err(Error::ExceedsPurchase {
                    left: Money::new(left, purchase.currency),
                });
            }
        }
        LinkTarget::Person(label) => {
            let owed = owed_to_in(&tx, label, inflow.currency)?;
            if amount > owed {
                return Err(Error::ExceedsOwed {
                    owed: Money::new(owed.max(0), inflow.currency),
                });
            }
        }
    }
    tx.execute(
        "UPDATE transaction_link SET status = 'confirmed' WHERE id = ?1",
        [id],
    )?;
    prune_suggestions(&tx, link.inflow, target_of(&link.target))?;
    tx.commit()?;
    tracing::debug!(link = %id, "confirmed a link");
    Ok(())
}

/// Rejects link `id`: "not a refund" for a suggestion, unlink for a
/// confirmed link. The pair is never suggested again; [`confirm`] links it
/// after all. Rejecting a rejected link does nothing.
pub fn reject(db: &mut Database, id: LinkId) -> Result<(), Error> {
    let conn = db.conn();
    load_link(conn, id)?;
    conn.execute(
        "UPDATE transaction_link SET status = 'rejected' WHERE id = ?1 AND status <> 'rejected'",
        [id],
    )?;
    tracing::debug!(link = %id, "rejected a link");
    Ok(())
}

/// Deletes link `id`, whatever its status. A deleted pair can be suggested
/// again; the inspector's unlink is [`reject`].
pub fn delete(db: &mut Database, id: LinkId) -> Result<(), Error> {
    let removed = db
        .conn()
        .execute("DELETE FROM transaction_link WHERE id = ?1", [id])?;
    if removed == 0 {
        return Err(Error::LinkNotFound(id));
    }
    Ok(())
}

/// Link `id`.
pub fn get(db: &Database, id: LinkId) -> Result<Link, Error> {
    load_link(db.conn(), id)
}

/// Every link with `transaction` on either side, any status, oldest first.
pub fn links_of(db: &Database, transaction: TransactionId) -> Result<Vec<Link>, Error> {
    let conn = db.conn();
    load_row(conn, transaction)?;
    let mut statement = conn.prepare(&format!(
        "SELECT {LINK_COLUMNS} FROM transaction_link l \
         JOIN \"transaction\" r ON r.id = l.inflow_id \
         WHERE l.inflow_id = ?1 OR l.target_id = ?1 ORDER BY l.id"
    ))?;
    let links = statement
        .query_map([transaction], read_link)?
        .collect::<Result<_, _>>()?;
    Ok(links)
}

/// Every suggested link, oldest first: for the inspector and Owed to you.
pub fn suggestions(db: &Database) -> Result<Vec<Link>, Error> {
    let mut statement = db.conn().prepare(&format!(
        "SELECT {LINK_COLUMNS} FROM transaction_link l \
         JOIN \"transaction\" r ON r.id = l.inflow_id \
         WHERE l.status = 'suggested' ORDER BY l.id"
    ))?;
    let links = statement
        .query_map([], read_link)?
        .collect::<Result<_, _>>()?;
    Ok(links)
}

/// What money in `inflow` has left to link: its amount less its confirmed
/// links. Zero for money out.
pub fn inflow_left(db: &Database, inflow: TransactionId) -> Result<Money, Error> {
    inflow_left_of(db.conn(), inflow)
}

/// [`inflow_left`] on any connection (inside a DB transaction).
pub(crate) fn inflow_left_of(conn: &Connection, inflow: TransactionId) -> Result<Money, Error> {
    let row = load_row(conn, inflow)?;
    if row.amount <= 0 {
        return Ok(Money::zero(row.currency));
    }
    Ok(Money::new(inflow_left_in(conn, &row)?, row.currency))
}

/// What purchase `purchase` can still take (positive): its amount less its
/// confirmed refunds and reimbursements. For a claim, what is still owed.
/// Zero for money in.
pub fn purchase_left(db: &Database, purchase: TransactionId) -> Result<Money, Error> {
    purchase_left_of(db.conn(), purchase)
}

/// [`purchase_left`] on any connection (inside a DB transaction).
pub(crate) fn purchase_left_of(conn: &Connection, purchase: TransactionId) -> Result<Money, Error> {
    let row = load_row(conn, purchase)?;
    if row.amount >= 0 {
        return Ok(Money::zero(row.currency));
    }
    Ok(Money::new(purchase_left_in(conn, &row)?, row.currency))
}

/// What `label` owes the user in `currency` (module docs): positive when
/// they owe, negative when the user owes them.
pub fn owed_to(db: &Database, label: &ShareLabel, currency: Currency) -> Result<Money, Error> {
    Ok(Money::new(
        owed_to_in(db.conn(), label, currency)?,
        currency,
    ))
}

// Shares ----------------------------------------------------------------------

/// Sets how `transaction` is shared, in one DB transaction: its share and
/// label, and whether it is claimed back, which exclude each other. Shared
/// and claimed back are for money out outside a transfer. A split
/// transaction is shared per line ([`set_line_share`]): `Shared` is refused,
/// and `JustMe` and `ClaimBack` clear its lines' shares.
pub fn set_sharing(
    db: &mut Database,
    transaction: TransactionId,
    sharing: &Sharing,
) -> Result<(), Error> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let row = load_row(&tx, transaction)?;
    let split = crate::split::is_split(&tx, transaction)?;
    if !matches!(sharing, Sharing::JustMe) {
        if row.amount >= 0 {
            return Err(Error::NotMoneyOut(transaction));
        }
        if row.in_transfer {
            return Err(Error::InTransfer(transaction));
        }
    }
    let (share, label, reimbursable) = match sharing {
        Sharing::JustMe => (None, None, false),
        Sharing::Shared { my_share, label } => {
            if split {
                return Err(Error::Split(transaction));
            }
            check_share(row.amount, row.currency, *my_share)?;
            (Some(my_share.minor()), Some(label.as_str()), false)
        }
        Sharing::ClaimBack => (None, None, true),
    };
    if split {
        tx.execute(
            "UPDATE transaction_split SET my_share_minor = NULL, share_label = NULL \
             WHERE transaction_id = ?1",
            [transaction],
        )?;
    } else {
        tx.execute(
            "UPDATE \"transaction\" SET my_share_minor = ?1, share_label = ?2 WHERE id = ?3",
            params![share, label, transaction],
        )?;
    }
    tx.execute(
        "UPDATE \"transaction\" SET reimbursable = ?1 WHERE id = ?2",
        params![reimbursable, transaction],
    )?;
    tx.commit()?;
    Ok(())
}

/// Sets or clears (`None`) split line `split`'s share. The transaction is
/// money out and not claimed back as a whole.
pub fn set_line_share(
    db: &mut Database,
    split: SplitId,
    share: Option<&LineShare>,
) -> Result<(), Error> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (transaction, line_minor): (TransactionId, i64) = tx
        .query_row(
            "SELECT transaction_id, amount_minor FROM transaction_split WHERE id = ?1",
            [split],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or(Error::SplitNotFound(split))?;
    if let Some(share) = share {
        let row = load_row(&tx, transaction)?;
        if row.amount >= 0 {
            return Err(Error::NotMoneyOut(transaction));
        }
        if row.reimbursable {
            return Err(Error::ClaimedBack(transaction));
        }
        check_share(line_minor, row.currency, share.my_share)?;
    }
    tx.execute(
        "UPDATE transaction_split SET my_share_minor = ?1, share_label = ?2 WHERE id = ?3",
        params![
            share.map(|share| share.my_share.minor()),
            share.map(|share| share.label.as_str()),
            split
        ],
    )?;
    tx.commit()?;
    Ok(())
}

/// Every share label in use (transactions, split lines, settlements),
/// sorted: the choices for "Owed by".
pub fn share_labels(db: &Database) -> Result<Vec<ShareLabel>, Error> {
    let mut statement = db.conn().prepare(
        "SELECT share_label FROM \"transaction\" WHERE share_label IS NOT NULL \
         UNION SELECT share_label FROM transaction_split WHERE share_label IS NOT NULL \
         UNION SELECT share_label FROM transaction_link WHERE share_label IS NOT NULL \
         ORDER BY 1",
    )?;
    let labels = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .map(|label| Ok(ShareLabel(label?)))
        .collect::<Result<_, Error>>()?;
    Ok(labels)
}

/// Renames share label `from` to `to` everywhere, in one DB transaction,
/// and returns how many rows changed. Renaming to a label in use merges the
/// two people; a settlement that would then repeat one from the same
/// payment is refused by the schema, and nothing changes.
pub fn rename_label(db: &mut Database, from: &ShareLabel, to: &ShareLabel) -> Result<usize, Error> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut changed = 0;
    for table in ["\"transaction\"", "transaction_split"] {
        changed += tx.execute(
            &format!("UPDATE {table} SET share_label = ?1 WHERE share_label = ?2"),
            params![to.as_str(), from.as_str()],
        )?;
    }
    // A link's label is read-only (its row is the record of what was
    // linked), so a renamed settlement is replaced by an equal one.
    let mut statement = tx.prepare(
        "SELECT id, inflow_id, amount_minor, status, created_at FROM transaction_link \
         WHERE kind = 'settlement' AND share_label = ?1",
    )?;
    let settlements: Vec<(LinkId, TransactionId, i64, String, String)> = statement
        .query_map([from.as_str()], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?
        .collect::<Result<_, _>>()?;
    drop(statement);
    for (id, inflow, amount, status, created_at) in settlements {
        tx.execute("DELETE FROM transaction_link WHERE id = ?1", [id])?;
        tx.execute(
            "INSERT INTO transaction_link \
             (kind, inflow_id, share_label, amount_minor, status, created_at) \
             VALUES ('settlement', ?1, ?2, ?3, ?4, ?5)",
            params![inflow, to.as_str(), amount, status, created_at],
        )?;
        changed += 1;
    }
    tx.commit()?;
    Ok(changed)
}

// Owed to you -----------------------------------------------------------------

/// What each person owes, per share label and currency, sorted by label:
/// the others' part of shared costs in reports (purchases and split lines,
/// less what refunds of them returned), minus confirmed settlements.
/// Claimed-back purchases aren't here: see [`outstanding_claims`].
pub fn owed_by_label(db: &Database) -> Result<Vec<PersonOwed>, Error> {
    let mut statement = db.conn().prepare(
        "SELECT share_label, currency, sum(my_amount_minor - amount_minor) AS owed, \
         sum(link_kind IS NULL AND amount_minor < 0 AND my_amount_minor <> amount_minor) \
           AS costs, \
         min(CASE WHEN link_kind IS NULL AND amount_minor < 0 \
                   AND my_amount_minor <> amount_minor THEN date END) AS first_cost, \
         max(CASE WHEN link_kind = 'settlement' THEN date END) AS last_paid \
         FROM ledger_line \
         WHERE share_label IS NOT NULL AND in_reports AND reimbursable = 0 \
         GROUP BY share_label, currency ORDER BY share_label, currency",
    )?;
    let people = statement
        .query_map([], |row| {
            let currency: Currency = row.get("currency")?;
            Ok(PersonOwed {
                label: ShareLabel(row.get("share_label")?),
                owed: Money::new(row.get("owed")?, currency),
                costs: row.get("costs")?,
                first_cost: crate::account::optional_date(row, "first_cost")?,
                last_paid: crate::account::optional_date(row, "last_paid")?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(people)
}

/// Purchases claimed back that aren't fully repaid, oldest first. A claim
/// in reports only: an excluded purchase isn't owed.
pub fn outstanding_claims(db: &Database) -> Result<Vec<Claim>, Error> {
    let mut statement = db.conn().prepare(
        "SELECT * FROM (SELECT t.id, t.account_id, t.date, t.amount_minor, t.currency, \
         -t.amount_minor - coalesce((SELECT sum(amount_minor) FROM transaction_link l \
                                     WHERE l.target_id = t.id AND l.status = 'confirmed'), 0) \
           AS owed \
         FROM \"transaction\" t \
         LEFT JOIN transfer_group g ON g.id = t.transfer_group_id \
         WHERE t.reimbursable = 1 AND t.amount_minor < 0 AND t.excluded_reason IS NULL \
           AND g.status IS NOT 'confirmed') \
         WHERE owed > 0 ORDER BY date, id",
    )?;
    let claims = statement
        .query_map([], |row| {
            let currency: Currency = row.get("currency")?;
            Ok(Claim {
                transaction: row.get("id")?,
                account: row.get("account_id")?,
                date: crate::account::date(row, "date")?,
                spent: Money::new(row.get("amount_minor")?, currency),
                owed: Money::new(row.get("owed")?, currency),
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(claims)
}

/// What is to come back in total, per currency (sorted by code): what each
/// person owes, counting only those who owe the user, plus outstanding
/// claims.
pub fn owed_total(db: &Database) -> Result<Vec<Money>, Error> {
    let mut totals: BTreeMap<String, (Currency, i64)> = BTreeMap::new();
    let mut add = |money: Money| {
        let entry = totals
            .entry(money.currency().to_string())
            .or_insert((money.currency(), 0));
        entry.1 = entry.1.saturating_add(money.minor());
    };
    for person in owed_by_label(db)? {
        if person.owed.is_positive() {
            add(person.owed);
        }
    }
    for claim in outstanding_claims(db)? {
        add(claim.owed);
    }
    Ok(totals
        .into_values()
        .map(|(currency, minor)| Money::new(minor, currency))
        .collect())
}

// Suggestions -----------------------------------------------------------------

/// Suggests links for the money in of import `batch` (mirrored rows
/// included), oldest first; see [`suggest_all`] for the rules. Writes
/// through `conn` without a transaction of its own: for the import commit,
/// after transfer pairing, inside its DB transaction. Returns how many
/// suggestions it made.
pub fn suggest_for_batch(conn: &Connection, batch: BatchId) -> Result<usize, Error> {
    let inflows = inflow_ids(
        conn,
        "SELECT id FROM \"transaction\" WHERE batch_id = ?1 AND amount_minor > 0 \
         ORDER BY date, id",
        params![batch],
    )?;
    suggest_for(conn, &inflows)
}

/// Suggests links for every transaction of money in, oldest first, and
/// returns how many suggestions it made. Deterministic (decision point 2):
///
/// - money in that is eligible: not a duplicate, not in a suggested or
///   confirmed transfer, not split, no share of its own, no suggestion
///   pending, and something left to link;
/// - a **refund** of the newest purchase on the same account with the same
///   `merchant_display` (ignoring ASCII case), dated in the
///   [`REFUND_WINDOW_DAYS`] up to the money in, with at least that much left
///   to refund once confirmed and suggested links are counted; one whose
///   left equals the money in exactly goes first. A purchase excluded as a
///   duplicate is never one; one excluded for a bank error or a dispute is
///   (its refund is what corrects it);
/// - otherwise a **reimbursement** of one outstanding claim, or a set of
///   them, whose outstanding amounts (less suggestions) sum to the money in
///   to the penny, in its currency, the newest claim of the set dated in the
///   [`REIMBURSEMENT_WINDOW_DAYS`] up to the money in. The newest
///   [`REIMBURSEMENT_CANDIDATES`] claims on or before the money in are
///   tried; the fewest claims win, then the newest. One suggestion per
///   claim;
/// - never a pair with a link already, so a rejected pair is never
///   suggested again.
///
/// Writes through `conn` without a transaction of its own.
pub fn suggest_all(conn: &Connection) -> Result<usize, Error> {
    let inflows = inflow_ids(
        conn,
        "SELECT id FROM \"transaction\" WHERE amount_minor > 0 ORDER BY date, id",
        [],
    )?;
    suggest_for(conn, &inflows)
}

fn inflow_ids(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<TransactionId>, Error> {
    let mut statement = conn.prepare(sql)?;
    let ids = statement
        .query_map(params, |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(ids)
}

fn suggest_for(conn: &Connection, inflows: &[TransactionId]) -> Result<usize, Error> {
    let mut made = 0;
    for &id in inflows {
        made += suggest_one(conn, id)?;
    }
    if made > 0 {
        tracing::debug!(suggestions = made, "suggested links");
    }
    Ok(made)
}

fn suggest_one(conn: &Connection, id: TransactionId) -> Result<usize, Error> {
    let inflow = load_row(conn, id)?;
    if inflow.amount <= 0
        || inflow.excluded_reason.as_deref() == Some("duplicate")
        || inflow.in_transfer
        || inflow.has_share
        || crate::split::is_split(conn, id)?
    {
        return Ok(0);
    }
    let pending: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM transaction_link \
         WHERE inflow_id = ?1 AND status = 'suggested')",
        [id],
        |row| row.get(0),
    )?;
    if pending {
        return Ok(0);
    }
    let left = inflow_left_in(conn, &inflow)?;
    if left <= 0 {
        return Ok(0);
    }
    if let Some(purchase) = refund_candidate(conn, &inflow, left)? {
        insert_suggestion(conn, LinkKind::Refund, id, purchase, left)?;
        return Ok(1);
    }
    let claims = reimbursement_set(conn, &inflow, left)?;
    for (claim, amount) in &claims {
        insert_suggestion(conn, LinkKind::Reimbursement, id, *claim, *amount)?;
    }
    Ok(claims.len())
}

fn refund_candidate(
    conn: &Connection,
    inflow: &Side,
    left: i64,
) -> Result<Option<TransactionId>, Error> {
    let Some(merchant) = inflow.merchant_display.as_deref() else {
        return Ok(None);
    };
    let from = inflow
        .date
        .checked_sub_days(Days::new(REFUND_WINDOW_DAYS))
        .unwrap_or(NaiveDate::MIN);
    conn.query_row(
        "SELECT id FROM (\
           SELECT p.id, p.date, \
           -p.amount_minor - coalesce((SELECT sum(amount_minor) FROM transaction_link l \
                                       WHERE l.target_id = p.id \
                                         AND l.status IN ('suggested', 'confirmed')), 0) \
             AS left_minor \
           FROM \"transaction\" p \
           LEFT JOIN transfer_group g ON g.id = p.transfer_group_id \
           WHERE p.account_id = ?1 AND p.date BETWEEN ?2 AND ?3 \
             AND p.amount_minor < 0 AND p.amount_minor >= -?4 \
             AND p.merchant_display = ?5 COLLATE NOCASE \
             AND p.excluded_reason IS NOT 'duplicate' \
             AND coalesce(g.status, 'rejected') = 'rejected' \
             AND NOT EXISTS (SELECT 1 FROM transaction_link l \
                             WHERE l.target_id = p.id AND l.inflow_id = ?6)) \
         WHERE left_minor >= ?7 \
         ORDER BY left_minor = ?7 DESC, date DESC, id DESC LIMIT 1",
        params![
            inflow.account,
            date_key(from),
            date_key(inflow.date),
            MAX_LINKED_PURCHASE_MINOR,
            merchant,
            inflow.id,
            left,
        ],
        |row| row.get(0),
    )
    .optional()
    .map_err(Error::from)
}

/// The claims a reimbursement of `left` repays exactly, with each one's
/// amount; empty when no set matches.
fn reimbursement_set(
    conn: &Connection,
    inflow: &Side,
    left: i64,
) -> Result<Vec<(TransactionId, i64)>, Error> {
    let mut statement = conn.prepare(
        "SELECT id, date, left_minor FROM (\
           SELECT p.id, p.date, \
           -p.amount_minor - coalesce((SELECT sum(amount_minor) FROM transaction_link l \
                                       WHERE l.target_id = p.id \
                                         AND l.status IN ('suggested', 'confirmed')), 0) \
             AS left_minor \
           FROM \"transaction\" p \
           LEFT JOIN transfer_group g ON g.id = p.transfer_group_id \
           WHERE p.reimbursable = 1 AND p.date <= ?1 AND p.currency = ?2 \
             AND p.amount_minor < 0 AND p.amount_minor >= -?3 \
             AND p.excluded_reason IS NULL \
             AND coalesce(g.status, 'rejected') = 'rejected' \
             AND NOT EXISTS (SELECT 1 FROM transaction_link l \
                             WHERE l.target_id = p.id AND l.inflow_id = ?4)) \
         WHERE left_minor > 0 ORDER BY date DESC, id DESC LIMIT ?5",
    )?;
    let candidates: Vec<(TransactionId, NaiveDate, i64)> = statement
        .query_map(
            params![
                date_key(inflow.date),
                inflow.currency,
                MAX_LINKED_PURCHASE_MINOR,
                inflow.id,
                REIMBURSEMENT_CANDIDATES as i64,
            ],
            |row| Ok((row.get(0)?, crate::account::date(row, "date")?, row.get(2)?)),
        )?
        .collect::<Result<_, _>>()?;
    let earliest_newest = inflow
        .date
        .checked_sub_days(Days::new(REIMBURSEMENT_WINDOW_DAYS))
        .unwrap_or(NaiveDate::MIN);

    // Candidates are newest first, so a set's newest claim is its lowest
    // index, and comparing index lists prefers the newest claims.
    let mut best: Option<Vec<usize>> = None;
    for mask in 1u32..(1u32 << candidates.len()) {
        let members: Vec<usize> = (0..candidates.len())
            .filter(|index| mask & (1 << index) != 0)
            .collect();
        if candidates[members[0]].1 < earliest_newest {
            continue;
        }
        let sum = members
            .iter()
            .try_fold(0i64, |sum, &index| sum.checked_add(candidates[index].2));
        if sum != Some(left) {
            continue;
        }
        let better = match &best {
            None => true,
            Some(best) => (members.len(), &members) < (best.len(), best),
        };
        if better {
            best = Some(members);
        }
    }
    Ok(best
        .unwrap_or_default()
        .into_iter()
        .map(|index| (candidates[index].0, candidates[index].2))
        .collect())
}

fn insert_suggestion(
    conn: &Connection,
    kind: LinkKind,
    inflow: TransactionId,
    target: TransactionId,
    amount: i64,
) -> Result<(), Error> {
    conn.execute(
        "INSERT INTO transaction_link (kind, inflow_id, target_id, amount_minor, status) \
         VALUES (?1, ?2, ?3, ?4, 'suggested')",
        params![kind.as_str(), inflow, target, amount],
    )?;
    Ok(())
}

// Clearing --------------------------------------------------------------------

/// Deletes every link with `transaction` on either side, whatever its
/// status, and returns how many there were. For whoever deletes a
/// transaction (a manual row): its links go first, or the foreign keys
/// refuse the delete. Opens no transaction of its own.
pub fn clear(conn: &Connection, transaction: TransactionId) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM transaction_link WHERE inflow_id = ?1 OR target_id = ?1",
        [transaction],
    )
}

/// Deletes every link with a transaction of import `batch` (mirrored rows
/// included) on either side, and returns how many there were. For undo,
/// before it deletes the batch's transactions. A link from another batch's
/// money in to one of these purchases goes too: that money in is unlinked
/// again. Opens no transaction of its own.
pub fn clear_batch(conn: &Connection, batch: BatchId) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM transaction_link \
         WHERE inflow_id IN (SELECT id FROM \"transaction\" WHERE batch_id = ?1) \
            OR target_id IN (SELECT id FROM \"transaction\" WHERE batch_id = ?1)",
        [batch],
    )
}

/// Deletes the suggestions with `transaction` on either side and returns
/// how many there were: before its amount changes (a manual edit) or it is
/// split, which the schema refuses while a suggestion stands. Opens no
/// transaction of its own.
pub fn clear_suggestions(conn: &Connection, transaction: TransactionId) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM transaction_link WHERE status = 'suggested' \
         AND (inflow_id = ?1 OR target_id = ?1)",
        [transaction],
    )
}

/// Whether `transaction` is on either side of a confirmed link. Opens no
/// transaction of its own.
pub fn is_linked(conn: &Connection, transaction: TransactionId) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM transaction_link WHERE status = 'confirmed' \
         AND (inflow_id = ?1 OR target_id = ?1))",
        [transaction],
        |row| row.get(0),
    )
}

// Internals -------------------------------------------------------------------

/// What links need of a transaction on either side.
struct Side {
    id: TransactionId,
    account: AccountId,
    date: NaiveDate,
    amount: i64,
    currency: Currency,
    merchant_display: Option<String>,
    excluded_reason: Option<String>,
    reimbursable: bool,
    has_share: bool,
    /// In a `suggested` or `confirmed` transfer group.
    in_transfer: bool,
}

fn load_row(conn: &Connection, id: TransactionId) -> Result<Side, Error> {
    conn.query_row(
        "SELECT t.account_id, t.date, t.amount_minor, t.currency, t.merchant_display, \
         t.excluded_reason, t.reimbursable, \
         (t.my_share_minor IS NOT NULL OR t.share_label IS NOT NULL) AS has_share, \
         coalesce(g.status IN ('suggested', 'confirmed'), 0) AS in_transfer \
         FROM \"transaction\" t LEFT JOIN transfer_group g ON g.id = t.transfer_group_id \
         WHERE t.id = ?1",
        [id],
        |row| {
            Ok(Side {
                id,
                account: row.get("account_id")?,
                date: crate::account::date(row, "date")?,
                amount: row.get("amount_minor")?,
                currency: row.get("currency")?,
                merchant_display: row.get("merchant_display")?,
                excluded_reason: row.get("excluded_reason")?,
                reimbursable: row.get("reimbursable")?,
                has_share: row.get("has_share")?,
                in_transfer: row.get("in_transfer")?,
            })
        },
    )
    .optional()?
    .ok_or(Error::TransactionNotFound(id))
}

fn check_inflow(conn: &Connection, inflow: &Side) -> Result<(), Error> {
    if inflow.amount <= 0 {
        return Err(Error::NotMoneyIn(inflow.id));
    }
    if inflow.in_transfer {
        return Err(Error::InTransfer(inflow.id));
    }
    if crate::split::is_split(conn, inflow.id)? {
        return Err(Error::InflowSplit(inflow.id));
    }
    if inflow.has_share {
        return Err(Error::InflowShared(inflow.id));
    }
    Ok(())
}

fn check_purchase(inflow: &Side, purchase: &Side, kind: LinkKind) -> Result<(), Error> {
    if purchase.amount >= 0 {
        return Err(Error::NotMoneyOut(purchase.id));
    }
    if purchase.currency != inflow.currency {
        return Err(Error::CurrencyMismatch {
            expected: purchase.currency,
            found: inflow.currency,
        });
    }
    if purchase.in_transfer {
        return Err(Error::InTransfer(purchase.id));
    }
    if purchase.amount < -MAX_LINKED_PURCHASE_MINOR {
        return Err(Error::TooLarge(purchase.id));
    }
    if kind == LinkKind::Reimbursement && !purchase.reimbursable {
        return Err(Error::NotClaim(purchase.id));
    }
    Ok(())
}

fn check_share(amount: i64, currency: Currency, share: Money) -> Result<(), Error> {
    if share.currency() != currency {
        return Err(Error::ShareCurrency {
            expected: currency,
            found: share.currency(),
        });
    }
    let share = share.minor();
    let within = if amount >= 0 {
        (0..=amount).contains(&share)
    } else {
        (amount..=0).contains(&share)
    };
    if within {
        Ok(())
    } else {
        Err(Error::ShareOutsideAmount)
    }
}

fn inflow_left_in(conn: &Connection, inflow: &Side) -> Result<i64, Error> {
    let linked: i64 = conn.query_row(
        "SELECT coalesce(sum(amount_minor), 0) FROM transaction_link \
         WHERE inflow_id = ?1 AND status = 'confirmed'",
        [inflow.id],
        |row| row.get(0),
    )?;
    Ok(inflow.amount - linked)
}

fn purchase_left_in(conn: &Connection, purchase: &Side) -> Result<i64, Error> {
    let linked: i64 = conn.query_row(
        "SELECT coalesce(sum(amount_minor), 0) FROM transaction_link \
         WHERE target_id = ?1 AND status = 'confirmed'",
        [purchase.id],
        |row| row.get(0),
    )?;
    Ok(-purchase.amount - linked)
}

fn owed_to_in(conn: &Connection, label: &ShareLabel, currency: Currency) -> Result<i64, Error> {
    let owed: i64 = conn.query_row(
        "SELECT coalesce(sum(my_amount_minor - amount_minor), 0) FROM ledger_line \
         WHERE share_label = ?1 AND currency = ?2 AND in_reports AND reimbursable = 0",
        params![label.as_str(), currency],
        |row| row.get(0),
    )?;
    Ok(owed)
}

fn target_of(target: &LinkTarget) -> Option<TransactionId> {
    match target {
        LinkTarget::Purchase(id) => Some(*id),
        LinkTarget::Person(_) => None,
    }
}

/// Drops suggestions on `inflow` or `target` that no longer fit what
/// confirmed links leave.
fn prune_suggestions(
    conn: &Connection,
    inflow: TransactionId,
    target: Option<TransactionId>,
) -> Result<(), Error> {
    conn.execute(
        "DELETE FROM transaction_link WHERE status = 'suggested' AND inflow_id = ?1 \
         AND amount_minor > (SELECT t.amount_minor FROM \"transaction\" t WHERE t.id = ?1) \
           - (SELECT coalesce(sum(amount_minor), 0) FROM transaction_link \
              WHERE inflow_id = ?1 AND status = 'confirmed')",
        [inflow],
    )?;
    if let Some(target) = target {
        conn.execute(
            "DELETE FROM transaction_link WHERE status = 'suggested' AND target_id = ?1 \
             AND amount_minor > -(SELECT t.amount_minor FROM \"transaction\" t WHERE t.id = ?1) \
               - (SELECT coalesce(sum(amount_minor), 0) FROM transaction_link \
                  WHERE target_id = ?1 AND status = 'confirmed')",
            [target],
        )?;
    }
    Ok(())
}

const LINK_COLUMNS: &str = "l.id, l.kind, l.inflow_id, l.target_id, l.share_label, l.amount_minor, r.currency, \
     l.status, l.created_at";

fn read_link(row: &Row<'_>) -> rusqlite::Result<Link> {
    let kind: String = row.get("kind")?;
    let status: String = row.get("status")?;
    let target: Option<TransactionId> = row.get("target_id")?;
    let label: Option<String> = row.get("share_label")?;
    let target = match (target, label) {
        (Some(target), _) => LinkTarget::Purchase(target),
        (None, Some(label)) => LinkTarget::Person(ShareLabel(label)),
        (None, None) => return Err(invalid(row, "target_id")),
    };
    Ok(Link {
        id: row.get("id")?,
        kind: LinkKind::from_key(&kind).ok_or_else(|| invalid(row, "kind"))?,
        inflow: row.get("inflow_id")?,
        target,
        amount: Money::new(row.get("amount_minor")?, row.get("currency")?),
        status: LinkStatus::from_key(&status).ok_or_else(|| invalid(row, "status"))?,
        created_at: row.get("created_at")?,
    })
}

fn load_link(conn: &Connection, id: LinkId) -> Result<Link, Error> {
    conn.query_row(
        &format!(
            "SELECT {LINK_COLUMNS} FROM transaction_link l \
             JOIN \"transaction\" r ON r.id = l.inflow_id WHERE l.id = ?1"
        ),
        [id],
        read_link,
    )
    .optional()?
    .ok_or(Error::LinkNotFound(id))
}

/// Before `transaction` is split: refuses ([`crate::split::Error::Linked`])
/// when it is the money in of a confirmed link, and drops the suggestions
/// where it is, which the schema would refuse too.
pub(crate) fn release_inflow(
    conn: &Connection,
    transaction: TransactionId,
) -> rusqlite::Result<bool> {
    let confirmed: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM transaction_link \
         WHERE inflow_id = ?1 AND status = 'confirmed')",
        [transaction],
        |row| row.get(0),
    )?;
    if !confirmed {
        conn.execute(
            "DELETE FROM transaction_link WHERE inflow_id = ?1 AND status = 'suggested'",
            [transaction],
        )?;
    }
    Ok(!confirmed)
}
