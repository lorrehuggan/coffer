//! The stock components board: `gpui-component` controls as the theme map
//! (`theme::component`) draws them, unwrapped, beside the spec they'll be
//! held to (`docs/DESIGN.md` → "Implementing in GPUI" → "Stock components",
//! the rows marked "check in D2.5").
//!
//! This board shows what the stock control does today, not what Coffer
//! ships. Where it disagrees with the caption, the control needs a wrapper
//! or a custom build before product UI uses it.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogFooter};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::select::{Select, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{IndexPath, TitleBar, WindowExt as _};
use gpui_kit::{
    AnyElement, AppContext as _, Context, Div, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Styled as _, Window, actions, div,
};

use super::{Showcase, caption, group, groups, section};
use crate::theme::tokens::{color, size, spacing, typography};
use crate::ui::text::TextRoleExt as _;

actions!(coffer_showcase, [Categorise, MarkAsTransfer, CreateRule]);

/// The accounts in the stock select.
const ACCOUNTS: [&str; 3] = ["All accounts", "Current account", "Joint account"];

/// The views in the underline tabs.
const TABS: [&str; 3] = ["Details", "Rules", "History"];

/// State the stock controls keep in their owner.
pub(super) struct StockState {
    search: Entity<InputState>,
    filled: Entity<InputState>,
    disabled: Entity<InputState>,
    account: Entity<SelectState<Vec<&'static str>>>,
    tab: usize,
    checked: bool,
    switched: bool,
}

impl StockState {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Showcase>) -> Self {
        Self {
            search: cx.new(|cx| InputState::new(window, cx).placeholder("Search transactions")),
            filled: cx.new(|cx| InputState::new(window, cx).default_value("Pret A Manger")),
            disabled: cx.new(|cx| InputState::new(window, cx).default_value("Current account")),
            account: cx.new(|cx| {
                SelectState::new(ACCOUNTS.to_vec(), Some(IndexPath::default()), window, cx)
            }),
            tab: 0,
            checked: true,
            switched: false,
        }
    }

    /// The search input's focus handle, for tests that need focus inside
    /// this board.
    #[cfg(test)]
    pub(super) fn search_focus(&self, cx: &gpui_kit::App) -> gpui_kit::FocusHandle {
        gpui_kit::Focusable::focus_handle(self.search.read(cx), cx)
    }
}

pub(super) fn board(view: &mut Showcase, cx: &mut Context<Showcase>) -> Div {
    super::board(
        "Coffer · Stock components",
        "Stock components",
        "gpui-component controls as the theme map draws them, unwrapped, each captioned with the spec it will be held to.",
        [
            section("Fields", fields(&view.stock)),
            section("Overlays", overlays(cx)),
            section("Navigation", navigation(&view.stock, cx)),
            section("Stock focus", stock_focus(&view.stock, cx)),
        ],
    )
    .on_action(cx.listener(|this, _: &Categorise, _, cx| this.did("Menu: Categorise", cx)))
    .on_action(cx.listener(|this, _: &MarkAsTransfer, _, cx| {
        this.did("Menu: Mark as transfer", cx)
    }))
    .on_action(cx.listener(|this, _: &CreateRule, _, cx| this.did("Menu: Create rule", cx)))
}

/// A group of one stock control and the spec to compare it with.
fn specimen(title: &'static str, control: impl IntoElement, spec: &'static str) -> Div {
    group(
        title,
        div()
            .flex()
            .flex_col()
            .gap(spacing::S2)
            .child(control)
            .child(caption(spec)),
    )
    .w(size::PANEL)
}

fn fields(stock: &StockState) -> AnyElement {
    groups()
        .child(specimen(
            "Input · empty",
            Input::new(&stock.search),
            "Spec: 28px · field fill · hairline · radius 6 · 12/16 · placeholder text-muted",
        ))
        .child(specimen(
            "Input · filled",
            Input::new(&stock.filled),
            "Stock fill is the input border at 30%; the wrapper sets field (D2.4)",
        ))
        .child(specimen(
            "Input · disabled",
            Input::new(&stock.disabled).disabled(true),
            "Spec: resting look at opacity-disabled",
        ))
        .child(specimen(
            "Select",
            Select::new(&stock.account),
            "Spec (KK-0): field style · 10px left / 8px right · value text · chevron 8px text-muted",
        ))
        .into_any_element()
}

fn overlays(cx: &mut Context<Showcase>) -> AnyElement {
    let dialog = Button::new("stock-dialog")
        .outline()
        .label("Delete import…")
        .on_click(cx.listener(|_, _, window, cx| {
            window.open_dialog(cx, |dialog, _, _| {
                dialog
                    .title("Delete this import?")
                    .child("312 transactions from monzo-october.csv will be removed. You can import the file again later.")
                    .footer(
                        DialogFooter::new()
                            .gap(spacing::S2)
                            .child(DialogClose::new().child(
                                Button::new("stock-dialog-cancel").outline().label("Cancel"),
                            ))
                            .child(DialogAction::new().child(
                                Button::new("stock-dialog-delete").danger().label("Delete 312 transactions"),
                            )),
                    )
            });
        }));

    groups()
        .child(specimen(
            "Popover",
            Popover::new("stock-popover")
                .trigger(Button::new("stock-popover-trigger").outline().label("Filters"))
                .content(|_, _, _| {
                    div()
                        .p(spacing::S3)
                        .text_role(typography::BODY)
                        .text_color(color::TEXT)
                        .child("Popover content")
                }),
            "Spec: float fill · 1px border · radius-lg · toolbar shadow (stock: shadcn ring at radius)",
        ))
        .child(specimen(
            "Menu",
            Button::new("stock-menu")
                .outline()
                .label("Actions")
                .dropdown_menu(|menu, _, _| {
                    menu.menu("Categorise", Box::new(Categorise))
                        .menu("Mark as transfer", Box::new(MarkAsTransfer))
                        .separator()
                        .menu("Create rule", Box::new(CreateRule))
                }),
            "Spec: as popover; items 28px · radius 6 · hover fill",
        ))
        .child(specimen(
            "Dialog",
            dialog,
            "Spec: float fill · radius-lg · backdrop chrome at opacity-muted (provisional)",
        ))
        .into_any_element()
}

fn navigation(stock: &StockState, cx: &mut Context<Showcase>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(spacing::S6)
        .child(
            group(
                "Title bar",
                div()
                    .flex()
                    .flex_col()
                    .gap(spacing::S2)
                    .child(
                        TitleBar::new().child(
                            div()
                                .text_role(typography::SECTION_TITLE)
                                .text_color(color::TEXT)
                                .child("Coffer"),
                        ),
                    )
                    .child(caption(
                        "Spec (OU-0): 40px · chrome · bottom hairline · 12px left / 8px right · 4px gap (stock: 34px constant)",
                    )),
            ),
        )
        .child(specimen(
            "Underline tabs",
            TabBar::new("stock-tabs")
                .underline()
                .selected_index(stock.tab)
                .on_click(cx.listener(|this, index: &usize, _, cx| {
                    this.stock.tab = *index;
                    cx.notify();
                }))
                .children(TABS.map(|title| Tab::new().label(title))),
            "Spec (R8-0): 12/16 500 · active text with 1.5px text underline · inactive text-muted · no height given",
        ))
        .into_any_element()
}

fn stock_focus(stock: &StockState, cx: &mut Context<Showcase>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(spacing::S2)
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(spacing::S3)
                .child(Button::new("stock-primary").primary().label("Primary"))
                .child(Button::new("stock-outline").outline().label("Outline"))
                .child(Button::new("stock-ghost").ghost().label("Ghost"))
                .child(Button::new("stock-danger").danger().label("Danger"))
                .child(
                    Checkbox::new("stock-checkbox")
                        .label("Include pots")
                        .checked(stock.checked)
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.stock.checked = *checked;
                            cx.notify();
                        })),
                )
                .child(
                    Switch::new("stock-switch")
                        .label("Hide transfers")
                        .checked(stock.switched)
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.stock.switched = *checked;
                            cx.notify();
                        })),
                ),
        )
        .child(caption(
            "Tab through these and the fields above. Spec: 1.5px molten ring, 2px out, keyboard focus only (the Coffer buttons in the Live row)",
        ))
        .into_any_element()
}
