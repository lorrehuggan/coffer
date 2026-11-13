//! Coffer's own suggestions (ROADMAP M2.14): after repeated identical
//! manual recategorisations, a `rule` suggestion from
//! [`COFFER_SOURCE`].
//!
//! # When
//!
//! Only the user's own recategorisations count: a [`ChangeKind::BulkEdit`]
//! change batch whose category change has source [`CategorySource::User`]
//! ([`recategorize`] makes one). [`Recorder::finish`] calls
//! `after_user_recategorization` in the same DB transaction. Imports,
//! rule applies, duplicate merges and approved suggestions (`ai_approved`)
//! never get here.
//!
//! # Counting
//!
//! For each display merchant (`merchant_display`) and category the user
//! just set, Coffer counts the transactions that hold that category with
//! `category_source = 'user'` and that display merchant now. The count is
//! of the database's state, not of the edits, so setting a row back or
//! undoing the batch stops it counting. Rows without a display merchant
//! and split rows never count. At [`RULE_SUGGESTION_THRESHOLD`] rows it
//! files a suggestion.
//!
//! # The rule
//!
//! `merchant` `is` the cleaned bank merchant ([`merchant::clean`]), the
//! text a merchant rule matches (aliases never change which rules match),
//! with one action, `set_category`. It is filed only when every counted row
//! has a bank merchant and they clean to one text, ignoring case, so the
//! rule matches each of them. The pattern is the smallest of their cleaned
//! spellings, so it doesn't depend on row order. The rationale is a fixed
//! sentence with the count, never the merchant's text.
//!
//! # Once
//!
//! Filing goes through `file_in`: it checks the payload,
//! and an identical pending suggestion from any source is a duplicate, not
//! a second suggestion. Coffer also files nothing when an identical
//! suggestion was approved or rejected (the user said no, or a rule already
//! came of it), or a stored rule has exactly this definition. At the
//! source's pending cap it files nothing: the user's edit never fails for
//! a suggestion. Only database errors propagate.

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::{COFFER_SOURCE, Error, Payload, SuggestionKind, file_in};
use crate::category::CategoryKey;
use crate::change::{self, CategorySource, CategoryValue, Change, ChangeKind, Recorder, Value};
use crate::db::Database;
use crate::id::{ChangeBatchId, TransactionId};
use crate::merchant;
use crate::rule::{AmountBounds, MatchField, MatchKind, Pattern, RuleAction, RuleDefinition, fold};

#[cfg(test)]
#[path = "own_tests.rs"]
mod own_tests;

/// How many transactions of one display merchant must hold one category the
/// user set before Coffer suggests a rule for them.
pub const RULE_SUGGESTION_THRESHOLD: usize = 3;

/// Why [`recategorize`] failed. Messages name IDs and keys, never text.
#[derive(Debug, thiserror::Error)]
pub enum RecategorizeError {
    #[error("there is no category {0}")]
    UnknownCategory(CategoryKey),

    #[error(transparent)]
    Change(#[from] change::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// Sets each transaction's category as the user, in one DB transaction,
/// recorded as one undoable [`ChangeKind::BulkEdit`] batch (`None` when
/// nothing changed). The app's and the CLI's way to recategorise rows: it
/// may file a `rule` suggestion in the same transaction (module docs).
///
/// A row that already has the category from a rule or a preset takes it as
/// the user's. Refuses an unknown category, and any change the row can't
/// take (a split row: categorise its lines); then nothing is written.
pub fn recategorize(
    db: &mut Database,
    items: &[(TransactionId, CategoryKey)],
) -> Result<Option<ChangeBatchId>, RecategorizeError> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut recorder = Recorder::new(ChangeKind::BulkEdit);
    for (transaction, key) in items {
        let category = tx
            .query_row(
                "SELECT id FROM category WHERE key = ?1",
                [key.as_str()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or_else(|| RecategorizeError::UnknownCategory(key.clone()))?;
        recorder.set(
            &tx,
            *transaction,
            Value::Category(CategoryValue {
                category_id: Some(category),
                source: Some(CategorySource::User),
                rule_id: None,
            }),
        )?;
    }
    let batch = recorder.finish(&tx)?;
    tx.commit()?;
    Ok(batch)
}

/// Called by [`Recorder::finish`] for a [`ChangeKind::BulkEdit`] batch,
/// inside its DB transaction, with the batch's changes. Looks only at
/// category changes to a category the user set, and files a `rule`
/// suggestion where they reach the threshold (module docs).
pub(crate) fn after_user_recategorization<'a>(
    conn: &Connection,
    changes: impl IntoIterator<Item = &'a Change>,
) -> rusqlite::Result<()> {
    let mut seen = BTreeSet::new();
    for change in changes {
        let Value::Category(CategoryValue {
            category_id: Some(category),
            source: Some(CategorySource::User),
            ..
        }) = change.new
        else {
            continue;
        };
        let display: Option<String> = conn
            .query_row(
                "SELECT merchant_display FROM \"transaction\" WHERE id = ?1",
                [change.transaction],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        let Some(display) = display.filter(|name| !name.trim().is_empty()) else {
            continue;
        };
        if seen.insert((display.clone(), category)) {
            consider(conn, &display, category)?;
        }
    }
    Ok(())
}

/// Files the rule for `display` → `category` if the user's rows reach the
/// threshold and nothing says not to (module docs).
fn consider(conn: &Connection, display: &str, category: i64) -> rusqlite::Result<()> {
    let mut statement = conn.prepare_cached(
        "SELECT t.merchant FROM \"transaction\" t \
         WHERE t.merchant_display = ?1 AND t.category_id = ?2 AND t.category_source = 'user' \
         AND NOT EXISTS (SELECT 1 FROM transaction_split s WHERE s.transaction_id = t.id)",
    )?;
    let merchants = statement
        .query_map(params![display, category], |row| {
            row.get::<_, Option<String>>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let count = merchants.len();
    if count < RULE_SUGGESTION_THRESHOLD {
        return Ok(());
    }
    let Some(pattern) = shared_pattern(&merchants) else {
        tracing::debug!(
            rows = count,
            "no rule suggested: the rows' bank merchants differ"
        );
        return Ok(());
    };
    let key: String = conn.query_row(
        "SELECT key FROM category WHERE id = ?1",
        [category],
        |row| row.get(0),
    )?;
    let (Ok(key), Ok(pattern)) = (
        CategoryKey::new(&key),
        Pattern::new(MatchKind::Is, &pattern),
    ) else {
        return Ok(());
    };
    let Ok(rule) = RuleDefinition::new(
        MatchField::Merchant,
        pattern,
        AmountBounds::ANY,
        None,
        vec![RuleAction::SetCategory(key)],
    ) else {
        return Ok(());
    };
    if rule_exists(conn, &rule)? {
        return Ok(());
    }
    let payload = Payload::Rule(rule);
    if decided_identical(conn, &payload.canonical_json())? {
        return Ok(());
    }
    let rationale = format!("You set this category on {count} transactions from this merchant.");
    match file_in(conn, COFFER_SOURCE, &payload, Some(&rationale)) {
        Ok(filed) => {
            tracing::debug!(
                suggestion = %filed.id,
                duplicate = filed.duplicate,
                "Coffer suggested a rule after repeated recategorisations"
            );
            Ok(())
        }
        Err(Error::Database(error)) => Err(error),
        Err(error) => {
            tracing::debug!(%error, "Coffer's rule suggestion wasn't filed");
            Ok(())
        }
    }
}

/// The pattern a merchant `is` rule needs to match every row: their cleaned
/// bank merchants, when all have one and they fold to one text; the
/// smallest spelling. `None` when a row has no bank merchant or they differ.
fn shared_pattern(merchants: &[Option<String>]) -> Option<String> {
    let mut cleaned = BTreeSet::new();
    for merchant in merchants {
        let text = merchant::clean(merchant.as_deref()?);
        if text.is_empty() {
            return None;
        }
        cleaned.insert(text);
    }
    let folded: BTreeSet<String> = cleaned.iter().map(|text| fold(text)).collect();
    if folded.len() != 1 {
        return None;
    }
    cleaned.into_iter().next()
}

/// Whether a `rule` suggestion with this canonical payload was approved or
/// rejected, from any source.
fn decided_identical(conn: &Connection, json: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM suggestion WHERE kind = ?1 AND payload_json = ?2 \
         AND status IN ('approved', 'rejected'))",
        params![SuggestionKind::Rule.as_str(), json],
        |row| row.get(0),
    )
}

/// Whether a stored rule has exactly this definition: a merchant `is` rule
/// on the pattern, with no amount or account condition and no action but
/// the one `set_category`.
fn rule_exists(conn: &Connection, rule: &RuleDefinition) -> rusqlite::Result<bool> {
    let [action @ RuleAction::SetCategory(_)] = rule.actions() else {
        return Ok(false);
    };
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM rule r \
         WHERE r.match_field = ?1 AND r.match_kind = ?2 AND r.pattern = ?3 \
         AND r.amount_min IS NULL AND r.amount_max IS NULL AND r.account_id IS NULL \
         AND (SELECT COUNT(*) FROM rule_action a WHERE a.rule_id = r.id) = 1 \
         AND EXISTS (SELECT 1 FROM rule_action a WHERE a.rule_id = r.id \
                     AND a.kind = ?4 AND json(a.value_json) = json(?5)))",
        params![
            rule.field().as_str(),
            rule.pattern().kind().as_str(),
            rule.pattern().as_str(),
            action.kind().as_str(),
            action.value_json(),
        ],
        |row| row.get(0),
    )
}
