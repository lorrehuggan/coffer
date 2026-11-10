//! Count badges (`docs/DESIGN.md` → "Components" → **Badge**).
//!
//! Custom, not the stock `Badge`: that one is a dot or count overlaid on
//! another element, while Coffer's badge is an inline pill.

use gpui_kit::{
    App, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _, Window, div,
};

use crate::theme::tokens::{color, radius, size, spacing, typography};
use crate::ui::text::TextRoleExt as _;

/// Whose count a [`Badge`] shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BadgeKind {
    /// The agent's pending work ("12" suggestions): `moldavite-soft` fill,
    /// `moldavite` text.
    #[default]
    Agent,
    /// Any other count: `hover` fill, `text-muted` text.
    Neutral,
}

/// A count in a 16px pill: radius 8, 4px padding, 10/12 700.
///
/// Stateless and not interactive; the caller formats the count for the
/// user's locale.
#[derive(IntoElement)]
pub struct Badge {
    count: SharedString,
    kind: BadgeKind,
}

impl Badge {
    /// An [`BadgeKind::Agent`] badge.
    pub fn new(count: impl Into<SharedString>) -> Self {
        Self {
            count: count.into(),
            kind: BadgeKind::default(),
        }
    }

    /// Sets whose count this is.
    pub fn with_kind(mut self, kind: BadgeKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn kind(&self) -> BadgeKind {
        self.kind
    }
}

impl RenderOnce for Badge {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let (fill, text) = match self.kind {
            BadgeKind::Agent => (color::MOLDAVITE_SOFT, color::MOLDAVITE),
            BadgeKind::Neutral => (color::HOVER, color::TEXT_MUTED),
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(size::CONTROL_XS)
            .min_w(size::CONTROL_XS)
            .px(spacing::S1)
            .rounded(radius::SURFACE)
            .bg(fill)
            .text_role(typography::BADGE)
            .text_color(text)
            .child(self.count)
    }
}
