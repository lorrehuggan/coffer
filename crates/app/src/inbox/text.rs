//! What the Inbox writes: labels for kinds, rules and names, untrusted text
//! made plain, mixed-font runs, relative times and the confirmation after
//! an approval.
//!
//! **Untrusted text.** A rationale, pattern or name is an AI client's (or,
//! for Coffer's own suggestions, the bank's) text, and bank text is the
//! bank's. Each goes through [`plain`] before it's drawn: the characters
//! [`envelope::shown`] strips (control, zero-width, bidi and other invisible
//! characters) are removed, whitespace runs become one space, and it's cut
//! to a length. It's drawn as GPUI text, which renders no markup and opens
//! no link, and nothing in it decides anything.

use std::ops::Range;

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use coffer_core::envelope;
use coffer_core::inbox::preview::{After, CategoryRef, Subject};
use coffer_core::inbox::{COFFER_SOURCE, Payload, SuggestionId, SuggestionKind};
use coffer_core::merchant::AliasKind;
use coffer_core::money::Money;
use coffer_core::reporting::palette::Palette;
use coffer_core::rule::{MatchField, MatchKind, RuleAction, RuleDefinition};
use gpui_kit::{Font, FontWeight, Rgba, StyledText, TextRun, Window};

use crate::format::{self, Sign};
use crate::theme::tokens::typography::TextRole;
use crate::theme::tokens::{color, font};
use crate::ui::icon::Glyph;
use crate::ui::marker::Marker;

use super::store::InboxData;

/// The longest rationale shown, in characters.
pub const RATIONALE_SHOWN: usize = 500;

/// The longest pattern, name, merchant or account name shown.
pub const NAME_SHOWN: usize = 80;

/// The longest bank text shown in a preview line.
pub const BANK_TEXT_SHOWN: usize = 120;

/// `raw` as plain text on one line, at most `max` characters (module docs).
pub fn plain(raw: &str, max: usize) -> String {
    envelope::shown(raw, max).text
}

/// Bank text with the part a pattern matched: the three parts before,
/// in and after `highlight`, each made [`plain`] (keeping the spaces at
/// their edges), together at most `max` characters.
pub fn plain_highlighted(
    raw: &str,
    highlight: Option<&Range<usize>>,
    max: usize,
) -> (String, String, String) {
    let part = |text: &str| -> String {
        envelope::strip_invisible(text)
            .chars()
            .map(|c| if c.is_whitespace() { ' ' } else { c })
            .collect()
    };
    let (before, matched, after) = match highlight {
        Some(range) if raw.is_char_boundary(range.start) && raw.is_char_boundary(range.end) => (
            part(&raw[..range.start]),
            part(&raw[range.clone()]),
            part(&raw[range.end..]),
        ),
        _ => (part(raw), String::new(), String::new()),
    };
    let mut left = max;
    let mut cut = |text: String| -> String {
        let taken: String = text.chars().take(left).collect();
        left -= taken.chars().count();
        taken
    };
    let before = cut(before);
    let matched = cut(matched);
    let after = cut(after);
    (before, matched, after)
}

/// A run of a line that mixes fonts or colours.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub text: String,
    pub mono: bool,
    pub color: Option<Rgba>,
    pub weight: Option<FontWeight>,
}

impl Part {
    /// Text in the line's own font and colour.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            mono: false,
            color: None,
            weight: None,
        }
    }

    /// Text in Geist Mono: a pattern, a code.
    pub fn mono(text: impl Into<String>) -> Self {
        Self {
            mono: true,
            ..Self::text(text)
        }
    }

    pub fn color(mut self, color: Rgba) -> Self {
        self.color = Some(color);
        self
    }

    pub fn weight(mut self, weight: FontWeight) -> Self {
        self.weight = Some(weight);
        self
    }
}

/// `parts` as one text element in `role` and `color`, each run in its own
/// font and colour. The runs set the family, weight and colour themselves:
/// a run doesn't inherit them from the element it sits in.
pub fn rich(window: &Window, role: TextRole, color: Rgba, parts: Vec<Part>) -> StyledText {
    let base = window.text_style().font();
    let mut text = String::new();
    let mut runs = Vec::with_capacity(parts.len());
    for part in parts.into_iter().filter(|part| !part.text.is_empty()) {
        let font = Font {
            family: if part.mono {
                font::MONO.into()
            } else {
                role.family.into()
            },
            weight: part.weight.unwrap_or(role.weight),
            ..base.clone()
        };
        runs.push(TextRun {
            len: part.text.len(),
            font,
            color: part.color.unwrap_or(color).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        });
        text.push_str(&part.text);
    }
    StyledText::new(text).with_runs(runs)
}

// Kinds, sources and rules ------------------------------------------------------

/// The kind's name in the panel and on its group ("Merchant names" is
/// D6.7's name for `merchant_alias`).
pub fn kind_name(kind: SuggestionKind) -> &'static str {
    match kind {
        SuggestionKind::Rule => "Rules",
        SuggestionKind::MerchantAlias => "Merchant names",
        SuggestionKind::TransferPair => "Transfer pairs",
        SuggestionKind::RefundLink => "Refund links",
        SuggestionKind::Categorization => "Categorisations",
    }
}

/// The kind's icon in the panel.
pub fn kind_glyph(kind: SuggestionKind) -> Glyph {
    match kind {
        SuggestionKind::Rule => Glyph::Plus,
        SuggestionKind::MerchantAlias => Glyph::NameTag,
        SuggestionKind::TransferPair => Glyph::Transfer,
        SuggestionKind::RefundLink => Glyph::Link,
        SuggestionKind::Categorization => Glyph::Arrow,
    }
}

/// A source's name, and the words after it: "Coffer" + "from your edits"
/// for Coffer's own, else the client's `--client` name (display names come
/// with the AI access settings).
pub fn source_name(source: &str) -> (String, Option<&'static str>) {
    if source == COFFER_SOURCE {
        ("Coffer".to_owned(), Some("from your edits"))
    } else {
        (plain(source, NAME_SHOWN), None)
    }
}

/// "Merchant contains": a rule's field and match kind, as a phrase.
pub fn rule_condition(rule: &RuleDefinition) -> String {
    let field = match rule.field() {
        MatchField::Merchant => "Merchant",
        MatchField::Description => "Description",
        MatchField::SourceCategory => "Bank category",
        MatchField::SourceType => "Payment type",
    };
    let kind = match rule.pattern().kind() {
        MatchKind::Is => "is",
        MatchKind::StartsWith => "starts with",
        MatchKind::Contains => "contains",
        MatchKind::Regex => "matches",
    };
    format!("{field} {kind}")
}

/// "Starts with": a merchant name's match kind.
pub fn alias_condition(kind: AliasKind) -> &'static str {
    match kind {
        AliasKind::Exact => "Is",
        AliasKind::Prefix => "Starts with",
        AliasKind::Regex => "Matches",
    }
}

/// A rule's pattern, plain.
pub fn pattern(rule: &RuleDefinition) -> String {
    plain(rule.pattern().as_str(), NAME_SHOWN)
}

/// What a rule does besides categorising, in words: "renames", "adds tags".
pub fn rule_extras(rule: &RuleDefinition) -> Vec<&'static str> {
    rule.actions()
        .iter()
        .filter_map(|action| match action {
            RuleAction::SetMerchant(_) => Some("renames"),
            RuleAction::AddTag(_) => Some("adds tags"),
            _ => None,
        })
        .collect()
}

// Values -------------------------------------------------------------------------

/// A category's marker: its palette colour, or "Other".
pub fn marker(palette: &Palette, category: &CategoryRef) -> Marker {
    match palette.slot(&category.key) {
        Some(slot) => color::CHART
            .get(usize::from(slot).saturating_sub(1))
            .copied()
            .map_or(Marker::Other, Marker::Colour),
        None => Marker::Other,
    }
}

/// A signed amount: "−£18.40", "+£29.90".
pub fn signed(amount: Money) -> String {
    format::money(amount, Sign::Always)
}

/// An amount without its sign: "£250.00".
pub fn unsigned(amount: Money) -> String {
    format::money(
        Money::new(amount.minor().saturating_abs(), amount.currency()),
        Sign::Negative,
    )
}

/// A list date: "4 Nov", "2 Oct 25" for another year.
pub fn date(date: NaiveDate, today: NaiveDate) -> String {
    format::list_date(date, today)
}

/// The day of a stored UTC timestamp (`2026-11-14T10:20:30.000Z`).
pub fn timestamp(stored: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(stored, "%Y-%m-%dT%H:%M:%S%.fZ")
        .ok()
        .map(|time| time.and_utc())
}

/// How long ago `stored` was, as the group footer and the card say it:
/// "just now", "12m ago", "2h ago", else the date ("4 Nov").
pub fn ago(stored: &str, now: DateTime<Utc>) -> String {
    let Some(then) = timestamp(stored) else {
        return String::new();
    };
    let minutes = (now - then).num_minutes();
    match minutes {
        ..1 => "just now".to_owned(),
        1..60 => format!("{minutes}m ago"),
        60..1440 => format!("{}h ago", minutes / 60),
        _ => date(then.date_naive(), now.date_naive()),
    }
}

/// When Coffer filed one of its own: "today", else the date.
pub fn day_of(stored: &str, now: DateTime<Utc>) -> String {
    match timestamp(stored) {
        Some(then) if then.date_naive() == now.date_naive() => "today".to_owned(),
        Some(then) => date(then.date_naive(), now.date_naive()),
        None => String::new(),
    }
}

/// `count` and the noun, singular or plural.
pub fn count(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// The category a rule sets, or its split's first line's.
pub fn rule_category(subject: &Subject) -> Option<&CategoryRef> {
    match subject {
        Subject::Rule(categories) => categories.first(),
        _ => None,
    }
}

/// What a line's AFTER lane says when nothing new is set.
pub fn after_words(after: &After, names: bool) -> Option<&'static str> {
    match after {
        After::Stays if names => Some("Stays, you named it"),
        After::Stays => Some("Stays, you set it"),
        After::Unchanged => Some("No change"),
        After::SplitSkipped => Some("Split: skipped"),
        After::Other => Some("Name or tags change"),
        _ => None,
    }
}

// After approving ------------------------------------------------------------------

/// The confirmation after approving `ids` (`DESIGN.md` → "Inbox impact
/// preview" → "After approving": "Approved rule UBER *EATS: 5 transactions
/// now Eating out"). Approving a categorisation on its own says nothing:
/// the row leaving the list is the result. With no undo yet (user decision
/// 2026-11-14), the bar has no Undo button.
pub fn approved_message(data: &InboxData, ids: &[SuggestionId]) -> Option<String> {
    if let [id] = ids {
        let entry = data.entries.iter().find(|entry| entry.id() == *id)?;
        let preview = entry.preview.as_ref()?;
        let changed = preview
            .lines
            .iter()
            .filter(|line| line.after.changes())
            .count();
        return match (&entry.suggestion.payload, &entry.subject) {
            (Payload::Rule(rule), subject) => {
                let recategorised = match &preview.impact.detail {
                    coffer_core::inbox::Detail::Rule(impact) => impact.recategorises,
                    _ => changed,
                };
                let category = rule_category(subject)
                    .map(|category| plain(&category.label, NAME_SHOWN))
                    .unwrap_or_else(|| "its category".to_owned());
                Some(format!(
                    "Approved rule {}: {} now {category}",
                    pattern(rule),
                    count(recategorised, "transaction", "transactions")
                ))
            }
            (Payload::MerchantAlias(alias), _) => {
                let renamed = match &preview.impact.detail {
                    coffer_core::inbox::Detail::MerchantAlias(impact) => impact.renames,
                    _ => changed,
                };
                Some(format!(
                    "Approved merchant name {}: {} renamed",
                    plain(alias.name().as_str(), NAME_SHOWN),
                    count(renamed, "transaction", "transactions")
                ))
            }
            (
                Payload::TransferPair(_),
                Subject::TransferPair {
                    outflow: Some(outflow),
                    inflow: Some(inflow),
                },
            ) => Some(format!(
                "Approved transfer: {} from {} to {}",
                unsigned(inflow.amount),
                plain(&outflow.account_name, NAME_SHOWN),
                plain(&inflow.account_name, NAME_SHOWN)
            )),
            (
                Payload::RefundLink(_),
                Subject::RefundLink {
                    purchase: Some(purchase),
                    ..
                },
            ) => {
                let amount = match &preview.impact.detail {
                    coffer_core::inbox::Detail::RefundLink(link) => unsigned(link.amount),
                    _ => String::new(),
                };
                Some(format!(
                    "Approved refund link: {amount} back on {}",
                    plain(purchase.name(), NAME_SHOWN)
                ))
            }
            _ => None,
        };
    }
    (ids.len() > 1).then(|| format!("Approved {} suggestions", ids.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untrusted_text_is_stripped_and_cut() {
        assert_eq!(
            plain("Uber\u{202E} Eats\u{200B}\nis food", 12),
            "Uber Eats is"
        );
        let (before, matched, after) =
            plain_highlighted("UBER *EATS\u{200B} HELP", Some(&(0..10)), 12);
        assert_eq!(
            (before.as_str(), matched.as_str(), after.as_str()),
            ("", "UBER *EATS", " H")
        );
    }

    #[test]
    fn ago_reads_minutes_hours_then_the_date() {
        let now = timestamp("2026-11-14T12:00:00.000Z").unwrap();
        assert_eq!(ago("2026-11-14T11:59:30.000Z", now), "just now");
        assert_eq!(ago("2026-11-14T11:48:00.000Z", now), "12m ago");
        assert_eq!(ago("2026-11-14T10:00:00.000Z", now), "2h ago");
        assert_eq!(ago("2026-11-04T10:00:00.000Z", now), "4 Nov");
        assert_eq!(day_of("2026-11-14T01:00:00.000Z", now), "today");
    }
}
