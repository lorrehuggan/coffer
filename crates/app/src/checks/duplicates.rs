//! Possible duplicates (`docs/DESIGN.md` → "Review queues"; Paper `8SE-0`,
//! `9MQ-0`).
//!
//! - **The table**: PAIR (the two names joined by "and") · ACCOUNT (140) ·
//!   WHY FLAGGED (180) · DATES (72, mono) · AMOUNT (96, mono); 28px rows,
//!   the selected one `molten-soft`.
//! - **The selected pair opens in place** side by side: the label lane
//!   (96), then the kept row and the excluded row 24px apart, each headed
//!   by what happens to it; values both rows share in `text-muted`, so the
//!   differences stand out. Merge (neutral primary), Not duplicates, and
//!   the caption; a duplicate in a locked period puts the locked-period
//!   block above them and fades Merge until the account is unlocked.
//! - **The inspector**: why the pair was flagged (met criteria with a
//!   muted ✓, what stopped the exact dedupe with a `warning` ≠), and "If
//!   you merge" ([`coffer_core::checks::MergeEffect`]): the row excluded,
//!   the values moved (user, 2026-11-14), spending, Coffer's count and the
//!   balance checks it changes.
//!
//! Names, bank text and bank IDs are untrusted text, shown plain.

use chrono::NaiveDate;
use coffer_core::checks::{MergeEffect, Pair, PairRow, RowCategory};
use coffer_core::duplicate::DuplicateWindow;
use coffer_core::reporting::palette::Palette;
use gpui_kit::component::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Entity, FocusHandle, InteractiveElement as _, IntoElement, ParentElement as _,
    Rgba, SharedString, StatefulInteractiveElement as _, Styled as _, TestSupportExt as _, Window,
    div,
};

use crate::format;
use crate::inbox::text::marker;
use crate::theme::tokens::{color, radius, size, spacing, typography, weight};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::field::FieldRow;
use crate::ui::icon::Glyph;
use crate::ui::inspector::{header, section, section_title};
use crate::ui::marker::Marker;
use crate::ui::text::TextRoleExt as _;
use crate::ui::tracked::Tracked;

use super::Checks;
use super::text::{self, BANK_TEXT_SHOWN, NAME_SHOWN, Part, plain, rich};
use super::view::{click, empty_queue};

/// The DATES lane: 72px (`spacing-6` × 3).
pub const DATES_LANE: gpui_kit::Pixels = gpui_kit::px(72.);

/// A fixed-width lane of a row.
fn lane(width: gpui_kit::Pixels) -> gpui_kit::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .w(width)
        .overflow_hidden()
        .whitespace_nowrap()
}

/// A caps lane label: 11/14 600 `text-muted`.
fn lane_label(label: &'static str) -> impl IntoElement {
    div()
        .flex()
        .text_color(color::TEXT_MUTED)
        .child(Tracked::new(typography::SECTION_LABEL, label))
}

fn table_header() -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S3)
        .h(size::CONTROL)
        .px(spacing::S2)
        .child(div().flex().flex_1().min_w_0().child(lane_label("Pair")))
        .child(lane(size::COLUMN_ACCOUNT).child(lane_label("Account")))
        .child(lane(size::COLUMN_CATEGORY).child(lane_label("Why flagged")))
        .child(lane(DATES_LANE).justify_end().child(lane_label("Dates")))
        .child(
            lane(size::COLUMN_AMOUNT)
                .justify_end()
                .child(lane_label("Amount")),
        )
}

/// "4–5 Nov", "30 Oct – 2 Nov", "6 Nov".
pub fn dates(a: NaiveDate, b: NaiveDate, today: NaiveDate) -> String {
    let (first, last) = if a <= b { (a, b) } else { (b, a) };
    if first == last {
        text::date(first, today)
    } else if first.format("%b %Y").to_string() == last.format("%b %Y").to_string() {
        format!("{}–{}", first.format("%-d"), text::date(last, today))
    } else {
        format!("{} – {}", text::date(first, today), text::date(last, today))
    }
}

fn pair_row(
    pair: &Pair,
    selected: bool,
    handle: &Entity<Checks>,
    focus: &FocusHandle,
    today: NaiveDate,
    window: &Window,
) -> AnyElement {
    let id = pair.candidate;
    let (name, joiner, lanes, dates_tone) = if selected {
        (
            color::WHITE,
            color::TEXT_SECONDARY,
            color::TEXT_STRONG,
            color::TEXT_SECONDARY,
        )
    } else {
        (
            color::TEXT,
            color::TEXT_MUTED,
            color::TEXT_SECONDARY,
            color::TEXT_MUTED,
        )
    };
    let names = rich(
        window,
        typography::BODY,
        name,
        vec![
            Part::text(text::name(&pair.rows[0])),
            Part::text(" and ").color(joiner),
            Part::text(text::name(&pair.rows[1])),
        ],
    );
    let (select, focus) = (handle.clone(), focus.clone());
    div()
        .id(SharedString::from(format!("checks-pair-{id}")))
        .test_support()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S3)
        .h(size::CONTROL)
        .px(spacing::S2)
        .rounded(radius::MD)
        .map(|this| {
            if selected {
                this.bg(color::MOLTEN_SOFT)
            } else {
                this.hover(|style| style.bg(color::HOVER))
            }
        })
        .on_click(click(move |_, window, cx| {
            focus.focus(window, cx);
            select.update(cx, |checks, cx| checks.toggle_pair(id, cx));
        }))
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_role(typography::BODY)
                .child(names),
        )
        .child(
            lane(size::COLUMN_ACCOUNT)
                .text_role(typography::BODY)
                .text_color(lanes)
                .child(plain(&pair.account_name, NAME_SHOWN)),
        )
        .child(
            lane(size::COLUMN_CATEGORY)
                .text_role(typography::BODY)
                .text_color(lanes)
                .child(text::why(&pair.why)),
        )
        .child(
            lane(DATES_LANE)
                .justify_end()
                .text_role(typography::MONO_VALUE)
                .text_color(dates_tone)
                .child(dates(pair.rows[0].date, pair.rows[1].date, today)),
        )
        .child(
            lane(size::COLUMN_AMOUNT)
                .justify_end()
                .text_role(typography::MONO_AMOUNT)
                .text_color(name)
                .child(text::signed(pair.rows[0].amount)),
        )
        .into_any_element()
}

/// One side's value in the comparison.
struct Cell {
    /// What's compared between the two rows.
    key: String,
    element: AnyElement,
}

fn value_cell(value: Option<String>, mono: bool, differs: bool, missing: &str) -> AnyElement {
    match value {
        Some(value) => div()
            .text_role(if mono {
                typography::MONO_AMOUNT
            } else {
                typography::BODY
            })
            .text_color(if differs {
                color::TEXT
            } else {
                color::TEXT_MUTED
            })
            .child(value)
            .into_any_element(),
        None => div()
            .text_role(typography::BODY)
            .text_color(color::TEXT_MUTED)
            .child(missing.to_owned())
            .into_any_element(),
    }
}

/// The comparison's rows for one field: the label, then the two values.
fn field_row(label: &'static str, cells: [AnyElement; 2]) -> impl IntoElement {
    let [kept, excluded] = cells;
    let column = |cell: AnyElement| {
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(spacing::S2)
            .pl(spacing::S5)
            .overflow_hidden()
            .whitespace_nowrap()
            .child(cell)
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S6)
        .h(size::CONTROL_SM)
        .child(
            div()
                .flex_none()
                .w(size::COLUMN_AMOUNT)
                .text_role(typography::BODY)
                .text_color(color::TEXT_MUTED)
                .child(label),
        )
        .child(column(kept))
        .child(column(excluded))
}

/// A text field compared: both values, `text` where they differ.
fn compared(
    label: &'static str,
    rows: &[PairRow; 2],
    value: impl Fn(&PairRow) -> Option<String>,
    mono: bool,
    missing: impl Fn(&PairRow) -> &'static str,
) -> impl IntoElement {
    let values = [value(&rows[0]), value(&rows[1])];
    let differs = values[0] != values[1];
    let cells = [
        value_cell(values[0].clone(), mono, differs, missing(&rows[0])),
        value_cell(values[1].clone(), mono, differs, missing(&rows[1])),
    ];
    field_row(label, cells)
}

fn category_cell(row: &PairRow, palette: &Palette, differs: bool) -> Cell {
    let tone = if differs {
        color::TEXT
    } else {
        color::TEXT_MUTED
    };
    let words = |words: String, tone: Rgba| {
        div()
            .text_role(typography::BODY)
            .text_color(tone)
            .child(words)
            .into_any_element()
    };
    match &row.category {
        RowCategory::Category(category) => Cell {
            key: category.key.as_str().to_owned(),
            element: div()
                .flex()
                .items_center()
                .gap(spacing::S2)
                .child(marker(palette, category).render())
                .child(words(plain(&category.label, NAME_SHOWN), tone))
                .into_any_element(),
        },
        RowCategory::Uncategorised => Cell {
            key: String::new(),
            element: div()
                .flex()
                .items_center()
                .gap(spacing::S2)
                .child(Marker::None.render())
                .child(words("Uncategorised".to_owned(), color::TEXT_MUTED))
                .into_any_element(),
        },
        RowCategory::Split => Cell {
            key: "split".to_owned(),
            element: words("Split".to_owned(), tone),
        },
    }
}

/// The heading over one side of the comparison.
fn side_heading(
    glyph: Glyph,
    glyph_tone: Rgba,
    title: &'static str,
    title_tone: Rgba,
    caption: String,
) -> gpui_kit::Div {
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .items_start()
        .gap(spacing::S2)
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .h(size::ICON_LG)
                .child(glyph.render().text_color(glyph_tone)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(spacing::S0_5)
                .child(
                    div()
                        .text_role(typography::BODY)
                        .font_weight(weight::STRONG)
                        .text_color(title_tone)
                        .child(title),
                )
                .child(
                    div()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_role(typography::CAPTION)
                        .text_color(color::TEXT_MUTED)
                        .child(caption),
                ),
        )
}

/// The value columns' left edge, for the footer: the label lane, the gap
/// and a heading's icon slot.
fn footer_indent() -> gpui_kit::Pixels {
    size::COLUMN_AMOUNT + spacing::S6 + spacing::S5
}

/// The locked-period block (Paper `9VU-0`).
fn locked_block(
    pair: &Pair,
    effect: &MergeEffect,
    handle: &Entity<Checks>,
    today: NaiveDate,
    window: &Window,
) -> Option<AnyElement> {
    let locked = effect.locked?;
    let account = pair.account;
    let unlock = handle.clone();
    Some(
        div()
            .flex()
            .items_start()
            .gap(spacing::S2_5)
            .p(spacing::S3)
            .rounded(radius::SURFACE)
            .bg(color::WARNING_SOFT)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(size::ICON_LG)
                    .child(Glyph::LockedPeriod.render().text_color(color::WARNING)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(spacing::S0_5)
                    .child(
                        div()
                            .text_role(typography::BODY)
                            .font_weight(weight::STRONG)
                            .text_color(color::WARNING)
                            .child("The excluded row is in a locked period"),
                    )
                    .child(div().text_role(typography::CAPTION).child(rich(
                        window,
                        typography::CAPTION,
                        color::TEXT_SECONDARY,
                        vec![
                            Part::text(format!(
                                "{} is locked through ",
                                plain(&pair.account_name, NAME_SHOWN)
                            )),
                            Part::mono(text::date(locked.through, today)),
                            Part::text(", and this merge would exclude a row dated "),
                            Part::mono(text::date(locked.date, today)),
                            Part::text(
                                ". Unlock to merge, or keep the other row instead if it’s \
                                     outside the lock.",
                            ),
                        ],
                    ))),
            )
            .child(
                Button::new("checks-pair-unlock", "Unlock…")
                    .with_variant(ButtonVariant::Outline)
                    .on_click(click(move |_, _, cx| {
                        unlock.update(cx, |checks, cx| checks.request_unlock(account, cx))
                    })),
            )
            .into_any_element(),
    )
}

/// The selected pair, open under its row.
fn comparison(
    pair: &Pair,
    handle: &Entity<Checks>,
    palette: &Palette,
    today: NaiveDate,
    window: &Window,
) -> AnyElement {
    let rows = &pair.rows;
    let keep = handle.clone();
    let headings = div()
        .flex()
        .items_start()
        .gap(spacing::S6)
        .pb(spacing::S1)
        .child(div().flex_none().w(size::COLUMN_AMOUNT))
        .child(side_heading(
            Glyph::Check,
            color::TEXT,
            "Keep",
            color::TEXT,
            text::added(&rows[0], today),
        ))
        .child(
            side_heading(
                Glyph::Excluded,
                color::TEXT_MUTED,
                "Exclude as duplicate",
                color::TEXT_SECONDARY,
                text::added(&rows[1], today),
            )
            .child(
                Button::new("checks-keep-other", "Keep this instead")
                    .with_variant(ButtonVariant::Outline)
                    .on_click(click(move |_, _, cx| {
                        keep.update(cx, |checks, cx| checks.keep_other(cx))
                    })),
            ),
        );

    let categories = [
        category_cell(&rows[0], palette, true),
        category_cell(&rows[1], palette, true),
    ];
    let categories_differ = categories[0].key != categories[1].key;
    let categories = [
        category_cell(&rows[0], palette, categories_differ).element,
        category_cell(&rows[1], palette, categories_differ).element,
    ];

    let effect = pair.effect.as_ref();
    let locked = effect.ok().and_then(|effect| effect.locked).is_some();
    let can_merge = effect.is_ok() && !locked;
    let (merge, dismiss) = (handle.clone(), handle.clone());
    let caption: AnyElement = match effect {
        Err(message) => div()
            .text_role(typography::CAPTION)
            .text_color(color::DANGER)
            .child(format!("Can’t merge: {message}"))
            .into_any_element(),
        Ok(_) if locked => div()
            .text_role(typography::CAPTION)
            .text_color(color::TEXT_MUTED)
            .child("Dismissing changes no transaction, so it isn’t locked.")
            .into_any_element(),
        Ok(_) => div()
            .text_role(typography::CAPTION)
            .text_color(color::TEXT_MUTED)
            .child("The excluded row stays under the Excluded filter. Undo any time.")
            .into_any_element(),
    };
    let footer = div()
        .flex()
        .items_center()
        .gap(spacing::S1_5)
        .pl(footer_indent())
        .pt(spacing::S3)
        .child(
            Button::new("checks-merge", "Merge")
                .with_variant(ButtonVariant::Primary)
                .disabled(!can_merge)
                .on_click(click(move |_, _, cx| {
                    merge.update(cx, |checks, cx| checks.merge(cx))
                })),
        )
        .child(
            Button::new("checks-dismiss", "Not duplicates")
                .with_variant(ButtonVariant::Secondary)
                .on_click(click(move |_, _, cx| {
                    dismiss.update(cx, |checks, cx| checks.dismiss(cx))
                })),
        )
        .child(div().flex_1().min_w_0().pl(spacing::S2).child(caption));

    div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(spacing::S1)
        .pt(spacing::S3)
        .pb(spacing::S4)
        .px(spacing::S2)
        .border_b(size::HAIRLINE)
        .border_color(color::HAIRLINE)
        .child(headings)
        .child(compared(
            "Date",
            rows,
            |row| Some(format::field_date(row.date)),
            true,
            |_| "None",
        ))
        .child(compared(
            "Merchant",
            rows,
            |row| row.merchant.as_deref().map(|name| plain(name, NAME_SHOWN)),
            false,
            |_| "None",
        ))
        .child(compared(
            "Amount",
            rows,
            |row| Some(text::signed(row.amount)),
            true,
            |_| "None",
        ))
        .child(field_row("Category", categories))
        .child(compared(
            "Bank text",
            rows,
            |row| (!row.is_manual()).then(|| plain(&row.bank_text, BANK_TEXT_SHOWN)),
            true,
            |row| {
                if row.is_manual() {
                    "None, added by hand"
                } else {
                    "None"
                }
            },
        ))
        .child(compared(
            "Bank ID",
            rows,
            |row| row.bank_id.as_deref().map(|id| plain(id, BANK_TEXT_SHOWN)),
            true,
            |_| "None",
        ))
        .children(effect.ok().and_then(|effect| {
            locked_block(pair, effect, handle, today, window).map(|block| {
                div()
                    .pl(footer_indent())
                    .pt(spacing::S3)
                    .child(block)
                    .into_any_element()
            })
        }))
        .child(footer)
        .into_any_element()
}

/// The canvas under the toolbar.
pub fn render_body(
    handle: &Entity<Checks>,
    checks: &Checks,
    focus: &FocusHandle,
    window: &Window,
) -> AnyElement {
    let today = checks.today();
    if let Some(error) = checks.load_error() {
        return empty_queue(
            Glyph::DuplicateLarge,
            "Possible duplicates couldn’t be read",
            error.clone(),
        )
        .into_any_element();
    }
    if checks.pairs().is_empty() {
        return empty_queue(
            Glyph::DuplicateLarge,
            "No possible duplicates",
            "Coffer checks every import. A pair you marked “Not duplicates” isn’t flagged again."
                .into(),
        )
        .into_any_element();
    }
    let selected = checks.selected_pair().map(|pair| pair.candidate);
    let palette = &checks.data().palette;
    let mut rows: Vec<AnyElement> = Vec::new();
    for listed in checks.pairs() {
        let open = selected == Some(listed.candidate);
        rows.push(pair_row(listed, open, handle, focus, today, window));
        if open && let Some(pair) = checks.selected_pair() {
            rows.push(comparison(pair, handle, palette, today, window));
        }
    }
    div()
        .id("checks-duplicates-body")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .child(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .pt(spacing::S2)
                .px(spacing::S2)
                .pb(spacing::S4)
                .child(table_header())
                .children(rows)
                .child(
                    div()
                        .pt(spacing::S4)
                        .px(spacing::S2)
                        .text_role(typography::CAPTION)
                        .text_color(color::TEXT_MUTED)
                        .child(format!(
                            "Exact duplicates never get this far: a row the bank already gave \
                             Coffer is skipped at import. A pair is flagged when one account \
                             has the same amount twice within {} days under different text.",
                            DuplicateWindow::DEFAULT.days()
                        )),
                ),
        )
        .into_any_element()
}

// The inspector ----------------------------------------------------------------

fn caption(words: impl Into<SharedString>) -> AnyElement {
    div()
        .text_role(typography::CAPTION)
        .text_color(color::TEXT_MUTED)
        .child(words.into())
        .into_any_element()
}

/// A field row's value: 8px inline padding, body type.
fn value(window: &Window, parts: Vec<Part>) -> AnyElement {
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
        .child(rich(window, typography::BODY, color::TEXT, parts))
        .into_any_element()
}

/// A criterion: a 12px icon, the criterion, its value on the right.
fn criterion(glyph: Glyph, tone: Rgba, label: &str, detail: Option<AnyElement>) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S2)
        .h(size::CONTROL_SM)
        .child(glyph.render().text_color(tone))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_role(typography::BODY)
                .text_color(color::TEXT_STRONG)
                .child(label.to_owned()),
        )
        .children(detail)
        .into_any_element()
}

fn detail(words: String, mono: bool) -> AnyElement {
    div()
        .whitespace_nowrap()
        .text_role(if mono {
            typography::MONO_VALUE
        } else {
            typography::CAPTION
        })
        .text_color(color::TEXT_MUTED)
        .child(words)
        .into_any_element()
}

fn why_section(pair: &Pair) -> AnyElement {
    let why = &pair.why;
    let mut rows = vec![
        criterion(
            Glyph::Check,
            color::TEXT_MUTED,
            "Same account",
            Some(detail(plain(&pair.account_name, NAME_SHOWN), false)),
        ),
        criterion(
            Glyph::Check,
            color::TEXT_MUTED,
            "Same amount",
            Some(detail(text::signed(pair.rows[0].amount), true)),
        ),
        criterion(
            Glyph::Check,
            color::TEXT_MUTED,
            &format!("Within {} days", why.window.days()),
            Some(detail(text::gap(why.days_apart).to_lowercase(), false)),
        ),
    ];
    for difference in text::differences(why) {
        rows.push(criterion(Glyph::Differs, color::WARNING, difference, None));
    }
    section(true)
        .gap(spacing::S1)
        .child(
            div()
                .pb(spacing::S1)
                .child(section_title("Why it was flagged")),
        )
        .children(rows)
        .child(div().pt(spacing::S1).child(caption(text::why_caption(why))))
        .into_any_element()
}

fn if_you_merge(
    pair: &Pair,
    effect: &MergeEffect,
    palette: &Palette,
    today: NaiveDate,
    window: &Window,
) -> AnyElement {
    let excluded = &pair.rows[1];
    let mut rows: Vec<AnyElement> = vec![
        FieldRow::new(
            "Excludes",
            value(
                window,
                vec![
                    Part::text(text::name(excluded)),
                    Part::text(" · ").color(color::TEXT_MUTED),
                    Part::mono(text::date(excluded.date, today)),
                ],
            ),
        )
        .into_any_element(),
    ];
    if !effect.carried.is_empty() {
        rows.push(
            FieldRow::new(
                "Moves",
                value(
                    window,
                    vec![
                        Part::text(text::carried(&effect.carried)),
                        Part::text(format!(" to {}", text::name(&pair.rows[0])))
                            .color(color::TEXT_MUTED),
                    ],
                ),
            )
            .into_any_element(),
        );
    }
    if effect.spending.is_empty() {
        rows.push(
            FieldRow::new("Spending", value(window, vec![Part::text("No change")]))
                .into_any_element(),
        );
    }
    for (index, change) in effect.spending.iter().enumerate() {
        let (mark, label) = match &change.category {
            Some(category) => (
                marker(palette, category),
                plain(&category.label, NAME_SHOWN),
            ),
            None => (Marker::None, "Uncategorised".to_owned()),
        };
        let words = if change.change.minor() > 0 {
            " more"
        } else {
            " less"
        };
        rows.push(
            FieldRow::new(
                if index == 0 { "Spending" } else { "" },
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(spacing::S2)
                    .px(spacing::S2)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(mark.render())
                    .child(rich(
                        window,
                        typography::BODY,
                        color::TEXT,
                        vec![
                            Part::text(format!("{label} ")),
                            Part::mono(text::unsigned(change.change)).color(color::TEXT_MUTED),
                            Part::text(words).color(color::TEXT_MUTED),
                        ],
                    )),
            )
            .into_any_element(),
        );
    }
    if let Some(count) = effect.count {
        rows.push(
            FieldRow::new(
                "Balance",
                value(
                    window,
                    vec![
                        Part::mono(text::balance(count.before)),
                        Part::text(" → ").color(color::TEXT_MUTED),
                        Part::mono(text::balance(count.after)),
                    ],
                ),
            )
            .into_any_element(),
        );
    }
    for (index, change) in effect.checks.iter().enumerate() {
        let (glyph, tone, verb) = if change.after.is_passed() {
            (Glyph::Check, color::TEXT, "passes")
        } else {
            (Glyph::Alert, color::DANGER, "fails")
        };
        rows.push(
            FieldRow::new(
                if index == 0 { "Check" } else { "" },
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(spacing::S1_5)
                    .px(spacing::S2)
                    .child(glyph.render().text_color(tone))
                    .child(rich(
                        window,
                        typography::BODY,
                        color::TEXT,
                        vec![
                            Part::mono(text::date(change.date, today)),
                            Part::text(format!(" {verb}")),
                        ],
                    )),
            )
            .into_any_element(),
        );
    }

    let mut captions: Vec<AnyElement> = Vec::new();
    for change in &effect.checks {
        let computed = match change.before {
            coffer_core::reconciliation::Outcome::Failed { computed_minor, .. } => {
                Some(computed_minor)
            }
            _ => None,
        };
        if change.fixes()
            && let Some(computed) = computed
        {
            let difference = i128::from(change.stated.minor()) - computed;
            let gap = i64::try_from(difference.unsigned_abs())
                .map(|minor| coffer_core::money::Money::new(minor, change.stated.currency()));
            if let Ok(gap) = gap {
                captions.push(
                    div()
                        .text_role(typography::CAPTION)
                        .child(rich(
                            window,
                            typography::CAPTION,
                            color::TEXT_MUTED,
                            vec![
                                Part::text(format!(
                                    "{} for {} on ",
                                    text::stated_by(change.source),
                                    plain(&pair.account_name, NAME_SHOWN)
                                )),
                                Part::mono(text::date(change.date, today)),
                                Part::text(" is "),
                                Part::mono(text::unsigned(gap)),
                                Part::text(if difference > 0 {
                                    " above Coffer’s. This pair explains all of it."
                                } else {
                                    " below Coffer’s. This pair explains all of it."
                                }),
                            ],
                        ))
                        .into_any_element(),
                );
            }
        } else if change.breaks() {
            captions.push(
                div()
                    .text_role(typography::CAPTION)
                    .child(rich(
                        window,
                        typography::CAPTION,
                        color::TEXT_MUTED,
                        vec![
                            Part::text("Merging makes the balance check on "),
                            Part::mono(text::date(change.date, today)),
                            Part::text(" fail: that balance counts both rows."),
                        ],
                    ))
                    .into_any_element(),
            );
        }
    }
    if effect.locked.is_some() {
        captions.push(caption(
            "The excluded row is in a locked period: this is what merging does once you unlock.",
        ));
    }
    section(true)
        .gap(spacing::S2)
        .child(section_title("If you merge"))
        .children(rows)
        .children(captions)
        .into_any_element()
}

/// The inspector.
pub fn render_inspector(checks: &Checks, window: &Window) -> AnyElement {
    let today = checks.today();
    let mut sections: Vec<AnyElement> = Vec::new();
    match checks.selected_pair() {
        Some(pair) => {
            sections.push(
                header(
                    text::name(&pair.rows[0]),
                    Some(text::signed(pair.rows[0].amount)),
                    Some(caption(format!(
                        "{} · possible duplicate",
                        plain(&pair.account_name, NAME_SHOWN)
                    ))),
                )
                .into_any_element(),
            );
            sections.push(why_section(pair));
            match &pair.effect {
                Ok(effect) => sections.push(if_you_merge(
                    pair,
                    effect,
                    &checks.data().palette,
                    today,
                    window,
                )),
                Err(message) => sections.push(
                    section(true)
                        .gap(spacing::S2)
                        .child(section_title("If you merge"))
                        .child(
                            div()
                                .text_role(typography::CAPTION)
                                .text_color(color::DANGER)
                                .child(format!("Can’t merge: {message}")),
                        )
                        .into_any_element(),
                ),
            }
        }
        None => sections.push(
            section(false)
                .child(caption(
                    "Select a pair to compare its two rows and see what merging would do.",
                ))
                .into_any_element(),
        ),
    }
    div()
        .id("checks-inspector")
        .flex()
        .flex_col()
        .flex_none()
        .w(size::INSPECTOR)
        .overflow_y_scroll()
        .bg(color::CHROME)
        .border_l(size::HAIRLINE)
        .border_color(color::HAIRLINE)
        .children(sections)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(month: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, month, d).unwrap()
    }

    #[test]
    fn dates_read_as_a_range() {
        let today = day(11, 14);
        assert_eq!(dates(day(11, 5), day(11, 4), today), "4–5 Nov");
        assert_eq!(dates(day(11, 6), day(11, 6), today), "6 Nov");
        assert_eq!(dates(day(10, 30), day(11, 2), today), "30 Oct – 2 Nov");
    }
}
