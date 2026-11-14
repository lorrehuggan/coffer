//! Dialogs (`docs/DESIGN.md` → "Transactions additions" → **Dialogs**; Paper
//! `51L-0`, and D6.1 New account `8IR-0`): a focused decision or short form.
//!
//! `--container-dialog` (440px) wide, `float` fill, 1px `border`,
//! `radius-lg`, the Dock shadow; 16px padding and 16px between the title
//! (title type), the body and the footer, whose actions sit right-aligned
//! with 8px between them. The backdrop is `chrome` at `opacity-muted`, still
//! provisional ("Implementing in GPUI").
//!
//! Behaviour is the unstyled `gpui-base` dialog host: it traps focus inside,
//! and Escape asks to cancel. The owner keeps whether the dialog is open
//! (it renders it only then), so cancelling just calls back; the owner
//! closes it and puts focus back where it was.

use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, FocusHandle, IntoElement, ParentElement as _, RenderOnce, Rgba, SharedString,
    Styled as _, Window, div,
};

use crate::theme::tokens::{color, opacity, radius, shadow, size, spacing, typography};
use crate::ui::text::TextRoleExt as _;
use crate::ui::tracked::Tracked;

type Cancel = Rc<dyn Fn(&mut Window, &mut App)>;

/// The backdrop behind a dialog: `chrome` at `opacity-muted`.
fn backdrop_color() -> Rgba {
    Rgba {
        a: opacity::MUTED,
        ..color::CHROME
    }
}

/// A modal dialog with a title, a body and footer actions.
#[derive(IntoElement)]
pub struct Dialog {
    title: AnyElement,
    /// Shown after the title, on its row (the export dialog's CSV / JSON).
    title_trailing: Option<AnyElement>,
    focus: FocusHandle,
    body: Vec<AnyElement>,
    footer: Vec<AnyElement>,
    /// Before the actions, taking the footer's free width (Paper `HDF-0`).
    footer_lead: Option<AnyElement>,
    on_cancel: Option<Cancel>,
}

impl Dialog {
    /// A dialog titled `title`. `focus` is the dialog's own handle: focus
    /// stays inside while it's open.
    pub fn new(title: impl Into<SharedString>, focus: &FocusHandle) -> Self {
        Self::with_title(
            div()
                .flex()
                .text_color(color::TEXT)
                .child(Tracked::new(typography::TITLE, title.into())),
            focus,
        )
    }

    /// A dialog with `title` drawn by the caller in title type, for a title
    /// with a part in mono ("Enable `my-agent`?", Paper `GYP-0`).
    pub fn with_title(title: impl IntoElement, focus: &FocusHandle) -> Self {
        Self {
            title: title.into_any_element(),
            title_trailing: None,
            focus: focus.clone(),
            body: Vec::new(),
            footer: Vec::new(),
            footer_lead: None,
            on_cancel: None,
        }
    }

    /// Puts `element` at the end of the title's row, the title taking the
    /// rest (Paper `HAH-0`).
    pub fn title_trailing(mut self, element: impl IntoElement) -> Self {
        self.title_trailing = Some(element.into_any_element());
        self
    }

    /// Adds `element` to the body, below the title: field rows, a caption.
    pub fn body(mut self, element: impl IntoElement) -> Self {
        self.body.push(element.into_any_element());
        self
    }

    /// Puts `element` (a caption) at the start of the footer, before the
    /// actions (the export dialog's "Saved where you choose.").
    pub fn footer_lead(mut self, element: impl IntoElement) -> Self {
        self.footer_lead = Some(element.into_any_element());
        self
    }

    /// Adds an action to the footer, after those already there.
    pub fn action(mut self, button: impl IntoElement) -> Self {
        self.footer.push(button.into_any_element());
        self
    }

    /// Runs `handler` when the user cancels with Escape.
    pub fn on_cancel(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_cancel = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Dialog {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let on_cancel = self.on_cancel;
        let surface = div()
            .flex()
            .flex_col()
            .gap(spacing::S4)
            .w(size::DIALOG)
            .p(spacing::S4)
            .bg(color::FLOAT)
            .border(size::HAIRLINE)
            .border_color(color::BORDER)
            .rounded(radius::LG)
            .shadow(vec![shadow::DOCK.box_shadow()])
            .text_role(typography::BODY)
            .text_color(color::TEXT)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::S2)
                    .child(div().flex().flex_1().min_w_0().child(self.title))
                    .children(self.title_trailing),
            )
            .children(self.body)
            .child(
                div()
                    .flex()
                    .justify_end()
                    .items_center()
                    .gap(spacing::S2)
                    .when_some(self.footer_lead, |this, lead| {
                        this.child(div().flex_1().min_w_0().child(lead))
                    })
                    .children(self.footer),
            );
        gpui_kit::base::Dialog::new(cx)
            .focus_handle(self.focus)
            .close_on_escape(true)
            // A short form: a stray click outside shouldn't lose it.
            .close_on_backdrop_press(false)
            .backdrop(div().size_full().bg(backdrop_color()))
            .on_cancel(move |_, window, cx| {
                if let Some(handler) = &on_cancel {
                    handler(window, cx);
                }
                true
            })
            // Enter in a field submits through the field; Enter elsewhere
            // presses the focused button.
            .on_ok(|_, _, _| false)
            .popup(surface)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_backdrop_is_chrome_at_muted_opacity() {
        let backdrop = backdrop_color();
        assert_eq!(
            (backdrop.r, backdrop.g, backdrop.b),
            (color::CHROME.r, color::CHROME.g, color::CHROME.b)
        );
        assert_eq!(backdrop.a, opacity::MUTED);
    }
}
