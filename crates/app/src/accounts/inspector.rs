//! The account inspector (`docs/DESIGN.md` → "Accounts panel" → **Account
//! inspector**; Paper `8GJ-0`): shown on Transactions while an account is
//! selected and no transaction is.
//!
//! - **Header**: the name, the latest balance as a display figure, and when
//!   it's from ("Balance on 9 Nov · pots not included").
//! - **Details**: Name and Bank (fields; saved on Enter or when the field
//!   loses focus), Kind (select), Currency (a select until the account holds
//!   money, then read-only with a lock), Preset (select, used by the next
//!   import; only for accounts imported from a bank).
//! - **Balances**: the balances the user entered, newest first, with "Add
//!   balance…" and a caption that each is checked against the transactions.
//! - **Pots** with their balances and total, and **Imports** (file, rows,
//!   date), each only when there are any.
//!
//! "Close account…" comes with M3.17 (decided with the user, 2026-11-14).
//!
//! Composed for M3.2 (not drawn): a kind the account can't change to stays
//! in the Kind menu, faded, with the reason as its hint; a pot shows Kind
//! read-only ("Pot", with the lock) and no Bank or Preset; a cash account
//! has no Bank or Preset (as D6.9's cash inspector); an account with no
//! balance has no figure, and its caption says so; a valuation account
//! lists its values under "Valuations", read-only until M3.17; a change
//! core refuses is put back and the reason shown under Details in `danger`.

use coffer_core::account::{AccountEdit, AccountKind, StoredBalance, Usage};
use coffer_core::id::AccountId;
use coffer_core::money::Currency;
use coffer_core::reconciliation::BalanceSource;
use coffer_core::reporting::account_tree::AccountNode;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Subscription, Window, div,
};

use crate::format::{Sign, list_date, money};
use crate::theme::tokens::{color, size, spacing, typography};
use crate::ui::button::Button;
use crate::ui::field::{FieldRow, LockedValue, SelectField, date_lane, error_caption, text_input};
use crate::ui::icon::Glyph;
use crate::ui::inspector::{caption, header, section, section_title_row};
use crate::ui::menu::{MenuGroup, MenuOption, Select, SelectState};
use crate::ui::text::TextRoleExt as _;

use super::Accounts;
use super::data::AccountsData;

/// The kinds the Kind menu offers, in its groups (Paper `FHZ-0`).
const IMPORTED: [AccountKind; 3] = [
    AccountKind::Current,
    AccountKind::Savings,
    AccountKind::CreditCard,
];
const KEPT: [AccountKind; 3] = [
    AccountKind::Cash,
    AccountKind::Asset,
    AccountKind::Liability,
];

/// The currencies listed first in the Currency menu.
const COMMON_CURRENCIES: [Currency; 3] = [Currency::GBP, Currency::EUR, Currency::USD];

/// What a kind is called.
pub fn kind_label(kind: AccountKind) -> &'static str {
    match kind {
        AccountKind::Current => "Current",
        AccountKind::Savings => "Savings",
        AccountKind::CreditCard => "Credit card",
        AccountKind::Pot => "Pot",
        AccountKind::Cash => "Cash",
        AccountKind::Asset => "Asset",
        AccountKind::Liability => "Liability",
    }
}

/// What a kept kind is for, after its name in the Kind menu.
fn kind_hint(kind: AccountKind) -> Option<&'static str> {
    match kind {
        AccountKind::Cash => Some("notes and coins"),
        AccountKind::Asset => Some("house, pension, investments"),
        AccountKind::Liability => Some("mortgage, loan"),
        _ => None,
    }
}

/// The Kind menu. `refused` says why a kind can't be chosen, if it can't.
pub fn kind_groups(refused: impl Fn(AccountKind) -> Option<&'static str>) -> Vec<MenuGroup> {
    let option = |kind: AccountKind| {
        let option =
            MenuOption::new(kind.as_str(), kind_label(kind)).icon(super::panel::glyph(kind));
        match refused(kind) {
            Some(reason) => option.hint(reason).disabled(true),
            None => match kind_hint(kind) {
                Some(hint) => option.hint(hint),
                None => option,
            },
        }
    };
    vec![
        MenuGroup::new(
            Some("Imported from your bank"),
            IMPORTED.map(option).to_vec(),
        ),
        MenuGroup::new(Some("Kept by you"), KEPT.map(option).to_vec()),
    ]
}

/// The Currency menu: the common ones, then every other ISO 4217 code.
pub fn currency_groups() -> Vec<MenuGroup> {
    let option = |currency: Currency| {
        MenuOption::new(currency.code().to_owned(), currency.code().to_owned())
    };
    let mut others: Vec<Currency> = Currency::all()
        .filter(|currency| !COMMON_CURRENCIES.contains(currency))
        .collect();
    others.sort_by(|a, b| a.code().cmp(b.code()));
    vec![
        MenuGroup::new(Some("Common"), COMMON_CURRENCIES.map(option).to_vec()),
        MenuGroup::new(
            Some("Every currency"),
            others.into_iter().map(option).collect(),
        ),
    ]
}

/// The key of "No preset" in the Preset menu.
const NO_PRESET: &str = "none";

fn preset_groups(data: &AccountsData) -> Vec<MenuGroup> {
    let mut options = vec![MenuOption::new(NO_PRESET, "No preset").hint("detect it at import")];
    options.extend(
        data.presets
            .iter()
            .map(|preset| MenuOption::new(preset.id.clone(), preset.name.clone())),
    );
    vec![MenuGroup::new(None, options)]
}

/// What a balance row says it is: the user's note, else where it came from.
fn balance_label(balance: &StoredBalance) -> String {
    if let Some(note) = &balance.note {
        return note.clone();
    }
    match balance.source {
        BalanceSource::Opening => "Opening balance",
        BalanceSource::Statement => "Statement",
        BalanceSource::Valuation => "Valuation",
        BalanceSource::Manual | BalanceSource::Ofx | BalanceSource::Csv => "Entered by you",
    }
    .to_owned()
}

/// Which of the inspector's selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Which {
    Kind,
    Currency,
    Preset,
}

/// The account inspector. It holds the Name and Bank fields and the
/// selects' state; the account itself is [`Accounts`]'.
pub struct AccountInspector {
    accounts: Entity<Accounts>,
    /// The account the fields were last filled from.
    shown: Option<AccountId>,
    name: Entity<InputState>,
    bank: Entity<InputState>,
    kind: SelectState,
    currency: SelectState,
    preset: SelectState,
    /// Why the last change wasn't saved.
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl AccountInspector {
    pub fn new(accounts: Entity<Accounts>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Account name"));
        let bank = cx.new(|cx| InputState::new(window, cx).placeholder("Bank"));
        let commit = |this: &mut Self,
                      _: &Entity<InputState>,
                      event: &InputEvent,
                      window: &mut Window,
                      cx: &mut Context<Self>| {
            if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                this.commit_text(window, cx);
            }
        };
        let subscriptions = vec![
            cx.subscribe_in(&name, window, commit),
            cx.subscribe_in(&bank, window, commit),
            cx.observe_in(&accounts, window, |this, _, window, cx| {
                this.sync(window, cx)
            }),
        ];
        let mut inspector = Self {
            accounts,
            shown: None,
            name,
            bank,
            kind: SelectState::new(cx),
            currency: SelectState::new(cx),
            preset: SelectState::new(cx),
            error: None,
            _subscriptions: subscriptions,
        };
        inspector.sync(window, cx);
        inspector
    }

    /// Fills the fields from the selected account when the selection
    /// changes.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.accounts.read(cx).selected();
        if selected == self.shown {
            return;
        }
        // Keep what was typed for the account being left.
        self.commit_text(window, cx);
        self.shown = selected;
        self.error = None;
        for which in [Which::Kind, Which::Currency, Which::Preset] {
            self.state_mut(which).dismiss();
        }
        self.reset_fields(window, cx);
        cx.notify();
    }

    /// Puts the stored name and bank of the shown account back in their
    /// fields.
    fn reset_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = self
            .shown
            .and_then(|id| self.accounts.read(cx).data().node(id));
        let (name, bank) = match shown {
            Some(node) => (
                node.account.name.clone(),
                node.account.institution.clone().unwrap_or_default(),
            ),
            None => (String::new(), String::new()),
        };
        self.name.update(cx, |input, cx| {
            input.set_value(SharedString::from(name), window, cx)
        });
        self.bank.update(cx, |input, cx| {
            input.set_value(SharedString::from(bank), window, cx)
        });
    }

    fn state_mut(&mut self, which: Which) -> &mut SelectState {
        match which {
            Which::Kind => &mut self.kind,
            Which::Currency => &mut self.currency,
            Which::Preset => &mut self.preset,
        }
    }

    /// The selected account's edit as stored, if one is selected.
    fn stored_edit(&self, cx: &App) -> Option<(AccountId, AccountEdit)> {
        let node = self.accounts.read(cx).selected_node()?;
        Some((node.account.id, AccountEdit::of(&node.account)))
    }

    /// Saves the Name and Bank fields if they changed, to the account they
    /// were filled from: a field left by clicking another account blurs
    /// after the selection has moved.
    fn commit_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.shown else {
            return;
        };
        let Some(stored) = self
            .accounts
            .read(cx)
            .data()
            .node(id)
            .map(|node| AccountEdit::of(&node.account))
        else {
            return;
        };
        let name = self.name.read(cx).value().trim().to_owned();
        let bank = self.bank.read(cx).value().trim().to_owned();
        let edit = AccountEdit {
            name,
            institution: (!bank.is_empty()).then_some(bank),
            ..stored.clone()
        };
        if edit.name == stored.name && edit.institution == stored.institution {
            return;
        }
        self.apply(id, edit, window, cx);
    }

    /// Saves `edit`; on a refusal, puts the fields back and says why.
    fn apply(
        &mut self,
        id: AccountId,
        edit: AccountEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self
            .accounts
            .update(cx, |accounts, cx| accounts.update(id, &edit, cx));
        match result {
            Ok(()) => self.error = None,
            Err(error) => {
                self.error = Some(error);
                self.reset_fields(window, cx);
            }
        }
        cx.notify();
    }

    /// Opens the Kind menu, as clicking it does.
    pub fn open_kind_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle(Which::Kind, true, window, cx);
    }

    fn toggle(&mut self, which: Which, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = self.chosen(which, cx);
        self.state_mut(which)
            .set_open(open, chosen.as_ref(), window, cx);
        cx.notify();
    }

    fn chosen(&self, which: Which, cx: &App) -> Option<SharedString> {
        let (_, edit) = self.stored_edit(cx)?;
        Some(match which {
            Which::Kind => edit.kind.as_str().into(),
            Which::Currency => edit.currency.code().to_owned().into(),
            Which::Preset => edit.preset.map_or(NO_PRESET.into(), SharedString::from),
        })
    }

    fn choose(
        &mut self,
        which: Which,
        key: &SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state_mut(which).set_open(false, None, window, cx);
        let Some((id, stored)) = self.stored_edit(cx) else {
            return;
        };
        let mut edit = stored.clone();
        match which {
            Which::Kind => match AccountKind::from_key(key) {
                Some(kind) => edit.kind = kind,
                None => return,
            },
            Which::Currency => match Currency::new(key) {
                Ok(currency) => edit.currency = currency,
                Err(_) => return,
            },
            Which::Preset => edit.preset = (key.as_ref() != NO_PRESET).then(|| key.to_string()),
        }
        if edit != stored {
            self.apply(id, edit, window, cx);
        }
        cx.notify();
    }

    fn move_highlight(
        &mut self,
        which: Which,
        step: isize,
        groups: &[MenuGroup],
        cx: &mut Context<Self>,
    ) {
        self.state_mut(which).move_highlight(groups, step);
        cx.notify();
    }

    /// A select wired to this inspector.
    fn select(
        &self,
        which: Which,
        label: &'static str,
        value: String,
        groups: Vec<MenuGroup>,
        cx: &mut Context<Self>,
    ) -> Select {
        let state = match which {
            Which::Kind => &self.kind,
            Which::Currency => &self.currency,
            Which::Preset => &self.preset,
        };
        let id = match which {
            Which::Kind => "inspector-kind",
            Which::Currency => "inspector-currency",
            Which::Preset => "inspector-preset",
        };
        let chosen = self.chosen(which, cx);
        let toggle = cx.entity().downgrade();
        let choose = toggle.clone();
        let step = toggle.clone();
        let moved_groups = groups.clone();
        Select::new(
            id,
            label,
            SelectField::new(format!("{id}-field"), value),
            groups,
            chosen,
            state,
        )
        .when(which == Which::Currency, |this| {
            this.max_height(size::CONTROL * 8.)
        })
        .on_toggle(move |open, window, cx| {
            let _ = toggle.update(cx, |this, cx| this.toggle(which, open, window, cx));
        })
        .on_choose(move |key, window, cx| {
            let _ = choose.update(cx, |this, cx| this.choose(which, key, window, cx));
        })
        .on_move(move |delta, _, cx| {
            let _ = step.update(cx, |this, cx| {
                this.move_highlight(which, delta, &moved_groups, cx)
            });
        })
    }

    fn render_header(&self, node: &AccountNode, data: &AccountsData) -> AnyElement {
        let figure = node
            .latest
            .map(|latest| money(latest.balance, Sign::Negative));
        let when = match node.latest {
            Some(latest) => {
                let verb = match node.account.kind {
                    AccountKind::Asset => "Valued on ",
                    AccountKind::Liability => "Owed on ",
                    _ => "Balance on ",
                };
                let mut line = caption(verb).child(
                    div()
                        .text_role(typography::MONO_VALUE)
                        .child(list_date(latest.date, data.today)),
                );
                if !node.pots.is_empty() {
                    line = line.child(" · pots not included");
                } else if let Some(parent) = data.parent_of(node.account.id) {
                    line = line.child(format!(" · in {}", parent.account.name));
                }
                line
            }
            None => caption("No balance yet: add one below, or import a file that has balances."),
        };
        header(
            node.account.name.clone(),
            figure,
            Some(when.into_any_element()),
        )
        .into_any_element()
    }

    fn render_details(
        &self,
        node: &AccountNode,
        usage: &Usage,
        data: &AccountsData,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let kind = node.account.kind;
        let has_bank = kind.is_imported();
        let kind_control = if kind == AccountKind::Pot {
            LockedValue::new(kind_label(kind)).into_any_element()
        } else {
            let from = kind;
            let usage = *usage;
            self.select(
                Which::Kind,
                "Kind",
                kind_label(kind).to_owned(),
                kind_groups(|to| {
                    usage
                        .kind_change_refused(from, to)
                        .map(|lock| lock.reason())
                }),
                cx,
            )
            .into_any_element()
        };
        let currency_control = if usage.currency_locked() {
            LockedValue::new(node.account.currency.code().to_owned())
                .mono(true)
                .into_any_element()
        } else {
            self.select(
                Which::Currency,
                "Currency",
                node.account.currency.code().to_owned(),
                currency_groups(),
                cx,
            )
            .into_any_element()
        };
        let preset_value = node
            .account
            .preset
            .as_deref()
            .map_or("No preset", |id| data.preset_name(id))
            .to_owned();
        section(true)
            .gap(spacing::S2)
            .child(FieldRow::new(
                "Name",
                text_input(&self.name).id("account-name"),
            ))
            .when(has_bank, |this| {
                this.child(FieldRow::new(
                    "Bank",
                    text_input(&self.bank).id("account-bank"),
                ))
            })
            .child(FieldRow::new("Kind", kind_control))
            .child(FieldRow::new("Currency", currency_control))
            .when(kind.is_imported(), |this| {
                this.child(FieldRow::new(
                    "Preset",
                    self.select(
                        Which::Preset,
                        "Preset",
                        preset_value,
                        preset_groups(data),
                        cx,
                    ),
                ))
            })
            .when_some(self.error.clone(), |this, error| {
                this.child(error_caption(error))
            })
            .into_any_element()
    }

    fn render_balances(
        &self,
        node: &AccountNode,
        balances: &[StoredBalance],
        data: &AccountsData,
    ) -> AnyElement {
        let valuations = !node.account.kind.holds_transactions();
        let with_year = balances
            .iter()
            .any(|balance| list_date(balance.date, data.today).len() > 6);
        let rows = balances.iter().map(|balance| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(spacing::S2)
                .h(size::CONTROL)
                .child(
                    div()
                        .flex_none()
                        .w(date_lane(with_year))
                        .text_role(typography::MONO_VALUE)
                        .text_color(color::TEXT_MUTED)
                        .child(list_date(balance.date, data.today)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_role(typography::BODY)
                        .text_color(color::TEXT_SECONDARY)
                        .child(balance_label(balance)),
                )
                .child(
                    div()
                        .flex_none()
                        .text_role(typography::MONO_AMOUNT)
                        .text_color(color::TEXT)
                        .child(money(balance.balance, Sign::Negative)),
                )
        });
        let id = node.account.id;
        let accounts = self.accounts.clone();
        let add = (!valuations).then(|| {
            Button::new("add-balance", "Add balance…")
                .on_click(move |_, _, cx| {
                    accounts.update(cx, |accounts, cx| accounts.request_add_balance(id, cx))
                })
                .into_any_element()
        });
        let monzo = node.account.preset.as_deref() == Some("uk-monzo");
        let note = if valuations {
            None
        } else if monzo {
            Some(
                "Monzo exports carry no balances, so Coffer counts forward from the ones you enter. Each one is checked against the transactions.",
            )
        } else {
            Some(
                "Coffer counts forward from the balances you enter and the ones in your imports. Each one is checked against the transactions.",
            )
        };
        section(true)
            .gap(spacing::S1)
            .child(section_title_row(
                if valuations { "Valuations" } else { "Balances" },
                add,
            ))
            .children(rows)
            .when_some(note, |this, note| {
                this.child(crate::ui::field::caption(note))
            })
            .into_any_element()
    }

    fn render_pots(&self, node: &AccountNode) -> Option<AnyElement> {
        if node.pots.is_empty() {
            return None;
        }
        let total = node.pots_total().ok().flatten().map(|total| {
            div()
                .text_role(typography::MONO_VALUE)
                .text_color(color::TEXT_MUTED)
                .child(money(total, Sign::Negative))
                .into_any_element()
        });
        let rows = node.pots.iter().map(|pot| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(spacing::S2)
                .h(size::CONTROL)
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .size(size::ICON_LG)
                        .child(Glyph::Pot.render().text_color(color::TEXT_MUTED)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_role(typography::BODY)
                        .text_color(color::TEXT_SECONDARY)
                        .child(pot.account.name.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_role(typography::MONO_AMOUNT)
                        .text_color(color::TEXT)
                        .child(pot.latest.map_or_else(
                            || "No balance".to_owned(),
                            |latest| money(latest.balance, Sign::Negative),
                        )),
                )
        });
        Some(
            section(true)
                .gap(spacing::S1)
                .child(section_title_row("Pots", total))
                .children(rows)
                .child(crate::ui::field::caption(
                    "Created from pot transfers in your imports. They count toward net worth.",
                ))
                .into_any_element(),
        )
    }

    fn render_imports(
        &self,
        imports: &[coffer_core::import::BatchInfo],
        data: &AccountsData,
    ) -> Option<AnyElement> {
        if imports.is_empty() {
            return None;
        }
        let rows = imports.iter().map(|batch| {
            let day = chrono::NaiveDate::parse_from_str(
                batch.imported_at.get(..10).unwrap_or(""),
                "%Y-%m-%d",
            )
            .map(|date| list_date(date, data.today))
            .unwrap_or_default();
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(spacing::S2)
                .h(size::CONTROL)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_role(typography::BODY)
                        .text_color(color::TEXT_SECONDARY)
                        .child(batch.source_file.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_role(typography::MONO_VALUE)
                        .text_color(color::TEXT_MUTED)
                        .child(format!("{} · {day}", batch.transactions)),
                )
        });
        Some(
            section(true)
                .gap(spacing::S1)
                .child(section_title_row("Imports", None))
                .children(rows)
                .into_any_element(),
        )
    }
}

impl Render for AccountInspector {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accounts = self.accounts.read(cx);
        let Some(node) = accounts.selected_node().cloned() else {
            return div().into_any_element();
        };
        let data = accounts.data().clone();
        let detail = data.detail(node.account.id).cloned().unwrap_or_default();
        let mut sections = vec![
            self.render_header(&node, &data),
            self.render_details(&node, &detail.usage, &data, cx),
            self.render_balances(&node, &detail.balances, &data),
        ];
        sections.extend(self.render_pots(&node));
        sections.extend(self.render_imports(&detail.imports, &data));
        div()
            .flex()
            .flex_col()
            .text_role(typography::BODY)
            .children(sections)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kind_menu_follows_paper_and_fades_refused_kinds() {
        let groups = kind_groups(|kind| (kind == AccountKind::Asset).then_some("has transactions"));
        let labels: Vec<Vec<String>> = groups
            .iter()
            .map(|group| {
                group
                    .options
                    .iter()
                    .map(|option| {
                        format!(
                            "{}{}{}",
                            option.label,
                            option
                                .hint
                                .as_ref()
                                .map(|hint| format!(" · {hint}"))
                                .unwrap_or_default(),
                            if option.disabled { " (off)" } else { "" }
                        )
                    })
                    .collect()
            })
            .collect();
        assert_eq!(
            labels,
            [
                vec!["Current", "Savings", "Credit card"],
                vec![
                    "Cash · notes and coins",
                    "Asset · has transactions (off)",
                    "Liability · mortgage, loan"
                ],
            ]
        );
        assert_eq!(groups[0].label.as_deref(), Some("Imported from your bank"));
        assert_eq!(groups[1].label.as_deref(), Some("Kept by you"));
    }

    #[test]
    fn the_currency_menu_starts_with_the_common_ones() {
        let groups = currency_groups();
        let first: Vec<_> = groups[0]
            .options
            .iter()
            .map(|o| o.key.to_string())
            .collect();
        assert_eq!(first, ["GBP", "EUR", "USD"]);
        let rest = &groups[1].options;
        assert!(rest.iter().all(|o| !first.contains(&o.key.to_string())));
        assert!(rest.windows(2).all(|pair| pair[0].key < pair[1].key));
        assert_eq!(rest.len() + 3, Currency::all().len());
    }
}
