//! The transactions table's parts (`docs/DESIGN.md` → "Data table
//! (Transactions)"; Paper `XJ-0`): [`TransactionHeader`], [`DayHeader`] and
//! [`TransactionRow`].
//!
//! The table sits directly on `canvas` and uses the list-row language: 28px
//! rows, radius 6, 8px inline padding, 12px between columns, no row
//! dividers. Columns: merchant (flexible, truncates), category (180px),
//! account (140px; hidden when the view is filtered to one account) and
//! amount (at least 96px, right-aligned, Geist Mono). The date lives in the
//! day header.
//!
//! These are presentation only. Virtualisation, sorting, keyboard movement
//! and the in-place category select belong to the table that owns the rows
//! (`crate::transactions`): the header reports clicks on its titles, and a
//! row reports a double-click on its category and shows the menu its owner
//! gives it there. Like [`ListRow`], a row wraps the stock `ListItem` and
//! draws the focus ring when its owner says it's the keyboard target.
//!
//! Amounts arrive formatted: sign, currency symbol and locale are the money
//! formatter's job (M1.5). `DESIGN.md` → "Money": a true minus (U+2212) for
//! outflows, a plus for inflows, and colour never carries the sign.
//!
//! [`ListRow`]: crate::ui::list_row::ListRow

use std::rc::Rc;

use gpui_kit::component::list::ListItem;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, Div, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, RenderOnce, Rgba, SharedString, StatefulInteractiveElement as _,
    Styled as _, Transformation, Window, div, radians,
};

use gpui_kit::base::TestSupportExt as _;

use crate::theme::focus::{FocusRingExt as _, FocusRingPlacement};
use crate::theme::tokens::{color, radius, size, spacing, typography};
use crate::ui::PreviewState;
use crate::ui::icon::Glyph;
use crate::ui::marker::Marker;
use crate::ui::text::TextRoleExt as _;
use crate::ui::tracked::Tracked;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type SortHandler = Rc<dyn Fn(TransactionColumn, &mut Window, &mut App)>;

/// A table column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransactionColumn {
    Merchant,
    Category,
    Account,
    Amount,
}

impl TransactionColumn {
    pub const ALL: [TransactionColumn; 4] = [
        TransactionColumn::Merchant,
        TransactionColumn::Category,
        TransactionColumn::Account,
        TransactionColumn::Amount,
    ];

    /// The element id of its header title.
    pub fn element_id(self) -> &'static str {
        match self {
            Self::Merchant => "sort-merchant",
            Self::Category => "sort-category",
            Self::Account => "sort-account",
            Self::Amount => "sort-amount",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Merchant => "Merchant",
            Self::Category => "Category",
            Self::Account => "Account",
            Self::Amount => "Amount",
        }
    }
}

/// One row's cells laid out in the table's columns. Header and rows share
/// it, so their columns line up.
fn columns(
    row: Div,
    merchant: impl IntoElement,
    category: impl IntoElement,
    account: Option<impl IntoElement>,
    amount: impl IntoElement,
) -> Div {
    let cell = |width| {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S2)
            .w(width)
            .min_w_0()
            .overflow_hidden()
    };
    row.flex()
        .items_center()
        .gap(spacing::S3)
        .child(
            div()
                .flex()
                .flex_1()
                .items_center()
                .gap(spacing::S2)
                .min_w_0()
                .overflow_hidden()
                .child(merchant),
        )
        .child(cell(size::COLUMN_CATEGORY).child(category))
        .when_some(account, |this, account| {
            this.child(cell(size::COLUMN_ACCOUNT).child(account))
        })
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_end()
                .gap(spacing::S1_5)
                .min_w(size::COLUMN_AMOUNT)
                .child(amount),
        )
}

/// Test selector on the category name, so layout tests can find its lane.
const CATEGORY_LABEL: &str = "coffer-category-label";

/// One line of text that truncates with an ellipsis.
fn truncating(text: impl Into<SharedString>) -> Div {
    div()
        .min_w_0()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(text.into())
}

/// The table's column header: 28px, `canvas` fill (it stays put while the
/// rows scroll under it), bottom `hairline`, section-label type.
///
/// The column the table is sorted by has a `text` label and a chevron in
/// `icon-faint`, pointing down for Z–A or largest first and up otherwise;
/// the others are `text-muted`. With [`TransactionHeader::on_sort`], every
/// title is clickable and shows the chevron on hover.
#[derive(IntoElement)]
pub struct TransactionHeader {
    sorted_by: Option<TransactionColumn>,
    descending: bool,
    show_account: bool,
    full_bleed: bool,
    on_sort: Option<SortHandler>,
}

impl TransactionHeader {
    /// A header with every column and no sort column.
    pub fn new() -> Self {
        Self {
            sorted_by: None,
            descending: true,
            show_account: true,
            full_bleed: false,
            on_sort: None,
        }
    }

    /// Marks the column the table is sorted by, largest or Z–A first.
    pub fn sorted_by(mut self, column: TransactionColumn) -> Self {
        self.sorted_by = Some(column);
        self.descending = true;
        self
    }

    /// Marks the column the table is sorted by, and which way.
    pub fn sorted(mut self, column: TransactionColumn, descending: bool) -> Self {
        self.sorted_by = Some(column);
        self.descending = descending;
        self
    }

    /// Runs `handler` with the column whose title was clicked.
    pub fn on_sort(
        mut self,
        handler: impl Fn(TransactionColumn, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_sort = Some(Rc::new(handler));
        self
    }

    /// Spans the canvas edge to edge over a table body inset 8px
    /// (`DESIGN.md` → "Transactions"): 16px inline padding instead of 8, so
    /// the titles still line up with the rows' text.
    pub fn full_bleed(mut self, full_bleed: bool) -> Self {
        self.full_bleed = full_bleed;
        self
    }

    /// Shows the account column. Hide it, on the header and every row, when
    /// the table is filtered to one account.
    pub fn show_account(mut self, show: bool) -> Self {
        self.show_account = show;
        self
    }
}

impl Default for TransactionHeader {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderOnce for TransactionHeader {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let role = typography::SECTION_LABEL;
        let on_sort = self.on_sort.clone();
        let descending = self.descending;
        let title = |column: TransactionColumn| {
            let sorted = self.sorted_by == Some(column);
            let sortable = on_sort.is_some();
            let group = column.element_id();
            let chevron = || {
                Glyph::ChevronDown
                    .render()
                    .text_color(color::ICON_FAINT)
                    .when(sorted && !descending, |this| {
                        this.with_transformation(Transformation::rotate(radians(
                            std::f32::consts::PI,
                        )))
                    })
            };
            let handler = on_sort.clone();
            // The chevron sits on the column's inner side, so no title moves:
            // before the right-aligned Amount (as Paper `XL-0` draws it),
            // after the others.
            let lead = column == TransactionColumn::Amount;
            let marker = || {
                (sorted || sortable).then(|| {
                    div()
                        .flex()
                        .flex_none()
                        .when(!sorted, |this| {
                            this.opacity(0.)
                                .group_hover(group, |style| style.opacity(1.))
                        })
                        .child(chevron())
                })
            };
            div()
                .id(column.element_id())
                .group(group)
                .flex()
                .items_center()
                .gap(spacing::S1)
                .when(lead, |this| this.children(marker()))
                .when_some(handler, |this, handler| {
                    this.cursor_pointer()
                        .on_click(move |_, window, cx| handler(column, window, cx))
                })
                .child(
                    div()
                        .flex()
                        .min_w_0()
                        .overflow_hidden()
                        .text_color(if sorted {
                            color::TEXT
                        } else {
                            color::TEXT_MUTED
                        })
                        .child(Tracked::new(role, column.title())),
                )
                .when(!lead, |this| this.children(marker()))
                .test_support()
        };
        columns(
            div()
                .flex_none()
                .h(size::CONTROL)
                .px(if self.full_bleed {
                    spacing::S4
                } else {
                    spacing::S2
                })
                .bg(color::CANVAS)
                .border_b(size::HAIRLINE)
                .border_color(color::HAIRLINE),
            title(TransactionColumn::Merchant),
            title(TransactionColumn::Category),
            self.show_account.then(|| title(TransactionColumn::Account)),
            title(TransactionColumn::Amount),
        )
    }
}

/// The header of one day's transactions: the date in 11/14 600
/// `text-secondary` and the day's net in mono 11/14 `text-muted`, on no
/// fill, 28px with 8px above.
///
/// Days are separated by a `hairline`, the table's only divider; the first
/// day has none.
#[derive(IntoElement)]
pub struct DayHeader {
    date: SharedString,
    net: SharedString,
    divided: bool,
}

impl DayHeader {
    /// A header with no divider above it. `date` and `net` arrive formatted
    /// ("Tue 14 Oct", "−£69.01").
    pub fn new(date: impl Into<SharedString>, net: impl Into<SharedString>) -> Self {
        Self {
            date: date.into(),
            net: net.into(),
            divided: false,
        }
    }

    /// Draws the `hairline` above the header that separates it from the day
    /// before. Every day but the first has one.
    pub fn divided(mut self, divided: bool) -> Self {
        self.divided = divided;
        self
    }
}

impl RenderOnce for DayHeader {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .h(size::CONTROL)
            .px(spacing::S2)
            .pt(spacing::S2)
            .when(self.divided, |this| {
                this.border_t(size::HAIRLINE).border_color(color::HAIRLINE)
            })
            .child(
                div()
                    .text_role(typography::CAPTION)
                    .font_weight(crate::theme::tokens::weight::STRONG)
                    .text_color(color::TEXT_SECONDARY)
                    .child(self.date),
            )
            .child(
                div()
                    .text_role(typography::MONO_VALUE)
                    .text_color(color::TEXT_MUTED)
                    .child(self.net),
            );
        // The 4px above the divider is padding on an outer box, not a
        // margin: a virtual list measures an item by its border box, so a
        // margin would overlap the next row.
        div()
            .flex()
            .flex_col()
            .flex_none()
            .when(self.divided, |this| this.pt(spacing::S1))
            .child(header)
    }
}

/// What a row shows in the category column.
#[derive(Clone, Debug, PartialEq)]
pub struct CategoryCell(CategoryKind);

#[derive(Clone, Debug, PartialEq)]
enum CategoryKind {
    Assigned { name: SharedString, marker: Marker },
    Uncategorised,
    Suggested { name: SharedString },
    Transfer { label: SharedString },
}

impl CategoryCell {
    /// A category: its 8×8 colour marker (radius 2) and its name in
    /// `text-secondary`. `marker` is the category's chart colour
    /// ([`color::CHART`]).
    pub fn assigned(name: impl Into<SharedString>, marker: Rgba) -> Self {
        Self::marked(name, Marker::Colour(marker))
    }

    /// A category with any marker: [`Marker::Other`] for "Other", which
    /// has no palette colour.
    pub fn marked(name: impl Into<SharedString>, marker: Marker) -> Self {
        Self(CategoryKind::Assigned {
            name: name.into(),
            marker,
        })
    }

    /// No category yet: a dashed `text-muted` marker and "Uncategorised" in
    /// `text-muted`.
    pub fn uncategorised() -> Self {
        Self(CategoryKind::Uncategorised)
    }

    /// A category the agent proposed and the user hasn't approved: a 10px
    /// sparkle and the name in `moldavite-text`.
    pub fn suggested(name: impl Into<SharedString>) -> Self {
        Self(CategoryKind::Suggested { name: name.into() })
    }

    /// A transfer between the user's own accounts ("Transfer · Holiday
    /// pot"). The whole row dims to `text-muted`, because transfers are
    /// left out of totals, and the merchant gets the ⇄ icon.
    pub fn transfer(label: impl Into<SharedString>) -> Self {
        Self(CategoryKind::Transfer {
            label: label.into(),
        })
    }

    pub fn is_transfer(&self) -> bool {
        matches!(self.0, CategoryKind::Transfer { .. })
    }

    /// The category's name as a screen reader hears it.
    fn spoken(&self) -> SharedString {
        match &self.0 {
            CategoryKind::Assigned { name, .. } => name.clone(),
            CategoryKind::Uncategorised => "Uncategorised".into(),
            CategoryKind::Suggested { name } => format!("{name}, suggested").into(),
            CategoryKind::Transfer { label } => label.clone(),
        }
    }
}

/// The text colours of a row's cells.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RowColors {
    merchant: Rgba,
    category: Rgba,
    account: Rgba,
    amount: Rgba,
}

impl RowColors {
    /// Selection lifts every grey label to `text` (`DESIGN.md`: "molten-soft
    /// fill with text labels"); hover changes only the fill. The agent's
    /// colour keeps its meaning in every state.
    fn new(category: &CategoryCell, selected: bool) -> Self {
        let lift = |rest: Rgba| if selected { color::TEXT } else { rest };
        let transfer = category.is_transfer();
        let dimmed = |rest: Rgba| lift(if transfer { color::TEXT_MUTED } else { rest });
        Self {
            merchant: dimmed(color::TEXT),
            category: match category.0 {
                CategoryKind::Assigned { .. } => lift(color::TEXT_SECONDARY),
                CategoryKind::Uncategorised | CategoryKind::Transfer { .. } => {
                    lift(color::TEXT_MUTED)
                }
                CategoryKind::Suggested { .. } => color::MOLDAVITE_TEXT,
            },
            account: lift(color::TEXT_MUTED),
            amount: dimmed(color::TEXT),
        }
    }
}

/// One transaction in the table.
///
/// Stateless; the owning table decides which rows are selected (one or
/// many) and which is the keyboard target, and what a click does. The
/// accessible name reads the merchant, category and amount.
///
/// ```
/// use coffer_app::theme::tokens::color;
/// use coffer_app::ui::transaction_row::{CategoryCell, TransactionRow};
///
/// let row = TransactionRow::new(
///     "txn-01",
///     "Pret A Manger",
///     CategoryCell::assigned("Eating out", color::CHART_2),
///     "−£4.85",
/// )
/// .account("Current account");
/// assert!(!row.is_selected());
/// ```
#[derive(IntoElement)]
pub struct TransactionRow {
    id: ElementId,
    merchant: SharedString,
    category: CategoryCell,
    account: Option<SharedString>,
    amount: SharedString,
    original_amount: Option<SharedString>,
    flagged: bool,
    selected: bool,
    focus_visible: bool,
    preview: Option<PreviewState>,
    on_click: Option<ClickHandler>,
    on_edit_category: Option<ClickHandler>,
    category_menu: Option<AnyElement>,
}

impl TransactionRow {
    /// A row with no account column. `amount` arrives formatted, with its
    /// sign and currency symbol.
    pub fn new(
        id: impl Into<ElementId>,
        merchant: impl Into<SharedString>,
        category: CategoryCell,
        amount: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            merchant: merchant.into(),
            category,
            account: None,
            amount: amount.into(),
            original_amount: None,
            flagged: false,
            selected: false,
            focus_visible: false,
            preview: None,
            on_click: None,
            on_edit_category: None,
            category_menu: None,
        }
    }

    /// Fills the account column. Leave it out on every row, and hide it on
    /// the header, when the table is filtered to one account.
    pub fn account(mut self, account: impl Into<SharedString>) -> Self {
        self.account = Some(account.into());
        self
    }

    /// Shows the amount in the original currency below the converted one, in
    /// mono 10/12 `text-muted` ("€14.50").
    pub fn original_amount(mut self, amount: impl Into<SharedString>) -> Self {
        self.original_amount = Some(amount.into());
        self
    }

    /// Marks a reconciliation problem: a 6px `danger` dot before the amount.
    /// The table explains it in a tooltip.
    pub fn flagged(mut self, flagged: bool) -> Self {
        self.flagged = flagged;
        self
    }

    /// Marks the row selected: `molten-soft` fill, labels in `text`.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Draws the inset focus ring. The owning table sets it on its current
    /// row while the table has keyboard focus.
    pub fn focus_visible(mut self, focus_visible: bool) -> Self {
        self.focus_visible = focus_visible;
        self
    }

    /// Runs `handler` when the row is clicked. The event carries the
    /// modifiers, for ⇧/⌘ multi-selection.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    /// Runs `handler` on a double-click on the category: open the in-place
    /// select (`DESIGN.md` → "Inline edit").
    pub fn on_edit_category(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_edit_category = Some(Box::new(handler));
        self
    }

    /// Shows `menu` below the category cell: the open in-place select.
    pub fn category_menu(mut self, menu: impl IntoElement) -> Self {
        self.category_menu = Some(menu.into_any_element());
        self
    }

    /// Draws `state` without the input that causes it; see [`PreviewState`].
    /// Rows have no pressed look, so [`PreviewState::Pressed`] draws the
    /// resting row.
    pub fn preview(mut self, state: PreviewState) -> Self {
        self.preview = Some(state);
        self
    }

    pub fn is_selected(&self) -> bool {
        self.selected
    }

    pub fn is_flagged(&self) -> bool {
        self.flagged
    }

    fn spoken(&self) -> SharedString {
        format!(
            "{}, {}, {}",
            self.merchant,
            self.category.spoken(),
            self.amount
        )
        .into()
    }

    fn render_category(&mut self, text: Rgba) -> AnyElement {
        let role = typography::BODY;
        let label = |name: SharedString| {
            truncating(name)
                .when(cfg!(test), |this| {
                    this.debug_selector(|| CATEGORY_LABEL.into())
                })
                .text_role(role)
                .text_color(text)
        };
        let cell = div().flex().items_center().gap(spacing::S2).min_w_0();
        let cell = match &self.category.0 {
            CategoryKind::Assigned { name, marker } => {
                cell.child(marker.render()).child(label(name.clone()))
            }
            CategoryKind::Uncategorised => cell
                .child(Marker::None.render())
                .child(label("Uncategorised".into())),
            // The sparkle is 2px wider than a marker, so a 2px tighter gap
            // keeps every category name in one lane (10 + 6 = 8 + 8).
            CategoryKind::Suggested { name } => cell
                .gap(spacing::S1_5)
                .child(
                    Glyph::Sparkle
                        .render_at(size::ICON_XS)
                        .text_color(color::MOLDAVITE),
                )
                .child(label(name.clone())),
            CategoryKind::Transfer { label: name } => cell.child(label(name.clone())),
        };
        let on_edit = self.on_edit_category.take();
        let menu = self.category_menu.take();
        cell.id("category")
            .flex_1()
            .when_some(on_edit, |this, handler| {
                this.on_click(move |event, window, cx| {
                    if event.click_count() >= 2 {
                        handler(event, window, cx);
                    }
                })
            })
            .when_some(menu, |this, menu| {
                // The cell is 16px tall, centred in the 28px row: 26px down
                // puts the menu 4px below the row.
                this.relative()
                    .child(crate::ui::menu::below(size::CONTROL - spacing::S0_5, menu))
            })
            .into_any_element()
    }
}

impl RenderOnce for TransactionRow {
    fn render(mut self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let colors = RowColors::new(&self.category, self.selected);
        let hovered = !self.selected && self.preview == Some(PreviewState::Hovered);
        let ring = self.focus_visible || self.preview == Some(PreviewState::FocusVisible);
        let body = typography::BODY;

        let merchant = div()
            .flex()
            .items_center()
            .gap(spacing::S2)
            .min_w_0()
            .when(self.category.is_transfer(), |this| {
                this.child(Glyph::Transfer.render().text_color(colors.merchant))
            })
            .child(
                truncating(self.merchant.clone())
                    .text_role(body)
                    .text_color(colors.merchant),
            );
        let category = self.render_category(colors.category);
        let account = self.account.clone().map(|account| {
            truncating(account)
                .text_role(body)
                .text_color(colors.account)
        });
        let amount = div()
            .flex()
            .items_center()
            .gap(spacing::S1_5)
            .when(self.flagged, |this| {
                this.child(
                    div()
                        .flex_none()
                        .size(size::DOT)
                        .rounded(radius::LABEL)
                        .bg(color::DANGER),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_end()
                    .child(
                        div()
                            .whitespace_nowrap()
                            .text_role(typography::MONO_AMOUNT)
                            .text_color(colors.amount)
                            .child(self.amount.clone()),
                    )
                    .when_some(self.original_amount.clone(), |this, original| {
                        this.child(
                            div()
                                .whitespace_nowrap()
                                .text_role(typography::MONO_MICRO)
                                .text_color(color::TEXT_MUTED)
                                .child(original),
                        )
                    }),
            );

        let item = ListItem::new(self.id.clone())
            .accessibility_label(self.spoken())
            .selected(self.selected)
            .h(size::CONTROL)
            .py_0()
            .px(spacing::S2)
            .gap_0()
            .rounded(radius::MD)
            .when(hovered, |this| this.bg(color::HOVER))
            .when_some(self.on_click, |this, handler| {
                this.on_click(move |event, window, cx| handler(event, window, cx))
            })
            .child(columns(
                div().w_full().min_w_0(),
                merchant,
                category,
                account,
                amount,
            ));

        div()
            .flex()
            .rounded(radius::MD)
            .child(item.flex_1().min_w_0())
            .when(ring, |this| {
                this.show_focus_ring(FocusRingPlacement::Inset, window)
            })
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{Bounds, Context, Pixels, Render, TestAppContext, VisualTestContext};

    use super::*;
    use crate::theme;

    #[test]
    fn colour_never_carries_the_sign() {
        // Outflow and inflow rows look the same: the sign is in the text.
        let category = CategoryCell::assigned("Shopping", color::CHART_3);
        assert_eq!(
            RowColors::new(&category, false).amount,
            color::TEXT,
            "amounts are `text`, never green or red"
        );
    }

    #[test]
    fn transfers_dim_the_whole_row() {
        let transfer = RowColors::new(&CategoryCell::transfer("Transfer · Holiday pot"), false);
        assert_eq!(
            transfer,
            RowColors {
                merchant: color::TEXT_MUTED,
                category: color::TEXT_MUTED,
                account: color::TEXT_MUTED,
                amount: color::TEXT_MUTED,
            }
        );
    }

    #[test]
    fn selection_lifts_grey_labels_but_keeps_the_agent_colour() {
        // Paper `XJ-0`, selected rows: category and account turn `text`.
        for category in [
            CategoryCell::assigned("Transport", color::CHART_4),
            CategoryCell::uncategorised(),
            CategoryCell::transfer("Transfer · Holiday pot"),
        ] {
            let colors = RowColors::new(&category, true);
            assert_eq!(
                colors,
                RowColors {
                    merchant: color::TEXT,
                    category: color::TEXT,
                    account: color::TEXT,
                    amount: color::TEXT,
                },
                "{category:?}"
            );
        }

        let suggested = CategoryCell::suggested("Shopping");
        for selected in [false, true] {
            assert_eq!(
                RowColors::new(&suggested, selected).category,
                color::MOLDAVITE_TEXT
            );
        }
    }

    #[test]
    fn resting_colours_match_the_table() {
        let assigned = RowColors::new(&CategoryCell::assigned("Groceries", color::CHART_1), false);
        assert_eq!(
            assigned,
            RowColors {
                merchant: color::TEXT,
                category: color::TEXT_SECONDARY,
                account: color::TEXT_MUTED,
                amount: color::TEXT,
            }
        );
        assert_eq!(
            RowColors::new(&CategoryCell::uncategorised(), false).category,
            color::TEXT_MUTED
        );
    }

    const HEADER: &str = "transaction-header";

    struct Harness {
        show_account: bool,
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let row = |id: &'static str, category, amount: &'static str| {
                let row = TransactionRow::new(
                    id,
                    "Sainsbury's Local, Clapham High Street, a very long merchant name",
                    category,
                    amount,
                );
                if self.show_account {
                    row.account("Current account")
                } else {
                    row
                }
            };
            div()
                .flex()
                .flex_col()
                .w(gpui_kit::px(1280.))
                .child(
                    div()
                        .debug_selector(|| HEADER.into())
                        .child(TransactionHeader::new().show_account(self.show_account)),
                )
                .child(DayHeader::new("Tue 14 Oct", "−£69.01"))
                .child(row(
                    "groceries",
                    CategoryCell::assigned("Groceries", color::CHART_1),
                    "−£23.17",
                ))
                .child(
                    row("cafe", CategoryCell::suggested("Eating out"), "−£12.66")
                        .original_amount("€14.50")
                        .flagged(true),
                )
        }
    }

    fn open(cx: &mut TestAppContext, show_account: bool) -> &mut VisualTestContext {
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::component::install(cx);
        });
        let (_, cx) = cx.add_window_view(move |_, _| Harness { show_account });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx
    }

    fn bounds(cx: &mut VisualTestContext, id: &'static str) -> Bounds<Pixels> {
        cx.update(|window, _| window.find(id).bounds())
    }

    fn header(cx: &mut VisualTestContext) -> Bounds<Pixels> {
        cx.debug_bounds(HEADER).expect("the header is drawn")
    }

    #[gpui_kit::test]
    fn rows_keep_28px_whatever_they_show(cx: &mut TestAppContext) {
        let cx = open(cx, true);
        assert_eq!(header(cx).size.height, size::CONTROL);
        // A long merchant truncates rather than wrapping; an original amount
        // fits inside the row (16 + 12).
        assert_eq!(bounds(cx, "groceries").size.height, size::CONTROL);
        assert_eq!(bounds(cx, "cafe").size.height, size::CONTROL);
        assert_eq!(
            cx.update(|window, _| window.find("cafe").label().map(str::to_owned)),
            Some("Sainsbury's Local, Clapham High Street, a very long merchant name, Eating out, suggested, −£12.66".to_owned())
        );
    }

    #[gpui_kit::test]
    fn hiding_the_account_column_keeps_rows_full_width(cx: &mut TestAppContext) {
        let cx = open(cx, false);
        let header = header(cx);
        let row = bounds(cx, "groceries");
        assert_eq!((row.left(), row.right()), (header.left(), header.right()));
    }

    /// A table holding one row, so the row's category label is the only one.
    struct OneRow(fn() -> CategoryCell);

    impl Render for OneRow {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(gpui_kit::px(1280.)).child(TransactionRow::new(
                "row",
                "Merchant",
                (self.0)(),
                "−£1.00",
            ))
        }
    }

    fn category_label_left(cx: &mut TestAppContext, category: fn() -> CategoryCell) -> Pixels {
        let (_, cx) = cx.add_window_view(move |_, _| OneRow(category));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.debug_bounds(CATEGORY_LABEL)
            .expect("the category label is drawn")
            .left()
    }

    #[gpui_kit::test]
    fn category_names_share_one_lane(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::component::install(cx);
        });
        let assigned =
            category_label_left(cx, || CategoryCell::assigned("Groceries", color::CHART_1));
        assert_eq!(
            category_label_left(cx, CategoryCell::uncategorised),
            assigned
        );
        assert_eq!(
            category_label_left(cx, || CategoryCell::suggested("Shopping")),
            assigned
        );
    }
}
