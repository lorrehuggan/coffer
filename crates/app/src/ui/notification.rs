//! The notification bar (`docs/DESIGN.md` → "Transactions additions" →
//! **Undo notification**; Paper `51I-0`, and Settings' "Back up now",
//! `HF7-0`): what just happened, floating over the canvas where the
//! bulk-action bar sits.
//!
//! `float`, 1px `border`, `radius-lg`, the Toolbar shadow; 8px block, 12px
//! start and 8px end padding, 12px gaps. A 14px `check`, the message in
//! body type, an optional outline action ("Undo", "Show in folder") and a
//! 24px dismiss icon button. It stays until dismissed or replaced.
//!
//! Composed for M3.16 (not drawn): a failure uses the same bar with the
//! 12px `alert` icon in `danger` and the message in `danger`.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, IntoElement, ParentElement as _, RenderOnce, Styled as _, Window,
    div,
};

use crate::theme::tokens::{color, radius, shadow, size, spacing, typography};
use crate::ui::button::{Button, IconButton, IconButtonSize};
use crate::ui::icon::Glyph;
use crate::ui::text::TextRoleExt as _;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A notification bar. The owner positions it: centred 24px above the
/// bottom of the canvas, on a layer that doesn't clip its shadow.
#[derive(IntoElement)]
pub struct NotificationBar {
    message: AnyElement,
    failed: bool,
    action: Option<Button>,
    on_dismiss: Option<ClickHandler>,
}

impl NotificationBar {
    /// A bar saying `message` (body type; the caller styles any mono or
    /// muted parts) after a check.
    pub fn new(message: impl IntoElement) -> Self {
        Self {
            message: message.into_any_element(),
            failed: false,
            action: None,
            on_dismiss: None,
        }
    }

    /// Marks the bar as reporting a failure: the `alert` icon and the
    /// message in `danger`.
    pub fn failed(mut self, failed: bool) -> Self {
        self.failed = failed;
        self
    }

    /// Adds an outline action after the message.
    pub fn action(mut self, button: Button) -> Self {
        self.action = Some(button);
        self
    }

    /// Runs `handler` when the dismiss button is clicked.
    pub fn on_dismiss(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_dismiss = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for NotificationBar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let icon = if self.failed {
            Glyph::Alert.render().text_color(color::DANGER)
        } else {
            Glyph::Check
                .render_at(size::ICON_MD)
                .text_color(color::TEXT)
        };
        let dismiss = IconButton::new("notification-dismiss", Glyph::Dismiss, "Dismiss")
            .with_size(IconButtonSize::Small)
            .when_some(self.on_dismiss, |this, handler| {
                this.on_click(move |event, window, cx| handler(event, window, cx))
            });
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S3)
            .py(spacing::S2)
            .pl(spacing::S3)
            .pr(spacing::S2)
            .bg(color::FLOAT)
            .border(size::HAIRLINE)
            .border_color(color::BORDER)
            .rounded(radius::LG)
            .shadow(vec![shadow::TOOLBAR.box_shadow()])
            .child(icon)
            .child(
                div()
                    .flex()
                    .items_center()
                    .whitespace_nowrap()
                    .text_role(typography::BODY)
                    .text_color(if self.failed {
                        color::DANGER
                    } else {
                        color::TEXT
                    })
                    .child(self.message),
            )
            .children(self.action)
            .child(dismiss)
    }
}
