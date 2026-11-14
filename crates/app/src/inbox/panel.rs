//! The Inbox's left panel (`docs/DESIGN.md` → "Inbox"; Paper `564-0`,
//! `ABK-0`): INBOX (Pending, Approved, Rejected, with counts), KIND (the
//! pending suggestions by kind, widest reach first) and SOURCE (each agent,
//! the connected dot on the ones that have filed, and "Coffer from your
//! edits"). KIND and SOURCE filter the list; a second click clears the
//! filter. D6.4's CHECKS section comes with M3.12 (user, 2026-11-14).

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
pub fn render(inbox: &Entity<Inbox>, cx: &App) -> impl IntoElement {
    let state = inbox.read(cx);
    let counts = &state.data().counts;

    let statuses = [
        SuggestionStatus::Pending,
        SuggestionStatus::Approved,
        SuggestionStatus::Rejected,
    ]
    .into_iter()
    .map(|status| {
        let handle = inbox.clone();
        ListRow::new(
            SharedString::from(format!("inbox-status-{status}")),
            status_label(status),
        )
        .icon(status_glyph(status))
        .trailing(counts.with_status(status).to_string())
        .trailing_tone(TrailingTone::Faint)
        .selected(state.status() == status)
        .on_click(move |_, _, cx| handle.update(cx, |inbox, cx| inbox.show_status(status, cx)))
    });

    let kinds = KIND_ORDER.into_iter().map(|kind| {
        let handle = inbox.clone();
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
        .on_click(move |_, _, cx| handle.update(cx, |inbox, cx| inbox.toggle_kind(kind, cx)))
    });

    let sources = state.data().sources.iter().map(|source| {
        let pending = counts.by_source.get(source).copied().unwrap_or(0);
        let (name, suffix) = source_name(source);
        let handle = inbox.clone();
        let filter = source.clone();
        let row = ListRow::new(SharedString::from(format!("inbox-source-{source}")), name)
            .trailing(pending.to_string())
            .trailing_tone(TrailingTone::Faint)
            .selected(state.source_filter() == Some(source.as_str()))
            .on_click(move |_, _, cx| {
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
