//! The review queues' canvas and inspector, in the Inbox's place while a
//! CHECKS row is selected (`docs/DESIGN.md` → "Review queues"; Paper
//! `8SE-0`, `98K-0`, `9MQ-0`).
//!
//! The toolbar (52px) names the queue, with its keys for Possible
//! duplicates (M merge, N not duplicates, ←→ keep, ↑↓ move); the body is
//! [`super::duplicates`] or [`super::balances`]; after a merge or a
//! dismissal the D6.3 undo bar floats at the foot of the canvas. A change
//! core refuses is said under the toolbar in caption type, `danger`.

use gpui_kit::base::actions::{Cancel, SelectDown, SelectUp};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
};

use crate::theme::tokens::{color, size, spacing, typography};
use crate::ui::icon::Glyph;
use crate::ui::kbd::Kbd;
use crate::ui::text::TextRoleExt as _;
use crate::ui::tracked::Tracked;
use crate::ui::undo_bar::UndoBar;

use super::text::count;
use super::{Checks, Outcome, Queue};

/// The key context of the queues' canvas, where M, N, ←→ and ↑↓ work.
pub const CONTEXT: &str = "CofferChecks";

gpui_kit::actions!(
    coffer_checks,
    [
        /// M: merge the selected pair.
        MergeSelected,
        /// N: the selected pair aren't duplicates.
        DismissSelected,
        /// ←→: keep the other row of the selected pair.
        KeepOther,
    ]
);

/// The queues' canvas and inspector.
pub struct ChecksView {
    checks: Entity<Checks>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl ChecksView {
    pub fn new(checks: Entity<Checks>, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.observe(&checks, |_, _, cx| cx.notify())];
        Self {
            checks,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    pub fn checks(&self) -> &Entity<Checks> {
        &self.checks
    }

    /// The canvas's focus: where the queue's keys work.
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    fn update(&self, cx: &mut Context<Self>, f: impl FnOnce(&mut Checks, &mut Context<Checks>)) {
        self.checks.update(cx, f);
    }

    fn render_toolbar(&self, checks: &Checks) -> impl IntoElement {
        let (title, caption) = match checks.queue() {
            Some(Queue::Balances) => {
                let failing = checks.failing();
                (
                    "Balance checks",
                    if failing > 0 {
                        format!(
                            "Every balance you enter or import is checked against the \
                             transactions · {failing} failing"
                        )
                    } else {
                        "Every balance you enter or import is checked against the transactions"
                            .to_owned()
                    },
                )
            }
            _ => (
                "Possible duplicates",
                format!(
                    "{} · nothing changes until you merge",
                    count(checks.pending_pairs(), "pair", "pairs")
                ),
            ),
        };
        let key = |keys: &'static str, label: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(spacing::S1_5)
                .child(Kbd::new(keys))
                .child(
                    div()
                        .text_role(typography::CAPTION)
                        .text_color(color::TEXT_MUTED)
                        .child(label),
                )
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S2)
            .h(size::CONTROL + spacing::S6)
            .py(spacing::S3)
            .px(spacing::S4)
            .border_b(size::HAIRLINE)
            .border_color(color::HAIRLINE)
            .child(
                div()
                    .flex()
                    .text_color(color::TEXT)
                    .child(Tracked::new(typography::TITLE, title)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_role(typography::CAPTION)
                    .text_color(color::TEXT_MUTED)
                    .child(caption),
            )
            .when(
                checks.queue() == Some(Queue::Duplicates) && checks.pending_pairs() > 0,
                |this| {
                    this.child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(spacing::S3)
                            .child(key("M", "Merge"))
                            .child(key("N", "Not duplicates"))
                            .child(key("←→", "Keep"))
                            .child(key("↑↓", "Move")),
                    )
                },
            )
    }

    fn render_bar(&self, checks: &Checks) -> Option<AnyElement> {
        let Some(Outcome::Done { message, undo }) = checks.outcome() else {
            return None;
        };
        let (undo_on, dismiss_on) = (self.checks.clone(), self.checks.clone());
        let mut bar = UndoBar::new(message.clone()).on_dismiss(click(move |_, _, cx| {
            dismiss_on.update(cx, |checks, cx| checks.dismiss_outcome(cx))
        }));
        if undo.is_some() {
            bar = bar.on_undo(click(move |_, _, cx| {
                undo_on.update(cx, |checks, cx| checks.undo(cx))
            }));
        }
        Some(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(spacing::S6)
                .flex()
                .justify_center()
                .child(bar)
                .into_any_element(),
        )
    }
}

/// A refusal under the toolbar: caption type, `danger`.
fn refused(message: SharedString) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S1_5)
        .pt(spacing::S2)
        .px(spacing::S4)
        .child(Glyph::Alert.render().text_color(color::DANGER))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_role(typography::CAPTION)
                .text_color(color::DANGER)
                .child(message),
        )
}

/// A centred empty queue (Paper `9WH-0`): a 36px `field` well with the
/// queue's 16px icon, the title in title type, and a caption.
pub fn empty_queue(
    glyph: Glyph,
    title: impl Into<SharedString>,
    body: SharedString,
) -> impl IntoElement {
    div()
        .flex()
        .flex_1()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(spacing::S2)
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(size::CONTROL_XL)
                .rounded(crate::theme::tokens::radius::SURFACE)
                .bg(color::FIELD)
                .child(glyph.render().text_color(color::TEXT_MUTED)),
        )
        .child(
            div()
                .flex()
                .pt(spacing::S1)
                .text_color(color::TEXT)
                .child(Tracked::new(typography::TITLE, title.into())),
        )
        .child(
            div()
                .w(size::PANEL + spacing::S6 * 2.)
                .text_center()
                .text_role(typography::CAPTION)
                .text_color(color::TEXT_MUTED)
                .child(body),
        )
}

impl Render for ChecksView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = div()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &SelectUp, _, cx| {
                this.update(cx, |checks, cx| match checks.queue() {
                    Some(Queue::Balances) => checks.select_next_check(-1, cx),
                    _ => checks.select_next_pair(-1, cx),
                })
            }))
            .on_action(cx.listener(|this, _: &SelectDown, _, cx| {
                this.update(cx, |checks, cx| match checks.queue() {
                    Some(Queue::Balances) => checks.select_next_check(1, cx),
                    _ => checks.select_next_pair(1, cx),
                })
            }))
            .on_action(cx.listener(|this, _: &Cancel, _, cx| {
                this.update(cx, |checks, cx| checks.clear_selection(cx))
            }))
            .on_action(cx.listener(|this, _: &MergeSelected, _, cx| {
                this.update(cx, |checks, cx| {
                    if checks.queue() == Some(Queue::Duplicates) {
                        checks.merge(cx)
                    }
                })
            }))
            .on_action(cx.listener(|this, _: &DismissSelected, _, cx| {
                this.update(cx, |checks, cx| {
                    if checks.queue() == Some(Queue::Duplicates) {
                        checks.dismiss(cx)
                    }
                })
            }))
            .on_action(cx.listener(|this, _: &KeepOther, _, cx| {
                this.update(cx, |checks, cx| {
                    if checks.queue() == Some(Queue::Duplicates) {
                        checks.keep_other(cx)
                    }
                })
            }))
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0();
        let cx: &App = cx;
        let checks = self.checks.read(cx);
        let (body, inspector): (AnyElement, AnyElement) = match checks.queue() {
            Some(Queue::Balances) => (
                super::balances::render_body(&self.checks, checks, &self.focus, window)
                    .into_any_element(),
                super::balances::render_inspector(&self.checks, checks, window).into_any_element(),
            ),
            _ => (
                super::duplicates::render_body(&self.checks, checks, &self.focus, window)
                    .into_any_element(),
                super::duplicates::render_inspector(checks, window).into_any_element(),
            ),
        };
        let refusal = match checks.outcome() {
            Some(Outcome::Refused(message)) => Some(message.clone()),
            _ => None,
        };
        let canvas = div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .bg(color::CANVAS)
            .child(self.render_toolbar(checks))
            .children(refusal.map(refused))
            .child(body)
            .children(self.render_bar(checks));
        root.child(canvas).child(inspector)
    }
}

/// A click handler: the type a button's `on_click` takes.
pub(super) fn click(
    handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    handler
}
