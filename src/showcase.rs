//! The component showcase, `coffer --showcase` (`docs/ROADMAP.md` D2.5).
//!
//! One scrolling window that puts Coffer's tokens and controls next to the
//! Paper file they come from, board by board:
//!
//! - **Foundations** (Paper `1-0`): colour swatches and the type scale.
//! - **Components** (Paper `CJ-0`): the button state matrix, list rows, the
//!   transactions table and the agent card. Each has a static copy of the
//!   Paper board, with states forced through [`PreviewState`], for a
//!   side-by-side comparison, and a live copy that reacts to the pointer and
//!   keyboard.
//! - **Stock components**: the `gpui-component` controls `docs/DESIGN.md`
//!   uses unwrapped, as the stock theme map draws them.
//!
//! Development only: nothing here is product UI, and it shows synthetic data.
//!
//! [`PreviewState`]: crate::ui::PreviewState

mod components;
mod foundations;
mod stock;

use std::collections::BTreeSet;

use gpui_kit::base::actions::{Confirm, SelectDown, SelectFirst, SelectLast, SelectUp};
use gpui_kit::{
    App, AppContext as _, Bounds, ClickEvent, Context, Div, FocusHandle, InteractiveElement as _,
    IntoElement, KeyBinding, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, WindowBounds, WindowOptions, div, px,
    size,
};

use crate::theme::tokens::typography::TextRole;
use crate::theme::tokens::{color, size as token_size, spacing, typography, weight};
use crate::ui::text::{CasedText as _, TextRoleExt as _};

/// The key context of the live lists, where the arrow keys move.
const LIST_CONTEXT: &str = "CofferShowcaseList";

/// Binds the live lists' keys. Call once, before [`open`].
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectUp, Some(LIST_CONTEXT)),
        KeyBinding::new("down", SelectDown, Some(LIST_CONTEXT)),
        KeyBinding::new("home", SelectFirst, Some(LIST_CONTEXT)),
        KeyBinding::new("end", SelectLast, Some(LIST_CONTEXT)),
        KeyBinding::new("enter", Confirm { secondary: false }, Some(LIST_CONTEXT)),
        KeyBinding::new("space", Confirm { secondary: false }, Some(LIST_CONTEXT)),
    ]);
}

/// Opens the showcase window, sized like the Paper artboards (1920 wide)
/// where the screen allows.
pub fn open(cx: &mut App) -> gpui_kit::Result<()> {
    let bounds = Bounds::centered(None, size(px(1920.), px(1200.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(
            token_size::WINDOW_MIN_WIDTH,
            token_size::WINDOW_MIN_HEIGHT,
        )),
        ..WindowOptions::default()
    };
    gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| Showcase::new(window, cx))
    })?;
    Ok(())
}

/// The showcase window's root view. Holds the live controls' state.
pub struct Showcase {
    accounts: LiveList,
    transactions: LiveList,
    /// What the last live button, chip or agent action was, as feedback.
    last_action: SharedString,
    stock: stock::StockState,
}

impl Showcase {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            accounts: LiveList::new(
                cx.focus_handle().tab_stop(true),
                components::ACCOUNTS.len(),
                Selection::Single,
            ),
            transactions: LiveList::new(
                cx.focus_handle().tab_stop(true),
                components::TRANSACTIONS.len(),
                Selection::Multiple,
            ),
            last_action: "Nothing yet".into(),
            stock: stock::StockState::new(window, cx),
        }
        .with_initial_selection()
    }

    fn with_initial_selection(mut self) -> Self {
        self.accounts.selected.insert(0);
        self
    }

    /// Records `action` as the last live action.
    fn did(&mut self, action: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.last_action = action.into();
        cx.notify();
    }

    /// A focus region for the live list `which`: its key context, focus and
    /// the arrow-key actions.
    fn list_region(
        &mut self,
        id: &'static str,
        which: fn(&mut Self) -> &mut LiveList,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<Div> {
        let focus = which(self).focus.clone();
        div()
            .id(id)
            .key_context(LIST_CONTEXT)
            .track_focus(&focus)
            .on_action(cx.listener(move |this, _: &SelectUp, _, cx| {
                which(this).move_by(-1);
                cx.notify();
            }))
            .on_action(cx.listener(move |this, _: &SelectDown, _, cx| {
                which(this).move_by(1);
                cx.notify();
            }))
            .on_action(cx.listener(move |this, _: &SelectFirst, _, cx| {
                which(this).current = 0;
                cx.notify();
            }))
            .on_action(cx.listener(move |this, _: &SelectLast, _, cx| {
                let list = which(this);
                list.current = list.len.saturating_sub(1);
                cx.notify();
            }))
            .on_action(cx.listener(move |this, _: &Confirm, _, cx| {
                which(this).confirm();
                cx.notify();
            }))
    }

    /// The click handler for row `index` of the live list `which`.
    fn row_click(
        which: fn(&mut Self) -> &mut LiveList,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        cx.listener(move |this, event: &ClickEvent, window, cx| {
            let list = which(this);
            let toggle = event.modifiers().secondary(cx);
            list.click(index, toggle);
            list.focus.clone().focus(window, cx);
            cx.notify();
        })
    }
}

impl Render for Showcase {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("showcase")
            .size_full()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .bg(color::CANVAS)
            .text_role(typography::BODY)
            .text_color(color::TEXT)
            .child(foundations::board())
            .child(components::board(self, window, cx))
            .child(stock::board(self, cx))
    }
}

/// How many rows of a live list can be selected at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Selection {
    Single,
    Multiple,
}

/// The state of a live list: which row the keyboard is on and which rows
/// are selected.
struct LiveList {
    focus: FocusHandle,
    len: usize,
    mode: Selection,
    current: usize,
    selected: BTreeSet<usize>,
}

impl LiveList {
    fn new(focus: FocusHandle, len: usize, mode: Selection) -> Self {
        Self {
            focus,
            len,
            mode,
            current: 0,
            selected: BTreeSet::new(),
        }
    }

    /// Moves the keyboard row by `delta`, stopping at the ends.
    fn move_by(&mut self, delta: isize) {
        let last = self.len.saturating_sub(1);
        self.current = self.current.saturating_add_signed(delta).min(last);
    }

    /// Enter or Space on the keyboard row: selects it, or toggles it in a
    /// multiple selection.
    fn confirm(&mut self) {
        self.select(self.current, self.mode == Selection::Multiple);
    }

    /// A click on row `index`; `toggle` when the platform's secondary
    /// modifier (⌘ or Ctrl) was held.
    fn click(&mut self, index: usize, toggle: bool) {
        self.current = index;
        self.select(index, toggle && self.mode == Selection::Multiple);
    }

    fn select(&mut self, index: usize, toggle: bool) {
        if toggle {
            if !self.selected.remove(&index) {
                self.selected.insert(index);
            }
        } else {
            self.selected = BTreeSet::from([index]);
        }
    }

    fn is_selected(&self, index: usize) -> bool {
        self.selected.contains(&index)
    }

    /// Whether row `index` draws the focus ring: it's the keyboard row and
    /// the list has keyboard focus.
    fn is_focus_visible(&self, index: usize, window: &Window) -> bool {
        index == self.current && self.focus.is_focused(window) && window.last_input_was_keyboard()
    }
}

// Layout shared by the boards, after the Paper artboards' own frames.

/// A whole artboard: a header, then its sections, 24px apart.
fn board(
    eyebrow: &'static str,
    title: &'static str,
    subtitle: &'static str,
    sections: impl IntoIterator<Item = gpui_kit::AnyElement>,
) -> Div {
    let label = typography::SECTION_LABEL;
    div()
        .flex()
        .flex_none()
        .flex_col()
        .gap(spacing::S6)
        .p(spacing::S6)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(spacing::S1)
                .pb(spacing::S3)
                .child(
                    div()
                        .text_role(label)
                        .text_color(color::TEXT_MUTED)
                        .child(label.cased(eyebrow)),
                )
                .child(
                    div()
                        .text_role(typography::DISPLAY)
                        .text_color(color::TEXT)
                        .child(title),
                )
                .child(
                    div()
                        .text_role(typography::READING_BODY)
                        .text_color(color::TEXT_SECONDARY)
                        .child(subtitle),
                ),
        )
        .children(sections)
}

/// A board section: its caps label, then its content 12px below.
fn section(label: &'static str, content: impl IntoElement) -> gpui_kit::AnyElement {
    let role = typography::SECTION_LABEL;
    div()
        .flex()
        .flex_col()
        .gap(spacing::S3)
        .child(
            div()
                .text_role(role)
                .text_color(color::TEXT_MUTED)
                .child(role.cased(label)),
        )
        .child(content)
        .into_any_element()
}

/// A titled group inside a section: the title in 12/16 600, 4px of extra
/// space, then the content 8px below.
fn group(title: &'static str, content: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(spacing::S2)
        .child(
            div()
                .pb(spacing::S1)
                .text_role(typography::SECTION_TITLE)
                .text_color(color::TEXT)
                .child(title),
        )
        .child(content)
}

/// The row name and spec line of a matrix (buttons, type scale): 272px wide,
/// the name in 12/16 500 and the spec in mono 10/12 `text-muted`.
fn spec(name: &'static str, detail: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_none()
        .flex_col()
        .gap(spacing::S0_5)
        .w(token_size::PANEL)
        .child(
            div()
                .text_role(ROW_NAME)
                .text_color(color::TEXT)
                .child(name),
        )
        .child(
            div()
                .text_role(typography::MONO_MICRO)
                .text_color(color::TEXT_MUTED)
                .child(detail.into()),
        )
}

/// A matrix row's name: body type at 500.
const ROW_NAME: TextRole = TextRole {
    weight: weight::MEDIUM,
    ..typography::BODY
};

/// A caption under a live control: mono 10/12 `text-muted`.
fn caption(text: impl Into<SharedString>) -> Div {
    div()
        .text_role(typography::MONO_MICRO)
        .text_color(color::TEXT_MUTED)
        .child(text.into())
}

/// A wrapping row of groups, 24px apart, top-aligned.
fn groups() -> Div {
    div().flex().flex_wrap().items_start().gap(spacing::S6)
}

#[cfg(test)]
mod tests {
    use gpui_kit::{TestAppContext, VisualTestContext};

    use super::*;
    use crate::theme;

    /// The whole showcase in a test window, drawn once.
    fn open_showcase(
        cx: &mut TestAppContext,
    ) -> (gpui_kit::Entity<Showcase>, &mut VisualTestContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::component::install(cx);
            init(cx);
        });
        let (view, cx) = cx.add_window_view(Showcase::new);
        cx.update(|window, cx| window.draw(cx).clear(cx));
        (view, cx)
    }

    #[gpui_kit::test]
    fn the_live_lists_follow_their_keys(cx: &mut TestAppContext) {
        let (view, cx) = open_showcase(cx);

        let focus = view.read_with(cx, |view, _| view.accounts.focus.clone());
        cx.update(|window, cx| focus.focus(window, cx));
        cx.simulate_keystrokes("down down enter");
        view.read_with(cx, |view, _| {
            assert_eq!(view.accounts.current, 2);
            assert_eq!(view.accounts.selected, BTreeSet::from([2]));
            assert!(
                view.transactions.selected.is_empty(),
                "keys stay in their list"
            );
        });

        let focus = view.read_with(cx, |view, _| view.transactions.focus.clone());
        cx.update(|window, cx| focus.focus(window, cx));
        cx.simulate_keystrokes("end enter home enter");
        view.read_with(cx, |view, _| {
            let last = components::TRANSACTIONS.len() - 1;
            assert_eq!(view.transactions.selected, BTreeSet::from([0, last]));
        });
    }
    #[gpui_kit::test]
    fn stock_menu_items_reach_the_showcase(cx: &mut TestAppContext) {
        let (view, cx) = open_showcase(cx);
        // Focus inside the stock board, where the menu dispatches from.
        let focus = view.read_with(cx, |view, cx| view.stock.search_focus(cx));
        cx.update(|window, cx| focus.focus(window, cx));
        cx.dispatch_action(stock::MarkAsTransfer);
        view.read_with(cx, |view, _| {
            assert_eq!(view.last_action.as_ref(), "Menu: Mark as transfer");
        });
    }

    fn list(cx: &mut TestAppContext, mode: Selection) -> LiveList {
        let focus = cx.update(|cx| cx.focus_handle());
        LiveList::new(focus, 4, mode)
    }

    #[gpui_kit::test]
    fn keyboard_movement_stops_at_the_ends(cx: &mut TestAppContext) {
        let mut list = list(cx, Selection::Single);
        list.move_by(-1);
        assert_eq!(list.current, 0);
        list.move_by(10);
        assert_eq!(list.current, 3);
    }

    #[gpui_kit::test]
    fn single_selection_replaces_and_multiple_toggles(cx: &mut TestAppContext) {
        let mut single = list(cx, Selection::Single);
        single.click(1, true);
        single.click(2, true);
        assert_eq!(single.selected, BTreeSet::from([2]), "⌘ doesn't add");

        let mut multiple = list(cx, Selection::Multiple);
        multiple.click(1, false);
        multiple.click(2, true);
        assert_eq!(multiple.selected, BTreeSet::from([1, 2]));
        multiple.current = 1;
        multiple.confirm();
        assert_eq!(multiple.selected, BTreeSet::from([2]), "Enter toggles");
        multiple.click(3, false);
        assert_eq!(
            multiple.selected,
            BTreeSet::from([3]),
            "a plain click replaces"
        );
    }
}
