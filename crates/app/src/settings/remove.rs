//! The Remove client dialog, opened by "Remove client…" in the client
//! inspector.
//!
//! Composed for M3.15 from the dialog spec (`DESIGN.md` → "Transactions
//! additions" → **Dialogs**), as a destructive confirmation: "Remove
//! `claude-code`?", what removing does and keeps (the inspector's caption,
//! `DB5-0`), Cancel / "Remove client" (destructive solid: the confirming
//! button of a destructive dialog).

use coffer_core::mcp_client::ClientName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Context, Entity, EventEmitter, FocusHandle, IntoElement, Render, SharedString, Styled as _,
    Window,
};

use crate::accounts::DialogClosed;
use crate::theme::tokens::{color, typography};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::dialog::Dialog;
use crate::ui::field::error_caption;

use super::SettingsView;
use super::table::{caption, mono, plain, rich};

/// The Remove client dialog.
pub struct RemoveDialog {
    settings: Entity<SettingsView>,
    name: ClientName,
    focus: FocusHandle,
    error: Option<SharedString>,
}

impl EventEmitter<DialogClosed> for RemoveDialog {}

impl RemoveDialog {
    pub fn new(
        settings: Entity<SettingsView>,
        name: ClientName,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            settings,
            name,
            focus,
            error: None,
        }
    }

    /// Removes the client, or says why it can't.
    pub fn confirm(&mut self, cx: &mut Context<Self>) {
        let name = self.name.clone();
        match self
            .settings
            .update(cx, |settings, cx| settings.remove(&name, cx))
        {
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
}

impl Render for RemoveDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = rich(
            typography::TITLE,
            color::TEXT,
            vec![plain("Remove "), mono(self.name.to_string()), plain("?")],
        )
        .whitespace_nowrap();
        let on_cancel = cx.entity().downgrade();
        Dialog::with_title(title, &self.focus)
            .body(caption(
                "Its next call is refused and its level is forgotten. Its log, suggestions and \
                 reviews stay. If it connects again, it's listed under Asked to connect.",
            ))
            .when_some(self.error.clone(), |this, error| {
                this.body(error_caption(error))
            })
            .action(
                Button::new("remove-cancel", "Cancel")
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
            )
            .action(
                Button::new("remove-confirm", "Remove client")
                    .with_variant(ButtonVariant::DestructiveSolid)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.confirm(cx))),
            )
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| this.cancel(cx));
            })
    }
}
