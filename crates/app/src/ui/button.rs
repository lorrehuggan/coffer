//! Coffer's buttons (`docs/DESIGN.md` → "Components" → **Buttons**).
//!
//! [`Button`] (a text label), [`IconButton`] and [`Chip`] wrap the stock
//! `gpui-component` button, which keeps its behaviour: pointer and
//! Enter/Space activation, Tab order, disabled handling and the accessibility
//! role and name. What it looks like comes from [`crate::theme::tokens`]:
//!
//! - Height, padding, radius, border, fill and label colour are set on the
//!   stock button per instance, and the content is one child styled with a
//!   [`TextRole`]. The stock label's text size is fixed by the stock `Size`,
//!   so it can't take Coffer's type scale, and the stock content row puts its
//!   own 0.5rem gap between children, so an icon and its label go in as one
//!   child with Coffer's gap.
//! - Hover and pressed fills go through `ButtonVariant::Custom`. The stock
//!   hover and pressed styles also set the border to the variant's own border
//!   colour; only `Custom` lets that colour be a token (`border-strong`,
//!   `hairline`, `danger`), so outlines keep their border in every state.
//! - An icon button's icon brightens from `text-muted` to `text` on hover and
//!   press. `Custom` has one foreground for every state, so the icon follows
//!   the button through `group_hover` instead.
//! - Disabled is the resting look faded to [`opacity::DISABLED`] as one
//!   group, as Paper fades it. GPUI's `opacity` isn't group opacity, so a
//!   button with a fill under its content (the solid variants, agent ghost,
//!   or a shortcut's key cap) paints every colour opaque, composited over the
//!   surface it sits on ([`faded`]; [`Button::with_surface`], `canvas` by
//!   default). The rest, with nothing under their content, use element
//!   `opacity`, which is exact for them. The stock button already drops hover
//!   and pressed, ignores clicks, leaves the Tab order and keeps the default
//!   cursor.
//! - Focus: the stock focus style is off. The button tracks a [`FocusHandle`]
//!   and draws Coffer's ring with
//!   [`focus_visible_ring`](crate::theme::focus::FocusRingExt::focus_visible_ring),
//!   so it shows on keyboard focus only. The stock button puts extra children
//!   inside its content box, which clips, not its outer box, so the ring is
//!   drawn on a wrapper with the button's radius.
//! - [`PreviewState`] draws hover, pressed or the focus ring without the
//!   input, for the showcase's state matrix.
//!
//! Sizes are the tokens' pixels, not rems, like every Coffer component
//! (`DESIGN.md` → "Rem base").

use gpui_kit::component::button::{
    Button as StockButton, ButtonCustomVariant, ButtonVariants as _,
};
use gpui_kit::component::{Disableable, FocusableExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, FocusHandle, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, RenderOnce, Rgba, SharedString, Styled as _, Window,
    div, transparent_black,
};

use crate::theme::focus::{FocusRingExt as _, FocusRingPlacement};
use crate::theme::tokens::typography::{self, TextRole};
use crate::theme::tokens::{color, opacity, radius, size, spacing, weight};
use crate::ui::icon::Glyph;
use crate::ui::kbd::{Kbd, KbdSize};
use crate::ui::text::TextRoleExt as _;
use crate::ui::{PreviewState, faded};

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The `group` name on a Coffer button, for content that follows its hover.
const GROUP: &str = "coffer-button";

/// Which row of the `DESIGN.md` button table a [`Button`] is.
///
/// Pick by meaning, not emphasis: [`Primary`](Self::Primary) is the one
/// default commit of a decision area, the action Enter runs. The agent
/// variants are for agent surfaces only (inbox actions, the agent card).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    /// The default commit of a decision area ("Share"): 30px, `text` fill,
    /// `on-light` label 12/16 600.
    Primary,
    /// An ordinary action ("Connect"): 24px, `border-strong` outline,
    /// `text-strong` label 11/14 500.
    #[default]
    Outline,
    /// A compact outline ("Main"): 26px, `hairline` outline, `text-secondary`
    /// label 11/14 500.
    OutlineCompact,
    /// A dialog's secondary action beside its primary ("Cancel"): 30px,
    /// `border-strong` outline, radius 7, 12px padding, `text-strong` label
    /// 12/16 500 (Paper `8JN-0`, D6.1 New account).
    Secondary,
    /// A destructive action that can be undone or is confirmed later ("Delete
    /// import"): 30px, `danger` outline and label 12/16 500.
    DestructiveOutline,
    /// The confirming button of a destructive dialog, and nowhere else
    /// ("Delete 312 transactions"): 30px, `danger` fill, `on-danger` label
    /// 12/16 600.
    DestructiveSolid,
    /// The default commit on an agent surface ("Approve all (12)"): 30px,
    /// `moldavite` fill, `on-moldavite` label 12/16 700. Hovers to
    /// `moldavite-hover`, presses to `moldavite-pressed`.
    AgentPrimary,
    /// A secondary action on an agent surface ("Reject all"): 30px,
    /// `moldavite-border-strong` outline, `moldavite-text` label 12/16 500.
    /// Hovers to `moldavite-surface-raised` and presses to `moldavite-well`:
    /// grey fills would look foreign there.
    AgentOutline,
    /// A way into the agent's work from a neutral surface ("Review in
    /// Inbox"): 36px, `moldavite-surface-raised` fill, radius 8, the sparkle
    /// in `moldavite` before a `moldavite-text` label 12/16 500. Hovers to
    /// `moldavite-well`, presses to `moldavite-border`.
    AgentGhost,
    /// An action in a floating toolbar (the bulk-action bar's
    /// "Categorise"): 28px, no border or fill, radius 6, 10px padding,
    /// `text-secondary` label 12/16 500. Hovers to `hover`, presses to
    /// `control-active`.
    Toolbar,
}

impl ButtonVariant {
    fn appearance(self) -> Appearance {
        // Neutral and destructive outlines share the generic hover and pressed
        // fills (`DESIGN.md` → "Interaction states").
        let outline = |height, padding_x, radius, border, foreground| Appearance {
            height,
            width: None,
            padding_x,
            padding_end: None,
            radius,
            border: Some(border),
            fill: None,
            hover: color::HOVER,
            pressed: color::CONTROL_ACTIVE,
            foreground,
            foreground_active: None,
        };
        let solid = |fill, hover, pressed, foreground| Appearance {
            height: size::CONTROL_LG,
            width: None,
            padding_x: spacing::S3_5,
            padding_end: None,
            radius: radius::BUTTON,
            border: None,
            fill: Some(fill),
            hover,
            pressed,
            foreground,
            foreground_active: None,
        };
        match self {
            Self::Primary => solid(
                color::TEXT,
                color::WHITE,
                color::TEXT_STRONG,
                color::ON_LIGHT,
            ),
            Self::Outline => outline(
                size::CONTROL_SM,
                spacing::S2,
                radius::MD,
                color::BORDER_STRONG,
                color::TEXT_STRONG,
            ),
            Self::OutlineCompact => outline(
                size::CONTROL_COMPACT,
                spacing::S2,
                radius::MD,
                color::HAIRLINE,
                color::TEXT_SECONDARY,
            ),
            Self::Secondary => outline(
                size::CONTROL_LG,
                spacing::S3,
                radius::BUTTON,
                color::BORDER_STRONG,
                color::TEXT_STRONG,
            ),
            Self::DestructiveOutline => outline(
                size::CONTROL_LG,
                spacing::S3,
                radius::BUTTON,
                color::DANGER,
                color::DANGER,
            ),
            Self::DestructiveSolid => solid(
                color::DANGER,
                color::DANGER_HOVER,
                color::DANGER_PRESSED,
                color::ON_DANGER,
            ),
            Self::AgentPrimary => solid(
                color::MOLDAVITE,
                color::MOLDAVITE_HOVER,
                color::MOLDAVITE_PRESSED,
                color::ON_MOLDAVITE,
            ),
            Self::AgentOutline => Appearance {
                hover: color::MOLDAVITE_SURFACE_RAISED,
                pressed: color::MOLDAVITE_WELL,
                ..outline(
                    size::CONTROL_LG,
                    spacing::S3,
                    radius::BUTTON,
                    color::MOLDAVITE_BORDER_STRONG,
                    color::MOLDAVITE_TEXT,
                )
            },
            Self::Toolbar => Appearance {
                border: None,
                ..outline(
                    size::CONTROL,
                    spacing::S2_5,
                    radius::MD,
                    color::HAIRLINE,
                    color::TEXT_SECONDARY,
                )
            },
            Self::AgentGhost => Appearance {
                height: size::CONTROL_XL,
                padding_x: spacing::S3,
                radius: radius::SURFACE,
                ..solid(
                    color::MOLDAVITE_SURFACE_RAISED,
                    color::MOLDAVITE_WELL,
                    color::MOLDAVITE_BORDER,
                    color::MOLDAVITE_TEXT,
                )
            },
        }
    }

    /// The label's type. The table's 550 is [`weight::MEDIUM`] and 650
    /// [`weight::STRONG`], as everywhere in `DESIGN.md`.
    fn text(self) -> TextRole {
        match self {
            Self::Primary | Self::DestructiveSolid => typography::BUTTON,
            Self::AgentPrimary => TextRole {
                weight: weight::BOLD,
                ..typography::BUTTON
            },
            Self::Outline | Self::OutlineCompact => TextRole {
                weight: weight::MEDIUM,
                ..typography::CAPTION
            },
            Self::Secondary
            | Self::DestructiveOutline
            | Self::AgentOutline
            | Self::AgentGhost
            | Self::Toolbar => TextRole {
                weight: weight::MEDIUM,
                ..typography::BUTTON
            },
        }
    }

    /// The icon before the label, and its colour.
    fn leading_icon(self) -> Option<(Glyph, Rgba)> {
        (self == Self::AgentGhost).then_some((Glyph::Sparkle, color::MOLDAVITE))
    }
}

/// How big an [`IconButton`] is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IconButtonSize {
    /// 24×24, radius 6: row actions, such as approve and reject on an agent
    /// card row.
    Small,
    /// 28×28, radius 6: panel toggles and other icon buttons in the app
    /// frame.
    #[default]
    Standard,
    /// 36×36, radius 8: tools in a dock.
    Dock,
}

impl IconButtonSize {
    fn appearance(self) -> Appearance {
        let (side, radius) = match self {
            Self::Small => (size::CONTROL_SM, radius::MD),
            Self::Standard => (size::CONTROL, radius::MD),
            Self::Dock => (size::CONTROL_XL, radius::SURFACE),
        };
        Appearance {
            height: side,
            width: Some(side),
            padding_x: Pixels::ZERO,
            padding_end: None,
            radius,
            border: None,
            fill: None,
            hover: color::HOVER,
            pressed: color::CONTROL_ACTIVE,
            foreground: color::TEXT_MUTED,
            foreground_active: Some(color::TEXT),
        }
    }
}

/// Everything a variant sets on the stock button.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Appearance {
    height: Pixels,
    /// `None`: as wide as the content and padding.
    width: Option<Pixels>,
    padding_x: Pixels,
    /// Replaces `padding_x` on the trailing side: a button ending in a
    /// shortcut sits its key cap 4px from the edge.
    padding_end: Option<Pixels>,
    radius: Pixels,
    /// A [`size::HAIRLINE`] border in this colour, in every state.
    border: Option<Rgba>,
    /// `None`: no fill at rest.
    fill: Option<Rgba>,
    hover: Rgba,
    pressed: Rgba,
    /// The label or icon colour at rest.
    foreground: Rgba,
    /// The icon colour while hovered or pressed; `None` keeps
    /// [`foreground`](Self::foreground).
    foreground_active: Option<Rgba>,
}

impl Appearance {
    /// This appearance's colours as `disabled` draws them. Hover and pressed
    /// stay: a disabled button never shows them.
    fn painted(self, disabled: Disabled) -> Self {
        Self {
            border: self.border.map(|border| disabled.paint(border)),
            fill: self.fill.map(|fill| disabled.paint(fill)),
            foreground: disabled.paint(self.foreground),
            foreground_active: self.foreground_active.map(|active| disabled.paint(active)),
            ..self
        }
    }

    /// This appearance as an active icon button draws it: `control-active`
    /// at rest, hovered and pressed, with a `text` icon throughout.
    fn active(self) -> Self {
        Self {
            fill: Some(color::CONTROL_ACTIVE),
            hover: color::CONTROL_ACTIVE,
            pressed: color::CONTROL_ACTIVE,
            foreground: color::TEXT,
            foreground_active: None,
            ..self
        }
    }

    /// This appearance with `foreground_active` replaced, for an agent card's
    /// row actions, which keep `moldavite-text`.
    fn with_foreground(self, foreground: Rgba) -> Self {
        Self {
            foreground,
            foreground_active: None,
            ..self
        }
    }
}

/// How a disabled button reaches [`opacity::DISABLED`].
///
/// Paper fades the finished button as one group (CSS `opacity`). GPUI's
/// `opacity` multiplies into each thing the element draws instead, so content
/// drawn over the button's own fill would blend with the already-faded fill:
/// the neutral primary's label would drop from Paper's 3.6:1 to 1.8:1
/// (`DESIGN.md` → "Rendering findings (D2.6)").
#[derive(Clone, Copy, Debug, PartialEq)]
enum Disabled {
    /// Element `opacity`. Exact when nothing is drawn under the content: the
    /// outline, toolbar and icon buttons and chips.
    Opacity,
    /// Every colour composited over the opaque surface the button sits on
    /// ([`faded`]) and drawn without `opacity`: for a fill under the content.
    On(Rgba),
}

impl Disabled {
    /// `color` as the disabled button draws it.
    fn paint(self, color: Rgba) -> Rgba {
        match self {
            Self::Opacity => color,
            Self::On(surface) => faded(color, surface),
        }
    }

    /// The element opacity this needs, if any.
    fn opacity(self) -> Option<f32> {
        (self == Self::Opacity).then_some(opacity::DISABLED)
    }
}

/// A button with a text label: Coffer's command control.
///
/// Stateless; the caller owns what a click does. The label is also the
/// button's accessible name. Without [`track_focus`](Self::track_focus) the
/// button keeps its own focus handle, keyed by its id.
///
/// ```
/// use coffer_app::ui::button::{Button, ButtonVariant};
///
/// let share = Button::new("share", "Share")
///     .with_variant(ButtonVariant::Primary)
///     .on_click(|_, _, _| {});
/// assert_eq!(share.variant(), ButtonVariant::Primary);
/// ```
#[derive(IntoElement)]
pub struct Button {
    control: Control,
    label: SharedString,
    variant: ButtonVariant,
    shortcut: Option<SharedString>,
}

impl Button {
    /// An enabled [`ButtonVariant::Outline`] button.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            control: Control::new(id.into()),
            label: label.into(),
            variant: ButtonVariant::default(),
            shortcut: None,
        }
    }

    /// Sets which row of the button table this is.
    pub fn with_variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Uses a caller-owned focus handle, so the caller can focus the button
    /// and restore focus to it. The handle becomes a Tab stop.
    pub fn track_focus(mut self, focus: &FocusHandle) -> Self {
        self.control.focus = Some(focus.clone());
        self
    }

    /// Runs `handler` when the button is clicked or activated from the
    /// keyboard. A disabled button doesn't run it.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.control.on_click = Some(Box::new(handler));
        self
    }

    /// Draws `state` without the input that causes it; see [`PreviewState`].
    pub fn preview(mut self, state: PreviewState) -> Self {
        self.control.preview = Some(state);
        self
    }

    /// The opaque surface the button sits on: `canvas` unless set. A disabled
    /// button with a fill (primary, destructive solid, the agent primary and
    /// ghost) or a shortcut fades towards it, so pass the surface it's
    /// actually on, such as `moldavite-surface` on an agent card.
    pub fn with_surface(mut self, surface: Rgba) -> Self {
        self.control.surface = surface;
        self
    }

    /// Makes the button take its share of the row's free width, centring its
    /// label, as "Approve all" on the agent card. Off by default: buttons hug
    /// their labels.
    pub fn with_full_width(mut self, full_width: bool) -> Self {
        self.control.full_width = full_width;
        self
    }

    /// Shows the button's shortcut in a small [`Kbd`] after the label, 4px
    /// from the trailing edge ("Import ⌘I"). For the 24px outline buttons,
    /// which the small key cap fits; the binding itself lives in the keymap.
    pub fn shortcut(mut self, keys: impl Into<SharedString>) -> Self {
        self.shortcut = Some(keys.into());
        self
    }

    pub fn is_full_width(&self) -> bool {
        self.control.full_width
    }

    pub fn variant(&self) -> ButtonVariant {
        self.variant
    }

    pub fn is_disabled(&self) -> bool {
        self.control.disabled
    }
}

impl Disableable for Button {
    fn disabled(mut self, disabled: bool) -> Self {
        self.control.disabled = disabled;
        self
    }
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut look = self.variant.appearance();
        // The key cap is a fill under its keys.
        let disabled = self.control.disabled_mode(&look, self.shortcut.is_some());
        let paint = |color| disabled.map_or(color, |disabled| disabled.paint(color));
        let label = label(self.variant.text(), self.label.clone());
        let content = match self.variant.leading_icon() {
            Some((glyph, icon_color)) => div()
                .flex()
                .items_center()
                .gap(spacing::S2)
                .child(glyph.render().text_color(paint(icon_color)))
                .child(label)
                .into_any_element(),
            None => label.into_any_element(),
        };
        let content = match self.shortcut {
            Some(keys) => {
                look.padding_end = Some(spacing::S1);
                let kbd = Kbd::new(keys).with_size(KbdSize::Small);
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::S1_5)
                    .child(content)
                    .child(match disabled {
                        Some(Disabled::On(surface)) => kbd.with_disabled_surface(surface),
                        _ => kbd,
                    })
                    .into_any_element()
            }
            None => content,
        };
        self.control
            .render(look, disabled, self.label, content, window, cx)
    }
}

/// A one-line button label in `role`, truncated with an ellipsis.
fn label(role: TextRole, text: SharedString) -> gpui_kit::Div {
    div()
        .min_w_0()
        .whitespace_nowrap()
        .text_ellipsis()
        .text_role(role)
        .child(text)
}

/// A button showing only an icon.
///
/// Stateless; the caller owns what a click does. The icon is drawn at its
/// own size in `text-muted`, and turns `text` while hovered or pressed.
/// `name` is the accessible name, since there's no visible label: say what
/// the button does ("Toggle sidebar").
#[derive(IntoElement)]
pub struct IconButton {
    control: Control,
    glyph: Glyph,
    name: SharedString,
    size: IconButtonSize,
    /// Replaces the icon colours, for icon buttons on agent surfaces.
    foreground: Option<Rgba>,
    active: bool,
}

impl IconButton {
    /// An enabled [`IconButtonSize::Standard`] button.
    pub fn new(id: impl Into<ElementId>, glyph: Glyph, name: impl Into<SharedString>) -> Self {
        Self {
            control: Control::new(id.into()),
            glyph,
            name: name.into(),
            size: IconButtonSize::default(),
            foreground: None,
            active: false,
        }
    }

    /// Marks the button as showing what's open, such as Settings in the
    /// title bar: `control-active` fill and a `text` icon in every state
    /// (`DESIGN.md` → "Settings").
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Sets how big the button is.
    pub fn with_size(mut self, size: IconButtonSize) -> Self {
        self.size = size;
        self
    }

    /// Draws the icon in `foreground` in every state instead of `text-muted`
    /// brightening to `text`. For agent surfaces (`moldavite-text`), where a
    /// grey icon would look foreign.
    pub fn with_foreground(mut self, foreground: Rgba) -> Self {
        self.foreground = Some(foreground);
        self
    }

    /// Uses a caller-owned focus handle, so the caller can focus the button
    /// and restore focus to it. The handle becomes a Tab stop.
    pub fn track_focus(mut self, focus: &FocusHandle) -> Self {
        self.control.focus = Some(focus.clone());
        self
    }

    /// Runs `handler` when the button is clicked or activated from the
    /// keyboard. A disabled button doesn't run it.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.control.on_click = Some(Box::new(handler));
        self
    }

    /// Draws `state` without the input that causes it; see [`PreviewState`].
    pub fn preview(mut self, state: PreviewState) -> Self {
        self.control.preview = Some(state);
        self
    }

    pub fn size(&self) -> IconButtonSize {
        self.size
    }

    pub fn is_disabled(&self) -> bool {
        self.control.disabled
    }

    pub fn is_active(&self) -> bool {
        self.active
    }
}

impl Disableable for IconButton {
    fn disabled(mut self, disabled: bool) -> Self {
        self.control.disabled = disabled;
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut look = self.size.appearance();
        if self.active {
            look = look.active();
        }
        if let Some(foreground) = self.foreground {
            look = look.with_foreground(foreground);
        }
        let disabled = self.control.disabled_mode(&look, false);
        let icon = self.control.foreground(
            disabled.map_or(look, |disabled| look.painted(disabled)),
            self.glyph.render(),
        );
        self.control.render(
            look,
            disabled,
            self.name,
            icon.into_any_element(),
            window,
            cx,
        )
    }
}

/// A filter chip ("Uncategorised 14"): a 28px pill with a `border` outline,
/// a `text-secondary` label 12/16 and an optional mono count in `text-muted`.
/// Hovers to `hover`, presses to `control-active`.
///
/// Stateless; the caller owns what a click does. The accessible name is the
/// label followed by the count.
#[derive(IntoElement)]
pub struct Chip {
    control: Control,
    label: SharedString,
    count: Option<SharedString>,
    menu: bool,
}

impl Chip {
    /// An enabled chip with no count.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            control: Control::new(id.into()),
            label: label.into(),
            count: None,
            menu: false,
        }
    }

    /// Shows `count` after the label, in Geist Mono. Pass it formatted for the
    /// user's locale.
    pub fn count(mut self, count: impl Into<SharedString>) -> Self {
        self.count = Some(count.into());
        self
    }

    /// Ends the chip in an 8px `icon-faint` chevron: it opens a menu (the
    /// Account, Category and Date filters), where a chip with a count
    /// toggles.
    pub fn menu(mut self, menu: bool) -> Self {
        self.menu = menu;
        self
    }

    pub fn has_menu(&self) -> bool {
        self.menu
    }

    /// Runs `handler` when the chip is clicked or activated from the
    /// keyboard. A disabled chip doesn't run it.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.control.on_click = Some(Box::new(handler));
        self
    }

    /// Draws `state` without the input that causes it; see [`PreviewState`].
    pub fn preview(mut self, state: PreviewState) -> Self {
        self.control.preview = Some(state);
        self
    }

    pub fn is_disabled(&self) -> bool {
        self.control.disabled
    }

    fn appearance() -> Appearance {
        Appearance {
            height: size::CONTROL,
            width: None,
            padding_x: spacing::S3,
            padding_end: None,
            radius: radius::PILL,
            border: Some(color::BORDER),
            fill: None,
            hover: color::HOVER,
            pressed: color::CONTROL_ACTIVE,
            foreground: color::TEXT_SECONDARY,
            foreground_active: None,
        }
    }
}

impl Disableable for Chip {
    fn disabled(mut self, disabled: bool) -> Self {
        self.control.disabled = disabled;
        self
    }
}

impl RenderOnce for Chip {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let name = match &self.count {
            Some(count) => format!("{} {count}", self.label).into(),
            None => self.label.clone(),
        };
        let look = Self::appearance();
        let disabled = self.control.disabled_mode(&look, false);
        let paint = |color| disabled.map_or(color, |disabled| disabled.paint(color));
        let content = div()
            .flex()
            .items_center()
            .gap(spacing::S1_5)
            .child(label(typography::BODY, self.label))
            .when_some(self.count, |this, count| {
                this.child(
                    div()
                        .flex_none()
                        .text_role(typography::MONO_VALUE)
                        .text_color(paint(color::TEXT_MUTED))
                        .child(count),
                )
            })
            .when(self.menu, |this| {
                this.child(
                    Glyph::ChevronDown
                        .render()
                        .text_color(paint(color::ICON_FAINT)),
                )
            });
        self.control
            .render(look, disabled, name, content.into_any_element(), window, cx)
    }
}

/// What every Coffer button has, whatever it shows.
struct Control {
    id: ElementId,
    disabled: bool,
    focus: Option<FocusHandle>,
    on_click: Option<ClickHandler>,
    preview: Option<PreviewState>,
    /// Fills its row instead of hugging its content.
    full_width: bool,
    /// The opaque surface under the control, which it fades towards while
    /// disabled.
    surface: Rgba,
}

impl Control {
    fn new(id: ElementId) -> Self {
        Self {
            id,
            disabled: false,
            focus: None,
            on_click: None,
            preview: None,
            full_width: false,
            surface: color::CANVAS,
        }
    }

    /// How the control draws while disabled with `look`, or `None` while
    /// enabled. `nested_fill`: the content draws a fill of its own under
    /// some of itself, such as a key cap.
    fn disabled_mode(&self, look: &Appearance, nested_fill: bool) -> Option<Disabled> {
        self.disabled.then(|| {
            if look.fill.is_some() || nested_fill {
                Disabled::On(self.surface)
            } else {
                Disabled::Opacity
            }
        })
    }

    /// `icon` coloured for `look`: brightening with the button's hover when
    /// the look says so, or already bright when hover or pressed is
    /// previewed.
    fn foreground(&self, look: Appearance, icon: gpui_kit::Svg) -> gpui_kit::Svg {
        let Some(active) = look.foreground_active else {
            return icon.text_color(look.foreground);
        };
        if self.disabled {
            return icon.text_color(look.foreground);
        }
        match self.preview {
            Some(PreviewState::Hovered | PreviewState::Pressed) => icon.text_color(active),
            _ => icon
                .text_color(look.foreground)
                .group_hover(GROUP, |style| style.text_color(active)),
        }
    }

    /// The stock button showing `content`, inside a wrapper that draws the
    /// focus ring. `disabled` is [`Self::disabled_mode`] for `look`; `content`
    /// must already be painted for it.
    fn render(
        self,
        look: Appearance,
        disabled: Option<Disabled>,
        name: SharedString,
        content: AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        // The stock button would key the same state by the same id, but only
        // when it isn't given a handle, and it always is.
        let focus = self.focus.unwrap_or_else(|| {
            window
                .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
                .read(cx)
                .clone()
        });
        let button = stock_button(self.id, look, name, disabled, self.preview, &focus, cx)
            .child(content)
            .when_some(self.on_click, |this, handler| {
                this.on_click(move |event, window, cx| handler(event, window, cx))
            });

        // The wrapper hugs the button, or shares out its row's free width,
        // and its radius puts the ring's corners concentric with the
        // button's.
        let button = button.when(self.full_width, |this| this.w_full());
        let wrapper = div()
            .flex()
            .rounded(look.radius)
            .map(|this| {
                if self.full_width {
                    this.flex_1().min_w_0()
                } else {
                    this.flex_none()
                }
            })
            .child(button);
        if self.preview == Some(PreviewState::FocusVisible) {
            wrapper.show_focus_ring(FocusRingPlacement::Outside, window)
        } else {
            wrapper.focus_visible_ring(&focus, FocusRingPlacement::Outside, window)
        }
    }
}

/// The stock button styled as `look`, without content or a click handler.
/// `disabled` paints `look` and fades it as [`Disabled`] says.
fn stock_button(
    id: ElementId,
    look: Appearance,
    name: SharedString,
    disabled: Option<Disabled>,
    preview: Option<PreviewState>,
    focus: &FocusHandle,
    cx: &App,
) -> StockButton {
    let none = transparent_black();
    let look = disabled.map_or(look, |disabled| look.painted(disabled));
    // `Custom` draws `color` as the border in every state; its resting fill
    // (`color` at 20%) is replaced below.
    let states = ButtonCustomVariant::new(cx)
        .color(look.border.map_or(none, Hsla::from))
        .foreground(look.foreground.into())
        .hover(look.hover.into())
        .active(look.pressed.into());
    // A previewed state is the resting fill; real hover and pressed still
    // draw over it.
    let fill = match preview {
        Some(PreviewState::Hovered) if disabled.is_none() => Some(look.hover),
        Some(PreviewState::Pressed) if disabled.is_none() => Some(look.pressed),
        _ => look.fill,
    };

    // Instance styles sit under hover and pressed, and the stock disabled
    // state replays them, so a disabled button keeps its resting colours,
    // painted for `disabled`.
    StockButton::new(id)
        .custom(states)
        .focus_ring(false)
        .track_focus(focus)
        .accessibility_label(name)
        .disabled(disabled.is_some())
        .group(GROUP)
        .h(look.height)
        .when_some(look.width, |this, width| this.w(width))
        .px(look.padding_x)
        .when_some(look.padding_end, |this, end| this.pr(end))
        .rounded(look.radius)
        .when_some(look.border, |this, border| {
            this.border(size::HAIRLINE).border_color(border)
        })
        .bg(fill.map_or(none, Hsla::from))
        .text_color(look.foreground)
        .when_some(disabled.and_then(Disabled::opacity), |this, opacity| {
            this.opacity(opacity)
        })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AnyElement, AppContext as _, Bounds, Context, Entity, Render, Styled, TestAppContext,
        VisualTestContext, WindowBounds, WindowOptions, point, px,
    };

    use super::*;
    use crate::theme;
    use crate::theme::focus::RING_SELECTOR;

    const BUTTON: &str = "button";

    #[test]
    fn variants_match_the_button_table() {
        let primary = ButtonVariant::Primary.appearance();
        assert_eq!(
            (primary.height, primary.padding_x, primary.radius),
            (px(30.), px(14.), px(7.))
        );
        assert_eq!(primary.border, None);
        assert_eq!(primary.fill, Some(color::TEXT));
        assert_eq!(primary.foreground, color::ON_LIGHT);
        assert_eq!(
            (primary.hover, primary.pressed),
            (color::WHITE, color::TEXT_STRONG)
        );

        let outline = ButtonVariant::Outline.appearance();
        assert_eq!(
            (outline.height, outline.padding_x, outline.radius),
            (px(24.), px(8.), px(6.))
        );
        assert_eq!(outline.border, Some(color::BORDER_STRONG));
        assert_eq!(outline.foreground, color::TEXT_STRONG);

        let compact = ButtonVariant::OutlineCompact.appearance();
        assert_eq!(
            (compact.height, compact.padding_x, compact.radius),
            (px(26.), px(8.), px(6.))
        );
        assert_eq!(compact.border, Some(color::HAIRLINE));
        assert_eq!(compact.foreground, color::TEXT_SECONDARY);

        let destructive = ButtonVariant::DestructiveOutline.appearance();
        assert_eq!(
            (
                destructive.height,
                destructive.padding_x,
                destructive.radius
            ),
            (px(30.), px(12.), px(7.))
        );
        assert_eq!(destructive.border, Some(color::DANGER));
        assert_eq!(destructive.foreground, color::DANGER);

        let solid = ButtonVariant::DestructiveSolid.appearance();
        assert_eq!(
            (solid.height, solid.padding_x, solid.radius),
            (px(30.), px(14.), px(7.))
        );
        assert_eq!(solid.border, None);
        assert_eq!(solid.fill, Some(color::DANGER));
        assert_eq!(solid.foreground, color::ON_DANGER);
        assert_eq!(
            (solid.hover, solid.pressed),
            (color::DANGER_HOVER, color::DANGER_PRESSED)
        );

        let agent = ButtonVariant::AgentOutline.appearance();
        assert_eq!(
            (agent.height, agent.padding_x, agent.radius),
            (px(30.), px(12.), px(7.))
        );
        assert_eq!(agent.border, Some(color::MOLDAVITE_BORDER_STRONG));
        assert_eq!(agent.fill, None);
        assert_eq!(agent.foreground, color::MOLDAVITE_TEXT);
        assert_eq!(
            (agent.hover, agent.pressed),
            (color::MOLDAVITE_SURFACE_RAISED, color::MOLDAVITE_WELL)
        );

        let agent_primary = ButtonVariant::AgentPrimary.appearance();
        assert_eq!(
            (
                agent_primary.height,
                agent_primary.padding_x,
                agent_primary.radius
            ),
            (px(30.), px(14.), px(7.))
        );
        assert_eq!(agent_primary.border, None);
        assert_eq!(agent_primary.fill, Some(color::MOLDAVITE));
        assert_eq!(agent_primary.foreground, color::ON_MOLDAVITE);
        assert_eq!(
            (agent_primary.hover, agent_primary.pressed),
            (color::MOLDAVITE_HOVER, color::MOLDAVITE_PRESSED)
        );

        let ghost = ButtonVariant::AgentGhost.appearance();
        assert_eq!(
            (ghost.height, ghost.padding_x, ghost.radius),
            (px(36.), px(12.), px(8.))
        );
        assert_eq!(ghost.border, None);
        assert_eq!(ghost.fill, Some(color::MOLDAVITE_SURFACE_RAISED));
        assert_eq!(ghost.foreground, color::MOLDAVITE_TEXT);
        assert_eq!(
            (ghost.hover, ghost.pressed),
            (color::MOLDAVITE_WELL, color::MOLDAVITE_BORDER)
        );
        assert_eq!(
            ButtonVariant::AgentGhost.leading_icon(),
            Some((Glyph::Sparkle, color::MOLDAVITE))
        );

        let chip = Chip::appearance();
        assert_eq!(
            (chip.height, chip.padding_x, chip.radius),
            (px(28.), px(12.), px(14.))
        );
        assert_eq!(chip.border, Some(color::BORDER));
        assert_eq!(chip.foreground, color::TEXT_SECONDARY);

        // Only the agent ghost has an icon: the sparkle is the agent's.
        for variant in [
            ButtonVariant::Primary,
            ButtonVariant::Outline,
            ButtonVariant::OutlineCompact,
            ButtonVariant::Secondary,
            ButtonVariant::DestructiveOutline,
            ButtonVariant::DestructiveSolid,
            ButtonVariant::AgentPrimary,
            ButtonVariant::AgentOutline,
        ] {
            assert_eq!(variant.leading_icon(), None, "{variant:?}");
        }

        // Neutral and destructive outlines and the chip: no fill, `hover`,
        // `control-active`.
        for look in [
            ButtonVariant::Outline.appearance(),
            ButtonVariant::OutlineCompact.appearance(),
            ButtonVariant::Secondary.appearance(),
            ButtonVariant::DestructiveOutline.appearance(),
            Chip::appearance(),
        ] {
            assert_eq!(look.fill, None, "{look:?}");
            assert_eq!(
                (look.hover, look.pressed),
                (color::HOVER, color::CONTROL_ACTIVE),
                "{look:?}"
            );
        }
    }

    #[test]
    fn an_active_icon_button_is_control_active_with_a_text_icon() {
        // Paper `BAJ-0`: the title bar's Settings button while Settings is
        // open.
        let resting = IconButtonSize::Standard.appearance();
        let active = resting.active();
        assert_eq!(active.fill, Some(color::CONTROL_ACTIVE));
        assert_eq!(
            (active.hover, active.pressed),
            (color::CONTROL_ACTIVE, color::CONTROL_ACTIVE)
        );
        assert_eq!(
            (active.foreground, active.foreground_active),
            (color::TEXT, None)
        );
        assert_eq!(
            (active.height, active.width, active.radius),
            (resting.height, resting.width, resting.radius),
            "only the colours change"
        );
    }

    #[test]
    fn the_secondary_button_pairs_with_the_primary_in_dialogs() {
        // Paper `8JN-0` (New account, Cancel): 30px like the neutral primary
        // beside it, `border-strong`, radius 7, 12px padding.
        let secondary = ButtonVariant::Secondary.appearance();
        let primary = ButtonVariant::Primary.appearance();
        assert_eq!(secondary.height, primary.height);
        assert_eq!((secondary.padding_x, secondary.radius), (px(12.), px(7.)));
        assert_eq!(secondary.border, Some(color::BORDER_STRONG));
        assert_eq!(secondary.foreground, color::TEXT_STRONG);
    }

    #[test]
    fn labels_use_the_button_type() {
        let text = |variant: ButtonVariant| {
            let role = variant.text();
            (role.size, role.line_height, role.weight)
        };
        assert_eq!(
            text(ButtonVariant::Primary),
            (px(12.), px(16.), weight::STRONG)
        );
        assert_eq!(
            text(ButtonVariant::Outline),
            (px(11.), px(14.), weight::MEDIUM)
        );
        assert_eq!(
            text(ButtonVariant::OutlineCompact),
            (px(11.), px(14.), weight::MEDIUM)
        );
        assert_eq!(
            text(ButtonVariant::Secondary),
            (px(12.), px(16.), weight::MEDIUM)
        );
        assert_eq!(
            text(ButtonVariant::DestructiveOutline),
            (px(12.), px(16.), weight::MEDIUM)
        );
        assert_eq!(
            text(ButtonVariant::DestructiveSolid),
            (px(12.), px(16.), weight::STRONG)
        );
        assert_eq!(
            text(ButtonVariant::AgentOutline),
            (px(12.), px(16.), weight::MEDIUM)
        );
        assert_eq!(
            text(ButtonVariant::AgentPrimary),
            (px(12.), px(16.), weight::BOLD)
        );
        assert_eq!(
            text(ButtonVariant::AgentGhost),
            (px(12.), px(16.), weight::MEDIUM)
        );
    }

    #[test]
    fn icon_buttons_are_square() {
        for (size, side, radius) in [
            (IconButtonSize::Small, px(24.), px(6.)),
            (IconButtonSize::Standard, px(28.), px(6.)),
            (IconButtonSize::Dock, px(36.), px(8.)),
        ] {
            let look = size.appearance();
            assert_eq!((look.height, look.width), (side, Some(side)), "{size:?}");
            assert_eq!(look.radius, radius, "{size:?}");
            assert_eq!((look.fill, look.border), (None, None), "{size:?}");
            assert_eq!(
                (look.hover, look.pressed),
                (color::HOVER, color::CONTROL_ACTIVE),
                "{size:?}"
            );
            // Paper: `text-muted` at rest, `text` hovered and pressed.
            assert_eq!(
                (look.foreground, look.foreground_active),
                (color::TEXT_MUTED, Some(color::TEXT)),
                "{size:?}"
            );
        }
    }

    /// The colour an icon was given, as GPUI will paint it.
    fn icon_color(icon: &mut gpui_kit::Svg) -> Option<Hsla> {
        icon.style().text.color
    }

    #[test]
    fn icons_brighten_only_when_active() {
        let look = IconButtonSize::Standard.appearance();
        let control = |preview, disabled| Control {
            preview,
            disabled,
            ..Control::new(BUTTON.into())
        };
        let icon =
            |control: Control| icon_color(&mut control.foreground(look, Glyph::Panel.render()));

        let muted = Some(Hsla::from(color::TEXT_MUTED));
        let bright = Some(Hsla::from(color::TEXT));
        assert_eq!(icon(control(None, false)), muted);
        assert_eq!(icon(control(Some(PreviewState::Hovered), false)), bright);
        assert_eq!(icon(control(Some(PreviewState::Pressed), false)), bright);
        assert_eq!(
            icon(control(Some(PreviewState::FocusVisible), false)),
            muted
        );
        assert_eq!(
            icon(control(Some(PreviewState::Hovered), true)),
            muted,
            "a disabled button doesn't react"
        );

        // An agent surface's icon keeps its colour in every state.
        let agent = look.with_foreground(color::MOLDAVITE_TEXT);
        let mut kept =
            control(Some(PreviewState::Hovered), false).foreground(agent, Glyph::Check.render());
        assert_eq!(icon_color(&mut kept), Some(color::MOLDAVITE_TEXT.into()));
    }

    /// WCAG 2 contrast ratio between two opaque colours.
    fn contrast(a: Rgba, b: Rgba) -> f32 {
        let luminance = |color: Rgba| {
            let linear = |channel: f32| {
                if channel <= 0.04045 {
                    channel / 12.92
                } else {
                    ((channel + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
        };
        let (light, dark) = (
            luminance(a).max(luminance(b)),
            luminance(a).min(luminance(b)),
        );
        (light + 0.05) / (dark + 0.05)
    }

    /// `#RRGGBB` as an opaque colour.
    fn rgb(hex: u32) -> Rgba {
        gpui_kit::rgb(hex)
    }

    #[test]
    fn disabled_buttons_keep_papers_label_contrast() {
        let disabled = Control {
            disabled: true,
            ..Control::new(BUTTON.into())
        };
        // Label on fill, measured in Paper's button matrix (`D4-0`, disabled
        // column on `canvas`, colour-managed from its Display P3 export).
        for (variant, paper_fill, paper_label) in [
            (ButtonVariant::Primary, 0x6D6D70, 0x141416),
            (ButtonVariant::AgentPrimary, 0x405D46, 0x141716),
            (ButtonVariant::DestructiveSolid, 0x6A3248, 0x191316),
            (ButtonVariant::AgentGhost, 0x181B19, 0x536556),
        ] {
            let look = variant.appearance();
            let mode = disabled.disabled_mode(&look, false);
            assert_eq!(mode, Some(Disabled::On(color::CANVAS)), "{variant:?}");
            let painted = look.painted(mode.expect("disabled"));
            let fill = painted.fill.expect("a filled variant");
            let ours = contrast(painted.foreground, fill);
            let paper = contrast(rgb(paper_label), rgb(paper_fill));
            assert!(
                (ours - paper).abs() < 0.02,
                "{variant:?}: {ours:.2}:1, Paper {paper:.2}:1"
            );
        }

        // The brief's number: the neutral primary on `canvas` is 3.6:1, where
        // element `opacity` gave 1.8:1.
        let primary = ButtonVariant::Primary
            .appearance()
            .painted(Disabled::On(color::CANVAS));
        let ratio = contrast(primary.foreground, primary.fill.expect("filled"));
        assert_eq!((ratio * 10.).round() / 10., 3.6);

        // A button on another surface fades towards that surface.
        let on_card = Control {
            surface: color::MOLDAVITE_SURFACE,
            ..disabled
        };
        assert_eq!(
            on_card.disabled_mode(&ButtonVariant::AgentPrimary.appearance(), false),
            Some(Disabled::On(color::MOLDAVITE_SURFACE))
        );
    }

    #[test]
    fn only_content_on_a_fill_is_composited() {
        let disabled = Control {
            disabled: true,
            ..Control::new(BUTTON.into())
        };
        // Nothing under the content: element opacity is already exact.
        for look in [
            ButtonVariant::Outline.appearance(),
            ButtonVariant::OutlineCompact.appearance(),
            ButtonVariant::Secondary.appearance(),
            ButtonVariant::DestructiveOutline.appearance(),
            ButtonVariant::AgentOutline.appearance(),
            ButtonVariant::Toolbar.appearance(),
            IconButtonSize::Standard.appearance(),
            Chip::appearance(),
        ] {
            assert_eq!(
                disabled.disabled_mode(&look, false),
                Some(Disabled::Opacity),
                "{look:?}"
            );
            assert_eq!(look.painted(Disabled::Opacity), look);
        }
        // A key cap is a fill under its keys.
        assert_eq!(
            disabled.disabled_mode(&ButtonVariant::Outline.appearance(), true),
            Some(Disabled::On(color::CANVAS))
        );
        // Enabled draws as it is.
        let enabled = Control::new(BUTTON.into());
        assert_eq!(
            enabled.disabled_mode(&ButtonVariant::Primary.appearance(), true),
            None
        );
    }

    #[gpui_kit::test]
    fn stock_focus_style_is_off_and_disabled_fades_as_a_group(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::component::install(cx);
            let focus = cx.focus_handle();
            let stock = |variant: ButtonVariant, disabled| {
                stock_button(
                    BUTTON.into(),
                    variant.appearance(),
                    "Share".into(),
                    disabled,
                    None,
                    &focus,
                    cx,
                )
            };

            let mut enabled = stock(ButtonVariant::Primary, None);
            assert!(!enabled.is_focus_ring_enabled());
            assert_eq!(enabled.style().opacity, None);

            // A solid button paints opaque composites and keeps full opacity.
            let mut solid = stock(ButtonVariant::Primary, Some(Disabled::On(color::CANVAS)));
            assert!(!solid.is_focus_ring_enabled());
            assert_eq!(solid.style().opacity, None);
            assert_eq!(
                solid.style().background,
                Some(gpui_kit::Fill::from(Hsla::from(faded(
                    color::TEXT,
                    color::CANVAS
                ))))
            );
            assert_eq!(
                solid.style().text.color,
                Some(Hsla::from(faded(color::ON_LIGHT, color::CANVAS)))
            );

            // An outline keeps its colours and fades with element opacity.
            let mut outline = stock(ButtonVariant::Outline, Some(Disabled::Opacity));
            assert_eq!(outline.style().opacity, Some(opacity::DISABLED));
            assert_eq!(
                outline.style().text.color,
                Some(Hsla::from(color::TEXT_STRONG))
            );
        });
    }

    #[gpui_kit::test]
    fn preview_replaces_the_resting_fill(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::component::install(cx);
            let focus = cx.focus_handle();
            let look = ButtonVariant::Primary.appearance();
            let fill = |preview, disabled| {
                let mut stock = stock_button(
                    BUTTON.into(),
                    look,
                    "Share".into(),
                    disabled,
                    preview,
                    &focus,
                    cx,
                );
                stock.style().background.clone()
            };
            let solid = |color: Rgba| Some(gpui_kit::Fill::from(Hsla::from(color)));

            assert_eq!(fill(None, None), solid(color::TEXT));
            assert_eq!(fill(Some(PreviewState::Hovered), None), solid(color::WHITE));
            assert_eq!(
                fill(Some(PreviewState::Pressed), None),
                solid(color::TEXT_STRONG)
            );
            assert_eq!(
                fill(Some(PreviewState::FocusVisible), None),
                solid(color::TEXT)
            );
            assert_eq!(
                fill(
                    Some(PreviewState::Hovered),
                    Some(Disabled::On(color::CANVAS))
                ),
                solid(faded(color::TEXT, color::CANVAS)),
                "a disabled button has no hover"
            );
        });
    }

    /// A view whose content is `build`'s element, with room around it for an
    /// outside focus ring. `region` is the view's own focus, as a real view
    /// has; `focus` is for a button the test wants to track.
    struct Harness {
        region: FocusHandle,
        focus: FocusHandle,
        build: Box<dyn Fn(&FocusHandle) -> AnyElement>,
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .track_focus(&self.region)
                .size_full()
                .flex()
                .items_start()
                .gap(size::CONTROL)
                .p(size::CONTROL)
                .child((self.build)(&self.focus))
        }
    }

    /// Opens a real window on the Coffer theme, with `Root` (which binds Tab)
    /// and the view's region focused, so Tab moves to the first button.
    fn open(
        cx: &mut TestAppContext,
        build: impl Fn(&FocusHandle) -> AnyElement + 'static,
    ) -> (Entity<Harness>, VisualTestContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::component::install(cx);
        });
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(0.), px(0.)),
                gpui_kit::size(px(640.), px(480.)),
            ))),
            ..Default::default()
        };
        let (window, view) = cx
            .update(|cx| {
                gpui_kit::open_window(options, cx, |_, cx| {
                    cx.new(|cx| Harness {
                        region: cx.focus_handle(),
                        focus: cx.focus_handle(),
                        build: Box::new(build),
                    })
                })
            })
            .expect("open test window");
        let mut cx = VisualTestContext::from_window(window, cx);
        cx.update(|window, cx| {
            let region = view.read(cx).region.clone();
            window.focus(&region, cx);
            window.render_frame(cx);
        });
        (view, cx)
    }

    fn counter() -> (
        Rc<Cell<usize>>,
        impl Fn(&ClickEvent, &mut Window, &mut App) + Clone,
    ) {
        let clicks = Rc::new(Cell::new(0));
        let handler = {
            let clicks = clicks.clone();
            move |_: &ClickEvent, _: &mut Window, _: &mut App| clicks.set(clicks.get() + 1)
        };
        (clicks, handler)
    }

    fn dilate(bounds: Bounds<Pixels>, amount: Pixels) -> Bounds<Pixels> {
        Bounds::from_corners(
            point(bounds.left() - amount, bounds.top() - amount),
            point(bounds.right() + amount, bounds.bottom() + amount),
        )
    }

    fn bounds(cx: &mut VisualTestContext, id: &'static str) -> Bounds<Pixels> {
        cx.update(|window, _| window.find(id).bounds())
    }

    fn ring(cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
        cx.debug_bounds(RING_SELECTOR)
    }

    fn press(cx: &mut VisualTestContext, key: &str) {
        cx.update(|window, cx| window.press(key, cx));
    }

    fn click(cx: &mut VisualTestContext, id: &'static str) {
        cx.update(|window, cx| window.click(id, cx));
    }

    #[gpui_kit::test]
    fn click_runs_the_handler(cx: &mut TestAppContext) {
        let (clicks, handler) = counter();
        let (_, mut cx) = open(cx, move |_| {
            Button::new(BUTTON, "Share")
                .with_variant(ButtonVariant::Primary)
                .on_click(handler.clone())
                .into_any_element()
        });

        assert_eq!(
            cx.update(|window, _| window.find(BUTTON).label().map(str::to_owned)),
            Some("Share".to_owned()),
            "the label is the accessible name"
        );
        click(&mut cx, BUTTON);
        assert_eq!(clicks.get(), 1);
    }

    #[gpui_kit::test]
    fn keyboard_activates_the_button(cx: &mut TestAppContext) {
        let (clicks, handler) = counter();
        let (_, mut cx) = open(cx, move |_| {
            IconButton::new(BUTTON, Glyph::Panel, "Toggle sidebar")
                .on_click(handler.clone())
                .into_any_element()
        });

        press(&mut cx, "tab");
        press(&mut cx, "enter");
        press(&mut cx, "space");
        assert_eq!(clicks.get(), 2);
    }

    #[gpui_kit::test]
    fn disabled_button_rejects_the_click(cx: &mut TestAppContext) {
        let (clicks, handler) = counter();
        let (_, mut cx) = open(cx, move |_| {
            Button::new(BUTTON, "Delete import")
                .with_variant(ButtonVariant::DestructiveOutline)
                .disabled(true)
                .on_click(handler.clone())
                .into_any_element()
        });

        click(&mut cx, BUTTON);
        press(&mut cx, "tab");
        press(&mut cx, "enter");
        assert_eq!(clicks.get(), 0);
        assert_eq!(ring(&mut cx), None, "a disabled button isn't a Tab stop");
    }

    #[gpui_kit::test]
    fn keyboard_focus_shows_the_ring(cx: &mut TestAppContext) {
        let (_, mut cx) = open(cx, |_| Button::new(BUTTON, "Connect").into_any_element());
        assert_eq!(ring(&mut cx), None, "unfocused: no ring");

        press(&mut cx, "tab");
        let ring = ring(&mut cx).expect("Tab focus draws the ring");
        assert_eq!(
            ring,
            dilate(
                bounds(&mut cx, BUTTON),
                size::FOCUS_RING_OFFSET + size::FOCUS_RING
            )
        );
    }

    #[gpui_kit::test]
    fn pointer_focus_hides_the_ring(cx: &mut TestAppContext) {
        let (view, mut cx) = open(cx, |focus| {
            Button::new(BUTTON, "Connect")
                .track_focus(focus)
                .into_any_element()
        });
        let focus = cx.update(|_, cx| view.read(cx).focus.clone());
        let is_focused =
            |cx: &mut VisualTestContext| cx.update(|window, _| focus.is_focused(window));

        // Focused from code, then the pointer moves.
        cx.update(|window, cx| window.focus(&focus, cx));
        cx.simulate_mouse_move(point(px(1.), px(1.)), None, gpui_kit::Modifiers::none());
        cx.update(|window, cx| window.render_frame(cx));
        assert!(is_focused(&mut cx));
        assert_eq!(ring(&mut cx), None, "pointer input: no ring");

        // Focused from the keyboard, then clicked: still focused, no ring.
        press(&mut cx, "a");
        assert!(ring(&mut cx).is_some(), "keyboard input: ring");
        click(&mut cx, BUTTON);
        assert!(is_focused(&mut cx));
        assert_eq!(ring(&mut cx), None, "after a click: no ring");
    }

    #[gpui_kit::test]
    fn rendered_height_matches_the_variant(cx: &mut TestAppContext) {
        let variants = [
            ("primary", ButtonVariant::Primary),
            ("outline", ButtonVariant::Outline),
            ("outline-compact", ButtonVariant::OutlineCompact),
            ("secondary", ButtonVariant::Secondary),
            ("destructive-outline", ButtonVariant::DestructiveOutline),
            ("destructive-solid", ButtonVariant::DestructiveSolid),
            ("agent-primary", ButtonVariant::AgentPrimary),
            ("agent-outline", ButtonVariant::AgentOutline),
            ("agent-ghost", ButtonVariant::AgentGhost),
            ("toolbar", ButtonVariant::Toolbar),
        ];
        let icons = [
            ("icon-small", IconButtonSize::Small),
            ("icon", IconButtonSize::Standard),
            ("icon-dock", IconButtonSize::Dock),
        ];
        let (_, mut cx) = open(cx, move |_| {
            div()
                .flex()
                .items_start()
                .gap(size::CONTROL)
                .children(
                    variants.map(|(id, variant)| Button::new(id, "Label").with_variant(variant)),
                )
                .children(
                    icons.map(|(id, size)| IconButton::new(id, Glyph::Plus, "Add").with_size(size)),
                )
                .child(Chip::new("chip", "Uncategorised").count("14"))
                .into_any_element()
        });

        for (id, variant) in variants {
            let height = bounds(&mut cx, id).size.height;
            assert_eq!(height, variant.appearance().height, "{id}");
        }
        for (id, size) in icons {
            let side = size.appearance().height;
            assert_eq!(bounds(&mut cx, id).size, gpui_kit::size(side, side), "{id}");
        }
        assert_eq!(bounds(&mut cx, "chip").size.height, size::CONTROL);
        assert_eq!(
            cx.update(|window, _| window.find("chip").label().map(str::to_owned)),
            Some("Uncategorised 14".to_owned()),
            "the count is part of the chip's name"
        );
    }

    #[gpui_kit::test]
    fn previewed_focus_draws_the_ring_without_focus(cx: &mut TestAppContext) {
        let (_, mut cx) = open(cx, |_| {
            Button::new(BUTTON, "Connect")
                .preview(PreviewState::FocusVisible)
                .into_any_element()
        });

        let ring = ring(&mut cx).expect("a previewed focus draws the ring");
        assert_eq!(
            ring,
            dilate(
                bounds(&mut cx, BUTTON),
                size::FOCUS_RING_OFFSET + size::FOCUS_RING
            )
        );
        press(&mut cx, "tab");
        assert!(
            cx.update(|window, _| window.find(BUTTON).focused()) == Some(true),
            "a previewed button is still a real Tab stop"
        );
    }
}
