//! The Inbox's left panel (`docs/DESIGN.md` → "Inbox"; Paper `564-0`,
//! `ABK-0`): INBOX (Pending, Approved, Rejected, with counts), KIND (the
//! pending suggestions by kind, widest reach first) and SOURCE (each agent,
//! the connected dot on the ones that have filed, and "Coffer from your
//! edits"). KIND and SOURCE filter the list; a second click clears the
//! filter. CHECKS (D6.4, M3.12; Paper `8SE-0`) sits between INBOX and
//! KIND: Possible duplicates and Balance checks, Coffer's own checks, so
//! neutral, with a `danger` dot before the count while a check fails.
//! Selecting one shows its queue ([`crate::checks`]) in the canvas; any
//! other row goes back to the suggestions.

use coffer_core::inbox::{COFFER_SOURCE, SuggestionStatus};
use gpui_kit::{
    App, Entity, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div,
};

use crate::theme::tokens::{color, size, spacing, typography};
use crate::ui::icon::Glyph;
use crate::ui::list_row::{ListRow, TrailingTone};
use crate::ui::section_label::SectionLabelRow;
use crate::ui::text::TextRoleExt as _;

use crate::checks::{Checks, Queue};

use super::text::{kind_glyph, kind_name, source_name};
use super::{Inbox, KIND_ORDER};

/// A section: 8px (the first) or 4px above, 6px inline, 10px below.
fn section(first: bool, label: &str) -> gpui_kit::Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .pt(if first { spacing::S2 } else { spacing::S1 })
        .px(spacing::S1_5)
        .pb(spacing::S2_5)
        .child(SectionLabelRow::new(SharedString::from(label.to_owned())))
}

fn status_label(status: SuggestionStatus) -> &'static str {
    match status {
        SuggestionStatus::Pending => "Pending",
        SuggestionStatus::Approved => "Approved",
        SuggestionStatus::Rejected => "Rejected",
    }
}

fn status_glyph(status: SuggestionStatus) -> Glyph {
    match status {
        SuggestionStatus::Pending => Glyph::Inbox,
        SuggestionStatus::Approved => Glyph::Check,
        SuggestionStatus::Rejected => Glyph::Reject,
    }
}

/// The panel.
pub fn render(inbox: &Entity<Inbox>, checks: &Entity<Checks>, cx: &App) -> impl IntoElement {
    let state = inbox.read(cx);
    let counts = &state.data().counts;
    let queue = checks.read(cx).queue();
    // A suggestion row closes an open queue.
    let back = {
        let checks = checks.clone();
        move |cx: &mut App| checks.update(cx, |checks, cx| checks.hide(cx))
    };

    let statuses = [
        SuggestionStatus::Pending,
        SuggestionStatus::Approved,
        SuggestionStatus::Rejected,
    ]
    .into_iter()
    .map(|status| {
        let (handle, back) = (inbox.clone(), back.clone());
        ListRow::new(
            SharedString::from(format!("inbox-status-{status}")),
            status_label(status),
        )
        .icon(status_glyph(status))
        .trailing(counts.with_status(status).to_string())
        .trailing_tone(TrailingTone::Faint)
        .selected(queue.is_none() && state.status() == status)
        .on_click(move |_, _, cx| {
            back(cx);
            handle.update(cx, |inbox, cx| inbox.show_status(status, cx))
        })
    });

    let check_rows = {
        let state = checks.read(cx);
        let row = |queue_row: Queue, label: &'static str, glyph: Glyph, count: usize| {
            let handle = checks.clone();
            let id = match queue_row {
                Queue::Duplicates => "inbox-checks-duplicates",
                Queue::Balances => "inbox-checks-balances",
            };
            ListRow::new(id, label)
                .icon(glyph)
                .trailing(count.to_string())
                .trailing_tone(TrailingTone::Faint)
                .selected(queue == Some(queue_row))
                .on_click(move |_, _, cx| {
                    handle.update(cx, |checks, cx| checks.show(queue_row, cx))
                })
        };
        [
            row(
                Queue::Duplicates,
                "Possible duplicates",
                Glyph::Duplicate,
                state.pending_pairs(),
            ),
            row(
                Queue::Balances,
                "Balance checks",
                Glyph::Balance,
                state.failing(),
            )
            .flagged(state.failing() > 0),
        ]
    };

    let kinds = KIND_ORDER.into_iter().map(|kind| {
        let (handle, back) = (inbox.clone(), back.clone());
        ListRow::new(
            SharedString::from(format!("inbox-kind-{kind}")),
            kind_name(kind),
        )
        .icon(kind_glyph(kind))
        .trailing(
            counts
                .pending_by_kind
                .get(&kind)
                .copied()
                .unwrap_or(0)
                .to_string(),
        )
        .trailing_tone(TrailingTone::Faint)
        .selected(state.kind_filter() == Some(kind))
        .on_click(move |_, _, cx| {
            back(cx);
            handle.update(cx, |inbox, cx| inbox.toggle_kind(kind, cx))
        })
    });

    let sources = state.data().sources.iter().map(|source| {
        let pending = counts.by_source.get(source).copied().unwrap_or(0);
        let (name, suffix) = source_name(source);
        let (handle, back) = (inbox.clone(), back.clone());
        let filter = source.clone();
        let row = ListRow::new(SharedString::from(format!("inbox-source-{source}")), name)
            .trailing(pending.to_string())
            .trailing_tone(TrailingTone::Faint)
            .selected(state.source_filter() == Some(source.as_str()))
            .on_click(move |_, _, cx| {
                back(cx);
                handle.update(cx, |inbox, cx| inbox.toggle_source(&filter, cx))
            });
        // The moldavite dot marks an agent with work waiting; Coffer's own
        // suggestions are neutral.
        let row = if pending > 0 && source != COFFER_SOURCE {
            row.dot(color::MOLDAVITE)
        } else {
            row.blank_slot()
        };
        match suffix {
            Some(suffix) => row.label_suffix(suffix),
            None => row,
        }
    });

    div()
        .id("inbox-panel")
        .flex()
        .flex_col()
        .flex_none()
        .w(size::PANEL)
        .overflow_y_scroll()
        .bg(color::CHROME)
        .border_r(size::HAIRLINE)
        .border_color(color::HAIRLINE)
        .child(section(true, "Inbox").children(statuses))
        .child(section(false, "Checks").children(check_rows))
        .child(section(false, "Kind").children(kinds))
        .child(section(false, "Source").children(sources))
        .children(state.load_error().cloned().map(|error| {
            div()
                .px(spacing::S3_5)
                .text_role(typography::CAPTION)
                .text_color(color::DANGER)
                .child(error)
        }))
}
