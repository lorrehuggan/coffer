//! The agent card (`docs/DESIGN.md` → "Components" → **Agent card**; Paper
//! `11R-0`): what an AI client proposed through MCP, waiting in the Inbox
//! for the user to approve or reject.
//!
//! Moldavite marks it as the agent's: `moldavite-surface` fill, 1px
//! `moldavite-border`, radius 10, 12px padding, 10px between its parts. The
//! header names the agent, where it ran and how many changes are pending;
//! below it are the proposed changes ([`AgentChange`]), then the actions
//! the caller passes (agent primary + agent outline buttons).
//!
//! Proposals are suggestions until the user acts (`AGENTS.md`: MCP writes
//! go to the inbox only), so nothing here changes data itself; the handlers
//! belong to the Inbox.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, HighlightStyle, InteractiveElement as _, IntoElement,
    ParentElement as _, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _,
    StyledText, Svg, TestSupportExt as _, Window, div,
};
use std::ops::Range;

use crate::theme::focus::{FocusRingExt as _, FocusRingPlacement};
use crate::theme::tokens::typography::TextRole;
use crate::theme::tokens::{color, radius, size, spacing, typography, weight};
use crate::ui::PreviewState;
use crate::ui::badge::Badge;
use crate::ui::button::{IconButton, IconButtonSize};
use crate::ui::icon::Glyph;
use crate::ui::text::TextRoleExt as _;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The agent's name and the card header's other text: body type at 600.
const AGENT_NAME: TextRole = TextRole {
    weight: weight::STRONG,
    ..typography::BODY
};

/// The glyph in a change's icon well: mono 11/14 at 600.
const KIND_GLYPH: TextRole = TextRole {
    weight: weight::STRONG,
    ..typography::MONO_VALUE
};

/// What a proposed change does, shown as a mark in its icon well.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    /// Sets a transaction's category: →.
    Categorise,
    /// Adds a categorisation rule: +.
    Rule,
    /// Pairs two transactions as a transfer between the user's accounts: ⇄,
    /// drawn as [`Glyph::Transfer`].
    TransferPair,
}

/// The mark in a change's icon well.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KindMark {
    /// A Geist Mono glyph.
    Text(&'static str),
    /// An icon in place of a glyph the bundled fonts lack, so it doesn't come
    /// from a platform fallback font (`DESIGN.md` → "Rendering findings
    /// (D2.6)"): Geist Mono and Inter have no ⇄ (U+21C4).
    Icon(Glyph),
}

impl ChangeKind {
    fn mark(self) -> KindMark {
        match self {
            Self::Categorise => KindMark::Text("→"),
            Self::Rule => KindMark::Text("+"),
            Self::TransferPair => KindMark::Icon(Glyph::Transfer),
        }
    }
}

/// An icon standing in for a well glyph: the glyph's colour, at its font
/// size (1em), as the glyph it replaces would be. Scaling the 12px icon to
/// 11px gives 1.1px strokes, inside "Iconography"'s range.
fn mark_icon(glyph: Glyph) -> Svg {
    glyph
        .render_at(KIND_GLYPH.size)
        .text_color(color::MOLDAVITE)
}

/// One proposed change on an [`AgentCard`]: a 26px row with the kind's
/// glyph in a `moldavite-well`, the subject in `text-strong` followed by the
/// detail in `moldavite-text-muted`, and its age in mono `moldavite-text-faint`.
///
/// While the pointer is over the row, or the keyboard is in it, the row
/// lifts to `moldavite-surface-raised` and the age gives way to approve and
/// reject icon buttons. The row is a tab stop so the buttons are reachable
/// without a pointer.
///
/// ```
/// use coffer::ui::agent_card::{AgentChange, ChangeKind};
///
/// let change = AgentChange::new("s-01", ChangeKind::Categorise, "Amazon Marketplace")
///     .detail("Shopping")
///     .age("2h");
/// assert_eq!(change.kind(), ChangeKind::Categorise);
/// ```
#[derive(IntoElement)]
pub struct AgentChange {
    id: ElementId,
    kind: ChangeKind,
    subject: SharedString,
    detail: Option<SharedString>,
    age: Option<SharedString>,
    on_approve: Option<ClickHandler>,
    on_reject: Option<ClickHandler>,
    preview: Option<PreviewState>,
}

impl AgentChange {
    /// A change to `subject` (the merchant, or "Rule", or "Transfer pair").
    pub fn new(
        id: impl Into<ElementId>,
        kind: ChangeKind,
        subject: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            subject: subject.into(),
            detail: None,
            age: None,
            on_approve: None,
            on_reject: None,
            preview: None,
        }
    }

    /// What the change proposes ("Shopping", "TfL → Transport").
    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// How long ago the agent proposed it, formatted short ("2h").
    pub fn age(mut self, age: impl Into<SharedString>) -> Self {
        self.age = Some(age.into());
        self
    }

    /// Runs `handler` when the user approves this change.
    pub fn on_approve(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_approve = Some(Box::new(handler));
        self
    }

    /// Runs `handler` when the user rejects this change.
    pub fn on_reject(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_reject = Some(Box::new(handler));
        self
    }

    /// Draws `state` without the input that causes it; see [`PreviewState`].
    /// Hover and pressed both draw the lifted row with its buttons.
    pub fn preview(mut self, state: PreviewState) -> Self {
        self.preview = Some(state);
        self
    }

    pub fn kind(&self) -> ChangeKind {
        self.kind
    }

    /// The subject and detail as one line, the detail in its own colour.
    fn summary(&self) -> StyledText {
        let (text, detail) = summary_text(&self.subject, self.detail.as_deref());
        let text = StyledText::new(text);
        match detail {
            Some(range) => text.with_highlights([(
                range,
                HighlightStyle {
                    color: Some(color::MOLDAVITE_TEXT_MUTED.into()),
                    ..HighlightStyle::default()
                },
            )]),
            None => text,
        }
    }

    fn spoken(&self) -> SharedString {
        match &self.detail {
            Some(detail) => format!("{}, {detail}", self.subject).into(),
            None => self.subject.clone(),
        }
    }
}

/// `subject` and `detail` joined by a space, and the byte range of the
/// detail in the result.
fn summary_text(subject: &str, detail: Option<&str>) -> (String, Option<Range<usize>>) {
    match detail {
        Some(detail) => {
            let start = subject.len() + 1;
            (
                format!("{subject} {detail}"),
                Some(start..start + detail.len()),
            )
        }
        None => (subject.to_owned(), None),
    }
}

impl RenderOnce for AgentChange {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let spoken = self.spoken();
        let summary_text = self.summary();
        let (hovered, focus) = window.with_element_namespace(self.id.clone(), |window| {
            let hovered = window.use_keyed_state("hovered", cx, |_, _| false);
            let focus =
                window.use_keyed_state("focus", cx, |_, cx| cx.focus_handle().tab_stop(true));
            (hovered, focus.read(cx).clone())
        });
        let preview_lifts = matches!(
            self.preview,
            Some(PreviewState::Hovered | PreviewState::Pressed | PreviewState::FocusVisible)
        );
        let lifted = preview_lifts || *hovered.read(cx) || focus.contains_focused(window, cx);

        let well = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(size::CONTROL_XS)
            .rounded(radius::SM)
            .bg(color::MOLDAVITE_WELL)
            .text_role(KIND_GLYPH)
            .text_color(color::MOLDAVITE)
            .map(|well| match self.kind.mark() {
                KindMark::Text(glyph) => well.child(glyph),
                KindMark::Icon(glyph) => well.child(mark_icon(glyph)),
            });
        let summary = div()
            .flex_1()
            .min_w_0()
            .whitespace_nowrap()
            .text_ellipsis()
            .text_role(typography::BODY)
            .text_color(color::TEXT_STRONG)
            .child(summary_text);

        let trailing = if lifted {
            let action =
                |id: &'static str, glyph, name: &'static str, handler: Option<ClickHandler>| {
                    IconButton::new(id, glyph, name)
                        .with_size(IconButtonSize::Small)
                        .with_foreground(color::MOLDAVITE_TEXT)
                        .when_some(handler, |this, handler| {
                            this.on_click(move |event, window, cx| handler(event, window, cx))
                        })
                };
            div()
                .flex()
                .flex_none()
                .items_center()
                .child(action("approve", Glyph::Check, "Approve", self.on_approve))
                .child(action("reject", Glyph::Close, "Reject", self.on_reject))
                .into_any_element()
        } else {
            div()
                .flex_none()
                .w(spacing::S6)
                .text_right()
                .text_role(typography::MONO_MICRO)
                .text_color(color::MOLDAVITE_TEXT_FAINT)
                .when_some(self.age, |this, age| this.child(age))
                .into_any_element()
        };

        let row = div()
            .id(self.id)
            // Lets UI tests find the row; a no-op outside them.
            .test_support()
            .track_focus(&focus)
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S2)
            .h(size::CONTROL_COMPACT)
            // The lifted fill reaches 6px past the text on both sides, so
            // every row carries the padding and pulls it back out.
            .mx(-spacing::S1_5)
            .px(spacing::S1_5)
            .rounded(radius::MD)
            .when(lifted, |this| this.bg(color::MOLDAVITE_SURFACE_RAISED))
            .on_hover(move |is_hovered, _, cx| {
                hovered.update(cx, |hovered, cx| {
                    if *hovered != *is_hovered {
                        *hovered = *is_hovered;
                        cx.notify();
                    }
                });
            })
            .aria_label(spoken)
            .child(well)
            .child(summary)
            .child(trailing);

        if self.preview == Some(PreviewState::FocusVisible) {
            row.show_focus_ring(FocusRingPlacement::Inset, window)
        } else {
            row.focus_visible_ring(&focus, FocusRingPlacement::Inset, window)
        }
    }
}

/// A card of changes an agent proposed.
///
/// ```
/// use coffer::theme::tokens::color;
/// use coffer::ui::agent_card::{AgentCard, AgentChange, ChangeKind};
/// use coffer::ui::button::{Button, ButtonVariant};
///
/// let card = AgentCard::new("Claude")
///     .context("Claude Code · 2h ago")
///     .pending("12")
///     .change(AgentChange::new("s-01", ChangeKind::Rule, "Rule").detail("TfL → Transport"))
///     .action(
///         Button::new("approve-all", "Approve all (12)")
///             .with_variant(ButtonVariant::AgentPrimary)
///             .with_full_width(true)
///             .with_surface(color::MOLDAVITE_SURFACE),
///     );
/// assert_eq!(card.change_count(), 1);
/// ```
#[derive(IntoElement)]
pub struct AgentCard {
    agent: SharedString,
    context: Option<SharedString>,
    pending: Option<SharedString>,
    changes: Vec<AgentChange>,
    actions: Vec<AnyElement>,
}

impl AgentCard {
    /// An empty card from `agent`, the name the MCP client gave ("Claude").
    pub fn new(agent: impl Into<SharedString>) -> Self {
        Self {
            agent: agent.into(),
            context: None,
            pending: None,
            changes: Vec::new(),
            actions: Vec::new(),
        }
    }

    /// Where and when the agent ran ("Claude Code · 2h ago"), after its name
    /// in `moldavite-text-muted`.
    pub fn context(mut self, context: impl Into<SharedString>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// The number of pending changes, formatted, in an agent badge at the
    /// end of the header. It can exceed the rows shown.
    pub fn pending(mut self, count: impl Into<SharedString>) -> Self {
        self.pending = Some(count.into());
        self
    }

    /// Adds a proposed change below the header.
    pub fn change(mut self, change: AgentChange) -> Self {
        self.changes.push(change);
        self
    }

    /// Adds a control to the actions row at the bottom, 6px apart: an agent
    /// primary button with [`with_full_width`], then an agent outline one.
    /// Give buttons [`with_surface`] with `moldavite-surface`, the card's fill,
    /// so they fade towards it while disabled.
    ///
    /// [`with_full_width`]: crate::ui::button::Button::with_full_width
    /// [`with_surface`]: crate::ui::button::Button::with_surface
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }

    pub fn change_count(&self) -> usize {
        self.changes.len()
    }
}

impl RenderOnce for AgentCard {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let header = div()
            .flex()
            .items_center()
            .gap(spacing::S2)
            .child(Glyph::Sparkle.render().text_color(color::MOLDAVITE))
            .child(
                div()
                    .flex_none()
                    .text_role(AGENT_NAME)
                    .text_color(color::MOLDAVITE)
                    .child(self.agent),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_role(typography::BODY)
                    .text_color(color::MOLDAVITE_TEXT_MUTED)
                    .when_some(self.context, |this, context| this.child(context)),
            )
            .when_some(self.pending, |this, count| this.child(Badge::new(count)));

        div()
            .flex()
            .flex_col()
            .gap(spacing::S2_5)
            .p(spacing::S3)
            .bg(color::MOLDAVITE_SURFACE)
            .border(size::HAIRLINE)
            .border_color(color::MOLDAVITE_BORDER)
            .rounded(radius::LG)
            .child(header)
            .when(!self.changes.is_empty(), |this| {
                this.child(div().flex().flex_col().children(self.changes))
            })
            .when(!self.actions.is_empty(), |this| {
                this.child(div().flex().gap(spacing::S1_5).children(self.actions))
            })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{Context, Render, TestAppContext, VisualTestContext};

    use super::*;
    use crate::theme;
    use crate::ui::button::{Button, ButtonVariant};

    #[test]
    fn kinds_use_the_paper_glyphs() {
        assert_eq!(ChangeKind::Categorise.mark(), KindMark::Text("→"));
        assert_eq!(ChangeKind::Rule.mark(), KindMark::Text("+"));
        // Not in the bundled fonts, so drawn as the table's transfer icon.
        assert_eq!(
            ChangeKind::TransferPair.mark(),
            KindMark::Icon(Glyph::Transfer)
        );
    }

    #[test]
    fn a_mark_icon_matches_the_well_glyphs() {
        let mut icon = mark_icon(Glyph::Transfer);
        let style = icon.style();
        // The well glyphs are mono 11px in moldavite.
        assert_eq!(KIND_GLYPH.size, gpui_kit::px(11.));
        assert_eq!(style.text.color, Some(color::MOLDAVITE.into()));
        let side = Some(gpui_kit::Length::from(KIND_GLYPH.size));
        assert_eq!((style.size.width, style.size.height), (side, side));
    }

    #[test]
    fn the_detail_is_highlighted_after_the_subject() {
        let change = AgentChange::new("s", ChangeKind::Rule, "Rule").detail("TfL → Transport");
        assert_eq!(change.spoken().as_ref(), "Rule, TfL → Transport");
        // A multi-byte detail still highlights exactly its own bytes.
        let (text, range) = summary_text("Transfer pair", Some("Savings ⇄ Current"));
        assert_eq!(&text[range.expect("a detail")], "Savings ⇄ Current");
        assert_eq!(summary_text("Rule", None), ("Rule".to_owned(), None));
    }

    struct Harness {
        approvals: Rc<Cell<usize>>,
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let approvals = self.approvals.clone();
            div().w(size::INSPECTOR).child(
                AgentCard::new("Claude")
                    .context("Claude Code · 2h ago")
                    .pending("12")
                    .change(
                        AgentChange::new("amazon", ChangeKind::Categorise, "Amazon Marketplace")
                            .detail("Shopping")
                            .age("2h")
                            .on_approve(move |_, _, _| approvals.set(approvals.get() + 1)),
                    )
                    .change(
                        AgentChange::new("rule", ChangeKind::Rule, "Rule")
                            .detail("TfL → Transport")
                            .age("2h"),
                    )
                    .action(
                        Button::new("approve-all", "Approve all (12)")
                            .with_variant(ButtonVariant::AgentPrimary)
                            .with_full_width(true),
                    )
                    .action(
                        Button::new("reject-all", "Reject all")
                            .with_variant(ButtonVariant::AgentOutline),
                    ),
            )
        }
    }

    fn open(cx: &mut TestAppContext) -> (&mut VisualTestContext, Rc<Cell<usize>>) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::component::install(cx);
        });
        let approvals = Rc::new(Cell::new(0));
        let (_, cx) = cx.add_window_view({
            let approvals = approvals.clone();
            move |_, _| Harness { approvals }
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        (cx, approvals)
    }

    #[gpui_kit::test]
    fn rows_are_26px_and_the_primary_action_fills_the_row(cx: &mut TestAppContext) {
        let (cx, _) = open(cx);
        let (amazon, approve_all, reject_all) = cx.update(|window, _| {
            (
                window.find("amazon").bounds(),
                window.find("approve-all").bounds(),
                window.find("reject-all").bounds(),
            )
        });
        assert_eq!(amazon.size.height, size::CONTROL_COMPACT);
        assert_eq!(approve_all.size.height, size::CONTROL_LG);
        assert_eq!(
            reject_all.left() - approve_all.right(),
            spacing::S1_5,
            "the primary takes the free width, 6px from the outline"
        );
        // The card's padding sits inside a 1px border.
        let content_right = amazon.right() - spacing::S1_5;
        assert_eq!(reject_all.right(), content_right);
    }

    #[gpui_kit::test]
    fn hovering_a_change_reveals_its_actions(cx: &mut TestAppContext) {
        let (cx, approvals) = open(cx);
        assert!(
            cx.update(|window, _| window.try_find("approve").is_none()),
            "at rest the row shows its age"
        );

        cx.update(|window, cx| window.hover("amazon", cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.update(|window, cx| window.click("approve", cx));
        assert_eq!(approvals.get(), 1);
    }
}
