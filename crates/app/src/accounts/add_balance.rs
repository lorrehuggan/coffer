//! The Add balance dialog for a bank or cash account (Paper `I7C-0` on the
//! Accounts panel · states board `8D8-0`; drawn for M3.2 and approved by
//! the user, 2026-11-14), opened by "Add balance…" in the account inspector.
//!
//! Adapted from D6.9's "Add a balance for Mortgage": Balance (the currency's
//! symbol, mono, signed: a minus when overdrawn) with its date in a 96px
//! field, Note, a caption, then Cancel and "Add balance" (neutral primary).
//! It records a `manual` entered balance ([`coffer_core::account::add_balance`]):
//! one per account and day, after the account's lock date, checked against
//! the transactions like any other. A refusal is said in `danger` under the
//! caption.

use chrono::NaiveDate;
use coffer_core::account::{EnteredBalance, EnteredSource};
use coffer_core::id::AccountId;
use coffer_core::money::Currency;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
};

use crate::format::{currency_prefix, field_date};
use crate::theme::tokens::{color, spacing};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::dialog::Dialog;
use crate::ui::field::{FieldRow, amount_input, caption, date_input, error_caption, text_input};

use super::new_account::{read_balance, sentence};
use super::{Accounts, DialogClosed};

/// The dialog's caption (Paper `I7I-0`).
const CAPTION: &str = "Your balance at the end of that day, as your bank shows it. Type a minus if the account is overdrawn. Coffer checks it against the transactions since the last balance.";

/// Add balance, for one account.
pub struct AddBalanceDialog {
    accounts: Entity<Accounts>,
    account: AccountId,
    title: SharedString,
    currency: Currency,
    focus: FocusHandle,
    amount: Entity<InputState>,
    date: Entity<InputState>,
    note: Entity<InputState>,
    error: Option<SharedString>,
    today: NaiveDate,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DialogClosed> for AddBalanceDialog {}

impl AddBalanceDialog {
    /// The dialog for `account`, which must be open.
    pub fn new(
        accounts: Entity<Accounts>,
        account: AccountId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let data = accounts.read(cx).data();
        let today = data.today;
        let (title, currency) = data.node(account).map_or_else(
            || ("Add a balance".into(), Currency::GBP),
            |node| {
                (
                    format!("Add a balance for {}", node.account.name).into(),
                    node.account.currency,
                )
            },
        );
        let amount = cx.new(|cx| InputState::new(window, cx));
        let date = cx.new(|cx| InputState::new(window, cx).default_value(field_date(today)));
        let note = cx.new(|cx| InputState::new(window, cx).placeholder("Optional"));
        let submit_on_enter = |this: &mut Self,
                               _: &Entity<InputState>,
                               event: &InputEvent,
                               window: &mut Window,
                               cx: &mut Context<Self>| {
            if let InputEvent::PressEnter { .. } = event {
                this.submit(window, cx);
            }
        };
        let subscriptions = [&amount, &date, &note]
            .into_iter()
            .map(|input| cx.subscribe_in(input, window, submit_on_enter))
            .collect();
        amount.read(cx).focus_handle(cx).focus(window, cx);
        Self {
            accounts,
            account,
            title,
            currency,
            focus: cx.focus_handle(),
            amount,
            date,
            note,
            error: None,
            today,
            _subscriptions: subscriptions,
        }
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    /// Records the balance from the fields, or says why it can't.
    pub fn submit(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let value =
            |input: &Entity<InputState>, cx: &Context<Self>| input.read(cx).value().to_string();
        let amount = value(&self.amount, cx);
        let read = if amount.trim().is_empty() {
            Err("Type the balance, as 1,284.10.".into())
        } else {
            read_balance(&amount, &value(&self.date, cx), self.currency, self.today)
        };
        let (date, balance) = match read {
            Ok(Some(read)) => read,
            Ok(None) => return,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return;
            }
        };
        let note = value(&self.note, cx).trim().to_owned();
        let entered = EnteredBalance {
            date,
            balance,
            source: EnteredSource::Manual,
            note: (!note.is_empty()).then_some(note),
        };
        let account = self.account;
        let result = self.accounts.update(cx, |accounts, cx| {
            accounts.add_balance(account, &entered, cx)
        });
        match result {
            Ok(()) => cx.emit(DialogClosed),
            Err(error) => {
                self.error = Some(sentence(&error));
                cx.notify();
            }
        }
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DialogClosed);
    }
}

impl Render for AddBalanceDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fields = div()
            .flex()
            .flex_col()
            .gap(spacing::S2)
            .child(FieldRow::new(
                "Balance",
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .gap(spacing::S2)
                    .child(
                        amount_input(&self.amount, currency_prefix(self.currency), cx)
                            .id("add-balance-amount")
                            .flex_1(),
                    )
                    .child(date_input(&self.date).id("add-balance-date")),
            ))
            .child(FieldRow::new(
                "Note",
                text_input(&self.note).id("add-balance-note"),
            ));
        let cancel = cx.listener(|this, _, _, cx| this.cancel(cx));
        let add = cx.listener(|this, _, window, cx| this.submit(window, cx));
        let on_cancel = cx.entity().downgrade();
        Dialog::new(self.title.clone(), &self.focus)
            .body(fields)
            .body(caption(CAPTION))
            .when_some(self.error.clone(), |this, error| {
                this.body(error_caption(error))
            })
            .action(
                Button::new("add-balance-cancel", "Cancel")
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(color::FLOAT)
                    .on_click(cancel),
            )
            .action(
                Button::new("add-balance-add", "Add balance")
                    .with_variant(ButtonVariant::Primary)
                    .with_surface(color::FLOAT)
                    .on_click(add),
            )
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| this.cancel(cx));
            })
    }
}
