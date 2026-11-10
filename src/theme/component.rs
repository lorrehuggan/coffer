//! `gpui-component`'s theme, mapped onto Coffer's tokens.
//!
//! Stock components read their colours, radii, fonts and motion from the
//! global [`Theme`]. [`install`] replaces every one of those values with a
//! token from [`super::tokens`], so no stock colour can reach the screen: a
//! test fills the palette with a sentinel, applies the map, and fails if any
//! field is left unmapped or holds a value that isn't a Coffer token. A field
//! added by a `gpui-component` upgrade fails the same test until it's mapped
//! here.
//!
//! The theme can't express everything `docs/DESIGN.md` specifies. Several
//! components need a Coffer build or per-instance styling on top; the list,
//! with the reason for each, is in `DESIGN.md` → "Implementing in GPUI" →
//! "Stock components".
//!
//! The map is by meaning, not by name. `gpui-component` follows shadcn's
//! roles, so its `primary` is Coffer's neutral primary button (`text` fill,
//! `on-light` label), not an accent. `molten` appears only where a stock role
//! means selection, focus or a drop target; `moldavite` only where it means
//! green as a literal hue. Coffer has no success or info colour: success and
//! info map to the neutral primary.

use gpui_kit::component::{Theme, ThemeColor, ThemeMode};
use gpui_kit::{App, Hsla, Pixels, Rgba, transparent_black};

use super::tokens::{color, motion, opacity, radius, size, text};

/// How many rems tall a medium stock control is: buttons, inputs and selects
/// all use `h_8`, which is 2rem.
const STOCK_CONTROL_REMS: f32 = 2.;

/// Dialog and sheet backdrop. Not designed in Paper yet; `chrome` at
/// [`opacity::MUTED`] until it is.
const SCRIM: Rgba = Rgba {
    a: opacity::MUTED,
    ..color::CHROME
};

/// The rem base: the size every rem-based stock length scales from.
///
/// `gpui-component` sizes its controls in rems (medium: `h_8`, `text_sm`).
/// Setting the rem to half of [`size::CONTROL`] makes those controls exactly
/// 28px. Their `text_sm` (0.875rem) then lands at 12.25px against Coffer's
/// 12px `--text-sm`. Coffer's own components take sizes from
/// [`super::tokens`], so they don't depend on this value.
pub fn rem() -> Pixels {
    size::CONTROL / STOCK_CONTROL_REMS
}

/// Switches `gpui-component` to dark mode and maps its whole theme onto the
/// tokens.
///
/// Call once at startup, after `gpui_kit::init` and before
/// [`super::fonts::install`] (which sets the font families). Switching mode
/// with [`Theme::change`] or `Theme::sync_system_appearance` afterwards would
/// reload the stock theme; Coffer has one theme and never does.
pub fn install(cx: &mut App) {
    // `Theme::update` loads the new mode's stock palette when the mode
    // changes, so switch first and map in a second update.
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, apply);
}

/// Everything [`install`] sets, on a theme value.
fn apply(theme: &mut Theme) {
    paint(&mut theme.colors);

    theme.font_size = rem();
    theme.mono_font_size = text::SM;
    // `radius` drives rows, fields, tabs and buttons; `radius_lg` dialogs,
    // notifications and other floating surfaces.
    theme.radius = radius::MD;
    theme.radius_lg = radius::LG;
    // Coffer is flat: only floating surfaces get a shadow, and those use
    // `tokens::shadow`. This drops the stock `shadow_xs` on buttons.
    theme.shadow = false;
    // Stock focus without the stock ring. The ring is a 3px band of `ring` at
    // 50% alpha, flush outside a `ring`-tinted border, and an ancestor that
    // clips cuts it off. Without it, a bordered control's border turns
    // `molten` and a borderless one draws a 1px `molten` line on or just
    // outside its edge: thinner than the 1.5px spec ring and with less or no
    // gap, but solid `molten`, and it takes no room, so nothing clips it.
    // Filled buttons draw that line inside the fill, in their label colour at
    // 60%; Coffer's buttons (`crate::ui::button`) turn it off and draw
    // `super::focus`'s ring. `DESIGN.md` → "Focus ring" has the comparison.
    theme.focus_ring = false;

    let motion_tokens = &mut theme.motion;
    motion_tokens.duration_fast = motion::FAST;
    motion_tokens.duration_normal = motion::BASE;
    motion_tokens.duration_slow = motion::SLOW;
    motion_tokens.easing_enter = easing(motion::EASE_ENTER);
    motion_tokens.easing_exit = easing(motion::EASE_EXIT);
    // DESIGN.md uses one curve for entering and moving.
    motion_tokens.easing_move = easing(motion::EASE_ENTER);
    // Critically damped (the default ratio): no bounce or spring.
    motion_tokens.spring_control = gpui_kit::base::Spring::new(motion::BASE);
    motion_tokens.spring_move = gpui_kit::base::Spring::new(motion::SLOW);
}

fn easing(curve: motion::CubicBezier) -> gpui_kit::base::Easing {
    gpui_kit::base::Easing::cubic_bezier(curve.x1, curve.y1, curve.x2, curve.y2)
        .expect("motion curves in tokens.rs are valid cubic-béziers")
}

/// Assigns every [`ThemeColor`] field a token (or no fill).
fn paint(colors: &mut ThemeColor) {
    fn token(value: Rgba) -> Hsla {
        value.into()
    }
    let none = transparent_black();

    // Surfaces and text.
    colors.background = token(color::CANVAS);
    colors.foreground = token(color::TEXT);
    colors.border = token(color::HAIRLINE);
    colors.window_border = token(color::BORDER);
    colors.overlay = token(SCRIM);
    colors.popover = token(color::FLOAT);
    colors.popover_foreground = token(color::TEXT);
    colors.muted = token(color::FIELD);
    colors.muted_foreground = token(color::TEXT_MUTED);
    colors.skeleton = token(color::FIELD);
    colors.accordion = none;
    colors.group_box = token(color::RAISED);
    colors.group_box_foreground = token(color::TEXT);
    colors.description_list_label = none;
    colors.description_list_label_foreground = token(color::TEXT_MUTED);

    // Interaction: hover, selection, focus.
    colors.accent = token(color::HOVER);
    colors.accent_foreground = token(color::TEXT);
    colors.ring = token(color::MOLTEN);
    colors.selection = token(color::MOLTEN_SOFT);
    colors.caret = token(color::TEXT);
    colors.drag_border = token(color::MOLTEN);
    colors.drop_target = token(color::MOLTEN_SOFT);
    colors.input = token(color::HAIRLINE);
    colors.link = token(color::TEXT);
    colors.link_hover = token(color::WHITE);
    colors.link_active = token(color::TEXT_STRONG);

    // Neutral primary: `text` fill, white on hover, `text-strong` pressed.
    // Success and info share it; Coffer has no colour for either.
    for (fill, hover, pressed, label) in [
        (
            &mut colors.primary,
            &mut colors.primary_hover,
            &mut colors.primary_active,
            &mut colors.primary_foreground,
        ),
        (
            &mut colors.button_primary,
            &mut colors.button_primary_hover,
            &mut colors.button_primary_active,
            &mut colors.button_primary_foreground,
        ),
        (
            &mut colors.success,
            &mut colors.success_hover,
            &mut colors.success_active,
            &mut colors.success_foreground,
        ),
        (
            &mut colors.button_success,
            &mut colors.button_success_hover,
            &mut colors.button_success_active,
            &mut colors.button_success_foreground,
        ),
        (
            &mut colors.info,
            &mut colors.info_hover,
            &mut colors.info_active,
            &mut colors.info_foreground,
        ),
        (
            &mut colors.button_info,
            &mut colors.button_info_hover,
            &mut colors.button_info_active,
            &mut colors.button_info_foreground,
        ),
    ] {
        *fill = token(color::TEXT);
        *hover = token(color::WHITE);
        *pressed = token(color::TEXT_STRONG);
        *label = token(color::ON_LIGHT);
    }

    // Secondary: a filled grey control (`field`, `hover`, `control-active`).
    for (fill, hover, pressed, label) in [
        (
            &mut colors.secondary,
            &mut colors.secondary_hover,
            &mut colors.secondary_active,
            &mut colors.secondary_foreground,
        ),
        (
            &mut colors.button_secondary,
            &mut colors.button_secondary_hover,
            &mut colors.button_secondary_active,
            &mut colors.button_secondary_foreground,
        ),
    ] {
        *fill = token(color::FIELD);
        *hover = token(color::HOVER);
        *pressed = token(color::CONTROL_ACTIVE);
        *label = token(color::TEXT_SECONDARY);
    }

    // Default button: Coffer's outline button. `input` is its border.
    colors.button = none;
    colors.button_hover = token(color::HOVER);
    colors.button_active = token(color::CONTROL_ACTIVE);
    colors.button_foreground = token(color::TEXT_STRONG);

    // Danger: the solid danger button and its states.
    for (fill, hover, pressed, label) in [
        (
            &mut colors.danger,
            &mut colors.danger_hover,
            &mut colors.danger_active,
            &mut colors.danger_foreground,
        ),
        (
            &mut colors.button_danger,
            &mut colors.button_danger_hover,
            &mut colors.button_danger_active,
            &mut colors.button_danger_foreground,
        ),
    ] {
        *fill = token(color::DANGER);
        *hover = token(color::DANGER_HOVER);
        *pressed = token(color::DANGER_PRESSED);
        *label = token(color::ON_DANGER);
    }

    // Warning. Coffer has no warning button, so there are no hover or pressed
    // designs; the states keep the base colour.
    for (fill, hover, pressed, label) in [
        (
            &mut colors.warning,
            &mut colors.warning_hover,
            &mut colors.warning_active,
            &mut colors.warning_foreground,
        ),
        (
            &mut colors.button_warning,
            &mut colors.button_warning_hover,
            &mut colors.button_warning_active,
            &mut colors.button_warning_foreground,
        ),
    ] {
        *fill = token(color::WARNING);
        *hover = token(color::WARNING);
        *pressed = token(color::WARNING);
        *label = token(color::ON_WARNING);
    }

    // Lists and tables sit on their surface: no fill, no stripes, no row
    // dividers; `hover` on hover, `molten-soft` when selected, sticky header
    // on `canvas` with section-label (`text-muted`) text.
    colors.list = none;
    colors.list_even = none;
    colors.list_head = none;
    colors.list_hover = token(color::HOVER);
    colors.list_active = token(color::MOLTEN_SOFT);
    colors.list_active_border = token(color::MOLTEN);
    colors.table = none;
    colors.table_even = none;
    colors.table_row_border = none;
    colors.table_hover = token(color::HOVER);
    colors.table_active = token(color::MOLTEN_SOFT);
    colors.table_active_border = token(color::MOLTEN);
    colors.table_head = token(color::CANVAS);
    colors.table_head_foreground = token(color::TEXT_MUTED);
    colors.table_foot = token(color::CANVAS);
    colors.table_foot_foreground = token(color::TEXT_MUTED);

    // App frame: `chrome` title bar and side panel, `float` status bar, all
    // with hairline borders.
    colors.title_bar = token(color::CHROME);
    colors.title_bar_border = token(color::HAIRLINE);
    colors.status_bar = token(color::FLOAT);
    colors.status_bar_border = token(color::HAIRLINE);
    colors.sidebar = token(color::CHROME);
    colors.sidebar_border = token(color::HAIRLINE);
    colors.sidebar_foreground = token(color::TEXT_SECONDARY);
    colors.sidebar_accent = token(color::HOVER);
    colors.sidebar_accent_foreground = token(color::TEXT);
    colors.sidebar_primary = token(color::MOLTEN_SOFT);
    colors.sidebar_primary_foreground = token(color::WHITE);

    // Tabs: muted label, `hover` fill and `text` label when active; the
    // segmented track is `field`.
    colors.tab = none;
    colors.tab_bar = none;
    colors.tab_bar_segmented = token(color::FIELD);
    colors.tab_foreground = token(color::TEXT_MUTED);
    colors.tab_active = token(color::HOVER);
    colors.tab_active_foreground = token(color::TEXT);

    // Toggle: off track `control-active`, off knob `text-muted`; the on track
    // is `primary` (`text`). The stock switch has one knob colour, so the on
    // knob (`chrome`) needs Coffer's own toggle.
    colors.switch = token(color::CONTROL_ACTIVE);
    colors.switch_thumb = token(color::TEXT_MUTED);
    colors.slider_bar = token(color::TEXT);
    colors.slider_thumb = token(color::WHITE);
    // Neutral progress (imports). Agent progress passes `moldavite` per
    // instance.
    colors.progress_bar = token(color::TEXT);

    // Scrollbars aren't designed in Paper yet.
    colors.scrollbar = none;
    colors.scrollbar_thumb = token(color::BORDER_STRONG);
    colors.scrollbar_thumb_hover = token(color::TOOLTIP_BORDER);

    // Charts: the categorical palette, hairline gridlines. Coffer never
    // colours a sign, so candlestick up/down are both neutral.
    colors.chart_1 = token(color::CHART_1);
    colors.chart_2 = token(color::CHART_2);
    colors.chart_3 = token(color::CHART_3);
    colors.chart_4 = token(color::CHART_4);
    colors.chart_5 = token(color::CHART_5);
    colors.chart_grid = token(color::HAIRLINE);
    colors.chart_bullish = token(color::TEXT_SECONDARY);
    colors.chart_bearish = token(color::TEXT_SECONDARY);

    // Literal hues, for components that ask for "red" or "blue" by name: the
    // status colours, the agent green, and the nearest chart colours.
    colors.red = token(color::DANGER);
    colors.red_light = token(color::DANGER_SOFT);
    colors.yellow = token(color::WARNING);
    colors.yellow_light = token(color::WARNING_SOFT);
    colors.green = token(color::MOLDAVITE);
    colors.green_light = token(color::MOLDAVITE_SOFT);
    colors.blue = token(color::CHART_5);
    colors.blue_light = token(color::CHART_1);
    colors.cyan = token(color::CHART_4);
    colors.cyan_light = token(color::CHART_1);
    colors.magenta = token(color::CHART_6);
    colors.magenta_light = token(color::CHART_2);
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use gpui_kit::base::ThemeAppearance;
    use gpui_kit::component::ThemeTokens;
    use gpui_kit::{TestAppContext, px};
    use serde_json::Value;

    use super::super::tokens;
    use super::*;

    /// A colour no token uses, for spotting fields the map doesn't set.
    const SENTINEL: &str = "#010203ff";

    /// Each field of a palette as gpui serialises it: `#rrggbbaa`.
    fn fields(colors: &ThemeColor) -> BTreeMap<String, String> {
        let Value::Object(map) = serde_json::to_value(colors).expect("palette serialises") else {
            panic!("a palette serialises to an object");
        };
        map.into_iter()
            .map(|(field, value)| {
                let hex = value.as_str().expect("colours serialise as strings");
                (field, hex.to_owned())
            })
            .collect()
    }

    /// A palette with every field set to [`SENTINEL`].
    fn sentinel_palette() -> ThemeColor {
        let filled: serde_json::Map<String, Value> = fields(&ThemeColor::default())
            .into_keys()
            .map(|field| (field, Value::from(SENTINEL)))
            .collect();
        serde_json::from_value(Value::Object(filled)).expect("palette deserialises")
    }

    /// The values a mapped field may hold, as `#rrggbbaa`: every exported
    /// colour token, no fill, and the derived values this module documents.
    fn allowed() -> BTreeSet<String> {
        let mut allowed: BTreeSet<String> = tokens::tests::code_tokens()
            .into_iter()
            .filter(|(name, _)| name.starts_with("--color-"))
            .map(|(_, hex)| {
                let hex = hex.to_lowercase();
                if hex.len() == 7 { hex + "ff" } else { hex }
            })
            .collect();
        allowed.insert("#00000000".into());
        allowed.insert(fields_hex(SCRIM));
        allowed
    }

    fn fields_hex(color: Rgba) -> String {
        serde_json::to_value(color)
            .expect("colour serialises")
            .as_str()
            .expect("colours serialise as strings")
            .to_owned()
    }

    #[test]
    fn every_stock_colour_is_a_coffer_token() {
        let mut colors = sentinel_palette();
        paint(&mut colors);

        let allowed = allowed();
        let leaks: Vec<_> = fields(&colors)
            .into_iter()
            .filter(|(_, hex)| !allowed.contains(hex))
            .collect();
        assert!(
            leaks.is_empty(),
            "fields left unmapped ({SENTINEL}) or set to a non-token colour: {leaks:?}"
        );
    }

    #[test]
    fn sentinel_reaches_every_field() {
        // Guards the leak test: if the sentinel didn't land everywhere, an
        // unmapped field could keep a default that happens to be allowed.
        let fields = fields(&sentinel_palette());
        assert!(fields.len() > 100, "only {} fields", fields.len());
        assert!(fields.values().all(|hex| hex == SENTINEL));
    }

    #[test]
    fn rem_makes_stock_medium_controls_control_height() {
        assert_eq!(rem() * STOCK_CONTROL_REMS, size::CONTROL);
        assert_eq!(rem(), px(14.));
    }

    #[test]
    fn motion_curves_are_valid() {
        let _ = easing(motion::EASE_ENTER);
        let _ = easing(motion::EASE_EXIT);
    }

    /// Checks a theme value holds everything [`apply`] sets.
    fn assert_applied(theme: &Theme) {
        let mut expected = sentinel_palette();
        paint(&mut expected);
        assert_eq!(fields(&theme.colors), fields(&expected));
        assert_eq!(theme.font_size, rem());
        assert_eq!(theme.mono_font_size, text::SM);
        assert_eq!(theme.radius, radius::MD);
        assert_eq!(theme.radius_lg, radius::LG);
        assert!(!theme.shadow);
        assert!(!theme.focus_ring);
        assert_eq!(theme.motion.duration_fast, motion::FAST);
        assert_eq!(theme.motion.duration_normal, motion::BASE);
        assert_eq!(theme.motion.duration_slow, motion::SLOW);

        // Components built on the semantic surface (and the Base layer's
        // scrollbars) read this projection of the same values.
        let semantic = theme.semantic_tokens();
        assert_eq!(semantic.colors.background, color::CANVAS.into());
        assert_eq!(semantic.colors.surface, color::FLOAT.into());
        assert_eq!(semantic.colors.ring, color::MOLTEN.into());
        assert_eq!(semantic.radius.md, radius::MD);
        assert!(semantic.shadow.md.is_empty());
    }

    /// Checks the global theme is what [`install`] sets, in all three copies
    /// `gpui-component` keeps: `colors`, the `tokens` painted from them, and
    /// the Base layer's projection.
    fn assert_installed(cx: &App) {
        let theme = Theme::global(cx);
        assert!(theme.mode.is_dark());
        assert_applied(theme);
        // A stock token whose colour the map didn't change would keep its
        // gradient and paint over the mapped colour.
        assert_eq!(theme.tokens, ThemeTokens::from(theme.colors));

        let base = gpui_kit::base::Theme::global(cx);
        assert_eq!(base.appearance, ThemeAppearance::Dark);
        assert_eq!(base.tokens, theme.semantic_tokens());
        assert_eq!(base.tokens.colors.background, color::CANVAS.into());
        assert_eq!(base.tokens.colors.ring, color::MOLTEN.into());
    }

    #[test]
    fn apply_sets_the_whole_theme() {
        let mut theme = Theme::default();
        apply(&mut theme);
        assert_applied(&theme);
    }

    /// The startup path: `gpui_kit::init` installs the stock theme, then
    /// [`install`] switches mode and maps it through `Theme::update`.
    #[gpui_kit::test]
    fn install_maps_the_global_theme(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            install(cx);
            assert_installed(cx);
        });
    }
}
