//! The segmented control, inspector size (`docs/DESIGN.md` → "Components" →
//! **Segmented control**; Paper `NR-0`, and the export dialog's CSV / JSON,
//! `HAJ-0`): a 28px `field` track, radius 6, 2px padding; segments radius
//! 4 with 12px inline padding, labels 12/16 500. The chosen segment has the
//! `control-active` fill and a `text` label; the others `text-muted`.
//!
//! Custom (`DESIGN.md` → "Implementing in GPUI": the stock segmented tabs
//! are 24 or 32px). Each segment is the unstyled `gpui-base` toggle: a Tab
//! stop, pressed when chosen, activated with Space or Enter. Not built: the
//! panel size (30px, inset highlight), which nothing uses yet.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    SharedString, Styled as _, Window, div,
};

use crate::theme::focus::{FocusRingExt as _, FocusRingPlacement};
use crate::theme::tokens::typography::TextRole;
use crate::theme::tokens::{color, radius, size, spacing, typography, weight};
use crate::ui::text::TextRoleExt as _;

type Choose = std::rc::Rc<dyn Fn(&SharedString, &mut Window, &mut App)>;

/// A segment's label: 12/16 500.
const LABEL: TextRole = TextRole {
    weight: weight::MEDIUM,
    ..typography::BODY
};

/// A segmented control choosing one of its segments.
#[derive(IntoElement)]
pub struct Segmented {
    id: ElementId,
    segments: Vec<(SharedString, SharedString)>,
    chosen: SharedString,
    on_choose: Option<Choose>,
}

impl Segmented {
    /// A control with `chosen` (a segment's key) chosen.
    pub fn new(id: impl Into<ElementId>, chosen: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            segments: Vec::new(),
            chosen: chosen.into(),
            on_choose: None,
        }
    }

    /// Adds a segment after the others.
    pub fn segment(mut self, key: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        self.segments.push((key.into(), label.into()));
        self
    }

    /// Runs `handler` with a segment's key when it's chosen.
    pub fn on_choose(
        mut self,
        handler: impl Fn(&SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_choose = Some(std::rc::Rc::new(handler));
        self
    }
}

impl RenderOnce for Segmented {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let segments = self.segments.into_iter().map(|(key, label)| {
            let chosen = key == self.chosen;
            let id = ElementId::Name(format!("{}-{key}", self.id).into());
            let focus = window
                .use_keyed_state(id.clone(), cx, |_, cx| cx.focus_handle())
                .read(cx)
                .clone();
            let on_choose = self.on_choose.clone();
            let toggle = gpui_kit::base::Toggle::new(id)
                .pressed(chosen)
                .accessibility_label(label.clone())
                .track_focus(&focus)
                .h_full()
                .px(spacing::S3)
                .rounded(radius::SM)
                .when(chosen, |this| this.bg(color::CONTROL_ACTIVE))
                .text_role(LABEL)
                .text_color(if chosen {
                    color::TEXT
                } else {
                    color::TEXT_MUTED
                })
                // Tabs hover to `hover` ("Interaction states").
                .when(!chosen, |this| this.hover(|style| style.bg(color::HOVER)))
                .when_some(on_choose, |this, handler| {
                    this.on_change(move |_, _, window, cx| handler(&key, window, cx))
                })
                .child(label);
            div()
                .flex()
                .h_full()
                .rounded(radius::SM)
                .child(toggle)
                .focus_visible_ring(&focus, FocusRingPlacement::Inset, window)
        });
        div()
            .id(self.id.clone())
            .flex()
            .flex_none()
            .h(size::CONTROL)
            .p(spacing::S0_5)
            .rounded(radius::MD)
            .bg(color::FIELD)
            .children(segments)
    }
}
