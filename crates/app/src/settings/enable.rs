//! The Enable dialog (`docs/DESIGN.md` → "Settings" → **Enable dialog**;
//! Paper `GYO-0`), opened by "Enable…" on a client that asked to connect.
//!
//! "Enable `my-agent`?", when it asked, and Level, starting at Aggregate,
//! the least shared, with what that level shares under it. Cancel /
//! "Enable" (neutral primary). The only way a client gets enabled in the
//! app: MCP stays off until the user approves one here.
//!
//! Composed for M3.15 (decided with the user): no Name field, since clients
//! have no display name; a refusal from core is said in `danger` under the
//! level's caption.

use chrono::NaiveDateTime;
use coffer_core::mcp_client::{AccessLevel, ClientName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Context, Entity, EventEmitter, FocusHandle, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Window, div,
};

use crate::accounts::DialogClosed;
use crate::theme::tokens::{color, size, spacing, typography};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::dialog::Dialog;
use crate::ui::field::{FieldRow, SelectField, error_caption};
use crate::ui::menu::{Select, SelectState};

use super::SettingsView;
use super::clients::{level_groups, level_hint, level_label, level_reads, level_writes};
use super::table::{caption, mono, plain, rich};

/// The Enable dialog.
pub struct EnableDialog {
    settings: Entity<SettingsView>,
    name: ClientName,
    /// When it asked, local time.
    asked: Option<NaiveDateTime>,
    today: chrono::NaiveDate,
    level: AccessLevel,
    level_select: SelectState,
    focus: FocusHandle,
    error: Option<SharedString>,
}

impl EventEmitter<DialogClosed> for EnableDialog {}

impl EnableDialog {
    pub fn new(
        settings: Entity<SettingsView>,
        name: ClientName,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (asked, today) = {
            let view = settings.read(cx);
            (
                view.ai()
                    .request(&name)
                    .and_then(|request| request.last_tried),
                view.clock().today,
            )
        };
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            settings,
            name,
            asked,
            today,
            level: AccessLevel::Aggregate,
            level_select: SelectState::new(cx),
            focus,
            error: None,
        }
    }

    pub fn level(&self) -> AccessLevel {
        self.level
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    /// Chooses the level to enable at.
    pub fn choose(&mut self, level: AccessLevel, cx: &mut Context<Self>) {
        self.level = level;
        cx.notify();
    }

    /// Enables the client, or says why it can't.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        let (name, level) = (self.name.clone(), self.level);
        let result = self
            .settings
            .update(cx, |settings, cx| settings.enable(&name, level, cx));
        match result {
            Ok(()) => cx.emit(DialogClosed),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DialogClosed);
    }

    fn select(&self, cx: &mut Context<Self>) -> Select {
        let view = cx.entity().downgrade();
        let (choose, step) = (view.clone(), view.clone());
        let chosen: SharedString = self.level.as_str().into();
        let open_chosen = chosen.clone();
        let groups = level_groups();
        let moved = groups.clone();
        Select::new(
            "enable-level",
            "Level",
            SelectField::new("enable-level-field", level_label(self.level))
                .hint(level_hint(self.level)),
            groups,
            Some(chosen),
            &self.level_select,
        )
        .menu_width(size::DIALOG - spacing::S4 * 2. - size::FIELD_LABEL - spacing::S2)
        .on_toggle(move |open, window, cx| {
            let _ = view.update(cx, |this, cx| {
                this.level_select
                    .set_open(open, Some(&open_chosen), window, cx);
                cx.notify();
            });
        })
        .on_choose(move |key, window, cx| {
            let _ = choose.update(cx, |this, cx| {
                this.level_select.set_open(false, None, window, cx);
                if let Ok(level) = key.parse() {
                    this.choose(level, cx);
                }
            });
        })
        .on_move(move |delta, _, cx| {
            let _ = step.update(cx, |this, cx| {
                this.level_select.move_highlight(&moved, delta);
                cx.notify();
            });
        })
    }
}

impl Render for EnableDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = rich(
            typography::TITLE,
            color::TEXT,
            vec![plain("Enable "), mono(self.name.to_string()), plain("?")],
        )
        .whitespace_nowrap();
        let asked = match self.asked {
            Some(time) if time.date() == self.today => vec![
                plain("It asked to connect today at "),
                mono(time.format("%H:%M").to_string()),
                plain(". "),
            ],
            Some(time) => vec![
                plain(format!(
                    "It asked to connect on {} at ",
                    time.format("%-d %b")
                )),
                mono(time.format("%H:%M").to_string()),
                plain(". "),
            ],
            None => Vec::new(),
        };
        let mut intro = asked;
        intro.push(plain(
            "Choose what it can see. You can change the level or turn the client off at any time.",
        ));
        let fields = div()
            .flex()
            .flex_col()
            .gap(spacing::S2)
            .child(FieldRow::new("Level", self.select(cx)))
            .child(
                div()
                    .pl(size::FIELD_LABEL + spacing::S2)
                    .child(caption(format!(
                        "{} {}",
                        level_reads(self.level),
                        level_writes(self.level)
                    ))),
            )
            .when_some(self.error.clone(), |this, error| {
                this.child(error_caption(error))
            });
        let on_cancel = cx.entity().downgrade();
        Dialog::with_title(title, &self.focus)
            .body(rich(typography::CAPTION, color::TEXT_MUTED, intro))
            .body(fields)
            .action(
                Button::new("enable-cancel", "Cancel")
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
            )
            .action(
                Button::new("enable-confirm", "Enable")
                    .with_variant(ButtonVariant::Primary)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
            )
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| this.cancel(cx));
            })
    }
}
