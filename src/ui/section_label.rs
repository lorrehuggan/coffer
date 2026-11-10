//! The section label row (`docs/DESIGN.md` → "Components" → **Section
//! label row**, Paper `SE-0`): a caps heading over a list, with icon
//! actions at the end ("ACCOUNTS +").

use gpui_kit::{
    App, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _, Window, div,
};

use crate::theme::tokens::{color, size, spacing, typography};
use crate::ui::button::{IconButton, IconButtonSize};
use crate::ui::tracked::Tracked;

/// 28px, 8px left and 4px right padding: the label in section-label type
/// (11/14 600 caps, `text-muted`), then its actions as 24px icon buttons.
#[derive(IntoElement)]
pub struct SectionLabelRow {
    label: SharedString,
    actions: Vec<IconButton>,
}

impl SectionLabelRow {
    /// A row with no actions. Pass `label` in sentence case; the row
    /// uppercases it.
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            actions: Vec::new(),
        }
    }

    /// Adds an action after the others, sized as a small icon button.
    pub fn action(mut self, action: IconButton) -> Self {
        self.actions.push(action.with_size(IconButtonSize::Small));
        self
    }
}

impl RenderOnce for SectionLabelRow {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S1_5)
            .h(size::CONTROL)
            .pl(spacing::S2)
            .pr(spacing::S1)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_color(color::TEXT_MUTED)
                    .child(Tracked::new(typography::SECTION_LABEL, self.label)),
            )
            .children(self.actions)
    }
}
