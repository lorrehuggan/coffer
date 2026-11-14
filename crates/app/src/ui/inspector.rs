//! The inspector's building blocks (`docs/DESIGN.md` → "Transactions";
//! Paper `1A0-0`, and the account inspector `8GJ-0`): 14px sections divided
//! by hairlines, a header with a display figure, and section titles.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{AnyElement, IntoElement, ParentElement as _, SharedString, Styled as _, div};

use crate::theme::tokens::{color, size, spacing, typography};
use crate::ui::text::TextRoleExt as _;
use crate::ui::tracked::Tracked;

/// A section: 14px padding; below the first, a `hairline` above it.
pub fn section(divided: bool) -> gpui_kit::Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .p(spacing::S3_5)
        .when(divided, |this| {
            this.border_t(size::HAIRLINE).border_color(color::HAIRLINE)
        })
}

/// A section's title: section-title type in `text`.
pub fn section_title(title: impl Into<SharedString>) -> gpui_kit::Div {
    div()
        .text_role(typography::SECTION_TITLE)
        .text_color(color::TEXT)
        .child(title.into())
}

/// A section's title row, 24px, with `trailing` (an outline button, a
/// total) at its end (Paper `8GJ-0`: Balances, Pots).
pub fn section_title_row(
    title: impl Into<SharedString>,
    trailing: Option<AnyElement>,
) -> gpui_kit::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(size::CONTROL_SM)
        .child(section_title(title).flex_1().min_w_0())
        .children(trailing)
}

/// The header: the title (13/16 600), a display figure and a caption.
///
/// The title and figure are [`Tracked`] for their roles' tracking, so a
/// title too long for the inspector is clipped rather than wrapped (D5
/// finding; `docs/DESIGN.md` → "Letter spacing").
pub fn header(
    title: impl Into<SharedString>,
    figure: Option<String>,
    caption: Option<AnyElement>,
) -> gpui_kit::Div {
    section(false)
        .gap(spacing::S1)
        .child(
            div()
                .flex()
                .min_w_0()
                .overflow_hidden()
                .text_color(color::TEXT)
                .child(Tracked::new(typography::TITLE, title)),
        )
        .when_some(figure, |this, figure| {
            this.child(
                div()
                    .flex()
                    .text_color(color::TEXT)
                    .child(Tracked::new(typography::DISPLAY_FIGURE, figure)),
            )
        })
        .children(caption)
}

/// A caption line: 11/14 `text-muted`.
pub fn caption(text: impl IntoElement) -> gpui_kit::Div {
    div()
        .flex()
        .items_center()
        .text_role(typography::CAPTION)
        .text_color(color::TEXT_MUTED)
        .child(text)
}
