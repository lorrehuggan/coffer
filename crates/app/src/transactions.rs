//! The Transactions screen (`docs/DESIGN.md` → "Transactions"; Paper
//! artboards `18E-0` and `1NP-0`), with fixture data until the table reads
//! the database (M3.3). Built in D5 to check that GPUI can deliver the
//! design.
//!
//! The view draws the canvas (toolbar, column header, the day-grouped table,
//! and the bulk-action bar while several rows are selected) and the
//! inspector (the selected transaction, or a summary of the selection). The
//! app shell ([`crate::shell`]) puts the title bar, the accounts panel and
//! the status bar around it.
//!
//! The table is a GPUI `list`: virtualised, with rows of their own heights
//! (day headers carry 4px more than a row when they draw a divider), which
//! the stock `DataTable` can't interleave. The view owns the data, the
//! selection (by transaction id) and the list's scroll state; rows only
//! present (`ui::transaction_row`).

pub mod fixture;
mod inspector;
pub mod selection;

use gpui_kit::base::actions::{Cancel, SelectDown, SelectFirst, SelectLast, SelectUp};
use gpui_kit::component::input::InputState;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, FocusHandle,
    InteractiveElement as _, IntoElement, KeyBinding, ListAlignment, ListState, ParentElement as _,
    Render, Styled as _, Window, div, list,
};

use crate::theme::tokens::{color, size as token_size, spacing};
use crate::ui::bulk_action_bar::BulkActionBar;
use crate::ui::button::{Button, ButtonVariant, Chip};
use crate::ui::field::search_input;
use crate::ui::transaction_row::{CategoryCell, DayHeader, TransactionHeader, TransactionRow};

use fixture::{Categorisation, Sign, Transaction, gbp};
use selection::{ClickKind, Selection};

/// The key context of the table, where the arrow keys move the selection.
const TABLE_CONTEXT: &str = "CofferTransactionsTable";

gpui_kit::actions!(
    coffer_transactions,
    [
        /// ⇧↑: extend the selection one row up.
        ExtendSelectionUp,
        /// ⇧↓: extend the selection one row down.
        ExtendSelectionDown,
    ]
);

/// A transaction's id: the fixture's, until the database's (M1).
pub type TransactionId = &'static str;

/// Binds the table's keys. Call once, before the first view is created.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectUp, Some(TABLE_CONTEXT)),
        KeyBinding::new("down", SelectDown, Some(TABLE_CONTEXT)),
        KeyBinding::new("shift-up", ExtendSelectionUp, Some(TABLE_CONTEXT)),
        KeyBinding::new("shift-down", ExtendSelectionDown, Some(TABLE_CONTEXT)),
        KeyBinding::new("home", SelectFirst, Some(TABLE_CONTEXT)),
        KeyBinding::new("end", SelectLast, Some(TABLE_CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(TABLE_CONTEXT)),
    ]);
}

/// One entry of the table's list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TableItem {
    /// A day's header; `first` has no divider above it.
    Day { day: usize, first: bool },
    /// A transaction, by index into [`TransactionsView::transactions`].
    Row(usize),
}

/// The list's entries: each day's header, then its transactions.
fn table_items(transactions: &[Transaction]) -> Vec<TableItem> {
    let mut items = Vec::new();
    let mut day = None;
    for (index, transaction) in transactions.iter().enumerate() {
        if day != Some(transaction.day) {
            items.push(TableItem::Day {
                day: transaction.day,
                first: day.is_none(),
            });
            day = Some(transaction.day);
        }
        items.push(TableItem::Row(index));
    }
    items
}

/// The net of a day's transactions, transfers left out (`DESIGN.md` →
/// "Money").
fn day_net(transactions: &[Transaction], day: usize) -> i64 {
    transactions
        .iter()
        .filter(|transaction| transaction.day == day && !transaction.is_transfer())
        .map(|transaction| transaction.amount)
        .sum()
}

/// The category cell for a transaction.
fn category_cell(categorisation: Categorisation) -> CategoryCell {
    match categorisation {
        Categorisation::Assigned(category) => {
            CategoryCell::marked(category.name(), category.marker())
        }
        Categorisation::Uncategorised => CategoryCell::uncategorised(),
        Categorisation::Suggested { category, .. } => CategoryCell::suggested(category.name()),
        Categorisation::Transfer { pot } => CategoryCell::transfer(format!("Transfer · {pot}")),
    }
}

/// The Transactions screen.
pub struct TransactionsView {
    transactions: Vec<Transaction>,
    /// Transaction ids in display order, for ranges and keyboard moves.
    order: Vec<TransactionId>,
    items: Vec<TableItem>,
    list: ListState,
    selection: Selection<TransactionId>,
    focus: FocusHandle,
    search: Entity<InputState>,
    note: Entity<InputState>,
}

impl TransactionsView {
    /// The screen with the fixture data and the Paper artboard's selection:
    /// the AI-suggested Amazon payment.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let transactions = fixture::TRANSACTIONS.to_vec();
        let order = transactions
            .iter()
            .map(|transaction| transaction.id)
            .collect();
        let items = table_items(&transactions);
        // Ten rows past each edge, so a fast scroll doesn't show blanks.
        let list = ListState::new(items.len(), ListAlignment::Top, token_size::CONTROL * 10.);
        let mut selection = Selection::default();
        selection.set(["txn-amazon-09"]);
        Self {
            transactions,
            order,
            items,
            list,
            selection,
            focus: cx.focus_handle(),
            search: cx.new(|cx| InputState::new(window, cx).placeholder("Search transactions")),
            note: cx.new(|cx| InputState::new(window, cx).placeholder("Add a note")),
        }
    }

    /// Selects exactly `ids`, as if clicked in turn with ⌘ held.
    pub fn select(&mut self, ids: impl IntoIterator<Item = TransactionId>, cx: &mut Context<Self>) {
        self.selection.set(ids);
        cx.notify();
    }

    /// The selected transactions, in display order.
    pub fn selected(&self) -> Vec<&Transaction> {
        self.selection
            .in_order(&self.order)
            .filter_map(|id| self.transaction(id))
            .collect()
    }

    fn transaction(&self, id: TransactionId) -> Option<&Transaction> {
        self.transactions
            .iter()
            .find(|transaction| transaction.id == id)
    }

    fn transaction_mut(&mut self, id: TransactionId) -> Option<&mut Transaction> {
        self.transactions
            .iter_mut()
            .find(|transaction| transaction.id == id)
    }

    /// Approves or rejects the agent's category for `id`. Rejecting leaves
    /// the transaction uncategorised.
    fn resolve_suggestion(&mut self, id: TransactionId, approve: bool, cx: &mut Context<Self>) {
        if let Some(transaction) = self.transaction_mut(id)
            && let Categorisation::Suggested { category, .. } = transaction.categorisation
        {
            transaction.categorisation = if approve {
                Categorisation::Assigned(category)
            } else {
                Categorisation::Uncategorised
            };
            cx.notify();
        }
    }

    fn click_row(
        &mut self,
        id: TransactionId,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.modifiers();
        let kind = if modifiers.shift {
            ClickKind::Extend
        } else if modifiers.secondary(cx) {
            ClickKind::Toggle
        } else {
            ClickKind::Replace
        };
        self.selection.click(id, kind, &self.order);
        self.focus.clone().focus(window, cx);
        cx.notify();
    }

    fn move_selection(&mut self, delta: isize, extend: bool, cx: &mut Context<Self>) {
        self.selection.move_by(delta, extend, &self.order);
        self.reveal_current();
        cx.notify();
    }

    /// Scrolls the keyboard row into view.
    fn reveal_current(&self) {
        let Some(current) = self.selection.current() else {
            return;
        };
        let index = self.items.iter().position(|item| match item {
            TableItem::Row(row) => self.transactions[*row].id == current,
            TableItem::Day { .. } => false,
        });
        if let Some(index) = index {
            self.list.scroll_to_reveal_item(index);
        }
    }

    fn render_item(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // A list item is laid out on its own, at the list's width, and
        // ignores the list's padding: the wrapper fills the width and
        // carries the table body's 8px inline padding.
        let item =
            match self.items[index] {
                TableItem::Day { day, first } => DayHeader::new(
                    fixture::DAYS[day].heading,
                    gbp(day_net(&self.transactions, day), Sign::Always),
                )
                .divided(!first)
                .into_any_element(),
                TableItem::Row(row) => {
                    let transaction = self.transactions[row];
                    let id = transaction.id;
                    let focus_visible = self.selection.current() == Some(id)
                        && self.focus.is_focused(window)
                        && window.last_input_was_keyboard();
                    TransactionRow::new(
                        id,
                        transaction.merchant,
                        category_cell(transaction.categorisation),
                        gbp(transaction.amount, Sign::Always),
                    )
                    .account(transaction.account)
                    .when_some(transaction.original_amount, |row, original| {
                        row.original_amount(original)
                    })
                    .flagged(transaction.flagged)
                    .selected(self.selection.contains(id))
                    .focus_visible(focus_visible)
                    .on_click(cx.listener(move |this, event, window, cx| {
                        this.click_row(id, event, window, cx)
                    }))
                    .into_any_element()
                }
            };
        div()
            .flex()
            .flex_col()
            .w_full()
            .px(spacing::S2)
            .child(item)
            .into_any_element()
    }

    fn render_toolbar(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S2)
            .py(spacing::S3)
            .px(spacing::S4)
            .child(search_input(&self.search))
            .child(Chip::new("filter-account", "Account").menu(true))
            .child(Chip::new("filter-category", "Category").menu(true))
            .child(Chip::new("filter-date", "Date").menu(true))
            .child(
                Chip::new("filter-uncategorised", "Uncategorised")
                    .count(fixture::UNCATEGORISED_COUNT.to_string()),
            )
            .child(div().flex_1())
            .child(
                Button::new("import", "Import")
                    .with_variant(ButtonVariant::Outline)
                    .shortcut("⌘I"),
            )
    }

    fn render_table(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context(TABLE_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &SelectUp, _, cx| this.move_selection(-1, false, cx)))
            .on_action(cx.listener(|this, _: &SelectDown, _, cx| this.move_selection(1, false, cx)))
            .on_action(
                cx.listener(|this, _: &ExtendSelectionUp, _, cx| this.move_selection(-1, true, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ExtendSelectionDown, _, cx| {
                    this.move_selection(1, true, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &SelectFirst, _, cx| {
                this.move_selection(isize::MIN, false, cx)
            }))
            .on_action(
                cx.listener(|this, _: &SelectLast, _, cx| {
                    this.move_selection(isize::MAX, false, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &Cancel, _, cx| {
                this.selection.clear();
                cx.notify();
            }))
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(TransactionHeader::new().full_bleed(true))
            .child(list(self.list.clone(), cx.processor(Self::render_item)).flex_1())
    }

    fn render_bulk_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(spacing::S6)
            .flex()
            .justify_center()
            .child(
                BulkActionBar::new(self.selection.len().to_string())
                    .action(
                        Button::new("bulk-categorise", "Categorise")
                            .with_variant(ButtonVariant::Toolbar),
                    )
                    .action(
                        Button::new("bulk-transfer", "Mark as transfer")
                            .with_variant(ButtonVariant::Toolbar),
                    )
                    .action(
                        Button::new("bulk-rule", "Create rule")
                            .with_variant(ButtonVariant::Toolbar),
                    )
                    .on_clear(cx.listener(|this, _, _, cx| {
                        this.selection.clear();
                        cx.notify();
                    })),
            )
    }

    fn render_canvas(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .bg(color::CANVAS)
            .child(self.render_toolbar())
            .child(self.render_table(cx))
            .when(self.selection.len() > 1, |this| {
                this.child(self.render_bulk_bar(cx))
            })
    }
}

impl Render for TransactionsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(self.render_canvas(cx))
            .child(inspector::render(self, window, cx))
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};

    use super::*;

    fn open(cx: &mut TestAppContext) -> (Entity<TransactionsView>, &mut VisualTestContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::component::install(cx);
            init(cx);
        });
        let (view, cx) = cx.add_window_view(TransactionsView::new);
        cx.update(|window, cx| window.draw(cx).clear(cx));
        (view, cx)
    }

    fn selected(view: &Entity<TransactionsView>, cx: &mut VisualTestContext) -> Vec<TransactionId> {
        cx.update(|_, cx| {
            view.read(cx)
                .selected()
                .iter()
                .map(|transaction| transaction.id)
                .collect()
        })
    }

    fn redraw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    #[gpui_kit::test]
    fn clicks_replace_extend_and_toggle(cx: &mut TestAppContext) {
        let (view, cx) = open(cx);
        cx.update(|window, cx| window.click("txn-pret-09", cx));
        assert_eq!(selected(&view, cx), ["txn-pret-09"]);

        redraw(cx);
        cx.update(|window, cx| window.click_with_modifiers("txn-tfl-09", Modifiers::shift(), cx));
        assert_eq!(selected(&view, cx).len(), 5, "⇧-click selects the range");

        redraw(cx);
        cx.update(|window, cx| {
            let toggle = Modifiers::secondary_key(cx);
            window.click_with_modifiers("txn-rainy-day-09", toggle, cx)
        });
        assert_eq!(
            selected(&view, cx),
            [
                "txn-pret-09",
                "txn-bluebottle-09",
                "txn-amazon-09",
                "txn-tfl-09"
            ],
            "⌘-click toggles one row"
        );
    }

    #[gpui_kit::test]
    fn keys_move_and_extend_the_selection(cx: &mut TestAppContext) {
        let (view, cx) = open(cx);
        // A click focuses the table, which the keys need.
        cx.update(|window, cx| window.click("txn-amazon-09", cx));
        cx.simulate_keystrokes("down");
        assert_eq!(selected(&view, cx), ["txn-tfl-09"]);

        cx.simulate_keystrokes("shift-up shift-up");
        assert_eq!(
            selected(&view, cx),
            ["txn-rainy-day-09", "txn-amazon-09", "txn-tfl-09"]
        );
        redraw(cx);
        assert!(
            cx.update(|window, _| window.try_find("bulk-clear").is_some()),
            "several rows show the bulk-action bar"
        );

        cx.simulate_keystrokes("escape");
        assert!(selected(&view, cx).is_empty());
        redraw(cx);
        assert!(cx.update(|window, _| window.try_find("bulk-clear").is_none()));

        cx.simulate_keystrokes("end");
        let last = fixture::TRANSACTIONS.last().expect("fixture rows").id;
        assert_eq!(selected(&view, cx), [last]);
    }

    #[gpui_kit::test]
    fn approving_a_suggestion_assigns_its_category(cx: &mut TestAppContext) {
        let (view, cx) = open(cx);
        // The window opens on the suggested Amazon payment.
        cx.update(|window, cx| window.click("suggestion-approve", cx));
        let categorisation = cx.update(|_, cx| view.read(cx).selected()[0].categorisation);
        assert_eq!(
            categorisation,
            Categorisation::Assigned(fixture::Category::Shopping)
        );
        redraw(cx);
        assert!(
            cx.update(|window, _| window.try_find("suggestion-approve").is_none()),
            "the card goes once resolved"
        );
    }

    #[gpui_kit::test]
    fn rejecting_a_suggestion_leaves_the_row_uncategorised(cx: &mut TestAppContext) {
        let (view, cx) = open(cx);
        cx.update(|window, cx| window.click("suggestion-reject", cx));
        let categorisation = cx.update(|_, cx| view.read(cx).selected()[0].categorisation);
        assert_eq!(categorisation, Categorisation::Uncategorised);
    }

    #[test]
    fn days_head_their_transactions() {
        let items = table_items(&fixture::TRANSACTIONS);
        assert_eq!(
            items[0],
            TableItem::Day {
                day: 0,
                first: true
            }
        );
        assert_eq!(items[1], TableItem::Row(0));
        let days = items
            .iter()
            .filter(|item| matches!(item, TableItem::Day { .. }))
            .count();
        assert_eq!(days, fixture::DAYS.len());
        assert_eq!(
            items.len(),
            fixture::TRANSACTIONS.len() + fixture::DAYS.len()
        );
        assert!(
            items[1..]
                .iter()
                .all(|item| !matches!(item, TableItem::Day { first: true, .. })),
            "only the first day goes without a divider"
        );
    }

    #[test]
    fn day_nets_leave_transfers_out() {
        // The nets on the Paper artboard.
        let nets: Vec<String> = (0..5)
            .map(|day| gbp(day_net(&fixture::TRANSACTIONS, day), Sign::Always))
            .collect();
        assert_eq!(
            nets,
            ["−£37.74", "−£68.24", "−£192.34", "−£70.20", "−£71.18"]
        );
    }
}
