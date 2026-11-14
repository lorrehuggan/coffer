//! The lock and unlock dialogs (`docs/DESIGN.md` → "Review queues" →
//! states board; Paper `9MQ-0`), `--container-dialog` wide.
//!
//! - **Lock** "Lock Savings through 31 Oct?": what gets flagged and that
//!   nothing changes now; Through, a select of the dates core accepts
//!   (passing checks with none failing before them, after the current
//!   lock; latest first, "latest passing check"); Cancel / "Lock" (neutral
//!   primary).
//! - **Unlock** "Unlock Joint account?": what stops being flagged, that
//!   the checks keep running, lock again once they pass; Cancel /
//!   "Unlock" (neutral primary: reversible, changes no data). Unlocking
//!   clears the lock entirely.
//!
//! A refusal from core is said under the form in caption type, `danger`.

use chrono::NaiveDate;
use coffer_core::id::AccountId;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Context, Entity, EventEmitter, FocusHandle, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Window, div,
};

use crate::accounts::DialogClosed;
use crate::format;
use crate::theme::tokens::{color, size, spacing, typography};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::dialog::Dialog;
use crate::ui::field::{FieldRow, SelectField, error_caption};
use crate::ui::menu::{MenuGroup, MenuOption, Select, SelectState};
use crate::ui::text::TextRoleExt as _;

use super::Checks;
use super::text::{self, NAME_SHOWN, Part, plain, rich};

/// The lock dialog.
pub struct LockDialog {
    checks: Entity<Checks>,
    account: AccountId,
    name: String,
    /// What the select offers, latest first.
    dates: Vec<NaiveDate>,
    through: Option<NaiveDate>,
    select: SelectState,
    focus: FocusHandle,
    today: NaiveDate,
    error: Option<SharedString>,
}

impl EventEmitter<DialogClosed> for LockDialog {}

impl LockDialog {
    pub fn new(
        checks: Entity<Checks>,
        account: AccountId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let (name, dates, today) = {
            let state = checks.read(cx);
            let review = state.account(account);
            (
                review.map(|review| review.name.clone()).unwrap_or_default(),
                review
                    .map(|review| review.lockable.clone())
                    .unwrap_or_default(),
                state.today(),
            )
        };
        Self {
            checks,
            account,
            name,
            through: dates.first().copied(),
            dates,
            select: SelectState::new(cx),
            focus,
            today,
            error: None,
        }
    }

    pub fn through(&self) -> Option<NaiveDate> {
        self.through
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    pub fn choose(&mut self, through: NaiveDate, cx: &mut Context<Self>) {
        self.through = Some(through);
        cx.notify();
    }

    /// Locks the account, or says why it can't.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        let Some(through) = self.through else {
            self.error = Some("No balance check passes yet, so there's nothing to lock.".into());
            cx.notify();
            return;
        };
        let account = self.account;
        match self
            .checks
            .update(cx, |checks, cx| checks.lock(account, through, cx))
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

    fn groups(&self) -> Vec<MenuGroup> {
        vec![MenuGroup::new(
            None,
            self.dates
                .iter()
                .enumerate()
                .map(|(index, date)| {
                    let option = MenuOption::new(date.to_string(), format::field_date(*date));
                    if index == 0 {
                        option.hint("latest passing check")
                    } else {
                        option
                    }
                })
                .collect(),
        )]
    }

    fn render_select(&self, cx: &mut Context<Self>) -> Select {
        let view = cx.entity().downgrade();
        let (choose, step) = (view.clone(), view.clone());
        let chosen: Option<SharedString> = self.through.map(|date| date.to_string().into());
        let open_chosen = chosen.clone();
        let groups = self.groups();
        let moved = groups.clone();
        let label = self.through.map(format::field_date).unwrap_or_default();
        let mut field = SelectField::new("lock-through-field", label).mono();
        if self.through.is_some() && self.through == self.dates.first().copied() {
            field = field.hint("latest passing check");
        }
        Select::new(
            "lock-through",
            "Through",
            field,
            groups,
            chosen,
            &self.select,
        )
        .menu_width(size::DIALOG - spacing::S4 * 2. - size::FIELD_LABEL - spacing::S2)
        .on_toggle(move |open, window, cx| {
            let _ = view.update(cx, |this, cx| {
                this.select.set_open(open, open_chosen.as_ref(), window, cx);
                cx.notify();
            });
        })
        .on_choose(move |key, window, cx| {
            let _ = choose.update(cx, |this, cx| {
                this.select.set_open(false, None, window, cx);
                if let Ok(date) = key.parse() {
                    this.choose(date, cx);
                }
            });
        })
        .on_move(move |delta, _, cx| {
            let _ = step.update(cx, |this, cx| {
                this.select.move_highlight(&moved, delta);
                cx.notify();
            });
        })
    }
}

impl Render for LockDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let name = plain(&self.name, NAME_SHOWN);
        let date = self
            .through
            .map(|through| text::date(through, self.today))
            .unwrap_or_default();
        let title = div()
            .whitespace_nowrap()
            .text_role(typography::TITLE)
            .text_color(color::TEXT)
            .child(format!("Lock {name} through {date}?"));
        let body = div().text_role(typography::CAPTION).child(rich(
            window,
            typography::CAPTION,
            color::TEXT_MUTED,
            vec![
                Part::text(format!("Every balance check up to {date} passes. ")),
                Part::text(format!(
                    "Once locked, an import, edit, merge or undo that touches {date} or earlier \
                     is flagged and waits until you unlock. Nothing changes now."
                )),
            ],
        ));
        let fields = div()
            .flex()
            .flex_col()
            .gap(spacing::S2)
            .child(FieldRow::new("Through", self.render_select(cx)))
            .when_some(self.error.clone(), |this, error| {
                this.child(error_caption(error))
            });
        let on_cancel = cx.entity().downgrade();
        Dialog::with_title(title, &self.focus)
            .body(body)
            .body(fields)
            .action(
                Button::new("lock-cancel", "Cancel")
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
            )
            .action(
                Button::new("lock-confirm", "Lock")
                    .with_variant(ButtonVariant::Primary)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
            )
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| this.cancel(cx));
            })
    }
}

/// The unlock dialog.
pub struct UnlockDialog {
    checks: Entity<Checks>,
    account: AccountId,
    name: String,
    through: Option<NaiveDate>,
    focus: FocusHandle,
    today: NaiveDate,
    error: Option<SharedString>,
}

impl EventEmitter<DialogClosed> for UnlockDialog {}

impl UnlockDialog {
    pub fn new(
        checks: Entity<Checks>,
        account: AccountId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let (name, through, today) = {
            let state = checks.read(cx);
            let review = state.account(account);
            (
                review.map(|review| review.name.clone()).unwrap_or_default(),
                review.and_then(|review| review.locked_through),
                state.today(),
            )
        };
        Self {
            checks,
            account,
            name,
            through,
            focus,
            today,
            error: None,
        }
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    /// Unlocks the account, or says why it can't.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        let account = self.account;
        match self
            .checks
            .update(cx, |checks, cx| checks.unlock(account, cx))
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

impl Render for UnlockDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = div()
            .whitespace_nowrap()
            .text_role(typography::TITLE)
            .text_color(color::TEXT)
            .child(format!("Unlock {}?", plain(&self.name, NAME_SHOWN)));
        let mut words = Vec::new();
        if let Some(through) = self.through {
            words.push(Part::text(format!(
                "Changes dated {} or earlier won’t be flagged any more. ",
                text::date(through, self.today)
            )));
        }
        words.push(Part::text(
            "The balance checks keep running, so a change that breaks one still shows here. \
             Lock again once they pass.",
        ));
        let on_cancel = cx.entity().downgrade();
        Dialog::with_title(title, &self.focus)
            .body(div().text_role(typography::CAPTION).child(rich(
                window,
                typography::CAPTION,
                color::TEXT_MUTED,
                words,
            )))
            .when_some(self.error.clone(), |this, error| {
                this.body(error_caption(error))
            })
            .action(
                Button::new("unlock-cancel", "Cancel")
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
            )
            .action(
                Button::new("unlock-confirm", "Unlock")
                    .with_variant(ButtonVariant::Primary)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
            )
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| this.cancel(cx));
            })
    }
}
