//! Approving and rejecting suggestions (ROADMAP M2.7).
//!
//! An approval applies the suggestion through the existing core APIs and
//! marks it `approved` in the same DB transaction (`BEGIN IMMEDIATE`), so
//! either both happen or neither does and the suggestion stays pending:
//!
//! - `categorization`: one `bulk_edit` change batch, undoable with
//!   [`change::undo`]. A transaction gets the category with
//!   `category_source = 'ai_approved'`, even over a category the user set
//!   (approving an explicit item is the user's choice; the impact preview
//!   counts those rows). A target already in that category is left as it
//!   is, whoever set it. A split line's category is set by writing its
//!   transaction's lines back with that line changed (one write per
//!   transaction), which gives the lines new IDs.
//! - `rule`: the rule store, first in the list (decision record "rules",
//!   point 2), plus `rule::commit_apply` to history only when asked
//!   ([`ApproveOptions::apply_to_history`]), with nothing included: it never
//!   changes a category the user set (decision record "Inbox impact
//!   preview", point 1). Pending categorisations whose every item names a
//!   transaction the apply gave that same category are settled: marked
//!   approved under the apply's batch (point 4, [`Approved::settled`]).
//! - `transfer_pair`: detection's `suggested` group for exactly this pair is
//!   confirmed; otherwise a new confirmed group ([`transfer`]). A row on
//!   either side of a confirmed link is refused ([`ApproveError::Linked`]):
//!   links refuse transfer rows the same way.
//! - `merchant_alias`: [`merchant`]'s store, recomputing derived names.
//! - `refund_link`: a confirmed [`link`] to the purchase, of the payload's
//!   amount or, without one, as much as both sides can still take.
//!
//! Before anything is written the payload's references are checked again
//! against today's data ([`Payload::check`]): a target deleted or undone
//! since, or a split line whose transaction was split again, refuses the
//! approval ([`ApproveError::Invalid`]). Approving a decided suggestion is
//! [`ApproveError::NotPending`].
//!
//! **Locks.** `reconciled_through` guards exclusions only (decision records
//! "rules", point 8, and "M1 wave 4", point 3), and a suggestion can't
//! exclude anything: categories, splits, names, tags, links and transfers
//! don't change a balance. So no suggestion kind meets a lock today. Should
//! a write underneath refuse for a lock, the approval is refused as
//! [`ApproveError::Locked`].
//!
//! The suggestion's `change_batch_id` is the batch the approval recorded (a
//! categorisation's, or a rule's apply to history), and `applied_json` what
//! else it made, by ID. Nothing here logs transaction text or the payload.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::json;

use super::payload::{CategorizationItem, RefundLink, Target, TransferPair};
use super::{
    Error, Invalid, MAX_REASON_CHARS, Payload, SuggestionId, SuggestionKind, SuggestionStatus,
    get_in,
};
use crate::change::{self, CategorySource, CategoryValue, ChangeKind, Field, Recorder, Value};
use crate::db::Database;
use crate::id::{
    ChangeBatchId, LinkId, MerchantAliasId, RuleId, SplitId, TransactionId, TransferGroupId,
};
use crate::link::{self, LinkTarget, NewLink};
use crate::merchant;
use crate::money::Money;
use crate::rule::{self, ApplyError, StoreError};
use crate::split;
use crate::transfer::{self, TransferStatus};

#[cfg(test)]
#[path = "approve_tests.rs"]
mod tests;

/// What the user chose when approving.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ApproveOptions {
    /// For a `rule`: also apply it to past transactions (only those with
    /// no category or a rule's, never one the user set). Ignored for other
    /// kinds.
    pub apply_to_history: bool,
}

/// What an approval made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Applied {
    /// The categories were set as one change batch.
    Categorized {
        batch: Option<ChangeBatchId>,
        transactions: usize,
    },
    /// The rule was stored, and applied to history when asked.
    Rule {
        rule: RuleId,
        history: Option<ChangeBatchId>,
    },
    /// The pair is a confirmed transfer.
    TransferPair { group: TransferGroupId },
    /// The alias was stored and names recomputed.
    MerchantAlias {
        alias: MerchantAliasId,
        renamed: usize,
    },
    /// The link was confirmed.
    RefundLink { link: LinkId },
}

/// An approved suggestion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Approved {
    pub id: SuggestionId,
    pub kind: SuggestionKind,
    pub applied: Applied,
    /// Pending categorisations a rule's approval settled (Inbox impact
    /// preview, point 4).
    pub settled: Vec<SuggestionId>,
}

/// Why approving or rejecting failed. The suggestion stays pending, and
/// nothing was written. Messages name IDs, keys and amounts, never text.
#[derive(Debug, thiserror::Error)]
pub enum ApproveError {
    #[error(transparent)]
    Inbox(Error),

    #[error("suggestion {id} is already {status}")]
    NotPending {
        id: SuggestionId,
        status: SuggestionStatus,
    },

    #[error("a rejection reason is at most {max} characters and not blank", max = MAX_REASON_CHARS)]
    Reason,

    /// What the suggestion names no longer fits today's data: a target
    /// deleted or undone since, a split line whose transaction was split
    /// again, a transaction split since, a category gone.
    #[error("the suggestion no longer fits: {0}")]
    Invalid(Invalid),

    /// A write would change these transactions on or before their
    /// account's locked date.
    #[error(
        "approving would change {} transaction(s) on or before their account's locked date; \
         unlock the account first",
        .0.len()
    )]
    Locked(Vec<TransactionId>),

    /// A transfer side that is on a side of a confirmed link.
    #[error("transaction {0} is linked as a refund, reimbursement or settlement; unlink it first")]
    Linked(TransactionId),

    /// A link with no amount, when one side has nothing left.
    #[error("transactions {refund} and {purchase} have nothing left to link")]
    NothingToLink {
        refund: TransactionId,
        purchase: TransactionId,
    },

    #[error("setting the categories: {0}")]
    Change(change::Error),

    #[error("storing the rule: {0}")]
    Rule(StoreError),

    #[error("applying the rule to past transactions: {0}")]
    Apply(ApplyError),

    #[error("recording the transfer: {0}")]
    Transfer(transfer::Error),

    #[error("storing the merchant name: {0}")]
    Merchant(merchant::Error),

    #[error("linking: {0}")]
    Link(link::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

impl From<Error> for ApproveError {
    fn from(error: Error) -> Self {
        match error {
            Error::Invalid(invalid) => ApproveError::Invalid(invalid),
            other => ApproveError::Inbox(other),
        }
    }
}

impl From<change::Error> for ApproveError {
    fn from(error: change::Error) -> Self {
        match error {
            change::Error::Locked { transaction, .. } => ApproveError::Locked(vec![transaction]),
            other => ApproveError::Change(other),
        }
    }
}

impl From<StoreError> for ApproveError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Change(change) => change.into(),
            other => ApproveError::Rule(other),
        }
    }
}

impl From<ApplyError> for ApproveError {
    fn from(error: ApplyError) -> Self {
        match error {
            ApplyError::Locked(transactions) => ApproveError::Locked(transactions),
            ApplyError::Rules(StoreError::Change(change::Error::Locked {
                transaction, ..
            })) => ApproveError::Locked(vec![transaction]),
            other => ApproveError::Apply(other),
        }
    }
}

impl From<transfer::Error> for ApproveError {
    fn from(error: transfer::Error) -> Self {
        ApproveError::Transfer(error)
    }
}

impl From<merchant::Error> for ApproveError {
    fn from(error: merchant::Error) -> Self {
        ApproveError::Merchant(error)
    }
}

impl From<link::Error> for ApproveError {
    fn from(error: link::Error) -> Self {
        ApproveError::Link(error)
    }
}

/// Approves suggestion `id`: applies it and marks it approved, in one DB
/// transaction (module docs). On an error nothing is written and the
/// suggestion stays pending.
pub fn approve(
    db: &mut Database,
    id: SuggestionId,
    options: ApproveOptions,
) -> Result<Approved, ApproveError> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let approved = approve_in(&tx, id, options)?;
    tx.commit()?;
    tracing::debug!(
        suggestion = %id,
        kind = approved.kind.as_str(),
        settled = approved.settled.len(),
        "approved a suggestion"
    );
    Ok(approved)
}

/// [`approve`] inside the caller's DB transaction: what
/// [`impact`](super::impact) dry-runs.
pub(crate) fn approve_in(
    conn: &Connection,
    id: SuggestionId,
    options: ApproveOptions,
) -> Result<Approved, ApproveError> {
    let suggestion = get_in(conn, id)?;
    if suggestion.status != SuggestionStatus::Pending {
        return Err(ApproveError::NotPending {
            id,
            status: suggestion.status,
        });
    }
    suggestion.payload.check(conn)?;

    let mut settled = Vec::new();
    let (applied, batch, record) = match &suggestion.payload {
        Payload::Categorization(items) => {
            let (batch, transactions) = categorize(conn, items)?;
            let record = json!({
                "change_batch_id": batch.map(ChangeBatchId::get),
                "transactions": transactions.iter().map(|t| t.get()).collect::<Vec<_>>(),
            });
            let applied = Applied::Categorized {
                batch,
                transactions: transactions.len(),
            };
            (applied, batch, record)
        }
        Payload::Rule(definition) => {
            let rule = rule::create_at_in(conn, definition, 0)?;
            let history = if options.apply_to_history {
                let preview = rule::preview_apply_in(conn, rule)?;
                rule::commit_apply_in(conn, &preview, &BTreeSet::new())?.batch
            } else {
                None
            };
            if let Some(batch) = history {
                settled = settle(conn, id, batch)?;
            }
            let record = json!({
                "rule_id": rule.get(),
                "history_batch_id": history.map(ChangeBatchId::get),
                "settled": settled.iter().map(|s| s.get()).collect::<Vec<_>>(),
            });
            (Applied::Rule { rule, history }, history, record)
        }
        Payload::TransferPair(pair) => {
            let group = confirm_transfer(conn, pair)?;
            let record = json!({ "transfer_group_id": group.get() });
            (Applied::TransferPair { group }, None, record)
        }
        Payload::MerchantAlias(alias) => {
            let (alias, renamed) = merchant::add_in(conn, alias)?;
            let record = json!({ "merchant_alias_id": alias.get(), "renamed": renamed });
            (Applied::MerchantAlias { alias, renamed }, None, record)
        }
        Payload::RefundLink(refund) => {
            let amount = link_amount(conn, refund)?;
            let link = link::create_in(conn, &new_link(refund, amount))?;
            let record = json!({ "link_id": link.get(), "amount_minor": amount.minor() });
            (Applied::RefundLink { link }, None, record)
        }
    };
    decide(conn, id, batch, &record)?;
    Ok(Approved {
        id,
        kind: suggestion.kind,
        applied,
        settled,
    })
}

/// Marks pending suggestion `id` approved, with what it recorded.
fn decide(
    conn: &Connection,
    id: SuggestionId,
    batch: Option<ChangeBatchId>,
    record: &serde_json::Value,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE suggestion SET status = 'approved', \
         decided_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), change_batch_id = ?1, \
         applied_json = ?2 WHERE id = ?3 AND status = 'pending'",
        params![batch, record.to_string(), id],
    )?;
    Ok(())
}

// Categorisations ---------------------------------------------------------------

/// What setting a categorisation's items would do, item by item: the
/// writes for [`approve`] and the counts for the impact preview. Built by
/// the same code for both.
#[derive(Debug, Default)]
pub(super) struct CategorizationPlan {
    /// One value per transaction to write: a category, or the whole set of
    /// split lines with the named lines' categories changed.
    pub writes: Vec<(TransactionId, Value)>,
    /// Targets whose category would change.
    pub changes: usize,
    /// Of those, targets with a category the user set (`user` or
    /// `ai_approved`; any category on a split line, whose source isn't
    /// recorded), which approving overwrites.
    pub user_set: usize,
    /// Targets that already have that category.
    pub unchanged: usize,
    /// Targets gone since (deleted, undone, a line split again), a
    /// transaction split since, or a category gone.
    pub missing: usize,
}

/// Plans `items` against today's data. Reads only.
pub(super) fn plan_categorization(
    conn: &Connection,
    items: &[CategorizationItem],
) -> Result<CategorizationPlan, ApproveError> {
    let mut plan = CategorizationPlan::default();
    let mut categories: HashMap<&str, Option<i64>> = HashMap::new();
    let mut lines: BTreeMap<TransactionId, Vec<(SplitId, i64)>> = BTreeMap::new();
    for item in items {
        let key = item.category.as_str();
        let category = match categories.get(key) {
            Some(id) => *id,
            None => {
                let id = conn
                    .query_row("SELECT id FROM category WHERE key = ?1", [key], |row| {
                        row.get(0)
                    })
                    .optional()?;
                categories.insert(key, id);
                id
            }
        };
        let Some(category) = category else {
            plan.missing += 1;
            continue;
        };
        match item.target {
            Target::Transaction(transaction) => {
                let now = match change::read(conn, transaction, Field::Category) {
                    Ok(Value::Category(now)) => now,
                    Ok(_) => unreachable!("reading a category gives a category"),
                    Err(change::Error::TransactionNotFound(_)) => {
                        plan.missing += 1;
                        continue;
                    }
                    Err(error) => return Err(error.into()),
                };
                if split::is_split(conn, transaction)? {
                    plan.missing += 1;
                    continue;
                }
                if now.category_id == Some(category) {
                    // Already there: it keeps who set it.
                    plan.unchanged += 1;
                    continue;
                }
                plan.changes += 1;
                plan.user_set += usize::from(matches!(
                    now.source,
                    Some(CategorySource::User | CategorySource::AiApproved)
                ));
                let target = CategoryValue {
                    category_id: Some(category),
                    source: Some(CategorySource::AiApproved),
                    rule_id: None,
                };
                plan.writes.push((transaction, Value::Category(target)));
            }
            Target::SplitLine(line) => {
                let parent: Option<TransactionId> = conn
                    .query_row(
                        "SELECT transaction_id FROM transaction_split WHERE id = ?1",
                        [line],
                        |row| row.get(0),
                    )
                    .optional()?;
                match parent {
                    Some(parent) => lines.entry(parent).or_default().push((line, category)),
                    None => plan.missing += 1,
                }
            }
        }
    }

    for (parent, wanted) in lines {
        let Value::Split(mut values) = change::read(conn, parent, Field::Split)? else {
            unreachable!("reading a split gives lines");
        };
        let ids: Vec<SplitId> = conn
            .prepare_cached(
                "SELECT id FROM transaction_split WHERE transaction_id = ?1 ORDER BY line_no",
            )?
            .query_map([parent], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        let mut changed = false;
        for (line, category) in wanted {
            let Some(value) = ids
                .iter()
                .position(|id| *id == line)
                .and_then(|index| values.get_mut(index))
            else {
                plan.missing += 1;
                continue;
            };
            if value.category_id == Some(category) {
                plan.unchanged += 1;
                continue;
            }
            plan.changes += 1;
            plan.user_set += usize::from(value.category_id.is_some());
            value.category_id = Some(category);
            changed = true;
        }
        if changed {
            plan.writes.push((parent, Value::Split(values)));
        }
    }
    Ok(plan)
}

/// Sets `items` as one `bulk_edit` change batch. Returns the batch (none
/// when nothing changed) and the transactions it changed.
fn categorize(
    conn: &Connection,
    items: &[CategorizationItem],
) -> Result<(Option<ChangeBatchId>, Vec<TransactionId>), ApproveError> {
    let plan = plan_categorization(conn, items)?;
    let mut recorder = Recorder::new(ChangeKind::BulkEdit);
    for (transaction, value) in plan.writes {
        recorder.set(conn, transaction, value)?;
    }
    let transactions: Vec<TransactionId> = recorder
        .changes()
        .map(|change| change.transaction)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let batch = recorder.finish(conn)?;
    Ok((batch, transactions))
}

/// Marks approved, under `batch` (a rule's apply to history), every pending
/// categorisation whose every item names a transaction the batch gave that
/// same category (Inbox impact preview, point 4). Returns them.
fn settle(
    conn: &Connection,
    rule_suggestion: SuggestionId,
    batch: ChangeBatchId,
) -> Result<Vec<SuggestionId>, ApproveError> {
    let mut set: HashMap<TransactionId, i64> = HashMap::new();
    {
        let mut statement = conn.prepare(
            "SELECT transaction_id, new_value FROM change_item \
             WHERE batch_id = ?1 AND field = 'category'",
        )?;
        let rows = statement.query_map([batch], |row| {
            Ok((row.get::<_, TransactionId>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (transaction, json) = row?;
            if let Some(Value::Category(CategoryValue {
                category_id: Some(category),
                ..
            })) = Value::from_json(Field::Category, &json)
            {
                set.insert(transaction, category);
            }
        }
    }
    if set.is_empty() {
        return Ok(Vec::new());
    }

    let pending: Vec<(SuggestionId, String)> = conn
        .prepare(
            "SELECT id, payload_json FROM suggestion \
             WHERE status = 'pending' AND kind = 'categorization' ORDER BY id",
        )?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut categories: HashMap<String, Option<i64>> = HashMap::new();
    let mut settled = Vec::new();
    for (id, json) in pending {
        let Ok(Payload::Categorization(items)) =
            Payload::from_stored(SuggestionKind::Categorization, &json)
        else {
            continue;
        };
        let mut covered = true;
        for item in &items {
            let Target::Transaction(transaction) = item.target else {
                covered = false;
                break;
            };
            let key = item.category.as_str();
            let category = match categories.get(key) {
                Some(id) => *id,
                None => {
                    let id = conn
                        .query_row("SELECT id FROM category WHERE key = ?1", [key], |row| {
                            row.get(0)
                        })
                        .optional()?;
                    categories.insert(key.to_owned(), id);
                    id
                }
            };
            if category.is_none() || set.get(&transaction) != category.as_ref() {
                covered = false;
                break;
            }
        }
        if covered {
            decide(
                conn,
                id,
                Some(batch),
                &json!({ "settled_by": rule_suggestion.get() }),
            )?;
            settled.push(id);
        }
    }
    Ok(settled)
}

// Transfers and links -------------------------------------------------------------

/// Confirms detection's suggested group for exactly this pair, or records a
/// new confirmed one.
fn confirm_transfer(
    conn: &Connection,
    pair: &TransferPair,
) -> Result<TransferGroupId, ApproveError> {
    for side in [pair.outflow, pair.inflow] {
        if link::is_linked(conn, side)? {
            return Err(ApproveError::Linked(side));
        }
    }
    let group_of = |id: TransactionId| -> rusqlite::Result<Option<TransferGroupId>> {
        conn.query_row(
            "SELECT transfer_group_id FROM \"transaction\" WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
    };
    match (group_of(pair.outflow)?, group_of(pair.inflow)?) {
        (Some(a), Some(b)) if a == b => {
            // `Payload::check` let it through: detection's suggestion of
            // exactly this pair.
            conn.execute(
                "UPDATE transfer_group SET status = ?1 WHERE id = ?2 AND status = ?3",
                params![
                    TransferStatus::Confirmed.as_str(),
                    a,
                    TransferStatus::Suggested.as_str()
                ],
            )?;
            Ok(a)
        }
        _ => Ok(transfer::record_confirmed_pair(
            conn,
            pair.outflow,
            pair.inflow,
        )?),
    }
}

/// What a link approved now would be: the payload's amount, or as much as
/// both sides can still take.
pub(super) fn link_amount(conn: &Connection, refund: &RefundLink) -> Result<Money, ApproveError> {
    let inflow_left = link::inflow_left_of(conn, refund.refund)?;
    let purchase_left = link::purchase_left_of(conn, refund.purchase)?;
    let amount = match refund.amount_minor {
        Some(amount) => amount,
        None => inflow_left.minor().min(purchase_left.minor()),
    };
    if amount <= 0 {
        return Err(ApproveError::NothingToLink {
            refund: refund.refund,
            purchase: refund.purchase,
        });
    }
    Ok(Money::new(amount, inflow_left.currency()))
}

fn new_link(refund: &RefundLink, amount: Money) -> NewLink {
    NewLink {
        kind: refund.kind,
        inflow: refund.refund,
        target: LinkTarget::Purchase(refund.purchase),
        amount,
    }
}

// Rejecting -----------------------------------------------------------------------

/// Rejects pending suggestion `id`, with the user's optional reason. Changes
/// nothing else.
pub fn reject(
    db: &mut Database,
    id: SuggestionId,
    reason: Option<&str>,
) -> Result<(), ApproveError> {
    let reason = reason.map(str::trim);
    if reason.is_some_and(|text| text.is_empty() || text.chars().count() > MAX_REASON_CHARS) {
        return Err(ApproveError::Reason);
    }
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let suggestion = get_in(&tx, id)?;
    if suggestion.status != SuggestionStatus::Pending {
        return Err(ApproveError::NotPending {
            id,
            status: suggestion.status,
        });
    }
    tx.execute(
        "UPDATE suggestion SET status = 'rejected', decision_reason = ?1, \
         decided_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?2",
        rusqlite::params![reason, id],
    )?;
    tx.commit()?;
    tracing::debug!(suggestion = %id, "rejected a suggestion");
    Ok(())
}
