//! The title bar (`docs/DESIGN.md` → "Components" → **Title bar**, Paper
//! `OU-0`; on the Rules artboard `B8Y-0`, with the Settings button on
//! Settings · AI clients `B08-0`): Coffer's mark and wordmark, the view
//! tabs, the command bar, the Settings button and the window controls.
//!
//! Custom rather than the stock `TitleBar`, which is a fixed 34px (Coffer's
//! is 40) and draws its own window controls. The window behaviour is the
//! stock title bar's: the bar is the window's drag area, a press-and-move
//! starts a window move, a double click maximises, and on Linux a right
//! click opens the window menu. The window must be opened with
//! [`window_options`], which hands the title bar to the app.
//!
//! The window controls are drawn on Linux only, where the window is
//! client-decorated. macOS draws its own traffic lights into the bar
//! (not yet laid out: the mark and wordmark would need to move clear of them).

use gpui_kit::base::{InteractiveElementExt as _, Tab, Tabs};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _,
    TitlebarOptions, Window, WindowControlArea, WindowDecorations, WindowOptions, div,
};

use crate::theme::tokens::typography::TextRole;
use crate::theme::tokens::{color, radius, size, spacing, typography, weight};
use crate::ui::badge::Badge;
use crate::ui::button::IconButton;
use crate::ui::icon::Glyph;
use crate::ui::kbd::Kbd;
use crate::ui::separator;
use crate::ui::text::TextRoleExt as _;
use crate::ui::tracked::Tracked;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The wordmark: title type in `text`, after the mark (`DESIGN.md` → "Logo").
const WORDMARK: TextRole = typography::TITLE;

/// A view tab's label: body type at medium weight.
const TAB_LABEL: TextRole = TextRole {
    weight: weight::MEDIUM,
    ..typography::BODY
};

/// Window options for a window whose title bar is a [`TitleBar`]: the app
/// draws the title bar, moves the window itself, and on Linux draws the
/// window's decorations.
pub fn window_options() -> WindowOptions {
    WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some("Coffer".into()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        app_owns_titlebar_drag: true,
        window_decorations: Some(WindowDecorations::Client),
        ..WindowOptions::default()
    }
}

/// One of Coffer's views, as a tab in the title bar.
///
/// Built on the unstyled `gpui-base` [`Tab`]: it owns the click, the
/// accessible tab role with its selected state and position, and stops the
/// press from reaching the title bar, so pressing a tab never starts a
/// window move. Tabs aren't Tab stops; the views have shortcuts instead
/// (`crate::shell`). Rendered by the [`TitleBar`] it's added to, which
/// numbers the tabs.
pub struct ViewTab {
    id: ElementId,
    label: SharedString,
    active: bool,
    badge: Option<SharedString>,
    on_click: Option<ClickHandler>,
}

impl ViewTab {
    /// An inactive tab with no badge.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            active: false,
            badge: None,
            on_click: None,
        }
    }

    /// Marks the current view: `hover` fill and a `text` label.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Shows an agent badge after the label: the Inbox's pending
    /// suggestions.
    pub fn badge(mut self, count: impl Into<SharedString>) -> Self {
        self.badge = Some(count.into());
        self
    }

    /// Runs `handler` when the tab is clicked.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// The tab as drawn, `position` of `count` in its list (one-based).
    fn into_tab(self, position: usize, count: usize) -> Tab {
        Tab::new(self.id)
            .selected(self.active)
            .set_position(position, count)
            .accessibility_label(self.label.clone())
            .flex_none()
            .gap(spacing::S1_5)
            .h(size::CONTROL)
            .px(spacing::S2_5)
            .rounded(radius::MD)
            .when(self.active, |this| this.bg(color::HOVER))
            .when(!self.active, |this| {
                this.hover(|style| style.bg(color::HOVER))
            })
            .child(
                div()
                    .whitespace_nowrap()
                    .text_role(TAB_LABEL)
                    .text_color(if self.active {
                        color::TEXT
                    } else {
                        color::TEXT_MUTED
                    })
                    .child(self.label),
            )
            .when_some(self.badge, |this, count| this.child(Badge::new(count)))
            .when_some(self.on_click, |this, handler| {
                this.on_click(move |event, window, cx| handler(event, window, cx))
            })
    }
}

/// The command bar: a 340px field that opens search and commands (⌘K).
#[derive(IntoElement)]
pub struct CommandBar {
    placeholder: SharedString,
    on_click: Option<ClickHandler>,
}

impl CommandBar {
    pub fn new(placeholder: impl Into<SharedString>) -> Self {
        Self {
            placeholder: placeholder.into(),
            on_click: None,
        }
    }

    /// Runs `handler` when the bar is clicked: open the command palette.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for CommandBar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .id("command-bar")
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S2)
            .w(size::COMMAND_BAR)
            .h(size::CONTROL)
            .pl(spacing::S2_5)
            .pr(spacing::S1_5)
            .rounded(radius::BUTTON)
            .bg(color::FIELD)
            .border(size::HAIRLINE)
            .border_color(color::HAIRLINE)
            .child(Glyph::Search.render().text_color(color::TEXT_MUTED))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_role(typography::BODY)
                    .text_color(color::TEXT_MUTED)
                    .child(self.placeholder),
            )
            .child(Kbd::new("⌘K"))
            .when_some(self.on_click, |this, handler| {
                this.on_click(move |event, window, cx| handler(event, window, cx))
            })
    }
}

/// Whether a press on the bar may still become a window move.
struct DragState {
    pressed: bool,
}

/// The title bar: 40px, `chrome`, bottom `hairline`.
///
/// Takes the brand-and-tabs group (mark, wordmark, view tabs), the command
/// bar, the Settings button and the window controls. The tabs group and the
/// controls share the width either side of the command bar, which is centred
/// when the tabs leave room. At 1440 the eight tabs don't, so it follows them
/// (`DESIGN.md` → "Transactions").
#[derive(IntoElement)]
pub struct TitleBar {
    tabs: Vec<ViewTab>,
    command_bar: CommandBar,
    settings: Option<IconButton>,
}

impl TitleBar {
    pub fn new(command_bar: CommandBar) -> Self {
        Self {
            tabs: Vec::new(),
            command_bar,
            settings: None,
        }
    }

    /// Puts `button` at the right, before the window controls and a 1×14
    /// `hairline` (`DESIGN.md` → "Settings"): the Settings button, active
    /// while Settings is open.
    pub fn settings(mut self, button: IconButton) -> Self {
        self.settings = Some(button);
        self
    }

    /// Adds a view tab after the others; tabs are separated by a 1×14
    /// `hairline`.
    pub fn tab(mut self, tab: ViewTab) -> Self {
        self.tabs.push(tab);
        self
    }
}

impl RenderOnce for TitleBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let drag = window.use_keyed_state("coffer-title-bar-drag", cx, |_, _| DragState {
            pressed: false,
        });
        let linux = cfg!(target_os = "linux");

        let mut views = div()
            .flex()
            .flex_1()
            .items_center()
            .gap(spacing::S1)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(spacing::S1_5)
                    .child(Glyph::Mark.render().text_color(color::MOLTEN))
                    .child(
                        div()
                            .text_color(color::TEXT)
                            .child(Tracked::new(WORDMARK, "Coffer")),
                    ),
            )
            .child(div().flex_none().w(spacing::S3));
        let count = self.tabs.len();
        let mut tabs = Tabs::new("view-tabs")
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S1);
        for (index, tab) in self.tabs.into_iter().enumerate() {
            if index > 0 {
                tabs = tabs.child(separator(spacing::S3_5, color::HAIRLINE));
            }
            tabs = tabs.child(tab.into_tab(index + 1, count));
        }
        views = views.child(tabs);

        // Paper spaces the rule by a 4px spacer on each side, inside the
        // controls' own 2px gap.
        let spacer = || div().flex_none().w(spacing::S1);
        let controls = div()
            .flex()
            .flex_1()
            .items_center()
            .justify_end()
            .gap(spacing::S0_5)
            .when_some(self.settings, |this, settings| {
                this.child(settings).when(linux, |this| {
                    this.child(spacer())
                        .child(separator(spacing::S3_5, color::HAIRLINE))
                        .child(spacer())
                })
            })
            .when(linux, |this| {
                this.child(
                    IconButton::new("window-minimize", Glyph::Minimize, "Minimise")
                        .on_click(|_, window, _| window.minimize_window()),
                )
                .child(
                    IconButton::new("window-maximize", Glyph::Maximize, "Maximise")
                        .on_click(|_, window, _| window.zoom_window()),
                )
                .child(
                    IconButton::new("window-close", Glyph::Close, "Close")
                        .on_click(|_, window, _| window.remove_window()),
                )
            });

        div()
            .id("title-bar")
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S1)
            .w_full()
            .h(size::TITLE_BAR)
            .pl(spacing::S3)
            .pr(spacing::S2)
            .bg(color::CHROME)
            .border_b(size::HAIRLINE)
            .border_color(color::HAIRLINE)
            .window_control_area(WindowControlArea::Drag)
            .on_double_click(|_, window, _| window.zoom_window())
            .on_mouse_down(
                MouseButton::Left,
                window.listener_for(&drag, |state, _, _, _| state.pressed = true),
            )
            .on_mouse_up(
                MouseButton::Left,
                window.listener_for(&drag, |state, _, _, _| state.pressed = false),
            )
            .on_mouse_down_out(window.listener_for(&drag, |state, _, _, _| state.pressed = false))
            .on_mouse_move(window.listener_for(&drag, |state, _, window, _| {
                if state.pressed {
                    state.pressed = false;
                    window.start_window_move();
                }
            }))
            .when(linux, |this| {
                this.on_mouse_down(MouseButton::Right, |event, window, _| {
                    window.show_window_menu(event.position)
                })
            })
            .child(views)
            .child(self.command_bar)
            .child(controls)
    }
}
