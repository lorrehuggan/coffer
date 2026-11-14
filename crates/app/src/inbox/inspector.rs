//! The Inbox's inspector (`docs/DESIGN.md` → "Inbox", "Inbox impact
//! preview"; Paper `ABK-0`, `DL8-0`, `EWZ-0`): the selected suggestion's
//! header, the card it came on (an agent's in moldavite, Coffer's own
//! neutral) with Approve and Reject, **If you approve** (what approving
//! would change, from [`coffer_core::inbox::preview`]), the rule or the
//! merchant name, and how many rows it touches.
//!
//! Nothing here is the agent's own words except the card's reason (and
//! the pattern and name it proposes), all shown plain ([`super::text`]).
//! Two things the D6.6 copy mentions aren't built yet, so it leaves them
//! out: undoing an approval and opening a rule in Rules (user,
//! 2026-11-14).

use chrono::NaiveDate;
use coffer_core::inbox::preview::{Now, Preview, SetBy, Subject, TransactionRef};
use coffer_core::inbox::{COFFER_SOURCE, Detail, Payload, SuggestionStatus};
use coffer_core::link::LinkKind;
use coffer_core::reporting::Period;
use coffer_core::reporting::palette::Palette;
use gpui_kit::component::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, InteractiveElement as _, IntoElement, ParentElement as _, Rgba,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
};

use crate::theme::tokens::typography::TextRole;
use crate::theme::tokens::{color, font, radius, size, spacing, typography, weight};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::field::FieldRow;
use crate::ui::icon::Glyph;
use crate::ui::inspector::{section, section_title};
use crate::ui::marker::Marker;
use crate::ui::text::TextRoleExt as _;
use crate::ui::tracked::Tracked;

use super::store::Entry;
use super::text::{
    self, NAME_SHOWN, Part, RATIONALE_SHOWN, alias_condition, count, marker, plain, rich,
    rule_condition, source_name,
};
use super::view::InboxView;
use super::{Inbox, Outcome, Scope};

/// The title in Geist Mono, for a pattern inside the title.
const TITLE_MONO: TextRole = TextRole {
    family: font::MONO,
    ..typography::TITLE
};

/// "Nov": the month in "less in Nov".
fn month(period: Period) -> String {
    NaiveDate::from_ymd_opt(period.year(), period.month(), 1)
        .map(|day| day.format("%b").to_string())
        .unwrap_or_default()
}

/// The header: a title (with an optional mono part), a display figure
/// (with an optional muted tail, "5 of 6") and a caption.
fn header(
    title: Vec<(String, bool)>,
    figure: Option<(String, Option<String>)>,
    caption: Option<AnyElement>,
) -> impl IntoElement {
    section(false)
        .gap(spacing::S1)
        .child(
            div()
                .flex()
                .min_w_0()
                .overflow_hidden()
                .text_color(color::TEXT)
                .children(title.into_iter().map(|(part, mono)| {
                    Tracked::new(if mono { TITLE_MONO } else { typography::TITLE }, part)
                })),
        )
        .when_some(figure, |this, (figure, tail)| {
            this.child(
                div()
                    .flex()
                    .text_color(color::TEXT)
                    .child(Tracked::new(typography::DISPLAY_FIGURE, figure))
                    .children(tail.map(|tail| {
                        div()
                            .text_color(color::TEXT_MUTED)
                            .child(Tracked::new(typography::DISPLAY_FIGURE, tail))
                    })),
            )
        })
        .children(caption)
}

fn caption(text: impl Into<SharedString>) -> AnyElement {
    div()
        .text_role(typography::CAPTION)
        .text_color(color::TEXT_MUTED)
        .child(text.into())
        .into_any_element()
}

fn danger(text: impl Into<SharedString>) -> AnyElement {
    div()
        .text_role(typography::CAPTION)
        .text_color(color::DANGER)
        .child(text.into())
        .into_any_element()
}

/// A field row's value: 8px inline padding, body type in `text`.
fn value(window: &Window, parts: Vec<Part>) -> AnyElement {
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .px(spacing::S2)
        .text_role(typography::BODY)
        .text_color(color::TEXT)
        .child(rich(window, typography::BODY, color::TEXT, parts))
        .into_any_element()
}

/// A value with a category marker before it.
fn marked(window: &Window, marker: Marker, parts: Vec<Part>) -> AnyElement {
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .gap(spacing::S2)
        .overflow_hidden()
        .whitespace_nowrap()
        .px(spacing::S2)
        .text_role(typography::BODY)
        .text_color(color::TEXT)
        .child(marker.render())
        .child(rich(window, typography::BODY, color::TEXT, parts))
        .into_any_element()
}

/// "5 past transactions": the number in mono, the words in UI type.
fn counted(value: usize, one: &str, many: &str) -> Vec<Part> {
    vec![
        Part::mono(value.to_string()),
        Part::text(format!(" {}", if value == 1 { one } else { many })),
    ]
}

fn muted_parts(text: impl Into<String>) -> Part {
    Part::text(text).color(color::TEXT_MUTED)
}

/// What a suggestion proposes, as the card's sentence: the proposed
/// category or name in `accent`, patterns in mono.
fn proposal(entry: &Entry, accent: Rgba, today: NaiveDate) -> Vec<Part> {
    let subject = |text: String| Part::text(text).color(accent);
    match (&entry.suggestion.payload, &entry.subject) {
        (Payload::Rule(rule), subject_ref) => match text::rule_category(subject_ref) {
            Some(category) => vec![
                Part::text("File "),
                Part::mono(text::pattern(rule)),
                Part::text(" under "),
                subject(plain(&category.label, NAME_SHOWN)),
            ],
            None => vec![
                Part::text("Apply a rule to "),
                Part::mono(text::pattern(rule)),
            ],
        },
        (Payload::MerchantAlias(alias), _) => vec![
            Part::text("Show "),
            Part::mono(plain(alias.pattern(), NAME_SHOWN)),
            Part::text(" as "),
            subject(plain(alias.name().as_str(), NAME_SHOWN)),
        ],
        (Payload::TransferPair(_), _) => {
            vec![Part::text("Pair as a "), subject("transfer".to_owned())]
        }
        (Payload::RefundLink(link), Subject::RefundLink { purchase, .. }) => {
            let of = match link.kind {
                LinkKind::Reimbursement => "Link as a reimbursement of ",
                _ => "Link as a refund of ",
            };
            let target = purchase
                .as_ref()
                .map_or("a transaction gone since".to_owned(), |p| {
                    format!(
                        "{} · {}",
                        plain(p.name(), NAME_SHOWN),
                        text::date(p.date, today)
                    )
                });
            vec![Part::text(of), subject(target)]
        }
        (_, Subject::Categorization(items)) => {
            let category = items
                .first()
                .and_then(|item| item.category.as_ref())
                .map_or("a category gone since".to_owned(), |c| {
                    plain(&c.label, NAME_SHOWN)
                });
            if items.len() > 1 {
                return vec![
                    Part::text(format!(
                        "Categorise {} as ",
                        count(items.len(), "transaction", "transactions")
                    )),
                    subject(category),
                ];
            }
            match items
                .first()
                .and_then(|item| item.transaction.as_ref())
                .map(|t| &t.now)
            {
                Some(Now::Category { category: now, .. }) => vec![
                    Part::text(format!("Change {} to ", plain(&now.label, NAME_SHOWN))),
                    subject(category),
                ],
                _ => vec![Part::text("Categorise as "), subject(category)],
            }
        }
        _ => vec![Part::text("This suggestion no longer fits today's data")],
    }
}

/// The card the suggestion came on, with Approve and Reject while it's
/// pending (`DESIGN.md` → "Inbox"; Coffer's own: "Inbox impact preview").
fn card(view: &InboxView, inbox: &Inbox, entry: &Entry, window: &Window) -> AnyElement {
    let agent = entry.suggestion.source_client != COFFER_SOURCE;
    let now = inbox.data().read_at.unwrap_or_default();
    let today = now.date_naive();
    let (name, suffix) = source_name(&entry.suggestion.source_client);
    let pending = entry.suggestion.status == SuggestionStatus::Pending;
    let header = div()
        .flex()
        .items_center()
        .gap(if agent { spacing::S2 } else { spacing::S1_5 })
        .w_full()
        .whitespace_nowrap()
        .text_role(typography::BODY)
        .child(if agent {
            Glyph::Sparkle.render().text_color(color::MOLDAVITE)
        } else {
            Glyph::Mark
                .render_at(size::ICON_SM)
                .text_color(color::TEXT_SECONDARY)
        })
        .child(
            div()
                .flex_none()
                .font_weight(weight::STRONG)
                .text_color(if agent { color::MOLDAVITE } else { color::TEXT })
                .child(name),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .text_color(if agent {
                    color::MOLDAVITE_TEXT_MUTED
                } else {
                    color::TEXT_MUTED
                })
                .child(match suffix {
                    Some(suffix) => format!(
                        "{suffix} · {}",
                        text::day_of(&entry.suggestion.created_at, now)
                    ),
                    None => text::ago(&entry.suggestion.created_at, now),
                }),
        );
    let accent = if agent {
        color::MOLDAVITE_TEXT
    } else {
        color::TEXT_STRONG
    };
    let reason = entry
        .suggestion
        .rationale
        .as_deref()
        .map(|reason| plain(reason, RATIONALE_SHOWN))
        .filter(|reason| !reason.is_empty());
    let proposal = div()
        .flex()
        .flex_col()
        .gap(spacing::S0_5)
        .child(
            div()
                .text_role(typography::BODY)
                .text_color(color::TEXT_STRONG)
                .child(rich(
                    window,
                    typography::BODY,
                    color::TEXT_STRONG,
                    proposal(entry, accent, today),
                )),
        )
        .children(reason.map(|reason| {
            div()
                .text_role(typography::CAPTION)
                .text_color(if agent {
                    color::MOLDAVITE_TEXT_MUTED
                } else {
                    color::TEXT_MUTED
                })
                .child(reason)
        }));
    let surface = if agent {
        color::MOLDAVITE_SURFACE
    } else {
        color::FLOAT
    };
    let blocked = entry
        .preview
        .as_ref()
        .and_then(|preview| preview.impact.blocked.clone())
        .or_else(|| entry.refused.clone());
    let id = entry.id();
    let (approve, reject) = (view.inbox().clone(), view.inbox().clone());
    let actions = pending.then(|| {
        div()
            .flex()
            .gap(spacing::S1_5)
            .w_full()
            .child(
                Button::new("inbox-card-approve", "Approve")
                    .with_variant(if agent {
                        ButtonVariant::AgentPrimary
                    } else {
                        ButtonVariant::Primary
                    })
                    .with_full_width(true)
                    .with_surface(surface)
                    .disabled(blocked.is_some())
                    .on_click(click(move |_, _, cx| {
                        approve.update(cx, |inbox, cx| inbox.approve(id, cx))
                    })),
            )
            .child(
                Button::new("inbox-card-reject", "Reject")
                    .with_variant(if agent {
                        ButtonVariant::AgentOutline
                    } else {
                        ButtonVariant::Secondary
                    })
                    .with_surface(surface)
                    .on_click(click(move |_, _, cx| {
                        reject.update(cx, |inbox, cx| inbox.reject(id, cx))
                    })),
            )
    });
    let refused = match inbox.outcome() {
        Some(Outcome::Refused {
            scope: Scope::Suggestion(scope),
            message,
        }) if *scope == id => Some(message.clone()),
        _ => None,
    };
    let card = div()
        .flex()
        .flex_col()
        .gap(spacing::S2_5)
        .p(spacing::S3)
        .rounded(radius::LG)
        .bg(surface)
        .border(size::HAIRLINE)
        .border_color(if agent {
            color::MOLDAVITE_BORDER
        } else {
            color::HAIRLINE
        })
        .child(header)
        .child(proposal)
        .children(actions);
    div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(spacing::S2)
        .px(spacing::S3_5)
        .pb(spacing::S3_5)
        .child(card)
        .children(
            pending
                .then_some(blocked)
                .flatten()
                .map(|why| danger(format!("Can't be approved now: {why}."))),
        )
        .children(refused.map(danger))
        .into_any_element()
}

/// One spending change: marker, category and "£91.20 less".
fn spending_rows(
    window: &Window,
    preview: &Preview,
    palette: &Palette,
    when: Option<&str>,
) -> Vec<AnyElement> {
    if preview.spending.is_empty() {
        return vec![
            FieldRow::new("Spending", value(window, vec![Part::text("No change")]))
                .into_any_element(),
        ];
    }
    preview
        .spending
        .iter()
        .enumerate()
        .map(|(index, change)| {
            let (mark, label) = match &change.category {
                Some(category) => (
                    marker(palette, category),
                    plain(&category.label, NAME_SHOWN),
                ),
                None => (Marker::None, "Uncategorised".to_owned()),
            };
            let minor = change.change.minor();
            let words = if minor > 0 { " more" } else { " less" };
            let mut parts = vec![
                Part::text(format!("{label} ")),
                Part::mono(text::unsigned(change.change)).color(color::TEXT_MUTED),
                muted_parts(words),
            ];
            if let Some(when) = when {
                parts.push(muted_parts(format!(" in {when}")));
            }
            FieldRow::new(
                if index == 0 { "Spending" } else { "" },
                marked(window, mark, parts),
            )
            .into_any_element()
        })
        .collect()
}

/// "If you approve" for each kind (`DESIGN.md` → "Inbox impact preview").
fn if_you_approve(inbox: &Inbox, entry: &Entry, preview: &Preview, window: &Window) -> AnyElement {
    let data = inbox.data();
    let palette = &data.palette;
    let today = data.read_at.unwrap_or_default().date_naive();
    let mut rows: Vec<AnyElement> = Vec::new();
    let mut note: Option<String> = None;
    let mut warning: Option<String> = None;
    let row = |label: &'static str, parts: Vec<Part>| {
        FieldRow::new(label, value(window, parts)).into_any_element()
    };
    match (&entry.suggestion.payload, &preview.impact.detail) {
        (Payload::Rule(_), Detail::Rule(impact)) => {
            rows.push(row(
                "Changes",
                counted(
                    impact.recategorises,
                    "past transaction",
                    "past transactions",
                ),
            ));
            if preview.kept > 0 {
                rows.push(row(
                    "Keeps",
                    counted(preview.kept, "category you set", "categories you set"),
                ));
            }
            rows.extend(spending_rows(window, preview, palette, None));
            if !preview.settles.is_empty() {
                let settled = match preview.settles.as_slice() {
                    [one] => data
                        .entries
                        .iter()
                        .find(|entry| entry.id() == *one)
                        .and_then(|entry| match &entry.subject {
                            Subject::Categorization(items) => items
                                .first()
                                .and_then(|i| i.transaction.as_ref())
                                .map(|t| t.date),
                            _ => None,
                        })
                        .map_or_else(
                            || vec![Part::text("1 suggestion below")],
                            |date| {
                                vec![
                                    Part::text("The "),
                                    Part::mono(text::date(date, today)),
                                    Part::text(" suggestion below"),
                                ]
                            },
                        ),
                    many => counted(many.len(), "suggestion below", "suggestions below"),
                };
                rows.push(row("Settles", settled));
            }
            note = Some("Approving never changes a category you set.".to_owned());
            warning = impact
                .history_blocked
                .clone()
                .map(|why| format!("It can't be applied to past transactions now: {why}."));
        }
        (Payload::MerchantAlias(_), Detail::MerchantAlias(impact)) => {
            rows.push(row(
                "Renames",
                counted(impact.renames, "transaction", "transactions"),
            ));
            if preview.kept > 0 {
                rows.push(row(
                    "Keeps",
                    counted(preview.kept, "name you set", "names you set"),
                ));
            }
            if preview.names_merged > 1 {
                rows.push(row(
                    "Groups",
                    vec![
                        Part::mono(preview.names_merged.to_string()),
                        Part::text(" names into one"),
                    ],
                ));
            }
            rows.push(row("Spending", vec![Part::text("No change")]));
            rows.push(row("Rules", vec![Part::text("No change")]));
            note = Some(
                "Rules match the bank's text, not the name, so none starts or stops matching."
                    .to_owned(),
            );
        }
        (Payload::TransferPair(_), _) => {
            if let Some(effect) = &preview.transfer {
                let less = |amount: Option<coffer_core::money::Money>| match amount {
                    Some(amount) => vec![
                        Part::mono(text::unsigned(amount)),
                        muted_parts(format!(" less in {}", month(effect.month))),
                    ],
                    None => vec![Part::text("No change")],
                };
                rows.push(row("Income", less(effect.income_less)));
                rows.push(row("Spending", less(effect.spending_less)));
                rows.push(row("Balances", vec![Part::text("No change")]));
            }
            note = Some(
                "Both rows become one transfer: money moving between your own accounts is neither \
                 income nor spending. Balances and net worth don't change."
                    .to_owned(),
            );
        }
        (Payload::RefundLink(_), Detail::RefundLink(link)) => {
            if let (
                Some(effect),
                Subject::RefundLink {
                    purchase: Some(purchase),
                    ..
                },
            ) = (&preview.link, &preview.subject)
            {
                let when = month(effect.month);
                rows.push(row(
                    "Links to",
                    vec![
                        Part::text(format!("{} · ", plain(purchase.name(), NAME_SHOWN))),
                        Part::mono(format!(
                            "{} · {}",
                            text::date(purchase.date, today),
                            text::signed(purchase.amount)
                        )),
                    ],
                ));
                rows.push(row(
                    "Refunded",
                    vec![
                        Part::mono(text::unsigned(effect.refunded)),
                        muted_parts(" of "),
                        Part::mono(text::unsigned(purchase.amount)),
                    ],
                ));
                rows.push(row(
                    "Income",
                    match effect.income_less {
                        Some(amount) => vec![
                            Part::mono(text::unsigned(amount)),
                            muted_parts(format!(" less in {when}")),
                        ],
                        None => vec![Part::text("No change")],
                    },
                ));
                rows.push(match &effect.spending_less {
                    Some((category, amount)) => {
                        let (mark, label) = match category {
                            Some(category) => (
                                marker(palette, category),
                                plain(&category.label, NAME_SHOWN),
                            ),
                            None => (Marker::None, "Uncategorised".to_owned()),
                        };
                        FieldRow::new(
                            "Spending",
                            marked(
                                window,
                                mark,
                                vec![
                                    Part::text(format!("{label} ")),
                                    Part::mono(text::unsigned(*amount)).color(color::TEXT_MUTED),
                                    muted_parts(format!(" less in {when}")),
                                ],
                            ),
                        )
                        .into_any_element()
                    }
                    None => row("Spending", vec![Part::text("No change")]),
                });
                let category = purchase
                    .now
                    .category()
                    .map_or("Uncategorised".to_owned(), |c| plain(&c.label, NAME_SHOWN));
                rows.push(row(
                    "Purchase",
                    vec![Part::text(category), muted_parts(" · refunded")],
                ));
            }
            let _ = link;
            note = Some(
                "A linked refund takes its purchase's category and counts in the month the money \
                 came back."
                    .to_owned(),
            );
        }
        (Payload::Categorization(items), Detail::Categorization(impact)) => {
            rows.push(if items.len() == 1 {
                row("Changes", vec![Part::text("This transaction only")])
            } else {
                row(
                    "Changes",
                    counted(impact.changes, "transaction", "transactions"),
                )
            });
            if impact.user_set > 0 {
                rows.push(row(
                    "Overwrites",
                    counted(impact.user_set, "category you set", "categories you set"),
                ));
            }
            rows.extend(spending_rows(window, preview, palette, None));
            if let [item] = items.as_slice()
                && let Some(Now::Category { set_by, .. }) = preview
                    .lines
                    .first()
                    .map(|line| &line.transaction.now)
                    .filter(|_| {
                        matches!(
                            item.target,
                            coffer_core::inbox::payload::Target::Transaction(_)
                        )
                    })
            {
                let was = match set_by {
                    SetBy::User | SetBy::Approved => "you",
                    SetBy::Rule(_) => "a rule",
                    SetBy::Preset => "the bank's category",
                };
                rows.push(row(
                    "Set by",
                    vec![Part::text("You"), muted_parts(format!(" · was {was}"))],
                ));
            }
            note = Some(match inbox.covered() {
                Some(covered) => format!(
                    "The suggested rule for {} would cover this one and {} more.",
                    text::pattern(&covered.definition),
                    covered.others
                ),
                None if items.len() == 1 => "Approving changes only this transaction.".to_owned(),
                None => "Approving changes only these transactions.".to_owned(),
            });
        }
        _ => {}
    }
    section(true)
        .gap(spacing::S2)
        .child(section_title("If you approve"))
        .children(rows)
        .children(note.map(caption))
        .children(warning.map(danger))
        .into_any_element()
}

/// "The rule" or "The merchant name": what it is, in D6.7's terms.
fn definition(inbox: &Inbox, entry: &Entry, window: &Window) -> Option<AnyElement> {
    let palette = &inbox.data().palette;
    let row =
        |label: &'static str, element: AnyElement| FieldRow::new(label, element).into_any_element();
    match &entry.suggestion.payload {
        Payload::Rule(rule) => {
            let then = match text::rule_category(&entry.subject) {
                Some(category) => marked(
                    window,
                    marker(palette, category),
                    vec![Part::text(plain(&category.label, NAME_SHOWN))],
                ),
                None => value(window, vec![Part::text(text::rule_extras(rule).join(", "))]),
            };
            let order = match entry.preview.as_ref().and_then(|p| p.before_rule.as_ref()) {
                Some(first) => vec![Part::text("Before Rule "), Part::mono(text::pattern(first))],
                None => vec![Part::text("First")],
            };
            Some(
                section(true)
                    .gap(spacing::S2)
                    .child(section_title("The rule"))
                    .child(row(
                        "When",
                        value(
                            window,
                            vec![
                                Part::text(format!("{} ", rule_condition(rule))),
                                Part::mono(text::pattern(rule)),
                            ],
                        ),
                    ))
                    .child(row("Then", then))
                    .when(
                        entry.suggestion.status == SuggestionStatus::Pending,
                        |this| this.child(row("Order", value(window, order))),
                    )
                    .into_any_element(),
            )
        }
        Payload::MerchantAlias(alias) => Some(
            section(true)
                .gap(spacing::S2)
                .child(section_title("The merchant name"))
                .child(row(
                    "Match",
                    value(
                        window,
                        vec![
                            Part::text(format!("{} ", alias_condition(alias.kind()))),
                            Part::mono(plain(alias.pattern(), NAME_SHOWN)),
                        ],
                    ),
                ))
                .child(row(
                    "Shows as",
                    value(
                        window,
                        vec![Part::text(plain(alias.name().as_str(), NAME_SHOWN))],
                    ),
                ))
                .into_any_element(),
        ),
        _ => None,
    }
}

/// The header for each kind.
fn entry_header(entry: &Entry, today: NaiveDate) -> AnyElement {
    let preview = entry.preview.as_ref();
    let of = |done: usize, matches: usize| (done.to_string(), Some(format!(" of {matches}")));
    match (&entry.suggestion.payload, &entry.subject) {
        (Payload::Rule(rule), _) => {
            let figure = preview.and_then(|p| match &p.impact.detail {
                Detail::Rule(impact) => Some(of(impact.recategorises, impact.matches)),
                _ => None,
            });
            header(
                vec![
                    (format!("{} ", rule_condition(rule)), false),
                    (text::pattern(rule), true),
                ],
                figure,
                preview.map(|_| caption("Past matches that change category · new rule")),
            )
            .into_any_element()
        }
        (Payload::MerchantAlias(alias), _) => {
            let figure = preview.and_then(|p| match &p.impact.detail {
                Detail::MerchantAlias(impact) => Some(of(impact.renames, p.matches)),
                _ => None,
            });
            header(
                vec![
                    (format!("{} ", alias_condition(alias.kind())), false),
                    (plain(alias.pattern(), NAME_SHOWN), true),
                ],
                figure,
                preview.map(|_| {
                    caption(format!(
                        "Matches that will show as {}",
                        plain(alias.name().as_str(), NAME_SHOWN)
                    ))
                }),
            )
            .into_any_element()
        }
        (Payload::TransferPair(_), Subject::TransferPair { outflow, inflow }) => {
            let name = |side: &Option<TransactionRef>| {
                side.as_ref()
                    .map_or("Gone".to_owned(), |r| plain(&r.account_name, NAME_SHOWN))
            };
            let same = match (outflow, inflow) {
                (Some(out), Some(back)) => {
                    out.date == back.date && out.amount.minor() == -back.amount.minor()
                }
                _ => false,
            };
            header(
                vec![(format!("{} → {}", name(outflow), name(inflow)), false)],
                inflow
                    .as_ref()
                    .map(|row| (text::unsigned(row.amount), None)),
                outflow.as_ref().map(|row| {
                    caption(format!(
                        "Transfer pair · {}{}",
                        text::date(row.date, today),
                        if same {
                            " · same day, same amount"
                        } else {
                            ""
                        }
                    ))
                }),
            )
            .into_any_element()
        }
        (Payload::RefundLink(_), Subject::RefundLink { refund, .. }) => header(
            vec![(
                refund
                    .as_ref()
                    .map_or("Gone".to_owned(), |r| plain(r.name(), NAME_SHOWN)),
                false,
            )],
            refund.as_ref().map(|row| (text::signed(row.amount), None)),
            refund.as_ref().map(|row| {
                caption(format!(
                    "{} · money in · {}",
                    plain(&row.account_name, NAME_SHOWN),
                    text::date(row.date, today)
                ))
            }),
        )
        .into_any_element(),
        (_, Subject::Categorization(items)) => match items.as_slice() {
            [item] => match &item.transaction {
                Some(row) => {
                    let how = row
                        .source_type
                        .as_deref()
                        .map(|kind| plain(kind, NAME_SHOWN))
                        .unwrap_or_else(|| plain(&row.account_name, NAME_SHOWN));
                    let when = row
                        .time
                        .as_deref()
                        .and_then(|time| time.get(..5))
                        .map_or_else(|| text::date(row.date, today), str::to_owned);
                    header(
                        vec![(plain(row.name(), NAME_SHOWN), false)],
                        Some((text::signed(row.amount), None)),
                        Some(caption(format!("{how} · {when}"))),
                    )
                    .into_any_element()
                }
                None => header(
                    vec![("A transaction gone since".to_owned(), false)],
                    None,
                    None,
                )
                .into_any_element(),
            },
            many => header(
                vec![(count(many.len(), "transaction", "transactions"), false)],
                None,
                Some(caption("One categorisation")),
            )
            .into_any_element(),
        },
        _ => header(vec![("Suggestion".to_owned(), false)], None, None).into_any_element(),
    }
}

/// The footer: how many rows the suggestion touches.
fn footer(entry: &Entry) -> AnyElement {
    let words = match (&entry.suggestion.payload, &entry.preview) {
        (Payload::Rule(_) | Payload::MerchantAlias(_), Some(preview)) => {
            count(preview.matches, "match", "matches")
        }
        (Payload::Rule(_) | Payload::MerchantAlias(_), None) => return div().into_any_element(),
        (Payload::Categorization(items), _) if items.len() == 1 => "One transaction".to_owned(),
        (Payload::Categorization(items), _) => count(items.len(), "transaction", "transactions"),
        _ => "2 transactions".to_owned(),
    };
    section(true)
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_role(typography::BODY)
                .text_color(color::TEXT_MUTED)
                .child(words),
        )
        .into_any_element()
}

/// A decided suggestion: when, and the user's reason for a rejection.
fn decision(entry: &Entry, window: &Window, today: NaiveDate) -> AnyElement {
    let label = match entry.suggestion.status {
        SuggestionStatus::Approved => "Approved",
        _ => "Rejected",
    };
    let when = entry
        .suggestion
        .decided_at
        .as_deref()
        .and_then(text::timestamp)
        .map(|at| text::date(at.date_naive(), today))
        .unwrap_or_default();
    section(true)
        .gap(spacing::S2)
        .child(section_title("Decision"))
        .child(FieldRow::new(label, value(window, vec![Part::mono(when)])))
        .children(entry.suggestion.decision_reason.as_deref().map(|reason| {
            FieldRow::new(
                "Reason",
                value(window, vec![Part::text(plain(reason, RATIONALE_SHOWN))]),
            )
        }))
        .into_any_element()
}

/// A click handler: the type a button's `on_click` takes.
fn click(
    handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    handler
}

/// The inspector for the selected suggestion.
pub fn render(view: &InboxView, window: &mut Window, cx: &App) -> impl IntoElement {
    let inbox = view.inbox().read(cx);
    let today = inbox.data().read_at.unwrap_or_default().date_naive();
    let mut sections: Vec<AnyElement> = Vec::new();
    match inbox.selected_entry() {
        Some(entry) => {
            sections.push(entry_header(entry, today));
            sections.push(card(view, inbox, entry, window));
            if let Some(preview) = &entry.preview {
                sections.push(if_you_approve(inbox, entry, preview, window));
            }
            if entry.suggestion.status != SuggestionStatus::Pending {
                sections.push(decision(entry, window, today));
            }
            sections.extend(definition(inbox, entry, window));
            sections.push(footer(entry));
        }
        None => sections.push(
            section(false)
                .child(caption(
                    "Select a suggestion to see what approving it would change.",
                ))
                .into_any_element(),
        ),
    }
    div()
        .id("inbox-inspector")
        .flex()
        .flex_col()
        .flex_none()
        .w(size::INSPECTOR)
        .overflow_y_scroll()
        .bg(color::CHROME)
        .border_l(size::HAIRLINE)
        .border_color(color::HAIRLINE)
        .children(sections)
}
