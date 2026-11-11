//! Rule actions and their `rule_action.value_json` encoding.
//!
//! Each [`ActionKind`] has one JSON shape, decoded strictly: wrong types,
//! unknown or repeated fields, floats where an integer is required, and
//! trailing text are refused. Encoding is canonical (no whitespace, fields
//! in a fixed order), so a decoded value re-encodes to the same text.
//!
//! | kind             | `value_json`                                              |
//! |------------------|-----------------------------------------------------------|
//! | `set_category`   | `"food.groceries"`: a category key                         |
//! | `set_merchant`   | `"Uber"`: the display merchant                             |
//! | `add_tag`        | `["work", "travel"]`: one or more distinct tag names        |
//! | `set_note`       | `"Claim on expenses"`: the user note                       |
//! | `exclude`        | `"bank_error"`: a reason key other than `other`            |
//! | `split_template` | `[{"category": "food", "basis_points": 6000}, …]`          |
//! | `my_share`       | `{"basis_points": 5000}`                                   |
//!
//! Shares are integer basis points (1/100 of a percent, `10000` is all of
//! it), never floats.

use std::collections::HashSet;
use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

use serde::de::value::MapAccessDeserializer;
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use super::{ActionKind, UnknownKey, key_enum};
use crate::category::{CategoryKey, CategoryKeyError};

key_enum! {
    /// Why a row is excluded, as `transaction.excluded_reason` stores it.
    ExcludeReason, "exclusion reason" {
        /// The row is a duplicate; it also leaves the computed balance.
        Duplicate => "duplicate",
        /// The bank's mistake, such as a double charge.
        BankError => "bank_error",
        /// A charge being contested, including fraud.
        Disputed => "disputed",
        /// Anything else; needs a note, so a rule can't use it.
        Other => "other",
    }
}

/// Text a rule sets on a transaction: a display merchant, a note or a tag
/// name. Not blank (not empty or only whitespace), and stored as given.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ActionText(String);

impl ActionText {
    /// Checks `text` isn't blank.
    pub fn new(text: &str) -> Result<ActionText, ActionValueError> {
        if text.trim().is_empty() {
            Err(ActionValueError::BlankText)
        } else {
            Ok(ActionText(text.to_owned()))
        }
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ActionText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // User text: length only.
        write!(f, "ActionText(len {})", self.0.len())
    }
}

/// The tag names an `add_tag` action adds: at least one, no repeats.
///
/// Tags are named, not referenced by ID, so a rule can name a tag that
/// doesn't exist yet; creating it is the rules engine's job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagList(Vec<ActionText>);

impl TagList {
    /// Checks `tags` is non-empty with no name twice.
    pub fn new(tags: Vec<ActionText>) -> Result<TagList, ActionValueError> {
        if tags.is_empty() {
            return Err(ActionValueError::NoTags);
        }
        let mut seen = HashSet::new();
        if !tags.iter().all(|tag| seen.insert(tag.as_str())) {
            return Err(ActionValueError::DuplicateTag);
        }
        Ok(TagList(tags))
    }

    /// The tag names, in the order given.
    pub fn tags(&self) -> &[ActionText] {
        &self.0
    }
}

/// A share of a transaction's amount in basis points: `0` is none of it,
/// [`Share::WHOLE`] (`10000`) all of it.
///
/// Turning a share into minor units (and rounding) is the rules engine's
/// job (M1.24); the result must still satisfy the schema's
/// `my_share_minor` check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Share(u16);

impl Share {
    /// All of the amount.
    pub const WHOLE: Share = Share(10_000);

    /// A share of `basis_points` out of 10000.
    pub fn new(basis_points: u64) -> Result<Share, ActionValueError> {
        u16::try_from(basis_points)
            .ok()
            .filter(|bp| *bp <= Share::WHOLE.0)
            .map(Share)
            .ok_or(ActionValueError::ShareOutOfRange)
    }

    /// The share in basis points, `0..=10000`.
    pub const fn basis_points(self) -> u16 {
        self.0
    }
}

/// One line of a split template: a category and its share of the amount.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitLine {
    category: CategoryKey,
    share: Share,
}

impl SplitLine {
    /// A line giving `share` of the amount to `category`.
    pub fn new(category: CategoryKey, share: Share) -> SplitLine {
        SplitLine { category, share }
    }

    /// The line's category.
    pub fn category(&self) -> &CategoryKey {
        &self.category
    }

    /// The line's share of the amount.
    pub const fn share(&self) -> Share {
        self.share
    }
}

/// A split without amounts: two or more lines in distinct categories, each
/// with a share above zero, the shares summing to exactly
/// [`Share::WHOLE`].
///
/// Turning shares into line amounts that sum exactly to the transaction is
/// the rules engine's job (M1.24).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitTemplate(Vec<SplitLine>);

impl SplitTemplate {
    /// Checks `lines` form a whole split.
    pub fn new(lines: Vec<SplitLine>) -> Result<SplitTemplate, ActionValueError> {
        if lines.len() < 2 {
            return Err(ActionValueError::SplitTooFewLines);
        }
        if lines.iter().any(|line| line.share.0 == 0) {
            return Err(ActionValueError::SplitZeroShare);
        }
        let mut seen = HashSet::new();
        if let Some(line) = lines.iter().find(|line| !seen.insert(&line.category)) {
            return Err(ActionValueError::SplitDuplicateCategory(
                line.category.clone(),
            ));
        }
        let total: u32 = lines.iter().map(|line| u32::from(line.share.0)).sum();
        if total != u32::from(Share::WHOLE.0) {
            return Err(ActionValueError::SplitNotWhole { total });
        }
        Ok(SplitTemplate(lines))
    }

    /// The lines, in the order given.
    pub fn lines(&self) -> &[SplitLine] {
        &self.0
    }
}

/// One rule action with its typed value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleAction {
    /// Sets the category.
    SetCategory(CategoryKey),
    /// Sets the display merchant.
    SetMerchant(ActionText),
    /// Adds tags.
    AddTag(TagList),
    /// Sets the user's note.
    SetNote(ActionText),
    /// Excludes the row. Never [`ExcludeReason::Other`].
    Exclude(ExcludeReason),
    /// Splits the row by category.
    SplitTemplate(SplitTemplate),
    /// Sets the user's share of the amount.
    MyShare(Share),
}

/// Why an action value was refused.
///
/// Messages never repeat the value: it is user (or agent) text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ActionValueError {
    /// The JSON isn't valid, or isn't the kind's shape: a wrong type, an
    /// unknown or repeated field, a float where an integer is required.
    #[error("the {kind} value isn't {}", expected_shape(*kind))]
    Shape {
        /// The action kind being decoded.
        kind: ActionKind,
    },

    /// A category key is misspelled.
    #[error(transparent)]
    Category(#[from] CategoryKeyError),

    /// A merchant, note or tag name is empty or only whitespace.
    #[error("the text is blank")]
    BlankText,

    /// An `add_tag` action names no tags.
    #[error("add at least one tag")]
    NoTags,

    /// An `add_tag` action names a tag twice.
    #[error("a tag is named twice")]
    DuplicateTag,

    /// An `exclude` reason isn't a reason key.
    #[error(transparent)]
    UnknownReason(#[from] UnknownKey),

    /// An `exclude` action with reason `other`, which needs a note a rule
    /// doesn't carry.
    #[error("a rule can't exclude with reason `other`: it needs a note; pick another reason")]
    ExcludeOther,

    /// A share is above 10000 basis points.
    #[error("a share is at most 10000 basis points (100%)")]
    ShareOutOfRange,

    /// A split template has fewer than two lines.
    #[error("a split needs at least two lines")]
    SplitTooFewLines,

    /// A split line has a zero share.
    #[error("every split line needs a share above zero")]
    SplitZeroShare,

    /// Two split lines have the same category.
    #[error("the split has two lines in {0}")]
    SplitDuplicateCategory(CategoryKey),

    /// The split lines don't sum to the whole amount.
    #[error("the split lines add up to {total} basis points, not 10000 (100%)")]
    SplitNotWhole {
        /// What they add up to.
        total: u32,
    },
}

/// The shape each kind takes, for [`ActionValueError::Shape`].
fn expected_shape(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::SetCategory => r#"a category key string, such as "food.groceries""#,
        ActionKind::SetMerchant | ActionKind::SetNote => "a string",
        ActionKind::AddTag => "an array of tag-name strings",
        ActionKind::Exclude => "a reason key string",
        ActionKind::SplitTemplate => {
            r#"an array of {"category": key, "basis_points": integer} objects"#
        }
        ActionKind::MyShare => r#"an object {"basis_points": integer}"#,
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSplitLine {
    category: String,
    basis_points: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawShare {
    basis_points: u64,
}

/// A JSON object decoded as `T`. Derived struct impls also accept an
/// array of the fields in order (`["food", 5000]`); this accepts only an
/// object.
struct Object<T>(T);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Object<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Object<T>, D::Error> {
        struct ObjectVisitor<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for ObjectVisitor<T> {
            type Value = Object<T>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object")
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Object<T>, A::Error> {
                T::deserialize(MapAccessDeserializer::new(map)).map(Object)
            }
        }

        deserializer.deserialize_map(ObjectVisitor(PhantomData))
    }
}

impl RuleAction {
    /// The action's kind.
    pub const fn kind(&self) -> ActionKind {
        match self {
            RuleAction::SetCategory(_) => ActionKind::SetCategory,
            RuleAction::SetMerchant(_) => ActionKind::SetMerchant,
            RuleAction::AddTag(_) => ActionKind::AddTag,
            RuleAction::SetNote(_) => ActionKind::SetNote,
            RuleAction::Exclude(_) => ActionKind::Exclude,
            RuleAction::SplitTemplate(_) => ActionKind::SplitTemplate,
            RuleAction::MyShare(_) => ActionKind::MyShare,
        }
    }

    /// An `exclude` action. Refuses [`ExcludeReason::Other`].
    pub fn exclude(reason: ExcludeReason) -> Result<RuleAction, ActionValueError> {
        match reason {
            ExcludeReason::Other => Err(ActionValueError::ExcludeOther),
            reason => Ok(RuleAction::Exclude(reason)),
        }
    }

    /// Decodes a `rule_action` row: `value_json` as a value of `kind`.
    pub fn decode(kind: ActionKind, value_json: &str) -> Result<RuleAction, ActionValueError> {
        fn parse<'a, T: Deserialize<'a>>(
            kind: ActionKind,
            json: &'a str,
        ) -> Result<T, ActionValueError> {
            // serde_json's message can quote the value; keep only the kind.
            serde_json::from_str(json).map_err(|_| ActionValueError::Shape { kind })
        }

        Ok(match kind {
            ActionKind::SetCategory => {
                RuleAction::SetCategory(CategoryKey::new(&parse::<String>(kind, value_json)?)?)
            }
            ActionKind::SetMerchant => {
                RuleAction::SetMerchant(ActionText::new(&parse::<String>(kind, value_json)?)?)
            }
            ActionKind::SetNote => {
                RuleAction::SetNote(ActionText::new(&parse::<String>(kind, value_json)?)?)
            }
            ActionKind::AddTag => {
                let tags = parse::<Vec<String>>(kind, value_json)?
                    .iter()
                    .map(|tag| ActionText::new(tag))
                    .collect::<Result<_, _>>()?;
                RuleAction::AddTag(TagList::new(tags)?)
            }
            ActionKind::Exclude => RuleAction::exclude(ExcludeReason::from_str(
                &parse::<String>(kind, value_json)?,
            )?)?,
            ActionKind::SplitTemplate => {
                let lines = parse::<Vec<Object<RawSplitLine>>>(kind, value_json)?
                    .into_iter()
                    .map(|Object(line)| {
                        Ok(SplitLine::new(
                            CategoryKey::new(&line.category)?,
                            Share::new(line.basis_points)?,
                        ))
                    })
                    .collect::<Result<_, ActionValueError>>()?;
                RuleAction::SplitTemplate(SplitTemplate::new(lines)?)
            }
            ActionKind::MyShare => RuleAction::MyShare(Share::new(
                parse::<RawShare>(kind, value_json)?.basis_points,
            )?),
        })
    }

    /// The canonical `value_json` for this action.
    pub fn value_json(&self) -> String {
        let json = match self {
            RuleAction::SetCategory(key) => serde_json::to_string(key.as_str()),
            RuleAction::SetMerchant(text) | RuleAction::SetNote(text) => {
                serde_json::to_string(text.as_str())
            }
            RuleAction::AddTag(tags) => serde_json::to_string(
                &tags
                    .tags()
                    .iter()
                    .map(ActionText::as_str)
                    .collect::<Vec<_>>(),
            ),
            RuleAction::Exclude(reason) => serde_json::to_string(reason.as_str()),
            RuleAction::SplitTemplate(template) => serde_json::to_string(
                &template
                    .lines()
                    .iter()
                    .map(|line| RawSplitLine {
                        category: line.category.as_str().to_owned(),
                        basis_points: line.share.0.into(),
                    })
                    .collect::<Vec<_>>(),
            ),
            RuleAction::MyShare(share) => serde_json::to_string(&RawShare {
                basis_points: share.0.into(),
            }),
        };
        json.expect("strings, integers, arrays and plain structs always serialize")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: &str) -> CategoryKey {
        CategoryKey::new(key).unwrap()
    }

    fn text(text: &str) -> ActionText {
        ActionText::new(text).unwrap()
    }

    fn share(bp: u64) -> Share {
        Share::new(bp).unwrap()
    }

    fn shape(kind: ActionKind) -> Result<RuleAction, ActionValueError> {
        Err(ActionValueError::Shape { kind })
    }

    #[test]
    fn every_kind_round_trips_through_canonical_json() {
        let cases = [
            (
                RuleAction::SetCategory(key("food.groceries")),
                r#""food.groceries""#,
            ),
            (RuleAction::SetMerchant(text("Uber")), r#""Uber""#),
            (
                RuleAction::AddTag(TagList::new(vec![text("work"), text("gift aid")]).unwrap()),
                r#"["work","gift aid"]"#,
            ),
            (
                RuleAction::SetNote(text("Claim \"back\"\nlater ✓")),
                r#""Claim \"back\"\nlater ✓""#,
            ),
            (
                RuleAction::Exclude(ExcludeReason::Duplicate),
                r#""duplicate""#,
            ),
            (
                RuleAction::Exclude(ExcludeReason::BankError),
                r#""bank_error""#,
            ),
            (
                RuleAction::Exclude(ExcludeReason::Disputed),
                r#""disputed""#,
            ),
            (
                RuleAction::SplitTemplate(
                    SplitTemplate::new(vec![
                        SplitLine::new(key("food.groceries"), share(6_667)),
                        SplitLine::new(key("home"), share(3_333)),
                    ])
                    .unwrap(),
                ),
                r#"[{"category":"food.groceries","basis_points":6667},{"category":"home","basis_points":3333}]"#,
            ),
            (
                RuleAction::MyShare(share(5_000)),
                r#"{"basis_points":5000}"#,
            ),
            (RuleAction::MyShare(share(0)), r#"{"basis_points":0}"#),
            (
                RuleAction::MyShare(Share::WHOLE),
                r#"{"basis_points":10000}"#,
            ),
        ];
        for (action, json) in cases {
            assert_eq!(action.value_json(), json);
            assert_eq!(RuleAction::decode(action.kind(), json), Ok(action));
        }
    }

    #[test]
    fn whitespace_in_the_json_is_accepted_and_dropped() {
        let action =
            RuleAction::decode(ActionKind::MyShare, " { \"basis_points\" : 2500 } ").unwrap();
        assert_eq!(action.value_json(), r#"{"basis_points":2500}"#);
    }

    #[test]
    fn invalid_json_and_wrong_types_are_refused() {
        let cases: &[(ActionKind, &[&str])] = &[
            (
                ActionKind::SetCategory,
                &[
                    "5",
                    "",
                    "food",
                    "null",
                    r#"["food"]"#,
                    r#"{"key":"food"}"#,
                    r#""food" x"#,
                ],
            ),
            (
                ActionKind::SetMerchant,
                &["5", "true", "null", r#"["Uber"]"#],
            ),
            (ActionKind::SetNote, &["{}", "1.5", "'note'"]),
            (
                ActionKind::AddTag,
                &[r#""work""#, "[1]", r#"[["work"]]"#, "{}", "null"],
            ),
            (
                ActionKind::Exclude,
                &["1", r#"{"reason":"duplicate"}"#, r#"["duplicate"]"#],
            ),
            (
                ActionKind::SplitTemplate,
                &[
                    "{}",
                    r#"[["food",5000],["home",5000]]"#,
                    r#"[{"category":"food"},{"category":"home","basis_points":10000}]"#,
                    r#"[{"category":5,"basis_points":5000},{"category":"home","basis_points":5000}]"#,
                ],
            ),
            (
                ActionKind::MyShare,
                &[
                    "5000",
                    r#""5000""#,
                    "{}",
                    r#"{"basis_points":"5000"}"#,
                    r#"{"basis_points":null}"#,
                ],
            ),
        ];
        for (kind, inputs) in cases {
            for json in *inputs {
                assert_eq!(
                    RuleAction::decode(*kind, json),
                    shape(*kind),
                    "{kind} {json}"
                );
            }
        }
    }

    #[test]
    fn floats_and_negatives_are_refused_where_integers_are_required() {
        for json in [
            r#"{"basis_points":5000.0}"#,
            r#"{"basis_points":50.5}"#,
            r#"{"basis_points":5e3}"#,
            r#"{"basis_points":-1}"#,
        ] {
            assert_eq!(
                RuleAction::decode(ActionKind::MyShare, json),
                shape(ActionKind::MyShare),
                "{json}"
            );
        }
        assert_eq!(
            RuleAction::decode(
                ActionKind::SplitTemplate,
                r#"[{"category":"food","basis_points":5000.0},{"category":"home","basis_points":5000}]"#
            ),
            shape(ActionKind::SplitTemplate)
        );
    }

    #[test]
    fn unknown_and_repeated_fields_are_refused() {
        for json in [
            r#"{"basis_points":5000,"label":"Sam"}"#,
            r#"{"basis_points":5000,"basis_points":4000}"#,
            r#"{"percent":50}"#,
        ] {
            assert_eq!(
                RuleAction::decode(ActionKind::MyShare, json),
                shape(ActionKind::MyShare),
                "{json}"
            );
        }
        assert_eq!(
            RuleAction::decode(
                ActionKind::SplitTemplate,
                r#"[{"category":"food","basis_points":5000,"note":"x"},{"category":"home","basis_points":5000}]"#
            ),
            shape(ActionKind::SplitTemplate)
        );
    }

    #[test]
    fn shape_errors_name_the_kind_not_the_value() {
        let err = RuleAction::decode(ActionKind::SetMerchant, r#"{"SECRET":1}"#).unwrap_err();
        assert_eq!(err.to_string(), "the set_merchant value isn't a string");
        let err = RuleAction::decode(ActionKind::MyShare, r#"{"SECRET":1}"#).unwrap_err();
        assert!(!err.to_string().contains("SECRET"), "{err}");
    }

    #[test]
    fn category_keys_are_checked() {
        assert_eq!(
            RuleAction::decode(ActionKind::SetCategory, r#""Food""#),
            Err(ActionValueError::Category(CategoryKeyError(
                "Food".to_owned()
            )))
        );
        assert!(matches!(
            RuleAction::decode(
                ActionKind::SplitTemplate,
                r#"[{"category":"a.b.c","basis_points":5000},{"category":"home","basis_points":5000}]"#
            ),
            Err(ActionValueError::Category(_))
        ));
    }

    #[test]
    fn blank_text_is_refused() {
        for kind in [ActionKind::SetMerchant, ActionKind::SetNote] {
            for json in [r#""""#, r#""   ""#, r#""\n\t""#] {
                assert_eq!(
                    RuleAction::decode(kind, json),
                    Err(ActionValueError::BlankText),
                    "{kind} {json}"
                );
            }
        }
        assert_eq!(
            RuleAction::decode(ActionKind::AddTag, r#"["work"," "]"#),
            Err(ActionValueError::BlankText)
        );
        assert_eq!(ActionText::new(" "), Err(ActionValueError::BlankText));
    }

    #[test]
    fn tags_need_one_and_no_repeats() {
        assert_eq!(
            RuleAction::decode(ActionKind::AddTag, "[]"),
            Err(ActionValueError::NoTags)
        );
        assert_eq!(
            RuleAction::decode(ActionKind::AddTag, r#"["work","home","work"]"#),
            Err(ActionValueError::DuplicateTag)
        );
        assert_eq!(TagList::new(vec![]), Err(ActionValueError::NoTags));
    }

    #[test]
    fn exclude_takes_a_reason_key_but_not_other() {
        assert_eq!(
            RuleAction::decode(ActionKind::Exclude, r#""other""#),
            Err(ActionValueError::ExcludeOther)
        );
        assert_eq!(
            RuleAction::exclude(ExcludeReason::Other),
            Err(ActionValueError::ExcludeOther)
        );
        assert_eq!(
            RuleAction::exclude(ExcludeReason::Disputed),
            Ok(RuleAction::Exclude(ExcludeReason::Disputed))
        );
        for json in [r#""fraud""#, r#""Duplicate""#, r#""""#] {
            assert!(
                matches!(
                    RuleAction::decode(ActionKind::Exclude, json),
                    Err(ActionValueError::UnknownReason(UnknownKey {
                        what: "exclusion reason",
                        ..
                    }))
                ),
                "{json}"
            );
        }
        for reason in ExcludeReason::ALL {
            assert_eq!(reason.as_str().parse::<ExcludeReason>(), Ok(*reason));
        }
    }

    #[test]
    fn shares_are_basis_points_up_to_whole() {
        assert_eq!(share(10_000), Share::WHOLE);
        assert_eq!(share(1).basis_points(), 1);
        for bp in [10_001, 65_536, u64::MAX] {
            assert_eq!(Share::new(bp), Err(ActionValueError::ShareOutOfRange));
        }
        assert_eq!(
            RuleAction::decode(ActionKind::MyShare, r#"{"basis_points":10001}"#),
            Err(ActionValueError::ShareOutOfRange)
        );
    }

    fn template(lines: &[(&str, u64)]) -> Result<SplitTemplate, ActionValueError> {
        SplitTemplate::new(
            lines
                .iter()
                .map(|(category, bp)| SplitLine::new(key(category), share(*bp)))
                .collect(),
        )
    }

    #[test]
    fn split_templates_must_be_whole() {
        let ok = template(&[("food", 5_000), ("home", 3_000), ("fun", 2_000)]).unwrap();
        assert_eq!(ok.lines().len(), 3);
        assert_eq!(ok.lines()[1].category().as_str(), "home");
        assert_eq!(ok.lines()[1].share(), share(3_000));

        assert_eq!(template(&[]), Err(ActionValueError::SplitTooFewLines));
        assert_eq!(
            template(&[("food", 10_000)]),
            Err(ActionValueError::SplitTooFewLines)
        );
        assert_eq!(
            template(&[("food", 10_000), ("home", 0)]),
            Err(ActionValueError::SplitZeroShare)
        );
        assert_eq!(
            template(&[("food", 5_000), ("food", 5_000)]),
            Err(ActionValueError::SplitDuplicateCategory(key("food")))
        );
        assert_eq!(
            template(&[("food", 5_000), ("home", 4_999)]),
            Err(ActionValueError::SplitNotWhole { total: 9_999 })
        );
        assert_eq!(
            template(&[("food", 10_000), ("home", 10_000)]),
            Err(ActionValueError::SplitNotWhole { total: 20_000 })
        );
        assert_eq!(
            RuleAction::decode(ActionKind::SplitTemplate, "[]"),
            Err(ActionValueError::SplitTooFewLines)
        );
    }

    #[test]
    fn debug_hides_action_text() {
        assert_eq!(format!("{:?}", text("SECRET")), "ActionText(len 6)");
    }
}
