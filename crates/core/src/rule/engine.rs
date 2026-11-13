//! The rules engine: matching transactions against the stored rules, in
//! priority order, field by field ([`Outcome`]). Applying what they set is
//! the `apply` module's.

use std::cell::OnceCell;

use regex::Regex;
use rusqlite::Connection;

use super::apply::BatchApplied;
use super::store::{self, StoreError, StoredRule};
use super::{
    ActionText, AmountBounds, ExcludeReason, MatchField, MatchKind, RuleAction, Share,
    SplitTemplate,
};
use crate::category::CategoryKey;
use crate::id::{AccountId, BatchId, RuleId};
use crate::tag::TagName;

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
/// matching rule sets is `None` (for tags: empty).
///
/// Rules win field by field (decision record "rules", point 1): in
/// priority order, the first matching rule that sets a field sets it, and a
/// later matching rule can still set a different field. The fields are:
///
/// - **categorisation**: [`category`](Outcome::category) or
///   [`split`](Outcome::split), never both. A rule can't hold both
///   (`RuleError::CategoryWithSplit`), and across rules the first that sets
///   either takes the field: a split categorises the row line by line;
/// - [`merchant`](Outcome::merchant), [`tags`](Outcome::tags),
///   [`note`](Outcome::note), [`exclude`](Outcome::exclude) and
///   [`share`](Outcome::share), one rule each: the first matching rule with
///   an `add_tag` action adds its tags, and a later one adds none (decision
///   record "rules", point 1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Outcome {
    /// The category (`set_category`).
    pub category: Option<SetBy<CategoryKey>>,
    /// The split (`split_template`). `None` whenever `category` is set.
    pub split: Option<SetBy<SplitTemplate>>,
    /// The display merchant (`set_merchant`), as the rule has it (it is
    /// trimmed when applied).
    pub merchant: Option<SetBy<ActionText>>,
    /// The tags to add (`add_tag`), in priority order, then in the order
    /// each rule lists them; no name twice, ignoring case.
    pub tags: Vec<SetBy<TagName>>,
    /// The user note (`set_note`), as the rule has it (it is trimmed when
    /// applied).
    pub note: Option<SetBy<ActionText>>,
    /// The exclusion reason (`exclude`).
    pub exclude: Option<SetBy<ExcludeReason>>,
    /// The user's share (`my_share`), in basis points. With a split it
    /// applies to each line.
    pub share: Option<SetBy<Share>>,
}

impl Outcome {
    /// Whether the rules set nothing.
    pub fn is_empty(&self) -> bool {
        *self == Outcome::default()
    }

    /// Whether the categorisation field is taken.
    fn categorised(&self) -> bool {
        self.category.is_some() || self.split.is_some()
    }

    /// Only the fields `rule` won here.
    fn only(self, rule: &CompiledRule) -> Outcome {
        let id = rule.id;
        Outcome {
            category: self.category.filter(|set| set.rule == id),
            split: self.split.filter(|set| set.rule == id),
            merchant: self.merchant.filter(|set| set.rule == id),
            tags: self.tags.into_iter().filter(|set| set.rule == id).collect(),
            note: self.note.filter(|set| set.rule == id),
            exclude: self.exclude.filter(|set| set.rule == id),
            share: self.share.filter(|set| set.rule == id),
        }
    }
}

/// The stored rules, compiled for matching and in evaluation order
/// (priority, then ID).
///
/// Rules win field by field (decision record "rules", point 1): in
/// priority order, the first matching rule that sets a field sets it, and
/// a later matching rule can still set a different field ([`Outcome`]).
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

    /// Whether the set has rule `rule`.
    pub fn contains(&self, rule: RuleId) -> bool {
        self.rules.iter().any(|compiled| compiled.id == rule)
    }

    /// What the rules set on a transaction with these fields. Depends only
    /// on the fields and the rules (in priority, then ID order), so it is
    /// deterministic.
    pub fn evaluate(&self, input: &MatchInput<'_>) -> Outcome {
        let row = Prepared::new(input);
        let mut outcome = Outcome::default();
        for rule in &self.rules {
            if rule.could_add_to(&outcome) && rule.matches(&row) {
                rule.add_to(&mut outcome);
            }
        }
        outcome
    }

    /// What rule `rule` sets on a transaction with these fields, among all
    /// the rules: the fields it wins ([`evaluate`](RuleSet::evaluate)),
    /// tags included. `None` when the set has no such rule or the rule
    /// doesn't match; an empty outcome when it matches but rules above it
    /// set every field it would.
    pub fn evaluate_rule(&self, rule: RuleId, input: &MatchInput<'_>) -> Option<Outcome> {
        let compiled = self.rules.iter().find(|compiled| compiled.id == rule)?;
        if !compiled.matches(&Prepared::new(input)) {
            return None;
        }
        Some(self.evaluate(input).only(compiled))
    }

    /// Applies the rules to every transaction of an import batch; see
    /// [`apply_to_batch`].
    pub fn apply_to_batch(
        &self,
        conn: &Connection,
        batch: BatchId,
    ) -> Result<BatchApplied, StoreError> {
        super::apply::apply_to_batch(self, conn, batch)
    }
}

/// Applies the stored rules to every transaction of an import batch,
/// mirrored rows included: every action kind, field by field ([`Outcome`]).
/// Loads the rules from `conn` first. What each action writes is in
/// the [`rule`](crate::rule) module docs ("Applying").
///
/// Nothing the user set is changed on import (decision record "rules",
/// point 3): a category from the user or an approved suggestion (`user`,
/// `ai_approved`), a name the user set, an existing note, exclusion or
/// share. Rules do override the preset's category map (`preset_map`) and
/// the derived merchant name. A split that can't be made (the row is in a
/// suggested or confirmed transfer, or too small to give every line a minor
/// unit) is skipped for that row, which keeps what it had, and the import
/// goes on. A rule naming a category that doesn't exist is an error when it
/// matches a row.
///
/// Opens no transaction of its own: the import commit runs it inside its
/// own, after merchant names and before transfer detection.
pub fn apply_to_batch(conn: &Connection, batch: BatchId) -> Result<BatchApplied, StoreError> {
    RuleSet::load(conn)?.apply_to_batch(conn, batch)
}

/// What a rule does, by field.
#[derive(Clone, Debug, Default)]
struct Actions {
    category: Option<CategoryKey>,
    split: Option<SplitTemplate>,
    merchant: Option<ActionText>,
    tags: Vec<TagName>,
    note: Option<ActionText>,
    exclude: Option<ExcludeReason>,
    share: Option<Share>,
}

impl Actions {
    fn new(actions: &[RuleAction]) -> Actions {
        let mut by_field = Actions::default();
        for action in actions {
            match action {
                RuleAction::SetCategory(key) => by_field.category = Some(key.clone()),
                RuleAction::SplitTemplate(template) => by_field.split = Some(template.clone()),
                RuleAction::SetMerchant(name) => by_field.merchant = Some(name.clone()),
                RuleAction::AddTag(tags) => by_field.tags = tags.tags().to_vec(),
                RuleAction::SetNote(note) => by_field.note = Some(note.clone()),
                RuleAction::Exclude(reason) => by_field.exclude = Some(*reason),
                RuleAction::MyShare(share) => by_field.share = Some(*share),
            }
        }
        by_field
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
    actions: Actions,
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
        CompiledRule {
            id: rule.id(),
            field: definition.field(),
            matcher,
            amount: definition.amount(),
            account_id: definition.account_id(),
            actions: Actions::new(definition.actions()),
        }
    }

    /// Whether the rule would set anything on a row that already has
    /// `outcome`, if it matched: a field still free that it sets, or tags.
    /// Checked before matching, which costs more.
    fn could_add_to(&self, outcome: &Outcome) -> bool {
        let actions = &self.actions;
        ((actions.category.is_some() || actions.split.is_some()) && !outcome.categorised())
            || (actions.merchant.is_some() && outcome.merchant.is_none())
            || (!actions.tags.is_empty() && outcome.tags.is_empty())
            || (actions.note.is_some() && outcome.note.is_none())
            || (actions.exclude.is_some() && outcome.exclude.is_none())
            || (actions.share.is_some() && outcome.share.is_none())
    }

    /// Sets the fields of `outcome` this rule sets that no rule above set,
    /// and adds its tags.
    fn add_to(&self, outcome: &mut Outcome) {
        fn by<T>(rule: RuleId) -> impl Fn(T) -> SetBy<T> {
            move |value| SetBy { value, rule }
        }
        let actions = &self.actions;
        if !outcome.categorised() {
            outcome.category = actions.category.clone().map(by(self.id));
            outcome.split = actions.split.clone().map(by(self.id));
        }
        if outcome.merchant.is_none() {
            outcome.merchant = actions.merchant.clone().map(by(self.id));
        }
        if outcome.tags.is_empty() {
            outcome.tags = actions.tags.iter().cloned().map(by(self.id)).collect();
        }
        if outcome.note.is_none() {
            outcome.note = actions.note.clone().map(by(self.id));
        }
        if outcome.exclude.is_none() {
            outcome.exclude = actions.exclude.map(by(self.id));
        }
        if outcome.share.is_none() {
            outcome.share = actions.share.map(by(self.id));
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
