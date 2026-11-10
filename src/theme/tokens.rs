//! Design tokens from `docs/DESIGN.md` as typed GPUI values.
//!
//! This is the only place in the app where colour, size, and duration literals
//! may appear. Everything else references these constants by name.
//!
//! Two kinds of values live here:
//!
//! - **Exported tokens**: the tokens in `design/tokens.json` /
//!   `design/tokens.css`, exported from the Coffer Paper file. Each constant's
//!   docs start with its CSS custom property (`--color-text-muted` →
//!   [`color::TEXT_MUTED`], `--spacing-2` → [`spacing::S2`]). A test checks every
//!   exported token against its constant, so a re-export that adds, removes or
//!   changes a token fails the build until this module follows.
//! - **Code-only values**: things Paper has no token type for (motion, shadows,
//!   border and ring widths, the minimum window size) and values derived from
//!   tokens ([`color::CHART`]). `DESIGN.md` is their source.

use std::time::Duration;

use gpui_kit::{BoxShadow, FontWeight, Pixels, Rgba, point, px};

/// `#RRGGBB` as an opaque colour.
const fn opaque(rgb: u32) -> Rgba {
    translucent((rgb << 8) | 0xFF)
}

/// `#RRGGBBAA` as a colour with alpha.
const fn translucent(rgba: u32) -> Rgba {
    let [r, g, b, a] = rgba.to_be_bytes();
    Rgba {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: a as f32 / 255.0,
    }
}

/// Colour tokens.
///
/// Two accents carry one meaning each: [`MOLTEN`](color::MOLTEN) is brand, selection, focus,
/// and "you are here"; [`MOLDAVITE`](color::MOLDAVITE) is the AI agent. Everything else is
/// greyscale. Colour never carries the sign of an amount.
pub mod color {
    use super::{Rgba, opaque, translucent};

    // Surfaces, text and the two accents.
    /// `--color-chrome`: title bar, side panels, inspector (the app frame).
    pub const CHROME: Rgba = opaque(0x0F0F10);
    /// `--color-canvas`: main content area (the "work" surface).
    pub const CANVAS: Rgba = opaque(0x18181A);
    /// `--color-raised`: raised surface on canvas.
    pub const RAISED: Rgba = opaque(0x1A1A1C);
    /// `--color-field`: inputs, selects, segmented-control tracks, command bar,
    /// loading skeletons.
    pub const FIELD: Rgba = opaque(0x1F1F22);
    /// `--color-hover`: hovered/current row or tab; kbd background; tooltip fill.
    pub const HOVER: Rgba = opaque(0x26262A);
    /// `--color-hairline`: 1px dividers and panel borders; chart gridlines.
    pub const HAIRLINE: Rgba = opaque(0x232326);
    /// `--color-text`: primary text and icons; amounts.
    pub const TEXT: Rgba = opaque(0xEDEDEF);
    /// `--color-text-muted`: secondary text, labels, inactive tabs, default
    /// icons, chart axis labels.
    pub const TEXT_MUTED: Rgba = opaque(0x8E8E93);
    /// `--color-text-faint`: tertiary text, counts, field prefixes.
    ///
    /// 2.7:1 on canvas, which fails WCAG for text: use it only for decorative
    /// or redundant information, never for something the user needs to read.
    pub const TEXT_FAINT: Rgba = opaque(0x5C5C61);
    /// `--color-molten`: brand, selection outline and handles, focus ring,
    /// unread dot.
    pub const MOLTEN: Rgba = opaque(0xFF6B1A);
    /// `--color-molten-soft`: selected row background.
    pub const MOLTEN_SOFT: Rgba = translucent(0xFF6B1A24);
    /// `--color-moldavite`: AI agent: primary agent buttons, agent name, live
    /// dots, agent progress.
    pub const MOLDAVITE: Rgba = opaque(0x7CC48A);
    /// `--color-moldavite-soft`: agent count badges.
    pub const MOLDAVITE_SOFT: Rgba = translucent(0x7CC48A1F);

    // Neutrals.

    /// `--color-text-secondary`: text one step below primary: list labels,
    /// chips, kbd glyphs, tooltip body, secondary icons, spending bars.
    pub const TEXT_SECONDARY: Rgba = opaque(0xB4B4B9);
    /// `--color-text-strong`: text on outline buttons, empty-state titles,
    /// emphasized values; pressed neutral primary button.
    pub const TEXT_STRONG: Rgba = opaque(0xD6D6DA);
    /// `--color-icon-faint`: chevrons, sort indicators, income bars in charts.
    pub const ICON_FAINT: Rgba = opaque(0x6E6E73);
    /// `--color-icon-disabled`: disabled icons.
    pub const ICON_DISABLED: Rgba = opaque(0x4A4A4F);
    /// `--color-float`: floating surfaces (toolbar, cards, popovers), status bar.
    pub const FLOAT: Rgba = opaque(0x141416);
    /// `--color-control-active`: active segment, active icon button, toggle
    /// track (off), pressed state.
    pub const CONTROL_ACTIVE: Rgba = opaque(0x2C2C30);
    /// `--color-border`: borders on floating surfaces and chips; neutral
    /// progress track.
    pub const BORDER: Rgba = opaque(0x2A2A2E);
    /// `--color-border-strong`: outline-button borders, dashed empty states,
    /// inline separators, chart baseline.
    pub const BORDER_STRONG: Rgba = opaque(0x2E2E33);
    /// `--color-tooltip-border`: tooltip border, kbd inside tooltips.
    pub const TOOLTIP_BORDER: Rgba = opaque(0x333338);
    /// `--color-avatar`: neutral avatar fill.
    pub const AVATAR: Rgba = opaque(0x3A3A3F);
    /// `--color-on-light` (an alias of `--color-chrome`): text on the neutral
    /// primary button.
    pub const ON_LIGHT: Rgba = CHROME;
    /// `--color-white`: selected-row labels, selection handles, text on molten
    /// measurement labels, hovered neutral primary button.
    pub const WHITE: Rgba = opaque(0xFFFFFF);

    // Agent (moldavite) family.

    /// `--color-on-moldavite`: text on solid moldavite buttons.
    pub const ON_MOLDAVITE: Rgba = opaque(0x0E1710);
    /// `--color-moldavite-hover`: hovered agent primary button (text contrast
    /// 10.2:1).
    pub const MOLDAVITE_HOVER: Rgba = opaque(0x8FD09C);
    /// `--color-moldavite-pressed`: pressed agent primary button (text contrast
    /// 7.2:1).
    pub const MOLDAVITE_PRESSED: Rgba = opaque(0x6DB27B);
    /// `--color-moldavite-text`: agent text on dark ("Approve all", agent
    /// labels, AI-suggested categories).
    pub const MOLDAVITE_TEXT: Rgba = opaque(0xA9D9B2);
    /// `--color-moldavite-text-muted`: secondary text inside agent surfaces.
    pub const MOLDAVITE_TEXT_MUTED: Rgba = opaque(0x7E8A81);
    /// `--color-moldavite-text-faint`: timestamps inside agent surfaces.
    /// Non-essential text only (3.2:1).
    pub const MOLDAVITE_TEXT_FAINT: Rgba = opaque(0x5C6B60);
    /// `--color-moldavite-surface`: agent card fill.
    pub const MOLDAVITE_SURFACE: Rgba = opaque(0x111813);
    /// `--color-moldavite-surface-raised`: agent ghost button, hover on agent
    /// card rows, hovered agent outline button.
    pub const MOLDAVITE_SURFACE_RAISED: Rgba = opaque(0x162019);
    /// `--color-moldavite-well`: icon wells and agent avatar fill.
    pub const MOLDAVITE_WELL: Rgba = opaque(0x1B241D);
    /// `--color-moldavite-border`: agent card border.
    pub const MOLDAVITE_BORDER: Rgba = opaque(0x24352A);
    /// `--color-moldavite-border-strong`: agent outline button border.
    pub const MOLDAVITE_BORDER_STRONG: Rgba = opaque(0x2E4535);
    /// `--color-moldavite-track`: agent progress-bar track.
    pub const MOLDAVITE_TRACK: Rgba = opaque(0x22332A);

    // Status colours.

    /// `--color-danger`: danger text, icons, 6px dots, chart overage,
    /// destructive outline buttons (5.2:1 on canvas).
    pub const DANGER: Rgba = opaque(0xE5598E);
    /// `--color-danger-soft`: danger banner / row background.
    pub const DANGER_SOFT: Rgba = translucent(0xE5598E1F);
    /// `--color-on-danger`: text on a solid danger button (5.6:1 on danger).
    pub const ON_DANGER: Rgba = opaque(0x1A0B11);
    /// `--color-danger-hover`: hovered solid danger button ([`ON_DANGER`] text
    /// on it 6.7:1).
    pub const DANGER_HOVER: Rgba = opaque(0xED6F9B);
    /// `--color-danger-pressed`: pressed solid danger button ([`ON_DANGER`]
    /// text on it 4.5:1).
    pub const DANGER_PRESSED: Rgba = opaque(0xD04A7E);
    /// `--color-warning`: warning text, icons, dots, budget bars at 85% or more
    /// (10.3:1 on canvas).
    pub const WARNING: Rgba = opaque(0xE7C24A);
    /// `--color-warning-soft`: warning banner background.
    pub const WARNING_SOFT: Rgba = translucent(0xE7C24A1F);
    /// `--color-on-warning`: text on solid warning (rare; 10.7:1 on warning).
    pub const ON_WARNING: Rgba = opaque(0x1A1405);

    // Categorical chart palette.

    /// `--color-chart-1`: chart series 1 (13.2:1 on canvas).
    pub const CHART_1: Rgba = opaque(0xC1E3FC);
    /// `--color-chart-2`: chart series 2 (9.3:1 on canvas).
    pub const CHART_2: Rgba = opaque(0xFBA0CC);
    /// `--color-chart-3`: chart series 3 (7.1:1 on canvas).
    pub const CHART_3: Rgba = opaque(0xB396E8);
    /// `--color-chart-4`: chart series 4 (5.7:1 on canvas).
    pub const CHART_4: Rgba = opaque(0x1AA0B9);
    /// `--color-chart-5`: chart series 5 (4.1:1 on canvas).
    pub const CHART_5: Rgba = opaque(0x5777C1);
    /// `--color-chart-6`: chart series 6 (3.2:1 on canvas).
    pub const CHART_6: Rgba = opaque(0x92508C);
    /// The six categorical series in rank order. "Other" is not a seventh
    /// colour: draw it as a 1px [`TEXT_MUTED`] outline on [`FIELD`].
    pub const CHART: [Rgba; 6] = [CHART_1, CHART_2, CHART_3, CHART_4, CHART_5, CHART_6];
    /// Code-only, showcase only: the 1px inset ring round a colour swatch
    /// (Paper Foundations `9-0`), so swatches as dark as `canvas` keep an
    /// edge. White at 12%.
    pub const SWATCH_RING: Rgba = translucent(0xFFFFFF1F);
}

/// Font families. Both are bundled with the app (SIL OFL 1.1).
pub mod font {
    /// `--font-ui`: Inter, for words.
    pub const UI: &str = "Inter";
    /// `--font-mono`: Geist Mono, for every amount, date, percentage, count,
    /// and shortcut.
    pub const MONO: &str = "Geist Mono";
}

/// Font sizes.
pub mod text {
    use super::{Pixels, px};

    /// `--text-2xs`: mono micro and badges: kbd glyphs, chart axis labels,
    /// selection labels.
    pub const MICRO: Pixels = px(10.);
    /// `--text-xs`: captions, section labels, mono values.
    pub const XS: Pixels = px(11.);
    /// `--text-sm`: body, buttons, mono amounts.
    pub const SM: Pixels = px(12.);
    /// `--text-md`: titles, reading body.
    pub const MD: Pixels = px(13.);
    /// `--text-lg`: reading headings.
    pub const LG: Pixels = px(15.);
    /// `--text-xl`: Overview "Income" and "Net" figures.
    pub const FIGURE: Pixels = px(20.);
    /// `--text-2xl`: display headings and the display figure.
    pub const DISPLAY: Pixels = px(32.);
}

/// Line heights. They snap to a 2px grid.
pub mod leading {
    use super::{Pixels, px};

    /// `--leading-micro`: paired with [`super::text::MICRO`].
    pub const MICRO: Pixels = px(12.);
    /// `--leading-caption`: paired with [`super::text::XS`].
    pub const CAPTION: Pixels = px(14.);
    /// `--leading-ui`: paired with [`super::text::SM`] and [`super::text::MD`].
    pub const UI: Pixels = px(16.);
    /// `--leading-reading`: reading type, 13/20 body and 15/20 headings.
    pub const READING: Pixels = px(20.);
    /// `--leading-figure`: paired with [`super::text::FIGURE`].
    pub const FIGURE: Pixels = px(24.);
    /// `--leading-display`: paired with [`super::text::DISPLAY`].
    pub const DISPLAY: Pixels = px(38.);
}

/// Font weights. All four exist as static Inter instances. Artboard specs from
/// the reference file that quote 550 mean [`MEDIUM`](weight::MEDIUM) and 650
/// mean [`STRONG`](weight::STRONG).
pub mod weight {
    use super::FontWeight;

    /// `--font-weight-regular`: body, captions, mono values and amounts.
    pub const REGULAR: FontWeight = FontWeight(400.);
    /// `--font-weight-medium`: emphasised body, outline buttons.
    pub const MEDIUM: FontWeight = FontWeight(500.);
    /// `--font-weight-strong`: titles, section labels, neutral primary button,
    /// display.
    pub const STRONG: FontWeight = FontWeight(600.);
    /// `--font-weight-bold`: badges, agent primary button.
    pub const BOLD: FontWeight = FontWeight(700.);
}

/// Letter spacing, in em.
///
/// GPUI's text style has no letter-spacing property, so these cannot be
/// applied yet. They are kept so the type roles stay complete.
pub mod tracking {
    /// `--tracking-tight`: titles, figures, the wordmark.
    pub const TIGHT: f32 = -0.01;
    /// `--tracking-caps`: uppercase section labels.
    pub const CAPS: f32 = 0.06;
    /// `--tracking-display`: display headings and the display figure.
    pub const DISPLAY: f32 = -0.025;
}

/// Spacing on the 4px grid. `Sn` is `--spacing-n` (`n × 4px`); half steps write
/// the `.` as `_` (`S1_5` is `--spacing-1_5`, 6px). Component geometry that Paper
/// also stores as `spacing` tokens lives in [`size`].
pub mod spacing {
    use super::{Pixels, px};

    /// `--spacing-0_5`: separator and toolbar gaps.
    pub const S0_5: Pixels = px(2.);
    /// `--spacing-1`.
    pub const S1: Pixels = px(4.);
    /// `--spacing-1_5`: tight icon + label pairs; list-section inline padding.
    pub const S1_5: Pixels = px(6.);
    /// `--spacing-2`.
    pub const S2: Pixels = px(8.);
    /// `--spacing-2_5`: tight icon + label pairs; list-section bottom padding.
    pub const S2_5: Pixels = px(10.);
    /// `--spacing-3`.
    pub const S3: Pixels = px(12.);
    /// `--spacing-3_5`: inspector section padding.
    pub const S3_5: Pixels = px(14.);
    /// `--spacing-4`.
    pub const S4: Pixels = px(16.);
    /// `--spacing-5`: nesting indent for pots under their parent account.
    pub const S5: Pixels = px(20.);
    /// `--spacing-6`: floating bulk-action bar offset above the bottom edge.
    pub const S6: Pixels = px(24.);
}

/// Corner radii.
pub mod radius {
    use super::{Pixels, px};

    /// `--radius-marker`: colour swatches and markers, chart bars, progress
    /// bars.
    pub const MARKER: Pixels = px(2.);
    /// `--radius-label`: molten measurement labels.
    pub const LABEL: Pixels = px(3.);
    /// `--radius-sm`: kbd, inner segments, tab close button, skeletons.
    pub const SM: Pixels = px(4.);
    /// `--radius-md`: rows, fields, tabs, icon buttons, outline buttons.
    pub const MD: Pixels = px(6.);
    /// `--radius-button`: primary buttons and the command bar.
    pub const BUTTON: Pixels = px(7.);
    /// `--radius-surface`: segmented track, tooltip, cards in panels, empty
    /// state, badges, toggle track.
    pub const SURFACE: Pixels = px(8.);
    /// `--radius-lg`: floating toolbar, agent card.
    pub const LG: Pixels = px(10.);
    /// `--radius-card`: cards.
    pub const CARD: Pixels = px(12.);
    /// `--radius-pill`: chips (28px pill).
    pub const PILL: Pixels = px(14.);
}

/// Fixed sizes: control heights, the app frame, and small component geometry.
///
/// Paper stores heights and geometry as `spacing` tokens and layout widths as
/// `container` tokens. Border and ring widths and the minimum window size have
/// no token type and are code-only.
pub mod size {
    use super::{Pixels, px};

    // Control heights.

    /// `--spacing-control-xs`: toggle, badges, kbd inside tooltips, agent-card
    /// icon wells.
    pub const CONTROL_XS: Pixels = px(16.);
    /// `--spacing-kbd`: kbd; tab close button.
    pub const KBD: Pixels = px(18.);
    /// `--spacing-control-sm`: small (outline) buttons.
    pub const CONTROL_SM: Pixels = px(24.);
    /// `--spacing-control-compact`: compact buttons, avatar.
    pub const CONTROL_COMPACT: Pixels = px(26.);
    /// `--spacing-control`: the default: rows, tabs, fields, icon buttons,
    /// chips.
    pub const CONTROL: Pixels = px(28.);
    /// `--spacing-control-lg`: primary buttons, segmented control.
    pub const CONTROL_LG: Pixels = px(30.);
    /// `--spacing-control-xl`: floating toolbar, agent ghost button.
    pub const CONTROL_XL: Pixels = px(36.);

    // App frame.

    /// `--spacing-title-bar`: title bar height.
    pub const TITLE_BAR: Pixels = px(40.);
    /// `--container-panel`: left panel width.
    pub const PANEL: Pixels = px(272.);
    /// `--container-inspector`: right inspector width.
    pub const INSPECTOR: Pixels = px(296.);
    /// `--spacing-status-bar`: status bar height.
    pub const STATUS_BAR: Pixels = px(28.);
    /// `--container-command-bar`: command bar width.
    pub const COMMAND_BAR: Pixels = px(340.);
    /// `--container-search-field`: transactions toolbar search field width.
    pub const SEARCH_FIELD: Pixels = px(240.);
    /// `--container-dialog`: short dialogs (a confirmation or a form of a few
    /// fields).
    pub const DIALOG: Pixels = px(440.);
    /// `--container-field-label`: inspector field-row label column.
    pub const FIELD_LABEL: Pixels = px(64.);
    /// `--container-column-category`: transactions table category column.
    pub const COLUMN_CATEGORY: Pixels = px(180.);
    /// `--container-column-account`: transactions table account column.
    pub const COLUMN_ACCOUNT: Pixels = px(140.);
    /// `--container-column-amount`: transactions table amount column (minimum).
    pub const COLUMN_AMOUNT: Pixels = px(96.);
    /// `--breakpoint-desktop`.
    pub const BREAKPOINT_DESKTOP: Pixels = px(1920.);
    /// Minimum window width.
    pub const WINDOW_MIN_WIDTH: Pixels = px(1280.);
    /// Minimum window height.
    pub const WINDOW_MIN_HEIGHT: Pixels = px(800.);

    // Component geometry.

    /// Dividers and borders.
    pub const HAIRLINE: Pixels = px(1.);
    /// Selection outline, underline-tab indicator, chart lines.
    pub const STROKE: Pixels = px(1.5);
    /// Focus ring width (focus-visible only).
    pub const FOCUS_RING: Pixels = px(1.5);
    /// Focus ring gap outside the element; its radius is the element's + 2.
    pub const FOCUS_RING_OFFSET: Pixels = px(2.);
    /// `--spacing-progress`: progress bars and ranked-bar-list bars.
    pub const PROGRESS: Pixels = px(3.);
    /// `--spacing-dot`: status dots and chart hover points.
    pub const DOT: Pixels = px(6.);
    /// `--spacing-handle`: selection handles.
    pub const HANDLE: Pixels = px(7.);
    /// `--spacing-marker`: colour markers (tabs, category squares, chart
    /// legends); select chevron.
    pub const MARKER: Pixels = px(8.);
    /// `--spacing-toggle-knob`: toggle knob.
    pub const TOGGLE_KNOB: Pixels = px(12.);
    /// `--spacing-toggle-width`: toggle track width (its height is
    /// [`CONTROL_XS`]).
    pub const TOGGLE_WIDTH: Pixels = px(28.);
    /// `--spacing-icon-xs`: icon size.
    pub const ICON_XS: Pixels = px(10.);
    /// `--spacing-icon-sm`: icon size.
    pub const ICON_SM: Pixels = px(12.);
    /// `--spacing-icon-md`: icon size.
    pub const ICON_MD: Pixels = px(14.);
    /// `--spacing-icon-lg`: icon size.
    pub const ICON_LG: Pixels = px(16.);
}

/// Opacity.
pub mod opacity {
    /// `--opacity-muted`: muted elements; the low end of the skeleton pulse.
    pub const MUTED: f32 = 0.6;
    /// `--opacity-disabled`: disabled controls.
    pub const DISABLED: f32 = 0.4;
    /// `--opacity-area-fill`: chart area fill under a series line.
    pub const AREA_FILL: f32 = 0.08;
}

/// Motion. Paper has no motion token type, so these are code-only and
/// `DESIGN.md` is their source. With the OS reduced-motion setting, every
/// animation, opacity fades included, goes to zero: `gpui_kit::init` reads
/// the setting into `App::reduce_motion`, and GPUI and Base animations then
/// jump to their end state (repeating ones hold still).
pub mod motion {
    use super::Duration;

    /// `--motion-fast`: hover and pressed colour changes.
    pub const FAST: Duration = Duration::from_millis(90);
    /// `--motion-base`: toggles, segmented control, tooltip and popover in.
    pub const BASE: Duration = Duration::from_millis(150);
    /// `--motion-slow`: panels, inspector, sheets, the import dialog.
    pub const SLOW: Duration = Duration::from_millis(220);
    /// `--motion-chart`: chart data changes.
    pub const CHART: Duration = Duration::from_millis(320);
    /// Hover delay before a tooltip appears.
    pub const TOOLTIP_DELAY: Duration = Duration::from_millis(500);
    /// One full skeleton pulse ([`super::opacity::MUTED`] ↔ 100%).
    pub const SKELETON_PULSE: Duration = Duration::from_millis(1200);

    /// A CSS `cubic-bezier(x1, y1, x2, y2)` timing curve.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct CubicBezier {
        pub x1: f32,
        pub y1: f32,
        pub x2: f32,
        pub y2: f32,
    }

    /// Entering and moving.
    pub const EASE_ENTER: CubicBezier = CubicBezier {
        x1: 0.2,
        y1: 0.,
        x2: 0.,
        y2: 1.,
    };
    /// Exits; run them for [`exit`] of the entering duration.
    pub const EASE_EXIT: CubicBezier = CubicBezier {
        x1: 0.4,
        y1: 0.,
        x2: 1.,
        y2: 1.,
    };

    /// The exit duration for an entering `duration`: two-thirds of it.
    pub const fn exit(duration: Duration) -> Duration {
        Duration::from_micros(duration.as_micros() as u64 * 2 / 3)
    }
}

/// Elevation. Flat by default; only floating things get shadow.
pub mod shadow {
    use super::{BoxShadow, Pixels, Rgba, point, px, translucent};

    /// One shadow layer, as CSS `box-shadow` would write it.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Shadow {
        pub offset_y: Pixels,
        /// The CSS blur radius. [`box_shadow`](Self::box_shadow) halves it
        /// for GPUI.
        pub blur: Pixels,
        pub spread: Pixels,
        pub color: Rgba,
        pub inset: bool,
    }

    impl Shadow {
        /// The layer as a GPUI shadow, for `Styled::shadow`.
        ///
        /// CSS draws a blur radius `b` as a Gaussian with σ = `b`/2; GPUI's
        /// shader takes `blur_radius` as σ itself. Passing the CSS value
        /// through would draw the shadow twice as soft, so it's halved here.
        pub fn box_shadow(self) -> BoxShadow {
            BoxShadow {
                color: self.color.into(),
                offset: point(px(0.), self.offset_y),
                blur_radius: self.blur / 2.,
                spread_radius: self.spread,
                inset: self.inset,
            }
        }
    }

    /// `inset 0 1px 0 #FFFFFF0A`: active segment in a segmented control.
    pub const INSET_HIGHLIGHT: Shadow = Shadow {
        offset_y: px(1.),
        blur: px(0.),
        spread: px(0.),
        color: translucent(0xFFFFFF0A),
        inset: true,
    };
    /// `inset 0 0 0 1px #FFFFFF1F`: colour swatches.
    pub const SWATCH_RING: Shadow = Shadow {
        offset_y: px(0.),
        blur: px(0.),
        spread: px(1.),
        color: translucent(0xFFFFFF1F),
        inset: true,
    };
    /// `0 6px 18px #00000066`: tooltips.
    pub const TOOLTIP: Shadow = Shadow {
        offset_y: px(6.),
        blur: px(18.),
        spread: px(0.),
        color: translucent(0x00000066),
        inset: false,
    };
    /// `0 8px 24px #00000073`: the floating bulk-action bar and popovers.
    pub const TOOLBAR: Shadow = Shadow {
        offset_y: px(8.),
        blur: px(24.),
        spread: px(0.),
        color: translucent(0x00000073),
        inset: false,
    };
}

/// Text roles from the type scale in `DESIGN.md`. Colour is not part of a role:
/// it depends on where the text sits.
pub mod typography {
    use super::{FontWeight, Pixels, font, leading, text, tracking, weight};

    /// One row of the type scale.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct TextRole {
        pub family: &'static str,
        pub size: Pixels,
        pub line_height: Pixels,
        pub weight: FontWeight,
        /// Letter spacing in em; see [`super::tracking`].
        pub tracking: f32,
        pub uppercase: bool,
    }

    const fn role(
        family: &'static str,
        size: Pixels,
        line_height: Pixels,
        weight: FontWeight,
        tracking: f32,
    ) -> TextRole {
        TextRole {
            family,
            size,
            line_height,
            weight,
            tracking,
            uppercase: false,
        }
    }

    /// 32/38 600: hero heading.
    pub const DISPLAY: TextRole = role(
        font::UI,
        text::DISPLAY,
        leading::DISPLAY,
        weight::STRONG,
        tracking::DISPLAY,
    );
    /// 32/38 mono 600: Overview "Spent" total; transaction amount in the
    /// inspector.
    pub const DISPLAY_FIGURE: TextRole = role(
        font::MONO,
        text::DISPLAY,
        leading::DISPLAY,
        weight::STRONG,
        tracking::DISPLAY,
    );
    /// 20/24 mono 600: Overview "Income" and "Net".
    pub const FIGURE: TextRole = role(
        font::MONO,
        text::FIGURE,
        leading::FIGURE,
        weight::STRONG,
        tracking::TIGHT,
    );
    /// 15/20 600: headings in saved AI reviews.
    pub const READING_HEADING: TextRole = role(
        font::UI,
        text::LG,
        leading::READING,
        weight::STRONG,
        tracking::TIGHT,
    );
    /// 13/20 400: body of saved AI reviews.
    pub const READING_BODY: TextRole =
        role(font::UI, text::MD, leading::READING, weight::REGULAR, 0.);
    /// 13/16 600: panel header, inspector title, wordmark.
    pub const TITLE: TextRole = role(
        font::UI,
        text::MD,
        leading::UI,
        weight::STRONG,
        tracking::TIGHT,
    );
    /// 13/16 600: card titles.
    pub const CARD_TITLE: TextRole = role(font::UI, text::MD, leading::UI, weight::STRONG, 0.);
    /// 12/16 600: inspector sections.
    pub const SECTION_TITLE: TextRole = role(font::UI, text::SM, leading::UI, weight::STRONG, 0.);
    /// 12/16 400: rows, tabs, labels. Active tabs and emphasis use
    /// [`weight::MEDIUM`].
    pub const BODY: TextRole = role(font::UI, text::SM, leading::UI, weight::REGULAR, 0.);
    /// 12/16 600: neutral primary button. Agent primary uses
    /// [`weight::BOLD`], outline buttons [`weight::MEDIUM`].
    pub const BUTTON: TextRole = role(font::UI, text::SM, leading::UI, weight::STRONG, 0.);
    /// 11/14 400: subtitles, status bar, small buttons.
    pub const CAPTION: TextRole = role(font::UI, text::XS, leading::CAPTION, weight::REGULAR, 0.);
    /// 11/14 600 +0.06em uppercase: "ACCOUNTS", table headers.
    pub const SECTION_LABEL: TextRole = TextRole {
        uppercase: true,
        ..role(
            font::UI,
            text::XS,
            leading::CAPTION,
            weight::STRONG,
            tracking::CAPS,
        )
    };
    /// 11/14 mono 400: dates, counts, balances in lists.
    pub const MONO_VALUE: TextRole =
        role(font::MONO, text::XS, leading::CAPTION, weight::REGULAR, 0.);
    /// 12/16 mono 400: amounts in tables.
    pub const MONO_AMOUNT: TextRole = role(font::MONO, text::SM, leading::UI, weight::REGULAR, 0.);
    /// 10/12 mono 400: kbd glyphs, chart axis labels. Selection labels use
    /// [`weight::STRONG`].
    pub const MONO_MICRO: TextRole =
        role(font::MONO, text::MICRO, leading::MICRO, weight::REGULAR, 0.);
    /// 10/12 700: count badges.
    pub const BADGE: TextRole = role(font::UI, text::MICRO, leading::MICRO, weight::BOLD, 0.);
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeMap;

    use super::*;

    const TOKENS_JSON: &str = include_str!("../../design/tokens.json");
    const TOKENS_CSS: &str = include_str!("../../design/tokens.css");

    fn export() -> serde_json::Value {
        serde_json::from_str(TOKENS_JSON).expect("design/tokens.json is valid JSON")
    }

    fn hex(color: Rgba) -> String {
        let byte = |channel: f32| (channel * 255.).round() as u8;
        let rgb = format!(
            "#{:02X}{:02X}{:02X}",
            byte(color.r),
            byte(color.g),
            byte(color.b)
        );
        if color.a < 1. {
            format!("{rgb}{:02X}", byte(color.a))
        } else {
            rgb
        }
    }

    fn pixels(value: Pixels) -> String {
        format!("{}px", f32::from(value))
    }

    fn opacity_percent(value: f32) -> String {
        format!("{}%", (value * 100.).round())
    }

    /// Every exported token, formatted the way the export writes it.
    pub(crate) fn code_tokens() -> BTreeMap<&'static str, String> {
        BTreeMap::from([
            // Colour.
            ("--color-chrome", hex(color::CHROME)),
            ("--color-float", hex(color::FLOAT)),
            ("--color-canvas", hex(color::CANVAS)),
            ("--color-raised", hex(color::RAISED)),
            ("--color-field", hex(color::FIELD)),
            ("--color-hover", hex(color::HOVER)),
            ("--color-control-active", hex(color::CONTROL_ACTIVE)),
            ("--color-hairline", hex(color::HAIRLINE)),
            ("--color-border", hex(color::BORDER)),
            ("--color-border-strong", hex(color::BORDER_STRONG)),
            ("--color-tooltip-border", hex(color::TOOLTIP_BORDER)),
            ("--color-avatar", hex(color::AVATAR)),
            ("--color-white", hex(color::WHITE)),
            ("--color-text", hex(color::TEXT)),
            ("--color-text-strong", hex(color::TEXT_STRONG)),
            ("--color-text-secondary", hex(color::TEXT_SECONDARY)),
            ("--color-text-muted", hex(color::TEXT_MUTED)),
            ("--color-icon-faint", hex(color::ICON_FAINT)),
            ("--color-text-faint", hex(color::TEXT_FAINT)),
            ("--color-icon-disabled", hex(color::ICON_DISABLED)),
            ("--color-on-light", hex(color::ON_LIGHT)),
            ("--color-molten", hex(color::MOLTEN)),
            ("--color-molten-soft", hex(color::MOLTEN_SOFT)),
            ("--color-moldavite", hex(color::MOLDAVITE)),
            ("--color-moldavite-hover", hex(color::MOLDAVITE_HOVER)),
            ("--color-moldavite-pressed", hex(color::MOLDAVITE_PRESSED)),
            ("--color-moldavite-soft", hex(color::MOLDAVITE_SOFT)),
            ("--color-on-moldavite", hex(color::ON_MOLDAVITE)),
            ("--color-moldavite-text", hex(color::MOLDAVITE_TEXT)),
            (
                "--color-moldavite-text-muted",
                hex(color::MOLDAVITE_TEXT_MUTED),
            ),
            (
                "--color-moldavite-text-faint",
                hex(color::MOLDAVITE_TEXT_FAINT),
            ),
            ("--color-moldavite-surface", hex(color::MOLDAVITE_SURFACE)),
            (
                "--color-moldavite-surface-raised",
                hex(color::MOLDAVITE_SURFACE_RAISED),
            ),
            ("--color-moldavite-well", hex(color::MOLDAVITE_WELL)),
            ("--color-moldavite-border", hex(color::MOLDAVITE_BORDER)),
            (
                "--color-moldavite-border-strong",
                hex(color::MOLDAVITE_BORDER_STRONG),
            ),
            ("--color-moldavite-track", hex(color::MOLDAVITE_TRACK)),
            ("--color-danger", hex(color::DANGER)),
            ("--color-danger-soft", hex(color::DANGER_SOFT)),
            ("--color-danger-hover", hex(color::DANGER_HOVER)),
            ("--color-danger-pressed", hex(color::DANGER_PRESSED)),
            ("--color-on-danger", hex(color::ON_DANGER)),
            ("--color-warning", hex(color::WARNING)),
            ("--color-warning-soft", hex(color::WARNING_SOFT)),
            ("--color-on-warning", hex(color::ON_WARNING)),
            ("--color-chart-1", hex(color::CHART_1)),
            ("--color-chart-2", hex(color::CHART_2)),
            ("--color-chart-3", hex(color::CHART_3)),
            ("--color-chart-4", hex(color::CHART_4)),
            ("--color-chart-5", hex(color::CHART_5)),
            ("--color-chart-6", hex(color::CHART_6)),
            // Typography.
            ("--font-ui", format!("\"{}\"", font::UI)),
            ("--font-mono", format!("\"{}\"", font::MONO)),
            ("--text-2xs", pixels(text::MICRO)),
            ("--text-xs", pixels(text::XS)),
            ("--text-sm", pixels(text::SM)),
            ("--text-md", pixels(text::MD)),
            ("--text-lg", pixels(text::LG)),
            ("--text-xl", pixels(text::FIGURE)),
            ("--text-2xl", pixels(text::DISPLAY)),
            ("--font-weight-regular", weight::REGULAR.0.to_string()),
            ("--font-weight-medium", weight::MEDIUM.0.to_string()),
            ("--font-weight-strong", weight::STRONG.0.to_string()),
            ("--font-weight-bold", weight::BOLD.0.to_string()),
            ("--tracking-display", format!("{}em", tracking::DISPLAY)),
            ("--tracking-tight", format!("{}em", tracking::TIGHT)),
            ("--tracking-caps", format!("{}em", tracking::CAPS)),
            ("--leading-micro", pixels(leading::MICRO)),
            ("--leading-caption", pixels(leading::CAPTION)),
            ("--leading-ui", pixels(leading::UI)),
            ("--leading-reading", pixels(leading::READING)),
            ("--leading-figure", pixels(leading::FIGURE)),
            ("--leading-display", pixels(leading::DISPLAY)),
            // Opacity.
            ("--opacity-area-fill", opacity_percent(opacity::AREA_FILL)),
            ("--opacity-disabled", opacity_percent(opacity::DISABLED)),
            ("--opacity-muted", opacity_percent(opacity::MUTED)),
            // Layout widths.
            ("--breakpoint-desktop", pixels(size::BREAKPOINT_DESKTOP)),
            ("--container-panel", pixels(size::PANEL)),
            ("--container-inspector", pixels(size::INSPECTOR)),
            ("--container-field-label", pixels(size::FIELD_LABEL)),
            ("--container-column-amount", pixels(size::COLUMN_AMOUNT)),
            ("--container-column-account", pixels(size::COLUMN_ACCOUNT)),
            ("--container-column-category", pixels(size::COLUMN_CATEGORY)),
            ("--container-command-bar", pixels(size::COMMAND_BAR)),
            ("--container-search-field", pixels(size::SEARCH_FIELD)),
            ("--container-dialog", pixels(size::DIALOG)),
            // Spacing.
            ("--spacing-0_5", pixels(spacing::S0_5)),
            ("--spacing-1", pixels(spacing::S1)),
            ("--spacing-1_5", pixels(spacing::S1_5)),
            ("--spacing-2", pixels(spacing::S2)),
            ("--spacing-2_5", pixels(spacing::S2_5)),
            ("--spacing-3", pixels(spacing::S3)),
            ("--spacing-3_5", pixels(spacing::S3_5)),
            ("--spacing-4", pixels(spacing::S4)),
            ("--spacing-5", pixels(spacing::S5)),
            ("--spacing-6", pixels(spacing::S6)),
            // Sizes stored as spacing tokens.
            ("--spacing-progress", pixels(size::PROGRESS)),
            ("--spacing-dot", pixels(size::DOT)),
            ("--spacing-handle", pixels(size::HANDLE)),
            ("--spacing-marker", pixels(size::MARKER)),
            ("--spacing-icon-xs", pixels(size::ICON_XS)),
            ("--spacing-icon-sm", pixels(size::ICON_SM)),
            ("--spacing-toggle-knob", pixels(size::TOGGLE_KNOB)),
            ("--spacing-icon-md", pixels(size::ICON_MD)),
            ("--spacing-icon-lg", pixels(size::ICON_LG)),
            ("--spacing-control-xs", pixels(size::CONTROL_XS)),
            ("--spacing-kbd", pixels(size::KBD)),
            ("--spacing-control-sm", pixels(size::CONTROL_SM)),
            ("--spacing-control-compact", pixels(size::CONTROL_COMPACT)),
            ("--spacing-control", pixels(size::CONTROL)),
            ("--spacing-status-bar", pixels(size::STATUS_BAR)),
            ("--spacing-toggle-width", pixels(size::TOGGLE_WIDTH)),
            ("--spacing-control-lg", pixels(size::CONTROL_LG)),
            ("--spacing-control-xl", pixels(size::CONTROL_XL)),
            ("--spacing-title-bar", pixels(size::TITLE_BAR)),
            // Radius.
            ("--radius-marker", pixels(radius::MARKER)),
            ("--radius-label", pixels(radius::LABEL)),
            ("--radius-sm", pixels(radius::SM)),
            ("--radius-md", pixels(radius::MD)),
            ("--radius-button", pixels(radius::BUTTON)),
            ("--radius-surface", pixels(radius::SURFACE)),
            ("--radius-lg", pixels(radius::LG)),
            ("--radius-card", pixels(radius::CARD)),
            ("--radius-pill", pixels(radius::PILL)),
        ])
    }

    /// Exported values as written, aliases (`var(--other)`) included.
    fn json_tokens() -> BTreeMap<String, String> {
        export()["tokens"]
            .as_array()
            .expect("design/tokens.json has a tokens array")
            .iter()
            .map(|token| {
                let field = |key: &str| {
                    token[key]
                        .as_str()
                        .unwrap_or_else(|| panic!("token {token} has no string {key:?}"))
                        .to_owned()
                };
                (field("name"), field("value"))
            })
            .collect()
    }

    /// Follows `var(--other)` aliases to a literal value.
    fn resolve<'a>(tokens: &'a BTreeMap<String, String>, name: &str) -> &'a str {
        let mut value = tokens[name].as_str();
        for _ in 0..tokens.len() {
            match value.strip_prefix("var(").and_then(|v| v.strip_suffix(')')) {
                Some(target) => {
                    value = tokens
                        .get(target)
                        .unwrap_or_else(|| panic!("{name} aliases unknown token {target}"))
                }
                None => return value,
            }
        }
        panic!("{name} is part of an alias cycle")
    }

    fn css_tokens() -> BTreeMap<String, String> {
        TOKENS_CSS
            .lines()
            .filter_map(|line| line.trim().strip_suffix(';')?.split_once(':'))
            .map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned()))
            .collect()
    }

    #[test]
    fn code_matches_token_export() {
        let exported = json_tokens();
        let code = code_tokens();

        for name in exported.keys() {
            let code_value = code
                .get(name.as_str())
                .unwrap_or_else(|| panic!("Exported token {name} has no constant in tokens.rs"));
            assert_eq!(
                code_value,
                resolve(&exported, name),
                "{name} drifted from design/tokens.json"
            );
        }
        for name in code.keys() {
            assert!(
                exported.contains_key(*name),
                "{name} is mapped in tokens.rs but missing from design/tokens.json"
            );
        }
    }

    #[test]
    fn export_comes_from_the_coffer_file() {
        let source = &export()["source"];
        assert_eq!(source["fileName"], "Coffer");
        assert_eq!(source["fileId"], "01M6YH440YAX6H5H322ZX4JYC9");
    }

    #[test]
    fn css_and_json_exports_agree() {
        assert_eq!(css_tokens(), json_tokens());
    }

    #[test]
    fn colors_round_trip_through_hex() {
        assert_eq!(hex(color::MOLTEN_SOFT), "#FF6B1A24");
        assert_eq!(hex(color::CHROME), "#0F0F10");
        assert_eq!(color::ON_LIGHT, color::CHROME);
    }

    #[test]
    fn exit_motion_is_two_thirds_of_enter() {
        assert_eq!(motion::exit(motion::BASE), Duration::from_millis(100));
        assert_eq!(motion::exit(motion::SLOW), Duration::from_micros(146_666));
    }

    #[test]
    fn shadows_convert_the_css_blur_to_gpui_sigma() {
        // `0 6px 18px`: CSS σ is 9px, which GPUI takes as `blur_radius`.
        let tooltip = shadow::TOOLTIP.box_shadow();
        assert_eq!(tooltip.blur_radius, px(9.));
        assert_eq!(tooltip.offset, point(px(0.), px(6.)));
        assert!(!tooltip.inset);

        // `inset 0 0 0 1px`: no blur, and spread passes through unchanged.
        let ring = shadow::SWATCH_RING.box_shadow();
        assert_eq!(ring.blur_radius, px(0.));
        assert_eq!(ring.spread_radius, px(1.));
        assert!(ring.inset);
    }
}
