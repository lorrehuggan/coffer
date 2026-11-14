//! The bulk-action bar (`docs/DESIGN.md` → "Data table (Transactions)";
//! Paper `112-0`, on the multi-select artboard `21X-0`): the actions for a
//! multi-selection, floating over the table.

use gpui_kit::{
    AnyElement, App, ClickEvent, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div,
};

use crate::theme::tokens::{color, radius, shadow, size, spacing, typography, weight};
use crate::ui::button::IconButton;
use crate::ui::icon::Glyph;
use crate::ui::separator;
use crate::ui::text::TextRoleExt as _;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The bar: `float`, 1px `border`, radius 10, the toolbar shadow, 36px
/// tall with 4px inline padding and 2px gaps. The selection count, a rule,
/// the actions, a rule, and a ✕ that clears the selection.
///
/// The owner positions it: centred 24px above the bottom of the table,
/// over the rows. Draw it on a layer that doesn't clip its shadow.
#[derive(IntoElement)]
pub struct BulkActionBar {
    count: SharedString,
    actions: Vec<AnyElement>,
    on_clear: Option<ClickHandler>,
}

impl BulkActionBar {
    /// A bar for `count` selected records ("3", formatted for the locale).
    pub fn new(count: impl Into<SharedString>) -> Self {
        Self {
            count: count.into(),
            actions: Vec::new(),
            on_clear: None,
        }
    }

    /// Adds an action after the others: a [`ButtonVariant::Toolbar`]
    /// button, or one wrapped by its owner (to open a menu from it).
    ///
    /// [`ButtonVariant::Toolbar`]: crate::ui::button::ButtonVariant::Toolbar
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }

    /// Runs `handler` when the ✕ is clicked: clear the selection.
    pub fn on_clear(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_clear = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for BulkActionBar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let count = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S1_5)
            .px(spacing::S2_5)
            .child(
                div()
                    .text_role(typography::MONO_AMOUNT)
                    .text_color(color::TEXT)
                    .child(self.count),
            )
            .child(
                div()
                    .text_role(typography::BODY)
                    .font_weight(weight::MEDIUM)
                    .text_color(color::TEXT_SECONDARY)
                    .child("selected"),
            );
        let mut clear = IconButton::new("bulk-clear", Glyph::Close, "Clear selection");
        if let Some(handler) = self.on_clear {
            clear = clear.on_click(handler);
        }
        let rule = || separator(spacing::S4, color::BORDER_STRONG);

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S0_5)
            .h(size::CONTROL_XL)
            .px(spacing::S1)
            .rounded(radius::LG)
            .bg(color::FLOAT)
            .border(size::HAIRLINE)
            .border_color(color::BORDER)
            .shadow(vec![shadow::TOOLBAR.box_shadow()])
            .child(count)
            .child(rule())
            .children(self.actions)
            .child(rule())
            .child(clear)
    }
}
