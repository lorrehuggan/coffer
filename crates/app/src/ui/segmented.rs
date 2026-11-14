//! The segmented control, inspector size (`docs/DESIGN.md` → "Components" →
//! **Segmented control**; Paper `NR-0`, and the export dialog's CSV / JSON,
//! `HAJ-0`): a 28px `field` track, radius 6, 2px padding; segments radius
//! 4 with 12px inline padding, labels 12/16 500. The chosen segment has the
//! `control-active` fill and a `text` label; the others `text-muted`.
//!
//! Custom (`DESIGN.md` → "Implementing in GPUI": the stock segmented tabs
//! are 24 or 32px). Each segment is the unstyled `gpui-base` toggle: a Tab
//! stop, pressed when chosen, activated with Space or Enter.
//!
//! The panel size (the Import dialog's File / Map / Review, Paper `6E4-0`,
//! built for M3.4): a 30px track, radius 8, as wide as the caller says,
//! segments sharing it equally (radius 6, centred labels); the chosen one
//! adds the inset highlight. A segment that can't be chosen yet (a step
//! not reachable) is at `opacity-disabled` and isn't a Tab stop.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    SharedString, Styled as _, Window, div,
};

use crate::theme::focus::{FocusRingExt as _, FocusRingPlacement};
use crate::theme::tokens::typography::TextRole;
use crate::theme::tokens::{color, opacity, radius, shadow, size, spacing, typography, weight};
use crate::ui::text::TextRoleExt as _;
use gpui_kit::Pixels;

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
    /// The panel size's width; `None` is the inspector size.
    panel: Option<Pixels>,
    disabled: Vec<SharedString>,
    on_choose: Option<Choose>,
}

impl Segmented {
    /// A control with `chosen` (a segment's key) chosen.
    pub fn new(id: impl Into<ElementId>, chosen: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            segments: Vec::new(),
            chosen: chosen.into(),
            panel: None,
            disabled: Vec::new(),
            on_choose: None,
        }
    }

    /// Draws the panel size, `width` wide (Paper `6E4-0`).
    pub fn panel(mut self, width: Pixels) -> Self {
        self.panel = Some(width);
        self
    }

    /// Marks the segment `key` as one that can't be chosen yet.
    pub fn disabled(mut self, key: impl Into<SharedString>, disabled: bool) -> Self {
        if disabled {
            self.disabled.push(key.into());
        }
        self
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
        let panel = self.panel;
        let segments = self.segments.into_iter().map(|(key, label)| {
            let chosen = key == self.chosen;
            let disabled = self.disabled.contains(&key);
            let id = ElementId::Name(format!("{}-{key}", self.id).into());
            let focus = window
                .use_keyed_state(id.clone(), cx, |_, cx| cx.focus_handle())
                .read(cx)
                .clone();
            let on_choose = self.on_choose.clone();
            let toggle = gpui_kit::base::Toggle::new(id)
                .pressed(chosen)
                .disabled(disabled)
                .accessibility_label(label.clone())
                .when(!disabled, |this| this.track_focus(&focus))
                .h_full()
                .px(spacing::S3)
                .when(panel.is_some(), |this| {
                    this.flex().flex_1().items_center().justify_center()
                })
                .rounded(if panel.is_some() {
                    radius::MD
                } else {
                    radius::SM
                })
                .when(chosen, |this| this.bg(color::CONTROL_ACTIVE))
                .when(chosen && panel.is_some(), |this| {
                    this.shadow(vec![shadow::INSET_HIGHLIGHT.box_shadow()])
                })
                .text_role(LABEL)
                .text_color(if chosen {
                    color::TEXT
                } else {
                    color::TEXT_MUTED
                })
                // Tabs hover to `hover` ("Interaction states").
                .when(!chosen && !disabled, |this| {
                    this.hover(|style| style.bg(color::HOVER))
                })
                .when_some(on_choose.filter(|_| !disabled), |this, handler| {
                    this.on_change(move |_, _, window, cx| handler(&key, window, cx))
                })
                .child(label);
            div()
                .flex()
                .h_full()
                .when(panel.is_some(), |this| this.flex_1())
                .when(disabled, |this| this.opacity(opacity::DISABLED))
                .rounded(if panel.is_some() {
                    radius::MD
                } else {
                    radius::SM
                })
                .child(toggle)
                .focus_visible_ring(&focus, FocusRingPlacement::Inset, window)
        });
        div()
            .id(self.id.clone())
            .flex()
            .flex_none()
            .p(spacing::S0_5)
            .bg(color::FIELD)
            .map(|this| match panel {
                Some(width) => this.w(width).h(size::CONTROL_LG).rounded(radius::SURFACE),
                None => this.h(size::CONTROL).rounded(radius::MD),
            })
            .children(segments)
    }
}
