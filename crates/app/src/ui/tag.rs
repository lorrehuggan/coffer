//! Tags in the Transactions inspector (`docs/DESIGN.md` → "Transactions
//! additions" → Tags; Paper `2Q3-0`): the pills in a wrapping box at field
//! width, 4px apart.
//!
//! A [`TagPill`] is 24px, `radius-pill`, `field` fill, 8px inline padding,
//! the name in caption `text-secondary`. A private tag leads with a 10px
//! `lock` in `text-muted`: private tags are never sent over MCP. The
//! [`AddTagPill`] ("+ Tag") has a dashed 1px `border-strong` outline and
//! `text-muted` content, and opens the tag menu.
//!
//! Composed for M3.9 (user, 2026-11-14; no Paper drawing): on hover a pill
//! shows the app's 10px Close glyph at its end, 4px after the name, in
//! `icon-faint` (`text-muted` under the pointer), and a click on it takes
//! the tag off. The tag menu removes it too, so the hover-only control is a
//! shortcut, never the only way. "+ Tag" takes the `hover` fill under the
//! pointer, like a chip, and Coffer's focus ring from the keyboard.

use gpui_kit::base::TestSupportExt as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ClickEvent, ElementId, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window, div,
};

use crate::theme::focus::{FocusRingExt as _, FocusRingPlacement};
use crate::theme::tokens::{color, radius, size, spacing, typography};
use crate::ui::icon::Glyph;
use crate::ui::text::TextRoleExt as _;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The pill's hover group, so its remove control shows on hover.
const GROUP: &str = "coffer-tag-pill";

/// The pill's shape: 24px, `radius-pill`, 8px inline padding, 4px gaps.
fn pill() -> gpui_kit::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S1)
        .h(size::CONTROL_SM)
        .px(spacing::S2)
        .rounded(radius::PILL)
}

/// One tag on a transaction.
#[derive(IntoElement)]
pub struct TagPill {
    id: ElementId,
    name: SharedString,
    private: bool,
    on_remove: Option<ClickHandler>,
}

impl TagPill {
    pub fn new(id: impl Into<ElementId>, name: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            private: false,
            on_remove: None,
        }
    }

    /// Leads with the lock: the tag never leaves the app.
    pub fn private(mut self, private: bool) -> Self {
        self.private = private;
        self
    }

    /// Shows the remove control on hover; `handler` runs when it's clicked.
    pub fn on_remove(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_remove = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for TagPill {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let remove_id = ElementId::Name(format!("{}-remove", self.id).into());
        pill()
            .id(self.id)
            // Lets UI tests find the pill; a no-op outside them.
            .test_support()
            .group(GROUP)
            .bg(color::FIELD)
            .when(self.private, |this| {
                this.child(Glyph::Lock.render().text_color(color::TEXT_MUTED))
            })
            .child(
                div()
                    .whitespace_nowrap()
                    .text_role(typography::CAPTION)
                    .text_color(color::TEXT_SECONDARY)
                    .child(self.name.clone()),
            )
            .when_some(self.on_remove, |this, handler| {
                this.child(
                    div()
                        .id(remove_id)
                        .test_support()
                        .hidden()
                        .group_hover(GROUP, |style| style.flex())
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .on_click(move |event, window, cx| {
                            cx.stop_propagation();
                            handler(event, window, cx);
                        })
                        .child(
                            Glyph::Close
                                .render()
                                .text_color(color::ICON_FAINT)
                                .hover(|style| style.text_color(color::TEXT_MUTED)),
                        ),
                )
            })
    }
}

/// The "+ Tag" pill that opens the tag menu.
#[derive(IntoElement)]
pub struct AddTagPill {
    id: ElementId,
    focus: FocusHandle,
    open: bool,
    on_click: Option<ClickHandler>,
}

impl AddTagPill {
    /// The pill, focused through `focus` so focus can come back to it when
    /// its menu closes.
    pub fn new(id: impl Into<ElementId>, focus: &FocusHandle) -> Self {
        Self {
            id: id.into(),
            focus: focus.clone(),
            open: false,
            on_click: None,
        }
    }

    /// Keeps the `hover` fill while its menu is open.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Runs `handler` on a click or Enter / Space while focused.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for AddTagPill {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let focus = self.focus.clone();
        div().flex().flex_none().rounded(radius::PILL).child(
            pill()
                .id(self.id)
                .test_support()
                .track_focus(&self.focus)
                .border(size::HAIRLINE)
                .border_dashed()
                .border_color(color::BORDER_STRONG)
                .when(self.open, |this| this.bg(color::HOVER))
                .hover(|style| style.bg(color::HOVER))
                .text_color(color::TEXT_MUTED)
                .when_some(self.on_click, |this, handler| {
                    this.on_click(move |event, window, cx| handler(event, window, cx))
                })
                .child(
                    Glyph::Plus
                        .render_at(size::ICON_XS)
                        .text_color(color::TEXT_MUTED),
                )
                .child(
                    div()
                        .whitespace_nowrap()
                        .text_role(typography::CAPTION)
                        .child("Tag"),
                )
                .focus_visible_ring(&focus, FocusRingPlacement::Outside, window),
        )
    }
}
