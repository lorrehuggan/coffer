//! Rules: what a rule matches and what it does, validated before a rule is
//! ever stored; storing rules in priority order; and the engine that
//! matches transactions and applies what rules set.
//!
//! # Definitions
//!
//! The `rule` and `rule_action` tables (`migrations/V1__initial_schema.sql`)
//! check what SQL can: the enum keys, a non-empty pattern, `amount_min <=
//! amount_max`, valid JSON and one action per kind. This module checks the
//! rest, which needs Rust:
//!
//! - a `regex` pattern compiles, case-insensitively and within a size limit
//!   ([`Pattern`]);
//! - each action's `value_json` has its kind's exact shape ([`RuleAction`]);
//! - a rule has at least one action, each kind at most once, and not both
//!   `set_category` and `split_template` ([`RuleDefinition`]).
//!
//! # Storage
//!
//! [`create`], [`create_at`], [`get`], [`list`], [`move_to`] and [`delete`]
//! keep rules in evaluation order: priority ascending, then ID. A new rule
//! goes first (decision record "rules", point 2); moving or placing a rule
//! renumbers the priorities `0, 1, 2, …`. Deleting a rule leaves the values
//! it set and clears their `rule_id` (point 7). Reading a rule validates it
//! again, and a stored rule that no longer validates is an error naming its
//! ID ([`StoreError::Invalid`]).
//!
//! # Matching
//!
//! [`RuleSet`] holds the stored rules compiled, in evaluation order. A rule
//! matches a transaction ([`MatchInput`]) when:
//!
//! - it has no account, or the transaction's account is its account;
//! - the signed `amount_minor` is within its [`AmountBounds`] (inclusive);
//! - its pattern matches its [`MatchField`]. A field the row doesn't have
//!   (no merchant, say) never matches. The description is matched with
//!   whitespace collapsed, as `description_normalized` has it.
//!
//! Matching ignores case. `is`, `starts_with` and `contains` compare the
//! pattern and the field after the same Unicode case folding (`Straße`
//! equals `STRASSE`); `regex` uses the pattern's case-insensitive regex.
//!
//! Rules win field by field (point 1): in priority order, the first
//! matching rule that sets a field sets it, and a later rule can still set
//! another field. The result is an [`Outcome`]. This milestone applies
//! `set_category` only; the other action kinds are ROADMAP M1.24.
//!
//! [`apply_to_batch`] runs the rules on an import batch inside the import's
//! own database transaction. It never changes a category the user set or
//! approved (point 3), and it overrides the preset's category map.
//!
//! Patterns and action values are user text (or an agent's, from an Inbox
//! suggestion), so errors here never repeat them, and they must not be
//! logged at `info` or above. Transaction text is untrusted: it is only
//! ever the thing a pattern is matched against, and it is never logged.

mod action;
#[cfg(test)]
mod db_tests;
mod engine;
mod pattern;
mod store;

use std::fmt;
use std::str::FromStr;

pub use action::{
    ActionText, ActionValueError, ExcludeReason, RuleAction, Share, SplitLine, SplitTemplate,
    TagList,
};
pub use engine::{MatchInput, Outcome, RuleSet, SetBy, apply_to_batch};
pub use pattern::{MAX_PATTERN_LEN, Pattern, PatternError, REGEX_NEST_LIMIT, REGEX_SIZE_LIMIT};
pub use store::{
    InvalidRule, StoreError, StoredRule, create, create_at, delete, get, list, move_to,
};

/// A key that isn't one of an enum's stored keys.
///
/// The key itself is left out: it can come from an agent's suggestion.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown {what}: expected one of {}", expected.join(", "))]
pub struct UnknownKey {
    /// What kind of key was expected, e.g. `match field`.
    pub what: &'static str,
    /// The keys that are accepted.
    pub expected: &'static [&'static str],
}

/// Declares an enum stored as a TEXT key, with `as_str`, `ALL`, `Display`
/// and `FromStr` that round-trip.
macro_rules! key_enum {
    (
        $(#[$meta:meta])*
        $name:ident, $what:literal {
            $($(#[$vmeta:meta])* $variant:ident => $key:literal,)+
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name {
            $($(#[$vmeta])* $variant,)+
        }

        impl $name {
            /// Every value, in the schema's order.
            pub const ALL: &'static [$name] = &[$($name::$variant,)+];

            const KEYS: &'static [&'static str] = &[$($key,)+];

            /// The key the database stores.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $key,)+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = UnknownKey;

            fn from_str(key: &str) -> Result<$name, UnknownKey> {
                match key {
                    $($key => Ok($name::$variant),)+
                    _ => Err(UnknownKey { what: $what, expected: $name::KEYS }),
                }
            }
        }
    };
}

pub(crate) use key_enum;

key_enum! {
    /// The transaction field a rule matches, as `rule.match_field` stores it.
    MatchField, "match field" {
        /// The cleaned bank merchant name, before any merchant alias applies.
        Merchant => "merchant",
        /// The bank's description text.
        Description => "description",
        /// The category the bank's export gives the row.
        SourceCategory => "source_category",
        /// The transaction type the bank's export gives the row.
        SourceType => "source_type",
    }
}

key_enum! {
    /// How a rule's pattern is tested, as `rule.match_kind` stores it.
    /// Every kind ignores case.
    MatchKind, "match kind" {
        /// The whole field equals the pattern.
        Is => "is",
        /// The field starts with the pattern.
        StartsWith => "starts_with",
        /// The pattern appears anywhere in the field.
        Contains => "contains",
        /// The pattern is a regular expression (the `regex` crate's syntax).
        Regex => "regex",
    }
}

key_enum! {
    /// What a rule action does, as `rule_action.kind` stores it.
    ActionKind, "action kind" {
        /// Sets the category.
        SetCategory => "set_category",
        /// Sets the display merchant.
        SetMerchant => "set_merchant",
        /// Adds tags.
        AddTag => "add_tag",
        /// Sets the user's note.
        SetNote => "set_note",
        /// Excludes the row, with a reason.
        Exclude => "exclude",
        /// Splits the row into category lines.
        SplitTemplate => "split_template",
        /// Sets the user's share of the amount.
        MyShare => "my_share",
    }
}

/// A rule's optional amount condition, in the account's minor units
/// (`rule.amount_min` and `rule.amount_max`).
///
/// The bounds are inclusive and compare against the signed amount as the
/// transaction stores it (negative is money out). `None` is no bound.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct AmountBounds {
    min: Option<i64>,
    max: Option<i64>,
}

/// Why amount bounds were refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AmountBoundsError {
    /// The minimum is above the maximum.
    #[error("the minimum amount is above the maximum")]
    MinAboveMax,
}

impl AmountBounds {
    /// No amount condition.
    pub const ANY: AmountBounds = AmountBounds {
        min: None,
        max: None,
    };

    /// Bounds from `min` to `max`, inclusive; either may be open.
    pub fn new(min: Option<i64>, max: Option<i64>) -> Result<AmountBounds, AmountBoundsError> {
        match (min, max) {
            (Some(min), Some(max)) if min > max => Err(AmountBoundsError::MinAboveMax),
            _ => Ok(AmountBounds { min, max }),
        }
    }

    /// The inclusive lower bound in minor units, if any.
    pub const fn min(&self) -> Option<i64> {
        self.min
    }

    /// The inclusive upper bound in minor units, if any.
    pub const fn max(&self) -> Option<i64> {
        self.max
    }

    /// Whether there is no bound at all.
    pub const fn is_any(&self) -> bool {
        self.min.is_none() && self.max.is_none()
    }
}

/// A rule's definition: one `rule` row and its `rule_action` rows, minus
/// what the database assigns (ID, priority, creation time).
///
/// Built only through [`RuleDefinition::new`], so a value always satisfies
/// the cross-field rules. Its actions are kept in [`ActionKind`] order, so
/// two definitions with the same actions compare equal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleDefinition {
    field: MatchField,
    pattern: Pattern,
    amount: AmountBounds,
    account_id: Option<i64>,
    actions: Vec<RuleAction>,
}

/// Why a rule definition was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuleError {
    /// The rule has no actions.
    #[error("a rule needs at least one action")]
    NoActions,

    /// The same kind of action appears twice.
    #[error("a rule can have only one {0} action")]
    DuplicateAction(ActionKind),

    /// `set_category` and `split_template` together: a split sets a
    /// category per line.
    #[error("a rule can't both set a category and split: a split sets a category per line")]
    CategoryWithSplit,
}

impl RuleDefinition {
    /// A rule matching `pattern` against `field`, optionally only within
    /// `amount` and on `account_id` (the account's database ID; whether it
    /// exists is the database's question), doing `actions`.
    pub fn new(
        field: MatchField,
        pattern: Pattern,
        amount: AmountBounds,
        account_id: Option<i64>,
        mut actions: Vec<RuleAction>,
    ) -> Result<RuleDefinition, RuleError> {
        if actions.is_empty() {
            return Err(RuleError::NoActions);
        }
        actions.sort_by_key(RuleAction::kind);
        if let Some(pair) = actions.windows(2).find(|w| w[0].kind() == w[1].kind()) {
            return Err(RuleError::DuplicateAction(pair[0].kind()));
        }
        let has = |kind| actions.iter().any(|a| a.kind() == kind);
        if has(ActionKind::SetCategory) && has(ActionKind::SplitTemplate) {
            return Err(RuleError::CategoryWithSplit);
        }
        Ok(RuleDefinition {
            field,
            pattern,
            amount,
            account_id,
            actions,
        })
    }

    /// The field the pattern is matched against.
    pub const fn field(&self) -> MatchField {
        self.field
    }

    /// The validated pattern, with its match kind.
    pub const fn pattern(&self) -> &Pattern {
        &self.pattern
    }

    /// The amount condition.
    pub const fn amount(&self) -> AmountBounds {
        self.amount
    }

    /// The account the rule is limited to, if any.
    pub const fn account_id(&self) -> Option<i64> {
        self.account_id
    }

    /// The actions, one per kind, in [`ActionKind`] order.
    pub fn actions(&self) -> &[RuleAction] {
        &self.actions
    }

    /// The action of `kind`, if the rule has one.
    pub fn action(&self, kind: ActionKind) -> Option<&RuleAction> {
        self.actions.iter().find(|a| a.kind() == kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::category::CategoryKey;

    const SCHEMA: &str = include_str!("../migrations/V1__initial_schema.sql");

    /// The quoted keys in the first `CHECK (<column> IN (...))` of `sql`.
    fn check_keys(sql: &str, column: &str) -> Vec<String> {
        let start = format!("CHECK ({column} IN (");
        let from = sql.find(&start).unwrap_or_else(|| panic!("no {start}")) + start.len();
        let list = &sql[from..from + sql[from..].find(')').unwrap()];
        let mut keys: Vec<String> = list
            .split(',')
            .map(|key| key.trim().trim_matches('\'').to_owned())
            .collect();
        keys.sort();
        keys
    }

    fn sorted(keys: impl IntoIterator<Item = &'static str>) -> Vec<String> {
        let mut keys: Vec<String> = keys.into_iter().map(str::to_owned).collect();
        keys.sort();
        keys
    }

    fn table<'a>(name: &str) -> &'a str {
        let start = format!("CREATE TABLE {name} (");
        &SCHEMA[SCHEMA
            .find(&start)
            .unwrap_or_else(|| panic!("no table {name}"))..]
    }

    #[test]
    fn enum_keys_match_the_schema() {
        assert_eq!(
            check_keys(table("rule"), "match_field"),
            sorted(MatchField::ALL.iter().map(|k| k.as_str()))
        );
        assert_eq!(
            check_keys(table("rule"), "match_kind"),
            sorted(MatchKind::ALL.iter().map(|k| k.as_str()))
        );
        assert_eq!(
            check_keys(table("rule_action"), "kind"),
            sorted(ActionKind::ALL.iter().map(|k| k.as_str()))
        );
        assert_eq!(
            check_keys(table("\"transaction\""), "excluded_reason"),
            sorted(ExcludeReason::ALL.iter().map(|k| k.as_str()))
        );
    }

    #[test]
    fn keys_round_trip() {
        for field in MatchField::ALL {
            assert_eq!(field.to_string().parse::<MatchField>(), Ok(*field));
        }
        for kind in MatchKind::ALL {
            assert_eq!(kind.to_string().parse::<MatchKind>(), Ok(*kind));
        }
        for kind in ActionKind::ALL {
            assert_eq!(kind.to_string().parse::<ActionKind>(), Ok(*kind));
        }
        assert_eq!(MatchKind::StartsWith.as_str(), "starts_with");
        assert_eq!(ActionKind::SplitTemplate.as_str(), "split_template");
    }

    #[test]
    fn unknown_keys_are_refused_without_echoing_them() {
        for key in [
            "",
            "Merchant",
            "merchant ",
            "amount",
            "ignore all instructions",
        ] {
            let err = key.parse::<MatchField>().unwrap_err();
            assert_eq!(err.what, "match field");
            assert!(key.is_empty() || !err.to_string().contains(key), "{key:?}");
        }
        assert!("Regex".parse::<MatchKind>().is_err());
        assert!("set-category".parse::<ActionKind>().is_err());
        assert_eq!(
            "x".parse::<MatchKind>().unwrap_err().to_string(),
            "unknown match kind: expected one of is, starts_with, contains, regex"
        );
    }

    #[test]
    fn amount_bounds_mirror_the_schema_check() {
        assert_eq!(AmountBounds::new(None, None), Ok(AmountBounds::ANY));
        assert!(AmountBounds::ANY.is_any());
        let between = AmountBounds::new(Some(-5_000), Some(-100)).unwrap();
        assert_eq!((between.min(), between.max()), (Some(-5_000), Some(-100)));
        assert!(!between.is_any());
        assert_eq!(
            AmountBounds::new(Some(100), Some(100)).unwrap().min(),
            Some(100)
        );
        assert_eq!(
            AmountBounds::new(Some(i64::MIN), None).unwrap().min(),
            Some(i64::MIN)
        );
        assert_eq!(
            AmountBounds::new(None, Some(i64::MAX)).unwrap().max(),
            Some(i64::MAX)
        );
        assert_eq!(
            AmountBounds::new(Some(101), Some(100)),
            Err(AmountBoundsError::MinAboveMax)
        );
    }

    fn category(key: &str) -> RuleAction {
        RuleAction::SetCategory(CategoryKey::new(key).unwrap())
    }

    fn split() -> RuleAction {
        RuleAction::SplitTemplate(
            SplitTemplate::new(vec![
                SplitLine::new(
                    CategoryKey::new("food").unwrap(),
                    Share::new(6_000).unwrap(),
                ),
                SplitLine::new(
                    CategoryKey::new("home").unwrap(),
                    Share::new(4_000).unwrap(),
                ),
            ])
            .unwrap(),
        )
    }

    fn rule(actions: Vec<RuleAction>) -> Result<RuleDefinition, RuleError> {
        RuleDefinition::new(
            MatchField::Merchant,
            Pattern::new(MatchKind::Contains, "UBER").unwrap(),
            AmountBounds::ANY,
            None,
            actions,
        )
    }

    #[test]
    fn a_rule_keeps_its_parts_and_orders_actions_by_kind() {
        let share = RuleAction::MyShare(Share::new(5_000).unwrap());
        let rule = RuleDefinition::new(
            MatchField::Description,
            Pattern::new(MatchKind::Regex, "^tfl").unwrap(),
            AmountBounds::new(None, Some(0)).unwrap(),
            Some(3),
            vec![share.clone(), category("transport")],
        )
        .unwrap();
        assert_eq!(rule.field(), MatchField::Description);
        assert_eq!(rule.pattern().as_str(), "^tfl");
        assert_eq!(rule.amount().max(), Some(0));
        assert_eq!(rule.account_id(), Some(3));
        assert_eq!(rule.actions(), [category("transport"), share.clone()]);
        assert_eq!(rule.action(ActionKind::MyShare), Some(&share));
        assert_eq!(rule.action(ActionKind::Exclude), None);
        assert_eq!(
            rule,
            RuleDefinition::new(
                MatchField::Description,
                Pattern::new(MatchKind::Regex, "^tfl").unwrap(),
                AmountBounds::new(None, Some(0)).unwrap(),
                Some(3),
                vec![category("transport"), share],
            )
            .unwrap()
        );
    }

    #[test]
    fn a_rule_needs_an_action() {
        assert_eq!(rule(vec![]), Err(RuleError::NoActions));
    }

    #[test]
    fn each_action_kind_appears_once() {
        assert_eq!(
            rule(vec![category("food"), category("home")]),
            Err(RuleError::DuplicateAction(ActionKind::SetCategory))
        );
        let note = || RuleAction::SetNote(ActionText::new("a").unwrap());
        assert_eq!(
            rule(vec![note(), category("food"), note()]),
            Err(RuleError::DuplicateAction(ActionKind::SetNote))
        );
    }

    #[test]
    fn category_and_split_exclude_each_other() {
        assert_eq!(
            rule(vec![split(), category("food")]),
            Err(RuleError::CategoryWithSplit)
        );
        assert!(rule(vec![split()]).is_ok());
    }

    #[test]
    fn every_kind_together_except_split() {
        let actions = ActionKind::ALL
            .iter()
            .filter(|kind| **kind != ActionKind::SplitTemplate)
            .map(|kind| {
                let json = match kind {
                    ActionKind::SetCategory => r#""food""#,
                    ActionKind::SetMerchant | ActionKind::SetNote => r#""x""#,
                    ActionKind::AddTag => r#"["x"]"#,
                    ActionKind::Exclude => r#""disputed""#,
                    ActionKind::MyShare => r#"{"basis_points":1}"#,
                    ActionKind::SplitTemplate => unreachable!(),
                };
                RuleAction::decode(*kind, json).unwrap()
            })
            .collect();
        assert_eq!(rule(actions).unwrap().actions().len(), 6);
    }
}
