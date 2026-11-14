//! The accounts panel (`docs/DESIGN.md` → "Finance screens" → "Accounts
//! panel"; Paper `19T-0`, states board `8D8-0`): net worth, the accounts
//! with their pots nested under them, and the categories. The left panel of
//! every view that filters by account (Overview, Transactions, Trends,
//! Budgets, Net worth).
//!
//! A row shows the account's latest balance: `danger` when an account that
//! can be overdrawn is below zero, a 6px `danger` dot before it when a
//! balance check failed, and "No balance" until one is entered. Clicking a
//! row selects the account, and clicking it again goes back to all
//! accounts. The `+` opens New account. The panel is a Tab stop: there the
//! arrow keys move the selection through the rows, in panel order, and
//! Escape goes back to all accounts; the selected row draws the inset focus
//! ring while the panel has keyboard focus.
//!
//! Composed for M3.2 (not drawn): with no accounts, the Accounts section
//! says so in a `text-muted` row, and net worth reads "—"; accounts in more
//! than one currency show one net worth line per currency, since nothing is
//! converted until exchange rates arrive (M4.6); asset and liability
//! accounts (made with the CLI until M3.17) are listed under ASSETS AND
//! LIABILITIES (D6.9), without its `+` until then.
//!
//! The categories are still the Transactions fixture's, with its counts,
//! until the table reads the database and filters by them (M3.3).

use coffer_core::account::AccountKind;
use coffer_core::id::AccountId;
use coffer_core::reporting::account_tree::AccountNode;
use gpui_kit::base::actions::{Cancel, SelectDown, SelectUp};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Entity, FocusHandle, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
};

use crate::format::{Sign, money};
use crate::theme::tokens::typography::TextRole;
use crate::theme::tokens::{color, font, size, spacing, typography};
use crate::transactions::fixture::{self, Category};
use crate::ui::button::IconButton;
use crate::ui::icon::Glyph;
use crate::ui::list_row::{ListRow, TrailingTone};
use crate::ui::marker::Marker;
use crate::ui::section_label::SectionLabelRow;
use crate::ui::text::TextRoleExt as _;

use super::Accounts;
use super::data::AccountsData;

/// The panel's key context, where the arrow keys move the selection.
pub const CONTEXT: &str = "CofferAccountsPanel";

/// Every account in the panel's order: accounts, each followed by its pots,
/// then the assets and liabilities.
pub fn order(data: &AccountsData) -> Vec<AccountId> {
    let (valued, kept): (Vec<&AccountNode>, Vec<&AccountNode>) = data
        .tree
        .accounts
        .iter()
        .partition(|node| !node.account.kind.holds_transactions());
    kept.into_iter()
        .flat_map(|node| std::iter::once(node).chain(&node.pots))
        .chain(valued)
        .map(|node| node.account.id)
        .collect()
}

/// The net worth figure: Geist Mono 13/16 600.
const NET_WORTH: TextRole = TextRole {
    family: font::MONO,
    ..typography::CARD_TITLE
};

/// The element id of the Accounts section's `+`.
pub const ADD_ACCOUNT: &str = "add-account";

/// The icon for an account of `kind`.
pub fn glyph(kind: AccountKind) -> Glyph {
    match kind {
        AccountKind::Pot => Glyph::Pot,
        AccountKind::Cash => Glyph::Cash,
        AccountKind::Asset => Glyph::Asset,
        AccountKind::Liability => Glyph::Liability,
        AccountKind::Current | AccountKind::Savings | AccountKind::CreditCard => Glyph::Account,
    }
}

/// The element id of an account's row.
pub fn row_id(node: &AccountNode) -> SharedString {
    format!("account-{}", node.account.id).into()
}

/// A list under a section label: 6px inline, 4px top and 10px bottom
/// padding.
fn list_section(label: SectionLabelRow) -> gpui_kit::Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .pt(spacing::S1)
        .px(spacing::S1_5)
        .pb(spacing::S2_5)
        .child(label)
}

/// A row standing for no rows: 28px, 8px inline padding, `text-muted`.
fn note_row(text: impl Into<SharedString>) -> gpui_kit::Div {
    div()
        .flex()
        .items_center()
        .min_h(size::CONTROL)
        .px(spacing::S2)
        .text_role(typography::BODY)
        .text_color(color::TEXT_MUTED)
        .child(text.into())
}

fn account_row(
    node: &AccountNode,
    nested: bool,
    accounts: &Entity<Accounts>,
    selected: bool,
    ring: bool,
    focus: &FocusHandle,
) -> ListRow {
    let id = node.account.id;
    let accounts = accounts.clone();
    let focus = focus.clone();
    let flagged = accounts.clone();
    let row = ListRow::new(row_id(node), node.account.name.clone())
        .icon(glyph(node.account.kind))
        .nested(nested)
        .selected(selected)
        .focus_visible(selected && ring)
        .flagged(node.status == coffer_core::reconciliation::Status::Mismatch)
        .on_flag_click(move |_, _, cx| {
            flagged.update(cx, |accounts, cx| accounts.request_checks(id, cx))
        })
        .on_click(move |_, window, cx| {
            // The panel takes focus, so the arrow keys go on from here.
            focus.focus(window, cx);
            accounts.update(cx, |accounts, cx| accounts.toggle(id, cx))
        });
    match node.latest {
        Some(latest) => row
            .trailing(money(latest.balance, Sign::Negative))
            .trailing_tone(if node.is_overdrawn() {
                TrailingTone::Danger
            } else {
                TrailingTone::Muted
            }),
        None => row.trailing_words("No balance"),
    }
}

/// The panel, 272px, `chrome` with a `hairline` on its canvas side.
/// `focus` is the panel's own handle, kept by its owner.
pub fn render(
    accounts: &Entity<Accounts>,
    focus: &FocusHandle,
    window: &Window,
    cx: &App,
) -> impl IntoElement {
    let state = accounts.read(cx);
    let data = state.data();
    let selected = state.selected();
    let ring = focus.is_focused(window) && window.last_input_was_keyboard();

    let totals: Vec<String> = data
        .tree
        .net_worth
        .iter()
        .map(|total| money(total, Sign::Negative))
        .collect();
    let net_worth = div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(spacing::S0_5)
        .pt(spacing::S3)
        .px(spacing::S3_5)
        .pb(spacing::S2)
        .child(
            div()
                .text_role(typography::CAPTION)
                .text_color(color::TEXT_MUTED)
                .child("Net worth"),
        )
        .map(|this| {
            if totals.is_empty() {
                this.child(
                    div()
                        .text_role(NET_WORTH)
                        .text_color(color::TEXT)
                        .child("—"),
                )
            } else {
                this.children(totals.into_iter().map(|total| {
                    div()
                        .text_role(NET_WORTH)
                        .text_color(color::TEXT)
                        .child(total)
                }))
            }
        });

    let (valued, kept): (Vec<&AccountNode>, Vec<&AccountNode>) = data
        .tree
        .accounts
        .iter()
        .partition(|node| !node.account.kind.holds_transactions());
    let add = {
        let accounts = accounts.clone();
        IconButton::new(ADD_ACCOUNT, Glyph::Plus, "Add account").on_click(move |_, _, cx| {
            accounts.update(cx, |accounts, cx| accounts.request_new_account(cx))
        })
    };
    let rows = kept.iter().flat_map(|node| {
        std::iter::once(account_row(
            node,
            false,
            accounts,
            selected == Some(node.account.id),
            ring,
            focus,
        ))
        .chain(node.pots.iter().map(|pot| {
            account_row(
                pot,
                true,
                accounts,
                selected == Some(pot.account.id),
                ring,
                focus,
            )
        }))
    });
    let accounts_section = list_section(SectionLabelRow::new("Accounts").action(add))
        .children(rows)
        .when(kept.is_empty() && state.load_error().is_none(), |this| {
            this.child(note_row("No accounts yet"))
        })
        .when_some(state.load_error().cloned(), |this, error| {
            this.child(
                div()
                    .px(spacing::S2)
                    .text_role(typography::CAPTION)
                    .text_color(color::DANGER)
                    .child(error),
            )
        });

    let valued_section = (!valued.is_empty()).then(|| {
        list_section(SectionLabelRow::new("Assets and liabilities")).children(valued.iter().map(
            |node| {
                account_row(
                    node,
                    false,
                    accounts,
                    selected == Some(node.account.id),
                    ring,
                    focus,
                )
            },
        ))
    });

    let categories = list_section(SectionLabelRow::new("Categories").action(IconButton::new(
        "add-category",
        Glyph::Plus,
        "Add category",
    )))
    .children(Category::ALL.into_iter().map(|category| {
        ListRow::new(category.name(), category.name())
            .marker(category.marker())
            .trailing(category.count().to_string())
            .trailing_tone(TrailingTone::Faint)
    }))
    .child(
        ListRow::new("uncategorised", "Uncategorised")
            .marker(Marker::None)
            .dimmed(true)
            .trailing(fixture::UNCATEGORISED_COUNT.to_string())
            .trailing_tone(TrailingTone::Faint),
    );

    let (up, down, clear) = (accounts.clone(), accounts.clone(), accounts.clone());
    div()
        .id("left-panel")
        .key_context(CONTEXT)
        .track_focus(&focus.clone().tab_stop(true))
        .on_action(move |_: &SelectUp, _, cx| {
            up.update(cx, |accounts, cx| accounts.select_next(-1, cx))
        })
        .on_action(move |_: &SelectDown, _, cx| {
            down.update(cx, |accounts, cx| accounts.select_next(1, cx))
        })
        .on_action(move |_: &Cancel, _, cx| {
            clear.update(cx, |accounts, cx| accounts.select(None, cx))
        })
        .flex()
        .flex_col()
        .flex_none()
        .w(size::PANEL)
        .overflow_y_scroll()
        .bg(color::CHROME)
        .border_r(size::HAIRLINE)
        .border_color(color::HAIRLINE)
        .child(net_worth)
        .child(accounts_section)
        .children(valued_section)
        .child(categories)
}
