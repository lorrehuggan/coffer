//! The undo notification (`docs/DESIGN.md` → "Transactions additions" →
//! "Undo notification"; Paper `53O-0`): after a change to many existing
//! rows, a floating bar in the bulk-action bar's place says what changed and
//! offers to undo it.

use gpui_kit::{
    App, ClickEvent, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _,
    Window, div,
};

use crate::theme::tokens::{color, radius, shadow, size, spacing, typography};
use crate::ui::button::{Button, ButtonVariant, IconButton, IconButtonSize};
use crate::ui::icon::Glyph;
use crate::ui::text::TextRoleExt as _;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The bar: `float`, 1px `border`, `radius-lg`, the Toolbar shadow; 8px
/// block, 12px left and 8px right padding, 12px gaps. A 14px `check` in
/// `text`, the change in body text, outline "Undo", and a 24px dismiss
/// button.
///
/// The owner positions it where the bulk-action bar sits, and keeps it until
/// it's dismissed or the next change replaces it.
#[derive(IntoElement)]
pub struct UndoBar {
    message: SharedString,
    on_undo: Option<ClickHandler>,
    on_dismiss: Option<ClickHandler>,
}

impl UndoBar {
    /// A bar saying `message` ("Categorised 3 transactions as Shopping").
    pub fn new(message: impl Into<SharedString>) -> Self {
        Self {
            message: message.into(),
            on_undo: None,
            on_dismiss: None,
        }
    }

    /// Runs `handler` on "Undo": revert the change batch.
    pub fn on_undo(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_undo = Some(Box::new(handler));
        self
    }

    /// Runs `handler` on the ✕: close the bar. The change stays undoable
    /// from the change history.
    pub fn on_dismiss(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_dismiss = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for UndoBar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let mut undo = Button::new("undo-change", "Undo").with_variant(ButtonVariant::Outline);
        if let Some(handler) = self.on_undo {
            undo = undo.on_click(handler);
        }
        let mut dismiss = IconButton::new("undo-dismiss", Glyph::Close, "Dismiss")
            .with_size(IconButtonSize::Small);
        if let Some(handler) = self.on_dismiss {
            dismiss = dismiss.on_click(handler);
        }
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S3)
            .py(spacing::S2)
            .pl(spacing::S3)
            .pr(spacing::S2)
            .rounded(radius::LG)
            .bg(color::FLOAT)
            .border(size::HAIRLINE)
            .border_color(color::BORDER)
            .shadow(vec![shadow::TOOLBAR.box_shadow()])
            .child(
                Glyph::Check
                    .render_at(size::ICON_MD)
                    .text_color(color::TEXT),
            )
            .child(
                div()
                    .whitespace_nowrap()
                    .text_role(typography::BODY)
                    .text_color(color::TEXT)
                    .child(self.message),
            )
            .child(undo)
            .child(dismiss)
    }
}
