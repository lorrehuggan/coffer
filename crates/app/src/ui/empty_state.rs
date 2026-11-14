//! The empty state (`docs/DESIGN.md` → "Components" → **Empty state**,
//! Paper `13L-0`): what a place shows when it has nothing in it yet, and the
//! shortcuts that get something there.

use gpui_kit::{
    App, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _, Window, div,
};

use crate::theme::tokens::typography::TextRole;
use crate::theme::tokens::{color, radius, size, spacing, typography, weight};
use crate::ui::kbd::Kbd;
use crate::ui::text::TextRoleExt as _;

/// The title: body type at medium weight, in `text-strong`.
const TITLE: TextRole = TextRole {
    weight: weight::MEDIUM,
    ..typography::BODY
};

/// A dashed 1px `border-strong` box, radius 8, 16px block and 14px inline
/// padding, 10px between its parts: the title (12/16 500 `text-strong`), the
/// body (caption type, `text-muted`, wrapping) and a list of shortcuts, each
/// a key cap and what it does, 6px apart.
///
/// [`size::PANEL`] wide, as drawn. Presentation only: the shortcuts are hints,
/// and their bindings live in the keymap.
#[derive(IntoElement)]
pub struct EmptyState {
    title: SharedString,
    body: Option<SharedString>,
    shortcuts: Vec<(SharedString, SharedString)>,
}

impl EmptyState {
    /// An empty state with a title only.
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            body: None,
            shortcuts: Vec::new(),
        }
    }

    /// One or two sentences under the title: why it's empty, or what fills it.
    pub fn body(mut self, body: impl Into<SharedString>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Adds a shortcut after the others: `keys` as written on the key cap
    /// ("⌘I") and what it does ("Import a statement").
    pub fn shortcut(
        mut self,
        keys: impl Into<SharedString>,
        action: impl Into<SharedString>,
    ) -> Self {
        self.shortcuts.push((keys.into(), action.into()));
        self
    }

    pub fn title(&self) -> &SharedString {
        &self.title
    }
}

impl RenderOnce for EmptyState {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let mut state = div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(spacing::S2_5)
            .w(size::PANEL)
            .py(spacing::S4)
            .px(spacing::S3_5)
            .rounded(radius::SURFACE)
            .border(size::HAIRLINE)
            .border_dashed()
            .border_color(color::BORDER_STRONG)
            .child(
                div()
                    .text_role(TITLE)
                    .text_color(color::TEXT_STRONG)
                    .child(self.title),
            );
        if let Some(body) = self.body {
            state = state.child(
                div()
                    .text_role(typography::CAPTION)
                    .text_color(color::TEXT_MUTED)
                    .child(body),
            );
        }
        if !self.shortcuts.is_empty() {
            state = state.child(div().flex().flex_col().gap(spacing::S1_5).children(
                self.shortcuts.into_iter().map(|(keys, action)| {
                    div()
                        .flex()
                        .items_center()
                        .gap(spacing::S2)
                        .child(Kbd::new(keys))
                        .child(
                            div()
                                .whitespace_nowrap()
                                .text_role(typography::CAPTION)
                                .text_color(color::TEXT_MUTED)
                                .child(action),
                        )
                }),
            ));
        }
        state
    }
}
