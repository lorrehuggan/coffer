//! The agent's suggestion for one record, in the inspector
//! (`docs/DESIGN.md` → "Transactions"; Paper `1MX-0`): who proposed it, what
//! it would change and why, with approve and reject.
//!
//! The agent card ([`crate::ui::agent_card`]) lists many pending changes;
//! this is the single-change form, so it has no count badge and its actions
//! are full buttons.

use gpui_kit::{
    App, ClickEvent, HighlightStyle, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, StyledText, Window, div,
};

use crate::theme::tokens::{color, radius, size, spacing, typography, weight};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::icon::Glyph;
use crate::ui::text::TextRoleExt as _;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A suggested change: `moldavite-surface`, 1px `moldavite-border`, radius
/// 10, 12px padding, 10px between the header, the proposal and the actions.
///
/// The proposal is `text-strong` with its subject (the category) in
/// `moldavite-text`; the reason is a caption in `moldavite-text-muted`.
/// "Approve" (agent primary) takes the free width, "Reject" (agent outline)
/// hugs its label. The reason is the agent's text: show it, never act on it.
#[derive(IntoElement)]
pub struct AgentSuggestion {
    agent: SharedString,
    context: SharedString,
    action: SharedString,
    subject: SharedString,
    reason: Option<SharedString>,
    on_approve: Option<ClickHandler>,
    on_reject: Option<ClickHandler>,
}

impl AgentSuggestion {
    /// A suggestion from `agent` ("Claude") with `context` ("Claude Code ·
    /// 2h ago"), reading "`action` `subject`" ("Categorise as" "Shopping").
    pub fn new(
        agent: impl Into<SharedString>,
        context: impl Into<SharedString>,
        action: impl Into<SharedString>,
        subject: impl Into<SharedString>,
    ) -> Self {
        Self {
            agent: agent.into(),
            context: context.into(),
            action: action.into(),
            subject: subject.into(),
            reason: None,
            on_approve: None,
            on_reject: None,
        }
    }

    /// The agent's reason, under the proposal.
    pub fn reason(mut self, reason: impl Into<SharedString>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    pub fn on_approve(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_approve = Some(Box::new(handler));
        self
    }

    pub fn on_reject(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_reject = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for AgentSuggestion {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let header = div()
            .flex()
            .items_center()
            .gap(spacing::S2)
            .w_full()
            .child(Glyph::Sparkle.render().text_color(color::MOLDAVITE))
            .child(
                div()
                    .flex_none()
                    .text_role(typography::BODY)
                    .font_weight(weight::STRONG)
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
                    .child(self.context),
            );

        let proposal_text = format!("{} {}", self.action, self.subject);
        let subject_start = proposal_text.len() - self.subject.len();
        let proposal = StyledText::new(proposal_text.clone()).with_highlights([(
            subject_start..proposal_text.len(),
            HighlightStyle {
                color: Some(color::MOLDAVITE_TEXT.into()),
                ..HighlightStyle::default()
            },
        )]);
        let proposal = div()
            .flex()
            .flex_col()
            .gap(spacing::S0_5)
            .child(
                div()
                    .text_role(typography::BODY)
                    .text_color(color::TEXT_STRONG)
                    .child(proposal),
            )
            .children(self.reason.map(|reason| {
                div()
                    .text_role(typography::CAPTION)
                    .text_color(color::MOLDAVITE_TEXT_MUTED)
                    .child(reason)
            }));

        let mut approve = Button::new("suggestion-approve", "Approve")
            .with_variant(ButtonVariant::AgentPrimary)
            .with_full_width(true)
            .with_surface(color::MOLDAVITE_SURFACE);
        if let Some(handler) = self.on_approve {
            approve = approve.on_click(handler);
        }
        let mut reject = Button::new("suggestion-reject", "Reject")
            .with_variant(ButtonVariant::AgentOutline)
            .with_surface(color::MOLDAVITE_SURFACE);
        if let Some(handler) = self.on_reject {
            reject = reject.on_click(handler);
        }

        div()
            .flex()
            .flex_col()
            .gap(spacing::S2_5)
            .p(spacing::S3)
            .rounded(radius::LG)
            .bg(color::MOLDAVITE_SURFACE)
            .border(size::HAIRLINE)
            .border_color(color::MOLDAVITE_BORDER)
            .child(header)
            .child(proposal)
            .child(
                div()
                    .flex()
                    .gap(spacing::S1_5)
                    .w_full()
                    .child(approve)
                    .child(reject),
            )
    }
}
