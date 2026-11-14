//! Selects and their menus (`docs/DESIGN.md` → "Accounts you keep
//! yourself" → Kind menu; Paper `FHZ-0`).
//!
//! The menu is a floating list: `float` fill, 1px `border`, `radius-lg`, the
//! Toolbar shadow and 4px padding. Options are 28px rows, radius 6, 8px
//! inline padding, 8px gaps: a 16px icon slot, the label in `text` with an
//! optional hint after a `·` in `text-muted`, and a 12px slot for the tick
//! that marks the chosen option, which also has the `hover` fill. Groups are
//! headed by a 24px row with a 10/12 caps label in `text-muted`; a group
//! after the first starts with a `hairline` 4px above its label.
//!
//! Composed for M3.2 (no Paper drawing): the highlighted option, moved with
//! the arrow keys, takes the `hover` fill too; an option that can't be
//! chosen is faded to `opacity-disabled`, with its hint saying why; a menu
//! taller than [`Select::max_height`] scrolls.
//!
//! Behaviour comes from the unstyled `gpui-base` select root: it owns the
//! combobox role, opening with Enter, Space and the arrows, closing with
//! Escape, and moving focus between the trigger and the menu. The owner
//! keeps the state ([`SelectState`]) and decides what choosing does.

use std::rc::Rc;

use gpui_kit::base::TestSupportExt as _;
use gpui_kit::base::actions::{SelectDown, SelectUp};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ElementId, FocusHandle, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, Pixels, RenderOnce, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, anchored, deferred, div,
};

use crate::theme::tokens::{color, opacity, radius, shadow, size, spacing, typography};
use crate::ui::field::SelectField;
use crate::ui::icon::Glyph;
use crate::ui::text::TextRoleExt as _;
use crate::ui::tracked::Tracked;

/// One choice in a menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuOption {
    /// Stable: what the owner gets back when it's chosen.
    pub key: SharedString,
    pub label: SharedString,
    /// Words after the label in `text-muted`: what the option is for, or
    /// why it can't be chosen.
    pub hint: Option<SharedString>,
    pub icon: Option<Glyph>,
    pub disabled: bool,
}

impl MenuOption {
    pub fn new(key: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            hint: None,
            icon: None,
            disabled: false,
        }
    }

    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn icon(mut self, glyph: Glyph) -> Self {
        self.icon = Some(glyph);
        self
    }

    /// Fades the option and keeps it from being chosen; its hint should say
    /// why.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// Options under an optional caps label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuGroup {
    pub label: Option<SharedString>,
    pub options: Vec<MenuOption>,
}

impl MenuGroup {
    pub fn new(label: Option<&'static str>, options: Vec<MenuOption>) -> Self {
        Self {
            label: label.map(SharedString::from),
            options,
        }
    }
}

/// The options that can be chosen, in menu order.
fn choosable(groups: &[MenuGroup]) -> impl Iterator<Item = &MenuOption> {
    groups
        .iter()
        .flat_map(|group| &group.options)
        .filter(|option| !option.disabled)
}

/// A select's state, kept by the view that owns it: whether its menu is
/// open, which option the keyboard has reached, and the focus handles of
/// the trigger and the menu.
pub struct SelectState {
    open: bool,
    highlighted: Option<SharedString>,
    trigger: FocusHandle,
    menu: FocusHandle,
}

impl SelectState {
    pub fn new(cx: &mut App) -> Self {
        Self {
            open: false,
            highlighted: None,
            trigger: cx.focus_handle(),
            menu: cx.focus_handle(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn highlighted(&self) -> Option<&SharedString> {
        self.highlighted.as_ref()
    }

    /// The trigger's focus handle, to focus the select from elsewhere.
    pub fn trigger(&self) -> &FocusHandle {
        &self.trigger
    }

    /// Opens or closes the menu. Opening highlights `chosen` and moves
    /// focus into the menu; closing returns it to the trigger.
    pub fn set_open(
        &mut self,
        open: bool,
        chosen: Option<&SharedString>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.open = open;
        if open {
            self.highlighted = chosen.cloned();
            self.menu.focus(window, cx);
        } else {
            self.trigger.focus(window, cx);
        }
    }

    /// Closes the menu without moving focus: the select has gone (another
    /// account was selected).
    pub fn dismiss(&mut self) {
        self.open = false;
        self.highlighted = None;
    }

    /// Moves the highlight `step` options down (up when negative), skipping
    /// options that can't be chosen and stopping at the ends.
    pub fn move_highlight(&mut self, groups: &[MenuGroup], step: isize) {
        let keys: Vec<&SharedString> = choosable(groups).map(|option| &option.key).collect();
        if keys.is_empty() {
            return;
        }
        let next = match self
            .highlighted
            .as_ref()
            .and_then(|key| keys.iter().position(|candidate| *candidate == key))
        {
            Some(at) => at.saturating_add_signed(step).min(keys.len() - 1),
            None if step < 0 => keys.len() - 1,
            None => 0,
        };
        self.highlighted = Some(keys[next].clone());
    }
}

type Toggle = Rc<dyn Fn(bool, &mut Window, &mut App)>;
type Choose = Rc<dyn Fn(&SharedString, &mut Window, &mut App)>;
type Move = Rc<dyn Fn(isize, &mut Window, &mut App)>;

/// A select: its closed field ([`SelectField`]) and, while open, its menu
/// below it. Stateless: the owner passes its [`SelectState`] in and handles
/// the three callbacks.
#[derive(IntoElement)]
pub struct Select {
    id: ElementId,
    field: SelectField,
    label: SharedString,
    groups: Vec<MenuGroup>,
    chosen: Option<SharedString>,
    open: bool,
    highlighted: Option<SharedString>,
    trigger: FocusHandle,
    menu: FocusHandle,
    width: Pixels,
    max_height: Option<Pixels>,
    on_toggle: Option<Toggle>,
    on_choose: Option<Choose>,
    on_move: Option<Move>,
}

impl Select {
    /// A select named `label` (its accessible name) drawn as `field`,
    /// offering `groups`, with `chosen` chosen.
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        field: SelectField,
        groups: Vec<MenuGroup>,
        chosen: Option<SharedString>,
        state: &SelectState,
    ) -> Self {
        Self {
            id: id.into(),
            field,
            label: label.into(),
            groups,
            chosen,
            open: state.open,
            highlighted: state.highlighted.clone(),
            trigger: state.trigger.clone(),
            menu: state.menu.clone(),
            // The Kind menu's width: the category and amount columns.
            width: size::COLUMN_CATEGORY + size::COLUMN_AMOUNT,
            max_height: None,
            on_toggle: None,
            on_choose: None,
            on_move: None,
        }
    }

    /// The menu's width; the Kind menu's 276px unless set.
    pub fn menu_width(mut self, width: Pixels) -> Self {
        self.width = width;
        self
    }

    /// Scrolls a menu taller than `height`.
    pub fn max_height(mut self, height: Pixels) -> Self {
        self.max_height = Some(height);
        self
    }

    /// Asked to open (`true`) or close: call [`SelectState::set_open`].
    pub fn on_toggle(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(handler));
        self
    }

    /// An option was chosen, by click or Enter. The menu stays open until
    /// the owner closes it.
    pub fn on_choose(
        mut self,
        handler: impl Fn(&SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_choose = Some(Rc::new(handler));
        self
    }

    /// The arrow keys moved the highlight: call
    /// [`SelectState::move_highlight`].
    pub fn on_move(mut self, handler: impl Fn(isize, &mut Window, &mut App) + 'static) -> Self {
        self.on_move = Some(Rc::new(handler));
        self
    }

    fn render_menu(&self) -> AnyElement {
        let mut rows: Vec<AnyElement> = Vec::new();
        for (index, group) in self.groups.iter().enumerate() {
            if let Some(label) = &group.label {
                rows.push(group_label(label.clone(), index > 0).into_any_element());
            }
            for option in &group.options {
                rows.push(self.render_option(option));
            }
        }
        let on_move_up = self.on_move.clone();
        let on_move_down = self.on_move.clone();
        let on_close = self.on_toggle.clone();
        div()
            .id("menu")
            .role(gpui_kit::Role::ListBox)
            .aria_label(self.label.clone())
            .track_focus(&self.menu)
            .occlude()
            .flex()
            .flex_col()
            .w(self.width)
            .p(spacing::S1)
            .bg(color::FLOAT)
            .border(size::HAIRLINE)
            .border_color(color::BORDER)
            .rounded(radius::LG)
            .shadow(vec![shadow::TOOLBAR.box_shadow()])
            .when_some(self.max_height, |this, height| {
                this.max_h(height).overflow_y_scroll()
            })
            .on_action(move |_: &SelectUp, window, cx| {
                cx.stop_propagation();
                if let Some(handler) = &on_move_up {
                    handler(-1, window, cx);
                }
            })
            .on_action(move |_: &SelectDown, window, cx| {
                cx.stop_propagation();
                if let Some(handler) = &on_move_down {
                    handler(1, window, cx);
                }
            })
            .on_mouse_down_out(move |_, window, cx| {
                if let Some(handler) = &on_close {
                    handler(false, window, cx);
                }
            })
            .children(rows)
            .into_any_element()
    }

    fn render_option(&self, option: &MenuOption) -> AnyElement {
        let chosen = self.chosen.as_ref() == Some(&option.key);
        let highlighted = self.highlighted.as_ref() == Some(&option.key);
        let key = option.key.clone();
        let on_choose = self.on_choose.clone();
        let icon_color = if chosen {
            color::TEXT_SECONDARY
        } else {
            color::TEXT_MUTED
        };
        div()
            .id(ElementId::Name(format!("option-{}", option.key).into()))
            .role(gpui_kit::Role::ListBoxOption)
            .aria_label(option.label.clone())
            .aria_selected(chosen)
            .when(highlighted, |this| this.aria_active_descendant())
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S2)
            .h(size::CONTROL)
            .px(spacing::S2)
            .rounded(radius::MD)
            .when(chosen || highlighted, |this| this.bg(color::HOVER))
            .when(option.disabled, |this| this.opacity(opacity::DISABLED))
            .when(!option.disabled, |this| {
                this.hover(|style| style.bg(color::HOVER))
                    .on_click(move |_, window, cx| {
                        if let Some(handler) = &on_choose {
                            handler(&key, window, cx);
                        }
                    })
            })
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(size::ICON_LG)
                    .when_some(option.icon, |this, glyph| {
                        this.child(glyph.render().text_color(icon_color))
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_role(typography::BODY)
                    .text_color(color::TEXT)
                    .child(option.label.clone())
                    .when_some(option.hint.clone(), |this, hint| {
                        this.child(
                            div()
                                .text_color(color::TEXT_MUTED)
                                .child(format!("\u{a0}· {hint}")),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .w(size::ICON_SM)
                    .when(chosen, |this| {
                        this.child(Glyph::Chosen.render().text_color(color::TEXT))
                    }),
            )
            .test_support()
            .into_any_element()
    }
}

/// A group's caps label: 24px, 8px inline padding; after the first group a
/// `hairline` 4px above it, and 4px more room (Paper `FHZ-0`).
fn group_label(label: SharedString, divided: bool) -> gpui_kit::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .px(spacing::S2)
        .h(size::CONTROL_SM)
        .text_color(color::TEXT_MUTED)
        .when(divided, |this| {
            this.mt(spacing::S1)
                .pt(spacing::S1)
                .h(size::CONTROL_SM + spacing::S1)
                .border_t(size::HAIRLINE)
                .border_color(color::HAIRLINE)
        })
        .child(Tracked::new(typography::MENU_LABEL, label))
}

impl RenderOnce for Select {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let menu = self.open.then(|| self.render_menu());
        let on_toggle = self.on_toggle.clone();
        let on_click = self.on_toggle.clone();
        let open = self.open;
        let highlighted = self.highlighted.clone();
        let on_confirm = self.on_choose.clone();
        let value = self
            .chosen
            .as_ref()
            .and_then(|key| {
                self.groups
                    .iter()
                    .flat_map(|group| &group.options)
                    .find(|option| &option.key == key)
            })
            .map(|option| option.label.clone());
        gpui_kit::base::Select::new(self.id)
            .open(self.open)
            .focus_handle(&self.trigger)
            .content_focus_handle(&self.menu)
            .accessibility_label(self.label)
            .when_some(value, |this, value| this.accessibility_value(value))
            .on_open_change(move |open, window, cx| {
                if let Some(handler) = &on_toggle {
                    handler(open, window, cx);
                }
            })
            .on_confirm(move |window, cx| {
                // After the root's own handling, which focuses the menu: the
                // owner closes it and puts focus back on the trigger.
                if let (Some(key), Some(handler)) = (highlighted.clone(), on_confirm.clone()) {
                    window.defer(cx, move |window, cx| handler(&key, window, cx));
                }
            })
            .flex()
            .flex_1()
            .min_w_0()
            .relative()
            // On press, not click: pressing the field while the menu is open
            // also closes it (outside the menu), and both must see this
            // frame's state, or the release would open it again.
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        if let Some(handler) = &on_click {
                            handler(!open, window, cx);
                        }
                    })
                    .child(self.field),
            )
            .when_some(menu, |this, menu| {
                // Below the field, drawn after everything else so neither a
                // scrolling inspector nor a dialog clips it; moved back
                // inside the window when it would run off.
                this.child(
                    div()
                        .absolute()
                        .top(size::CONTROL + spacing::S1)
                        .left_0()
                        .child(
                            deferred(
                                anchored()
                                    .snap_to_window_with_margin(spacing::S2)
                                    .child(menu),
                            )
                            .with_priority(2),
                        ),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn groups() -> Vec<MenuGroup> {
        vec![
            MenuGroup::new(
                Some("Imported from your bank"),
                vec![
                    MenuOption::new("current", "Current"),
                    MenuOption::new("savings", "Savings"),
                ],
            ),
            MenuGroup::new(
                Some("Kept by you"),
                vec![
                    MenuOption::new("asset", "Asset").disabled(true),
                    MenuOption::new("cash", "Cash"),
                ],
            ),
        ]
    }

    #[gpui_kit::test]
    fn the_highlight_skips_options_that_cant_be_chosen(cx: &mut gpui_kit::TestAppContext) {
        let groups = groups();
        cx.update(|cx| {
            let mut state = SelectState::new(cx);
            state.move_highlight(&groups, 1);
            assert_eq!(state.highlighted().map(|k| k.as_ref()), Some("current"));
            state.move_highlight(&groups, 1);
            state.move_highlight(&groups, 1);
            assert_eq!(state.highlighted().map(|k| k.as_ref()), Some("cash"));
            // Stops at the end, and skips the disabled asset going back.
            state.move_highlight(&groups, 1);
            assert_eq!(state.highlighted().map(|k| k.as_ref()), Some("cash"));
            state.move_highlight(&groups, -1);
            assert_eq!(state.highlighted().map(|k| k.as_ref()), Some("savings"));
            state.move_highlight(&groups, -5);
            assert_eq!(state.highlighted().map(|k| k.as_ref()), Some("current"));
        });
    }
}
