//! Approving and rejecting suggestions (ROADMAP M2.7; subagent 1 of Block
//! E fills this in).
//!
//! An approval applies the suggestion through the existing core APIs and
//! marks it `approved` in the same DB transaction:
//!
//! - `categorization`: one change batch;
//! - `rule`: the rule store, plus `rule::commit_apply` to history only when
//!   asked ([`ApproveOptions::apply_to_history`]);
//! - `transfer_pair`: `transfer`;
//! - `merchant_alias`: `merchant`;
//! - `refund_link`: `link`.
//!
//! A lock, or a target undone or deleted since it was filed, refuses the
//! approval and leaves the suggestion pending.

use rusqlite::TransactionBehavior;

use super::{Error, MAX_REASON_CHARS, SuggestionId, SuggestionKind, SuggestionStatus, get_in};
use crate::db::Database;
use crate::id::{ChangeBatchId, LinkId, MerchantAliasId, RuleId, TransferGroupId};

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

/// Why approving or rejecting failed. The suggestion stays pending.
#[derive(Debug, thiserror::Error)]
pub enum ApproveError {
    #[error(transparent)]
    Inbox(#[from] Error),

    #[error("suggestion {id} is already {status}")]
    NotPending {
        id: SuggestionId,
        status: SuggestionStatus,
    },

    #[error("a rejection reason is at most {max} characters and not blank", max = MAX_REASON_CHARS)]
    Reason,

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// Approves suggestion `id`: applies it and marks it approved, in one DB
/// transaction (module docs).
pub fn approve(
    _db: &mut Database,
    _id: SuggestionId,
    _options: ApproveOptions,
) -> Result<Approved, ApproveError> {
    todo!("Block E subagent 1")
}

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
