//! The Components board (Paper `CJ-0`): buttons (`D4-0`), list rows
//! (`VI-0`), the transactions table (`XF-0`) and the agent card (`11N-0`).
//!
//! Each board has a static copy of the Paper frame, states forced through
//! [`PreviewState`], and a live copy driven by [`Showcase`]'s state.

use gpui_kit::component::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, Div, IntoElement, ParentElement as _, Rgba, SharedString, Styled as _,
    Window, div,
};

use super::{LiveList, Showcase, caption, group, groups, section, spec};
use crate::theme::tokens::{color, radius, size, spacing, typography};
use crate::ui::PreviewState;
use crate::ui::agent_card::{AgentCard, AgentChange, ChangeKind};
use crate::ui::badge::{Badge, BadgeKind};
use crate::ui::button::{Button, ButtonVariant, Chip, IconButton};
use crate::ui::icon::Glyph;
use crate::ui::list_row::ListRow;
use crate::ui::text::{CasedText as _, TextRoleExt as _};
use crate::ui::transaction_row::{
    CategoryCell, DayHeader, TransactionColumn, TransactionHeader, TransactionRow,
};

pub(super) fn board(view: &mut Showcase, window: &mut Window, cx: &mut Context<Showcase>) -> Div {
    super::board(
        "Coffer · Components",
        "Components",
        "Every component DESIGN.md uses, in its states. Static rows copy the Paper board; live rows take real input.",
        [
            section("Buttons", buttons(view, cx)),
            section("Lists and table", lists_and_table(view, window, cx)),
            section("Surfaces and feedback", surfaces(cx)),
        ],
    )
}

// Buttons.

/// What a matrix row shows.
#[derive(Clone, Copy)]
enum Control {
    Button(ButtonVariant, &'static str),
    Icon,
    Chip,
}

struct MatrixRow {
    name: &'static str,
    spec: &'static str,
    control: Control,
}

/// The button matrix's rows, in Paper's order and with its copy.
const BUTTON_ROWS: &[MatrixRow] = &[
    MatrixRow {
        name: "Primary",
        spec: "control-lg · radius-button · on-light",
        control: Control::Button(ButtonVariant::Primary, "Import 312 transactions"),
    },
    MatrixRow {
        name: "Agent primary",
        spec: "control-lg · radius-button · on-moldavite",
        control: Control::Button(ButtonVariant::AgentPrimary, "Approve all (12)"),
    },
    MatrixRow {
        name: "Agent outline",
        spec: "control-lg · moldavite-border-strong",
        control: Control::Button(ButtonVariant::AgentOutline, "Reject all"),
    },
    MatrixRow {
        name: "Agent ghost",
        spec: "control-xl · moldavite-surface-raised",
        control: Control::Button(ButtonVariant::AgentGhost, "Review in Inbox"),
    },
    MatrixRow {
        name: "Outline",
        spec: "control-sm · border-strong · caption",
        control: Control::Button(ButtonVariant::Outline, "Import"),
    },
    MatrixRow {
        name: "Outline compact",
        spec: "control-compact · hairline · text-secondary",
        control: Control::Button(ButtonVariant::OutlineCompact, "All accounts"),
    },
    MatrixRow {
        name: "Destructive outline",
        spec: "control-lg · danger border and text",
        control: Control::Button(ButtonVariant::DestructiveOutline, "Delete import"),
    },
    MatrixRow {
        name: "Destructive solid",
        spec: "Dialog confirmation only · on-danger",
        control: Control::Button(ButtonVariant::DestructiveSolid, "Delete 312 transactions"),
    },
    MatrixRow {
        name: "Toolbar action",
        spec: "control · radius-md · text-secondary",
        control: Control::Button(ButtonVariant::Toolbar, "Categorise"),
    },
    MatrixRow {
        name: "Icon button",
        spec: "control × control · radius-md · icon-lg",
        control: Control::Icon,
    },
    MatrixRow {
        name: "Chip",
        spec: "control · radius-pill · border · mono count",
        control: Control::Chip,
    },
];

/// A matrix column: the state a control is drawn in.
#[derive(Clone, Copy)]
enum Column {
    Default,
    Preview(PreviewState),
    Disabled,
}

const COLUMNS: [(&str, Column); 5] = [
    ("Default", Column::Default),
    ("Hover", Column::Preview(PreviewState::Hovered)),
    ("Pressed", Column::Preview(PreviewState::Pressed)),
    ("Focus", Column::Preview(PreviewState::FocusVisible)),
    ("Disabled", Column::Disabled),
];

/// What a live control does when clicked.
type LiveClick = Box<dyn Fn(&mut gpui_kit::Window, &mut gpui_kit::App)>;

/// The row's control with `id`, in `column`'s state. Live controls pass
/// [`Column::Default`] and a click handler.
fn control(
    row: &MatrixRow,
    id: SharedString,
    column: Column,
    on_click: Option<LiveClick>,
) -> AnyElement {
    let disabled = matches!(column, Column::Disabled);
    let preview = match column {
        Column::Preview(state) => Some(state),
        _ => None,
    };
    match row.control {
        Control::Button(variant, label) => Button::new(id, label)
            .with_variant(variant)
            .disabled(disabled)
            .when_some(preview, |this, state| this.preview(state))
            .when_some(on_click, |this, handler| {
                this.on_click(move |_, window, cx| handler(window, cx))
            })
            .into_any_element(),
        Control::Icon => IconButton::new(id, Glyph::Panel, "Toggle sidebar")
            .disabled(disabled)
            .when_some(preview, |this, state| this.preview(state))
            .when_some(on_click, |this, handler| {
                this.on_click(move |_, window, cx| handler(window, cx))
            })
            .into_any_element(),
        Control::Chip => Chip::new(id, "Uncategorised")
            .count("14")
            .disabled(disabled)
            .when_some(preview, |this, state| this.preview(state))
            .when_some(on_click, |this, handler| {
                this.on_click(move |_, window, cx| handler(window, cx))
            })
            .into_any_element(),
    }
}

/// A matrix row: the spec column, then one cell per state column.
fn matrix_row(left: Div, cells: impl IntoIterator<Item = AnyElement>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(spacing::S6)
        .child(left)
        .children(
            cells
                .into_iter()
                .map(|cell| div().flex().flex_1().min_w_0().items_center().child(cell)),
        )
}

fn buttons(view: &mut Showcase, cx: &mut Context<Showcase>) -> AnyElement {
    let role = typography::SECTION_LABEL;
    let header = matrix_row(
        div().flex_none().w(size::PANEL),
        COLUMNS.iter().map(|(title, _)| {
            div()
                .text_role(role)
                .text_color(color::TEXT_MUTED)
                .child(role.cased(*title))
                .into_any_element()
        }),
    )
    .pb(spacing::S1);

    let rows = BUTTON_ROWS.iter().enumerate().map(|(index, row)| {
        matrix_row(
            spec(row.name, row.spec),
            COLUMNS.iter().map(|(title, column)| {
                control(row, format!("matrix-{index}-{title}").into(), *column, None)
            }),
        )
    });

    // Every control once, taking real input; a click is echoed below.
    let live = div()
        .flex()
        .items_center()
        .gap(spacing::S6)
        .child(spec("Live", "real pointer and keyboard · Tab moves focus"))
        .child(
            div()
                .flex()
                .flex_1()
                .flex_wrap()
                .items_center()
                .gap(spacing::S3)
                .children(BUTTON_ROWS.iter().enumerate().map(|(index, row)| {
                    let name = row.name;
                    let entity = cx.entity().downgrade();
                    control(
                        row,
                        format!("live-{index}").into(),
                        Column::Default,
                        Some(Box::new(move |_, cx| {
                            entity
                                .update(cx, |view, cx| view.did(format!("Clicked {name}"), cx))
                                .ok();
                        })),
                    )
                })),
        );

    div()
        .flex()
        .flex_col()
        .gap(spacing::S4)
        .child(header)
        .children(rows)
        .child(live)
        .child(
            div()
                .flex()
                .gap(spacing::S6)
                .child(div().flex_none().w(size::PANEL))
                .child(caption(format!("Last action: {}", view.last_action))),
        )
        .into_any_element()
}

// Lists.

pub(super) struct Account {
    label: &'static str,
    glyph: Glyph,
    balance: &'static str,
    nested: bool,
}

const fn account(label: &'static str, balance: &'static str) -> Account {
    Account {
        label,
        glyph: Glyph::Account,
        balance,
        nested: false,
    }
}

const fn pot(label: &'static str, balance: &'static str) -> Account {
    Account {
        label,
        glyph: Glyph::Pot,
        balance,
        nested: true,
    }
}

/// The live list's accounts and pots (Paper left panel `S2-0`).
pub(super) const ACCOUNTS: &[Account] = &[
    account("Current account", "£1,284.10"),
    pot("Holiday", "£1,200.00"),
    pot("Rainy day", "£3,000.00"),
    account("Joint account", "−£24.10"),
    account("Savings", "£5,750.00"),
    account("Stocks ISA", "£7,030.55"),
];

fn accounts(view: &mut Showcase) -> &mut LiveList {
    &mut view.accounts
}

fn transactions(view: &mut Showcase) -> &mut LiveList {
    &mut view.transactions
}

/// A left-panel surface: `chrome`, a `hairline` border, radius 8, rows
/// inset 6px and 10px from its edges.
fn panel() -> Div {
    div()
        .flex()
        .flex_col()
        .w(size::PANEL)
        .bg(color::CHROME)
        .border(size::HAIRLINE)
        .border_color(color::HAIRLINE)
        .rounded(radius::SURFACE)
}

fn panel_body() -> Div {
    div().flex().flex_col().py(spacing::S2_5).px(spacing::S1_5)
}

/// The list-row states (Paper `VI-0`), each with its caption.
fn list_row_states() -> Div {
    type State = fn(ListRow) -> ListRow;
    let states: [(&str, State); 5] = [
        ("Default", |row| row),
        ("Hover / current", |row| row.preview(PreviewState::Hovered)),
        ("Selected", |row| row.selected(true)),
        ("Focus · inset ring", |row| row.focus_visible(true)),
        ("Selected and focus", |row| {
            row.selected(true).focus_visible(true)
        }),
    ];
    panel().child(
        panel_body()
            .gap(spacing::S3)
            .children(
                states
                    .into_iter()
                    .enumerate()
                    .map(|(index, (name, state))| {
                        let row = ListRow::new(("state", index), "Current account")
                            .icon(Glyph::Account)
                            .trailing("£1,284.10");
                        div()
                            .flex()
                            .flex_col()
                            .gap(spacing::S1)
                            .child(div().px(spacing::S2).child(caption(name)))
                            .child(state(row))
                    }),
            ),
    )
}

fn live_accounts(view: &mut Showcase, window: &Window, cx: &mut Context<Showcase>) -> Div {
    let rows: Vec<_> = ACCOUNTS
        .iter()
        .enumerate()
        .map(|(index, account)| {
            ListRow::new(("account", index), account.label)
                .icon(account.glyph)
                .trailing(account.balance)
                .nested(account.nested)
                .selected(view.accounts.is_selected(index))
                .focus_visible(view.accounts.is_focus_visible(index, window))
                .on_click(Showcase::row_click(accounts, index, cx))
        })
        .collect();
    div()
        .flex()
        .flex_col()
        .gap(spacing::S2)
        .child(
            panel().child(
                view.list_region("live-accounts", accounts, cx)
                    .child(panel_body().children(rows)),
            ),
        )
        .child(caption(
            "Click, or Tab in: ↑ ↓ Home End move · Enter selects",
        ))
}

// The transactions table.

/// A category in the sample data, as a `const`.
#[derive(Clone, Copy)]
enum Category {
    Assigned(&'static str, Rgba),
    Uncategorised,
    Suggested(&'static str),
    Transfer(&'static str),
}

impl Category {
    fn cell(self) -> CategoryCell {
        match self {
            Self::Assigned(name, marker) => CategoryCell::assigned(name, marker),
            Self::Uncategorised => CategoryCell::uncategorised(),
            Self::Suggested(name) => CategoryCell::suggested(name),
            Self::Transfer(label) => CategoryCell::transfer(label),
        }
    }
}

pub(super) struct Transaction {
    merchant: &'static str,
    category: Category,
    account: &'static str,
    amount: &'static str,
    original: Option<&'static str>,
    flagged: bool,
}

const fn txn(
    merchant: &'static str,
    category: Category,
    account: &'static str,
    amount: &'static str,
) -> Transaction {
    Transaction {
        merchant,
        category,
        account,
        amount,
        original: None,
        flagged: false,
    }
}

const CURRENT: &str = "Current account";
const JOINT: &str = "Joint account";

/// The Paper table's rows (`XF-0`), in order.
pub(super) const TRANSACTIONS: &[Transaction] = &[
    txn(
        "Pret A Manger",
        Category::Assigned("Eating out", color::CHART_2),
        CURRENT,
        "−£4.85",
    ),
    txn(
        "Sainsbury's Local, Clapham High Street",
        Category::Assigned("Groceries", color::CHART_1),
        CURRENT,
        "−£23.17",
    ),
    txn(
        "Transport for London",
        Category::Assigned("Transport", color::CHART_4),
        CURRENT,
        "−£2.80",
    ),
    txn(
        "SQ *BLUE BOTTLE COFFEE LDN",
        Category::Uncategorised,
        CURRENT,
        "−£6.20",
    ),
    txn(
        "Amazon Marketplace",
        Category::Suggested("Shopping"),
        JOINT,
        "−£31.99",
    ),
    txn(
        "To Holiday",
        Category::Transfer("Transfer · Holiday pot"),
        CURRENT,
        "−£100.00",
    ),
    Transaction {
        original: Some("€14.50"),
        ..txn(
            "Café de Flore, Paris",
            Category::Assigned("Eating out", color::CHART_2),
            CURRENT,
            "−£12.66",
        )
    },
    txn(
        "ASOS refund",
        Category::Assigned("Shopping", color::CHART_3),
        JOINT,
        "+£45.00",
    ),
    Transaction {
        flagged: true,
        ..txn(
            "Octopus Energy",
            Category::Assigned("Bills", color::CHART_5),
            JOINT,
            "−£86.40",
        )
    },
];

/// The days the rows fall on: date, net, and the index of the day's first
/// row.
const DAYS: &[(&str, &str, usize)] = &[("Tue 14 Oct", "−£69.01", 0), ("Mon 13 Oct", "−£54.06", 6)];

/// The table: header, then each day's header and rows. `row` builds the
/// row for a transaction's index.
fn table(mut row: impl FnMut(usize, &Transaction) -> TransactionRow) -> Div {
    let mut body = div()
        .flex()
        .flex_col()
        .child(TransactionHeader::new().sorted_by(TransactionColumn::Amount));
    for (day, (date, net, first)) in DAYS.iter().enumerate() {
        let end = DAYS.get(day + 1).map_or(TRANSACTIONS.len(), |next| next.2);
        body = body.child(DayHeader::new(*date, *net).divided(day > 0));
        for (index, transaction) in TRANSACTIONS.iter().enumerate().take(end).skip(*first) {
            body = body.child(row(index, transaction));
        }
    }
    body
}

fn transaction_row(id: (&'static str, usize), transaction: &Transaction) -> TransactionRow {
    TransactionRow::new(
        id,
        transaction.merchant,
        transaction.category.cell(),
        transaction.amount,
    )
    .account(transaction.account)
    .flagged(transaction.flagged)
    .when_some(transaction.original, |this, original| {
        this.original_amount(original)
    })
}

/// Paper's table: Sainsbury's hovered, TfL and the coffee shop selected.
fn static_table() -> Div {
    table(|index, transaction| {
        let row = transaction_row(("static-txn", index), transaction);
        match index {
            1 => row.preview(PreviewState::Hovered),
            2 | 3 => row.selected(true),
            _ => row,
        }
    })
}

fn live_table(view: &mut Showcase, window: &Window, cx: &mut Context<Showcase>) -> Div {
    let table = table(|index, transaction| {
        transaction_row(("live-txn", index), transaction)
            .selected(view.transactions.is_selected(index))
            .focus_visible(view.transactions.is_focus_visible(index, window))
            .on_click(Showcase::row_click(transactions, index, cx))
    });
    div()
        .flex()
        .flex_col()
        .gap(spacing::S2)
        .child(
            view.list_region("live-transactions", transactions, cx)
                .child(table),
        )
        .child(caption(
            "Click; ⌘/Ctrl-click adds · Tab in: ↑ ↓ Home End move · Enter toggles",
        ))
}

fn lists_and_table(view: &mut Showcase, window: &Window, cx: &mut Context<Showcase>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(spacing::S6)
        .child(
            groups()
                .child(group("Live list", live_accounts(view, window, cx)))
                .child(group("List row states", list_row_states()))
                .child(
                    group("Transactions table", static_table())
                        .flex_1()
                        .min_w_0(),
                ),
        )
        .child(
            groups().child(
                group("Transactions table · live", live_table(view, window, cx))
                    .flex_1()
                    .min_w_0(),
            ),
        )
        .into_any_element()
}

// Surfaces.

/// The agent card's sample: Paper's copy, with `changes` replaced by live
/// handlers when `live`.
fn agent_card(prefix: &'static str, live: Option<&Context<Showcase>>) -> AgentCard {
    let changes = [
        (ChangeKind::Categorise, "Amazon Marketplace", "Shopping"),
        (ChangeKind::Categorise, "SQ *BLUE BOTTLE", "Eating out"),
        (ChangeKind::Rule, "Rule", "TfL → Transport"),
        (
            ChangeKind::TransferPair,
            "Transfer pair",
            "Savings ⇄ Current",
        ),
    ];
    let mut card = AgentCard::new("Claude")
        .context("Claude Code · 2h ago")
        .pending("12");
    for (index, (kind, subject, detail)) in changes.into_iter().enumerate() {
        let change = AgentChange::new((prefix, index), kind, subject)
            .detail(detail)
            .age("2h");
        let change = match live {
            Some(cx) => {
                let approve = cx.entity().downgrade();
                let reject = approve.clone();
                change
                    .on_approve(move |_, _, cx| {
                        approve
                            .update(cx, |view, cx| view.did(format!("Approved {subject}"), cx))
                            .ok();
                    })
                    .on_reject(move |_, _, cx| {
                        reject
                            .update(cx, |view, cx| view.did(format!("Rejected {subject}"), cx))
                            .ok();
                    })
            }
            // Paper shows the second change lifted, as under the pointer.
            None if index == 1 => change.preview(PreviewState::Hovered),
            None => change,
        };
        card = card.change(change);
    }
    card.action(
        Button::new(
            SharedString::from(format!("{prefix}-approve-all")),
            "Approve all (12)",
        )
        .with_variant(ButtonVariant::AgentPrimary)
        .with_full_width(true)
        .with_surface(color::MOLDAVITE_SURFACE),
    )
    .action(
        Button::new(
            SharedString::from(format!("{prefix}-reject-all")),
            "Reject all",
        )
        .with_variant(ButtonVariant::AgentOutline)
        .with_surface(color::MOLDAVITE_SURFACE),
    )
}

fn badges() -> Div {
    div().flex().flex_col().gap(spacing::S2).children(
        [
            ("Agent · pending suggestions", BadgeKind::Agent, "12"),
            ("Neutral · counts", BadgeKind::Neutral, "3"),
        ]
        .map(|(name, kind, count)| {
            div()
                .flex()
                .items_center()
                .gap(spacing::S2)
                .child(Badge::new(count).with_kind(kind))
                .child(caption(name))
        }),
    )
}

fn surfaces(cx: &mut Context<Showcase>) -> AnyElement {
    groups()
        .child(group("Agent card", agent_card("static-change", None)).w(size::INSPECTOR))
        .child(
            group(
                "Agent card · live",
                div()
                    .flex()
                    .flex_col()
                    .gap(spacing::S2)
                    .child(agent_card("live-change", Some(cx)))
                    .child(caption(
                        "Hover or Tab into a change to approve or reject it",
                    )),
            )
            .w(size::INSPECTOR),
        )
        .child(group("Badge", badges()).w(size::PANEL))
        .into_any_element()
}
