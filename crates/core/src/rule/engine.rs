//! The rules engine: matching transactions against the stored rules, in
//! priority order, and applying what they set to an import batch.

use std::cell::OnceCell;
use std::collections::HashMap;

use regex::Regex;
use rusqlite::{Connection, OptionalExtension};

use super::store::{self, StoreError, StoredRule};
use super::{AmountBounds, MatchField, MatchKind, RuleAction};
use crate::category::CategoryKey;
use crate::id::{AccountId, BatchId, RuleId, TransactionId};

/// The fields of a transaction that rules match on.
///
/// The text fields are untrusted bank text: they are only ever matched.
#[derive(Clone, Copy, Debug)]
pub struct MatchInput<'a> {
    /// The transaction's account.
    pub account_id: AccountId,
    /// The signed amount in minor units (negative is money out).
    pub amount_minor: i64,
    /// The bank's merchant name (`transaction.merchant`), if any, as
    /// stored. Rules match it [cleaned](crate::merchant::clean), never with
    /// a merchant alias or a display name applied.
    pub merchant: Option<&'a str>,
    /// The bank's description as imported (`transaction.description_raw`).
    /// Rules match it with whitespace collapsed, as
    /// [`normalize_description`](crate::statement::normalize_description)
    /// does.
    pub description: &'a str,
    /// The bank's category for the row, if any.
    pub source_category: Option<&'a str>,
    /// The bank's transaction type for the row, if any.
    pub source_type: Option<&'a str>,
}

/// A value a rule set, and the rule that set it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetBy<T> {
    /// The value.
    pub value: T,
    /// The rule that set it: the first matching rule, in priority order,
    /// that sets this field.
    pub rule: RuleId,
}

/// What the rules set on one transaction, field by field. A field no
/// matching rule sets is `None`.
///
/// Only the category for now; the other action kinds (merchant, tags,
/// note, exclusion, split, share) are ROADMAP M1.24 and become fields here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Outcome {
    /// The category (`set_category`).
    pub category: Option<SetBy<CategoryKey>>,
}

impl Outcome {
    /// Whether every field the engine applies is set, so no later rule can
    /// change anything.
    fn is_complete(&self) -> bool {
        self.category.is_some()
    }
}

/// The stored rules, compiled for matching and in evaluation order
/// (priority, then ID).
///
/// Rules win field by field (decision record "rules", point 1): in
/// priority order, the first matching rule that sets a field sets it, and
/// a later matching rule can still set a different field.
#[derive(Clone, Debug, Default)]
pub struct RuleSet {
    rules: Vec<CompiledRule>,
}

impl RuleSet {
    /// Loads and validates every stored rule. Takes a connection, not a
    /// [`Database`](crate::db::Database), so it can run inside another
    /// module's open transaction (`rusqlite::Transaction` derefs to
    /// `Connection`).
    pub fn load(conn: &Connection) -> Result<RuleSet, StoreError> {
        Ok(RuleSet::new(store::load_all(conn)?))
    }

    /// A rule set from stored rules, in any order: they are sorted by
    /// priority, then ID.
    pub fn new(rules: impl IntoIterator<Item = StoredRule>) -> RuleSet {
        let mut rules: Vec<StoredRule> = rules.into_iter().collect();
        rules.sort_by_key(|rule| (rule.priority(), rule.id()));
        RuleSet {
            rules: rules.iter().map(CompiledRule::new).collect(),
        }
    }

    /// How many rules there are.
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether there are no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// What the rules set on a transaction with these fields.
    pub fn evaluate(&self, input: &MatchInput<'_>) -> Outcome {
        let row = Prepared::new(input);
        let mut outcome = Outcome::default();
        for rule in &self.rules {
            if outcome.is_complete() {
                break;
            }
            if let Some(key) = &rule.category
                && outcome.category.is_none()
                && rule.matches(&row)
            {
                outcome.category = Some(SetBy {
                    value: key.clone(),
                    rule: rule.id,
                });
            }
        }
        outcome
    }

    /// Applies the rules to every transaction of an import batch and
    /// returns how many rows they set a category on.
    ///
    /// A row whose category the rules set gets `category_id`,
    /// `category_source = 'rule'` and `rule_id`. Rows with a category from
    /// the user or an approved suggestion (`user`, `ai_approved`) are
    /// never changed on import (decision record "rules", point 3); rows
    /// with no category or the preset's (`preset_map`) are, because rules
    /// override the preset category map. Rows no rule matches are left as
    /// they are. A rule naming a category that doesn't exist is an error.
    ///
    /// Opens no transaction of its own: the import commit runs it inside
    /// its own.
    pub fn apply_to_batch(&self, conn: &Connection, batch: BatchId) -> Result<usize, StoreError> {
        if self.is_empty() {
            return Ok(0);
        }
        let mut statement = conn.prepare(
            "SELECT id, account_id, amount_minor, merchant, description_raw, source_category, \
             source_type FROM \"transaction\" \
             WHERE batch_id = ?1 AND (category_source IS NULL OR category_source = 'preset_map') \
             ORDER BY id",
        )?;
        let rows = statement
            .query_map([batch], |row| {
                Ok(BatchRow {
                    id: row.get(0)?,
                    account_id: row.get(1)?,
                    amount_minor: row.get(2)?,
                    merchant: row.get(3)?,
                    description: row.get(4)?,
                    source_category: row.get(5)?,
                    source_type: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut update = conn.prepare(
            "UPDATE \"transaction\" SET category_id = ?1, category_source = 'rule', rule_id = ?2 \
             WHERE id = ?3",
        )?;
        let mut category_ids: HashMap<CategoryKey, i64> = HashMap::new();
        let mut set = 0;
        for row in &rows {
            let outcome = self.evaluate(&row.input());
            let Some(SetBy { value: key, rule }) = outcome.category else {
                continue;
            };
            let category_id = match category_ids.get(&key) {
                Some(id) => *id,
                None => {
                    let id = conn
                        .query_row("SELECT id FROM category WHERE key = ?1", [&key], |row| {
                            row.get(0)
                        })
                        .optional()?
                        .ok_or_else(|| StoreError::RuleCategoryMissing {
                            rule,
                            key: key.clone(),
                        })?;
                    category_ids.insert(key, id);
                    id
                }
            };
            update.execute(rusqlite::params![category_id, rule, row.id])?;
            set += 1;
        }
        Ok(set)
    }
}

/// Applies the stored rules to every transaction of an import batch; see
/// [`RuleSet::apply_to_batch`]. Loads the rules from `conn` first.
pub fn apply_to_batch(conn: &Connection, batch: BatchId) -> Result<usize, StoreError> {
    RuleSet::load(conn)?.apply_to_batch(conn, batch)
}

/// A transaction of a batch, as [`RuleSet::apply_to_batch`] reads it.
struct BatchRow {
    id: TransactionId,
    account_id: AccountId,
    amount_minor: i64,
    merchant: Option<String>,
    description: String,
    source_category: Option<String>,
    source_type: Option<String>,
}

impl BatchRow {
    fn input(&self) -> MatchInput<'_> {
        MatchInput {
            account_id: self.account_id,
            amount_minor: self.amount_minor,
            merchant: self.merchant.as_deref(),
            description: &self.description,
            source_category: self.source_category.as_deref(),
            source_type: self.source_type.as_deref(),
        }
    }
}

/// One rule, ready to match.
#[derive(Clone, Debug)]
struct CompiledRule {
    id: RuleId,
    field: MatchField,
    matcher: Matcher,
    amount: AmountBounds,
    account_id: Option<i64>,
    category: Option<CategoryKey>,
}

/// A pattern ready to test. Plain patterns are kept [folded](fold).
#[derive(Clone)]
enum Matcher {
    Is(String),
    StartsWith(String),
    Contains(String),
    Regex(Regex),
}

impl std::fmt::Debug for Matcher {
    // The pattern is user text: show the kind only.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Matcher::Is(_) => "Is",
            Matcher::StartsWith(_) => "StartsWith",
            Matcher::Contains(_) => "Contains",
            Matcher::Regex(_) => "Regex",
        })
    }
}

impl CompiledRule {
    fn new(rule: &StoredRule) -> CompiledRule {
        let definition = rule.definition();
        let pattern = definition.pattern();
        let matcher = match pattern.kind() {
            MatchKind::Is => Matcher::Is(fold(pattern.as_str())),
            MatchKind::StartsWith => Matcher::StartsWith(fold(pattern.as_str())),
            MatchKind::Contains => Matcher::Contains(fold(pattern.as_str())),
            MatchKind::Regex => Matcher::Regex(
                pattern
                    .regex()
                    .expect("a regex pattern keeps its compiled regex")
                    .clone(),
            ),
        };
        let category = definition.actions().iter().find_map(|action| match action {
            RuleAction::SetCategory(key) => Some(key.clone()),
            _ => None,
        });
        CompiledRule {
            id: rule.id(),
            field: definition.field(),
            matcher,
            amount: definition.amount(),
            account_id: definition.account_id(),
            category,
        }
    }

    fn matches(&self, row: &Prepared<'_>) -> bool {
        let input = row.input;
        if self
            .account_id
            .is_some_and(|account| account != input.account_id.get())
        {
            return false;
        }
        if self
            .amount
            .min()
            .is_some_and(|min| input.amount_minor < min)
            || self
                .amount
                .max()
                .is_some_and(|max| input.amount_minor > max)
        {
            return false;
        }
        match &self.matcher {
            Matcher::Regex(regex) => row
                .text(self.field)
                .is_some_and(|text| regex.is_match(text)),
            Matcher::Is(pattern) => row.folded(self.field).is_some_and(|text| text == pattern),
            Matcher::StartsWith(pattern) => row
                .folded(self.field)
                .is_some_and(|text| text.starts_with(pattern.as_str())),
            Matcher::Contains(pattern) => row
                .folded(self.field)
                .is_some_and(|text| text.contains(pattern.as_str())),
        }
    }
}

/// A transaction's fields as rules see them: the merchant
/// [cleaned](crate::merchant::clean), the description with whitespace
/// collapsed, and each field folded once. The merchant is cleaned and each
/// field folded when a rule first needs it.
struct Prepared<'a> {
    input: &'a MatchInput<'a>,
    merchant: OnceCell<Option<String>>,
    description: String,
    folded: [OnceCell<Option<String>>; 4],
}

impl<'a> Prepared<'a> {
    fn new(input: &'a MatchInput<'a>) -> Prepared<'a> {
        Prepared {
            input,
            merchant: OnceCell::new(),
            description: collapse_whitespace(input.description),
            folded: Default::default(),
        }
    }

    /// The field as a regex sees it. `None` when the row has no value.
    fn text(&self, field: MatchField) -> Option<&str> {
        match field {
            // The cleaned bank text, before any merchant alias: names never
            // change which rules match (decision record "Inbox impact
            // preview", point 5).
            MatchField::Merchant => self
                .merchant
                .get_or_init(|| self.input.merchant.map(crate::merchant::clean))
                .as_deref(),
            MatchField::Description => Some(&self.description),
            MatchField::SourceCategory => self.input.source_category,
            MatchField::SourceType => self.input.source_type,
        }
    }

    /// The field [folded](fold), as plain patterns see it.
    fn folded(&self, field: MatchField) -> Option<&str> {
        let index = match field {
            MatchField::Merchant => 0,
            MatchField::Description => 1,
            MatchField::SourceCategory => 2,
            MatchField::SourceType => 3,
        };
        self.folded[index]
            .get_or_init(|| self.text(field).map(fold))
            .as_deref()
    }
}

/// Whitespace collapsed to single spaces and trimmed, like
/// [`normalize_description`](crate::statement::normalize_description) but
/// keeping the case, so a regex sees the letters the bank wrote.
pub(crate) fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Text folded for caseless comparison: each character lowercased,
/// uppercased and lowercased again. This approximates Unicode full case
/// folding without tables: `Straße`, `STRASSE` and `strasse` all fold to
/// `strasse`, and final `ς` to `σ`. Plain patterns and the fields they test
/// are folded the same way.
pub(crate) fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .flat_map(char::to_uppercase)
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_ignores_case_across_scripts() {
        assert_eq!(fold("Tesco STORES"), "tesco stores");
        assert_eq!(fold("CAFÉ"), fold("café"));
        assert_eq!(fold("Straße"), "strasse");
        assert_eq!(fold("STRASSE"), fold("straße"));
        assert_eq!(fold("ẞ"), fold("ß"));
        // Capital, final and medial sigma.
        assert_eq!(fold("ΟΔΟΣ"), fold("οδο\u{3C2}"));
        assert_eq!(fold("οδο\u{3C2}"), fold("οδο\u{3C3}"));
        assert_eq!(fold("ДОМ"), "дом");
        assert_eq!(fold(""), "");
    }

    #[test]
    fn whitespace_collapses_without_changing_case() {
        assert_eq!(
            collapse_whitespace("  Tesco  Stores\t1234 \n"),
            "Tesco Stores 1234"
        );
        assert_eq!(collapse_whitespace("a\u{A0}b"), "a b");
        assert_eq!(collapse_whitespace(" \t "), "");
    }
}
