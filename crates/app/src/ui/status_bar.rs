//! The status bar (`docs/DESIGN.md` → "Components" → **Status bar**, Paper
//! `RG-0`; on the Transactions artboard `1CU-0`): import status, the agent
//! connection, counts and the home currency, along the bottom of the window.

use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, Rgba, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div,
};

use crate::theme::tokens::{color, radius, size, spacing, typography};
use crate::ui::icon::Glyph;
use crate::ui::separator;
use crate::ui::text::TextRoleExt as _;

/// What leads a [`StatusItem`].
#[derive(Clone, Copy, Debug, PartialEq)]
enum Lead {
    /// A 6px status dot.
    Dot(Rgba),
    /// A 12px icon in `text`.
    Icon(Glyph),
    /// A 12px icon in `text-muted`: a state that's off ("AI access off").
    MutedIcon(Glyph),
}

/// One item in the status bar: caption text in `text-muted`, optionally in
/// Geist Mono, optionally led by a status dot or an icon.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusItem {
    text: SharedString,
    mono: bool,
    lead: Option<Lead>,
    action: Option<Action>,
}

/// A click handler.
type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

/// What clicking an item does, under an element id.
#[derive(Clone)]
struct Action {
    id: SharedString,
    handler: Handler,
}

impl std::fmt::Debug for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Action").field(&self.id).finish()
    }
}

impl PartialEq for Action {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl StatusItem {
    /// Caption text in Inter.
    pub fn text(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            mono: false,
            lead: None,
            action: None,
        }
    }

    /// Runs `handler` when the item is clicked: "1 agent connected" and
    /// "AI access off" open Settings → AI clients (`DESIGN.md` →
    /// "Settings"). `id` is the item's element id.
    pub fn on_click(
        mut self,
        id: impl Into<SharedString>,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some(Action {
            id: id.into(),
            handler: Rc::new(handler),
        });
        self
    }

    /// A count or a code ("328 transactions", "GBP"), in Geist Mono.
    pub fn mono(text: impl Into<SharedString>) -> Self {
        Self {
            mono: true,
            ..Self::text(text)
        }
    }

    /// Leads the text with a `moldavite` dot: an agent is connected over
    /// MCP.
    pub fn agent_dot(mut self) -> Self {
        self.lead = Some(Lead::Dot(color::MOLDAVITE));
        self
    }

    /// Leads the text with a `molten` dot: something unread or new.
    pub fn new_dot(mut self) -> Self {
        self.lead = Some(Lead::Dot(color::MOLTEN));
        self
    }

    /// Leads the text with `glyph` in `text-muted`: the `refused` icon
    /// before "AI access off" (Paper `GXO-0`).
    pub fn muted_icon(mut self, glyph: Glyph) -> Self {
        self.lead = Some(Lead::MutedIcon(glyph));
        self
    }

    /// Leads the text with `glyph` in `text`, as the check before
    /// "Reconciled" (success is neutral: `DESIGN.md` → "Colour semantics").
    pub fn icon(mut self, glyph: Glyph) -> Self {
        self.lead = Some(Lead::Icon(glyph));
        self
    }

    fn render(self) -> Vec<gpui_kit::AnyElement> {
        let lead = self.lead.map(|lead| match lead {
            Lead::Dot(fill) => div()
                .flex_none()
                .size(size::DOT)
                .rounded(radius::LABEL)
                .bg(fill)
                .into_any_element(),
            Lead::Icon(glyph) => glyph.render().text_color(color::TEXT).into_any_element(),
            Lead::MutedIcon(glyph) => glyph
                .render()
                .text_color(color::TEXT_MUTED)
                .into_any_element(),
        });
        let text = div()
            .flex_none()
            .whitespace_nowrap()
            .text_role(if self.mono {
                typography::MONO_VALUE
            } else {
                typography::CAPTION
            })
            .text_color(color::TEXT_MUTED)
            .child(self.text)
            .into_any_element();
        let parts: Vec<gpui_kit::AnyElement> =
            lead.into_iter().chain(std::iter::once(text)).collect();
        match self.action {
            // One target for the lead and the text, spaced as the bar spaces
            // them.
            Some(action) => vec![
                div()
                    .id(gpui_kit::ElementId::Name(action.id))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(spacing::S2)
                    .cursor_pointer()
                    .on_click(move |_, window, cx| (action.handler)(window, cx))
                    .children(parts)
                    .into_any_element(),
            ],
            None => parts,
        }
    }
}

/// The status bar: 28px, `float`, top `hairline`, 12px inline padding.
/// Items at the start and at the end, each group's items separated by 1×12
/// `border-strong` rules.
#[derive(IntoElement, Default)]
pub struct StatusBar {
    start: Vec<StatusItem>,
    end: Vec<StatusItem>,
}

impl StatusBar {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an item after the others at the start of the bar.
    pub fn start(mut self, item: StatusItem) -> Self {
        self.start.push(item);
        self
    }

    /// Adds an item after the others at the end of the bar.
    pub fn end(mut self, item: StatusItem) -> Self {
        self.end.push(item);
        self
    }
}

/// `items` with a separator between each pair.
fn separated(items: Vec<StatusItem>) -> Vec<gpui_kit::AnyElement> {
    let mut children = Vec::new();
    for (index, item) in items.into_iter().enumerate() {
        if index > 0 {
            children.push(separator(spacing::S3, color::BORDER_STRONG).into_any_element());
        }
        children.extend(item.render());
    }
    children
}

impl RenderOnce for StatusBar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S2)
            .w_full()
            .h(size::STATUS_BAR)
            .px(spacing::S3)
            .bg(color::FLOAT)
            .border_t(size::HAIRLINE)
            .border_color(color::HAIRLINE)
            .overflow_hidden()
            .children(separated(self.start))
            .child(div().flex_1())
            .when(!self.end.is_empty(), |this| {
                this.children(separated(self.end))
            })
    }
}
