//! The toggle (`docs/DESIGN.md` → "Components" → **Toggle**; Paper `MA-0`,
//! `MG-0`): a 28×16 track, radius 8, 2px padding, with a 12px knob.
//!
//! Off: track `control-active`, knob `text-muted`, at the start. On: track
//! `text`, knob `chrome`, at the end. Neutral, never an accent. Custom, on
//! the unstyled `gpui-base` switch, which owns the switch role, the checked
//! state for assistive tech, Tab focus and Space/Enter activation
//! (`DESIGN.md` → "Implementing in GPUI": the stock switch has one knob
//! colour for both states).
//!
//! Not built: the `motion-base` slide (the knob moves at once).

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ElementId, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _, Window,
    div,
};

use crate::theme::focus::{FocusRingExt as _, FocusRingPlacement};
use crate::theme::tokens::{color, opacity, radius, size, spacing};

type Change = Box<dyn Fn(&bool, &mut Window, &mut App)>;

/// A toggle: on or off, controlled by its owner.
#[derive(IntoElement)]
pub struct Toggle {
    id: ElementId,
    on: bool,
    label: SharedString,
    disabled: bool,
    on_change: Option<Change>,
}

impl Toggle {
    /// A toggle named `label` (its accessible name: it has no visible text)
    /// showing `on`.
    pub fn new(id: impl Into<ElementId>, on: bool, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            on,
            label: label.into(),
            disabled: false,
            on_change: None,
        }
    }

    /// Runs `handler` with the new value when the toggle is switched (a
    /// `cx.listener` fits). The owner shows it by passing it back.
    pub fn on_change(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Box::new(handler));
        self
    }

    /// Shows the toggle faded to `opacity-disabled`, ignoring input.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn is_on(&self) -> bool {
        self.on
    }
}

impl RenderOnce for Toggle {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let on = self.on;
        let focus = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let knob = div()
            .flex_none()
            .size(size::TOGGLE_KNOB)
            .rounded(radius::MD)
            .bg(if on { color::CHROME } else { color::TEXT_MUTED });
        let switch = gpui_kit::base::Switch::new(self.id)
            .checked(on)
            .disabled(self.disabled)
            .accessibility_label(self.label)
            .track_focus(&focus)
            .flex()
            .flex_none()
            .items_center()
            .when(on, |this| this.justify_end())
            .w(size::TOGGLE_WIDTH)
            .h(size::CONTROL_XS)
            .p(spacing::S0_5)
            .rounded(radius::SURFACE)
            .bg(if on {
                color::TEXT
            } else {
                color::CONTROL_ACTIVE
            })
            .when(self.disabled, |this| this.opacity(opacity::DISABLED))
            .when_some(self.on_change, |this, handler| {
                this.on_change(move |next, _, window, cx| handler(&next, window, cx))
            })
            .child(knob);
        // The ring's corners follow the wrapper's, concentric with the track.
        div()
            .flex()
            .flex_none()
            .rounded(radius::SURFACE)
            .child(switch)
            .focus_visible_ring(&focus, FocusRingPlacement::Outside, window)
    }
}
