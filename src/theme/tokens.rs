//! Design tokens from `docs/DESIGN.md` as typed GPUI values.
//!
//! This is the only place in the app where colour, size, and duration literals
//! may appear. Everything else references these constants by name.
//!
//! Two kinds of values live here:
//!
//! - **Exported tokens**: the tokens in `design/tokens.json` /
//!   `design/tokens.css`. Their names mirror the CSS custom properties
//!   (`--color-text-muted` → [`color::TEXT_MUTED`], `--spacing-2` →
//!   [`spacing::S2`]). A test checks them against
//!   `design/tokens.json`, so a re-export that changes a value fails the build.
//! - **Proposed tokens**: values `DESIGN.md` marks as *Proposed*. They are
//!   decided and may be used, but are not in the token export yet. Each one
//!   says so in its docs. When one is added to the export, add it to the drift
//!   test.
//!
//! The font-weight tokens are the one deliberate disagreement with the export:
//! it still has 450/550/650, and `DESIGN.md` decides 400/500/600/700. The drift
//! test pins the exported values, so it fails (and asks for this override to
//! be removed) once the export is updated.

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

    // Exported tokens.

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

    // Proposed: neutrals typed into the artboards.

    /// Proposed. Text one step below primary: list labels, chips, kbd glyphs,
    /// tooltip body, secondary icons, spending bars.
    pub const TEXT_SECONDARY: Rgba = opaque(0xB4B4B9);
    /// Proposed. Text on outline buttons, empty-state titles, emphasized
    /// values; pressed neutral primary button.
    pub const TEXT_STRONG: Rgba = opaque(0xD6D6DA);
    /// Proposed. Chevrons, sort indicators, income bars in charts.
    pub const ICON_FAINT: Rgba = opaque(0x6E6E73);
    /// Proposed. Disabled icons.
    pub const ICON_DISABLED: Rgba = opaque(0x4A4A4F);
    /// Proposed. Floating surfaces (toolbar, cards, popovers), status bar.
    pub const FLOAT: Rgba = opaque(0x141416);
    /// Proposed. Active segment, active icon button, toggle track (off),
    /// pressed state.
    pub const CONTROL_ACTIVE: Rgba = opaque(0x2C2C30);
    /// Proposed. Borders on floating surfaces and chips; neutral progress track.
    pub const BORDER: Rgba = opaque(0x2A2A2E);
    /// Proposed. Outline-button borders, dashed empty states, inline
    /// separators, chart baseline.
    pub const BORDER_STRONG: Rgba = opaque(0x2E2E33);
    /// Proposed. Tooltip border, kbd inside tooltips.
    pub const TOOLTIP_BORDER: Rgba = opaque(0x333338);
    /// Proposed. Neutral avatar fill.
    pub const AVATAR: Rgba = opaque(0x3A3A3F);
    /// Proposed. Text on the neutral primary button.
    pub const ON_LIGHT: Rgba = CHROME;
    /// White: selected-row labels, selection handles, text on molten
    /// measurement labels, hovered neutral primary button.
    pub const WHITE: Rgba = opaque(0xFFFFFF);

    // Proposed: agent (moldavite) family.

    /// Proposed. Text on solid moldavite buttons.
    pub const ON_MOLDAVITE: Rgba = opaque(0x0E1710);
    /// Proposed. Hovered agent primary button (text contrast 10.2:1).
    pub const MOLDAVITE_HOVER: Rgba = opaque(0x8FD09C);
    /// Proposed. Pressed agent primary button (text contrast 7.2:1).
    pub const MOLDAVITE_PRESSED: Rgba = opaque(0x6DB27B);
    /// Proposed. Agent text on dark ("Approve all", agent labels,
    /// AI-suggested categories).
    pub const MOLDAVITE_TEXT: Rgba = opaque(0xA9D9B2);
    /// Proposed. Secondary text inside agent surfaces.
    pub const MOLDAVITE_TEXT_MUTED: Rgba = opaque(0x7E8A81);
    /// Proposed. Timestamps inside agent surfaces. Non-essential text only
    /// (3.2:1).
    pub const MOLDAVITE_TEXT_FAINT: Rgba = opaque(0x5C6B60);
    /// Proposed. Agent card fill.
    pub const MOLDAVITE_SURFACE: Rgba = opaque(0x111813);
    /// Proposed. Agent ghost button, hover on agent card rows.
    pub const MOLDAVITE_SURFACE_RAISED: Rgba = opaque(0x162019);
    /// Proposed. Icon wells and agent avatar fill.
    pub const MOLDAVITE_WELL: Rgba = opaque(0x1B241D);
    /// Proposed. Agent card border.
    pub const MOLDAVITE_BORDER: Rgba = opaque(0x24352A);
    /// Proposed. Agent outline button border.
    pub const MOLDAVITE_BORDER_STRONG: Rgba = opaque(0x2E4535);
    /// Proposed. Agent progress-bar track.
    pub const MOLDAVITE_TRACK: Rgba = opaque(0x22332A);

    // Proposed: status colours.

    /// Proposed. Danger text, icons, 6px dots, chart overage, destructive
    /// outline buttons (5.2:1 on canvas).
    pub const DANGER: Rgba = opaque(0xE5598E);
    /// Proposed. Danger banner / row background.
    pub const DANGER_SOFT: Rgba = translucent(0xE5598E1F);
    /// Proposed. Text on a solid danger button (5.6:1 on danger).
    pub const ON_DANGER: Rgba = opaque(0x1A0B11);
    /// Proposed. Warning text, icons, dots, budget bars at 85% or more
    /// (10.3:1 on canvas).
    pub const WARNING: Rgba = opaque(0xE7C24A);
    /// Proposed. Warning banner background.
    pub const WARNING_SOFT: Rgba = translucent(0xE7C24A1F);
    /// Proposed. Text on solid warning (rare; 10.7:1 on warning).
    pub const ON_WARNING: Rgba = opaque(0x1A1405);

    // Proposed: categorical chart palette.

    /// Proposed. Chart series 1 (13.2:1 on canvas).
    pub const CHART_1: Rgba = opaque(0xC1E3FC);
    /// Proposed. Chart series 2 (9.3:1 on canvas).
    pub const CHART_2: Rgba = opaque(0xFBA0CC);
    /// Proposed. Chart series 3 (7.1:1 on canvas).
    pub const CHART_3: Rgba = opaque(0xB396E8);
    /// Proposed. Chart series 4 (5.7:1 on canvas).
    pub const CHART_4: Rgba = opaque(0x1AA0B9);
    /// Proposed. Chart series 5 (4.1:1 on canvas).
    pub const CHART_5: Rgba = opaque(0x5777C1);
    /// Proposed. Chart series 6 (3.2:1 on canvas).
    pub const CHART_6: Rgba = opaque(0x92508C);
    /// The six categorical series in rank order. "Other" is not a seventh
    /// colour: draw it as a 1px [`TEXT_MUTED`] outline on [`FIELD`].
    pub const CHART: [Rgba; 6] = [CHART_1, CHART_2, CHART_3, CHART_4, CHART_5, CHART_6];
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

    /// Mono micro and badges: kbd glyphs, chart axis labels, selection labels.
    pub const MICRO: Pixels = px(10.);
    /// `--text-xs`: captions, section labels, mono values.
    pub const XS: Pixels = px(11.);
    /// `--text-sm`: body, buttons, mono amounts.
    pub const SM: Pixels = px(12.);
    /// `--text-md`: titles, reading body.
    pub const MD: Pixels = px(13.);
    /// `--text-lg`: reading headings.
    pub const LG: Pixels = px(15.);
    /// Overview "Income" and "Net" figures.
    pub const FIGURE: Pixels = px(20.);
    /// Display headings and the display figure.
    pub const DISPLAY: Pixels = px(32.);
}

/// Line heights. They snap to a 2px grid.
pub mod leading {
    use super::{Pixels, px};

    /// Paired with [`super::text::MICRO`].
    pub const MICRO: Pixels = px(12.);
    /// Paired with [`super::text::XS`].
    pub const CAPTION: Pixels = px(14.);
    /// `--leading-ui`: paired with [`super::text::SM`] and [`super::text::MD`].
    pub const UI: Pixels = px(16.);
    /// Reading type: 13/20 body and 15/20 headings.
    pub const READING: Pixels = px(20.);
    /// Paired with [`super::text::FIGURE`].
    pub const FIGURE: Pixels = px(24.);
    /// Paired with [`super::text::DISPLAY`].
    pub const DISPLAY: Pixels = px(38.);
}

/// Font weights.
///
/// Proposed values (400/500/600/700) that override the exported 450/550/650 tokens;
/// all four exist as static Inter instances. Artboard specs quoting 550 mean
/// [`MEDIUM`](weight::MEDIUM) and 650 mean [`STRONG`](weight::STRONG).
pub mod weight {
    use super::FontWeight;

    /// `--font-weight-regular` (exported: 450).
    pub const REGULAR: FontWeight = FontWeight(400.);
    /// `--font-weight-medium` (exported: 550).
    pub const MEDIUM: FontWeight = FontWeight(500.);
    /// `--font-weight-strong` (exported: 650).
    pub const STRONG: FontWeight = FontWeight(600.);
    /// Proposed `--font-weight-bold`: badges, agent primary button.
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
    /// Display headings and the display figure.
    pub const DISPLAY: f32 = -0.025;
}

/// Spacing on the 4px grid. `Sn` is `n × 4px`, so `S1`–`S4` are the exported
/// `--spacing-1`–`--spacing-4`; the half steps are the observed tight gaps.
pub mod spacing {
    use super::{Pixels, px};

    /// Separator and toolbar gaps.
    pub const S0_5: Pixels = px(2.);
    /// `--spacing-1`.
    pub const S1: Pixels = px(4.);
    /// Tight icon + label pairs; list-section inline padding.
    pub const S1_5: Pixels = px(6.);
    /// `--spacing-2`.
    pub const S2: Pixels = px(8.);
    /// Tight icon + label pairs; list-section bottom padding.
    pub const S2_5: Pixels = px(10.);
    /// `--spacing-3`.
    pub const S3: Pixels = px(12.);
    /// Inspector section padding.
    pub const S3_5: Pixels = px(14.);
    /// `--spacing-4`.
    pub const S4: Pixels = px(16.);
    /// Nesting indent for pots under their parent account.
    pub const S5: Pixels = px(20.);
    /// Floating bulk-action bar offset above the bottom edge.
    pub const S6: Pixels = px(24.);
}

/// Corner radii. [`SM`](radius::SM), [`MD`](radius::MD), and [`LG`](radius::LG) are exported tokens; the rest are the
/// radii the component specs use.
pub mod radius {
    use super::{Pixels, px};

    /// Colour swatches and markers, chart bars, progress bars.
    pub const MARKER: Pixels = px(2.);
    /// Molten measurement labels.
    pub const LABEL: Pixels = px(3.);
    /// `--radius-sm`: kbd, inner segments, tab close button, skeletons.
    pub const SM: Pixels = px(4.);
    /// `--radius-md`: rows, fields, tabs, icon buttons, outline buttons.
    pub const MD: Pixels = px(6.);
    /// Primary buttons and the command bar.
    pub const BUTTON: Pixels = px(7.);
    /// Segmented track, tooltip, cards in panels, empty state, badges,
    /// toggle track.
    pub const SURFACE: Pixels = px(8.);
    /// `--radius-lg`: floating toolbar, agent card.
    pub const LG: Pixels = px(10.);
    /// Cards.
    pub const CARD: Pixels = px(12.);
    /// Chips (28px pill).
    pub const PILL: Pixels = px(14.);
}

/// Fixed sizes: control heights, the app frame, and small component geometry.
pub mod size {
    use super::{Pixels, px};

    // Control heights.

    /// Toggle, badges, kbd inside tooltips.
    pub const CONTROL_XS: Pixels = px(16.);
    /// Kbd; tab close button.
    pub const KBD: Pixels = px(18.);
    /// Small (outline) buttons.
    pub const CONTROL_SM: Pixels = px(24.);
    /// Compact buttons, avatar.
    pub const CONTROL_COMPACT: Pixels = px(26.);
    /// The default: rows, tabs, fields, icon buttons, chips.
    pub const CONTROL: Pixels = px(28.);
    /// Primary buttons, segmented control.
    pub const CONTROL_LG: Pixels = px(30.);
    /// Floating toolbar, agent ghost button.
    pub const CONTROL_XL: Pixels = px(36.);

    // App frame.

    /// Title bar height.
    pub const TITLE_BAR: Pixels = px(40.);
    /// `--container-panel`: left panel width.
    pub const PANEL: Pixels = px(272.);
    /// Right inspector width.
    pub const INSPECTOR: Pixels = px(296.);
    /// Status bar height.
    pub const STATUS_BAR: Pixels = px(28.);
    /// Command bar width.
    pub const COMMAND_BAR: Pixels = px(340.);
    /// Inspector field-row label column.
    pub const FIELD_LABEL: Pixels = px(64.);
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
    /// Progress bars and ranked-bar-list bars.
    pub const PROGRESS: Pixels = px(3.);
    /// Status dots and chart hover points.
    pub const DOT: Pixels = px(6.);
    /// Selection handles.
    pub const HANDLE: Pixels = px(7.);
    /// Colour markers (tabs, category squares, chart legends); select chevron.
    pub const MARKER: Pixels = px(8.);
    /// Toggle knob.
    pub const TOGGLE_KNOB: Pixels = px(12.);
    /// Toggle track width (its height is [`CONTROL_XS`]).
    pub const TOGGLE_WIDTH: Pixels = px(28.);
    /// Icon sizes.
    pub const ICON_XS: Pixels = px(10.);
    /// Icon sizes.
    pub const ICON_SM: Pixels = px(12.);
    /// Icon sizes.
    pub const ICON_MD: Pixels = px(14.);
    /// Icon sizes.
    pub const ICON_LG: Pixels = px(16.);
}

/// Opacity.
pub mod opacity {
    /// `--opacity-muted`; the low end of the skeleton pulse.
    pub const MUTED: f32 = 0.6;
    /// Proposed `--opacity-disabled`.
    pub const DISABLED: f32 = 0.4;
    /// Chart area fill under a series line.
    pub const AREA_FILL: f32 = 0.08;
}

/// Proposed motion tokens. Motion is not an exported token type, so these stay in
/// `DESIGN.md`. With the OS reduced-motion setting, everything goes to zero
/// except opacity fades, which stay at [`FAST`](motion::FAST) or less.
pub mod motion {
    use super::Duration;

    /// Proposed `--motion-fast`: hover and pressed colour changes.
    pub const FAST: Duration = Duration::from_millis(90);
    /// Proposed `--motion-base`: toggles, segmented control, tooltip and
    /// popover in.
    pub const BASE: Duration = Duration::from_millis(150);
    /// Proposed `--motion-slow`: panels, inspector, sheets, the import dialog.
    pub const SLOW: Duration = Duration::from_millis(220);
    /// Proposed `--motion-chart`: chart data changes.
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
        pub blur: Pixels,
        pub spread: Pixels,
        pub color: Rgba,
        pub inset: bool,
    }

    impl Shadow {
        /// The layer as a GPUI shadow, for `Styled::shadow`.
        pub fn box_shadow(self) -> BoxShadow {
            BoxShadow {
                color: self.color.into(),
                offset: point(px(0.), self.offset_y),
                blur_radius: self.blur,
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
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    const TOKENS_JSON: &str = include_str!("../../design/tokens.json");
    const TOKENS_CSS: &str = include_str!("../../design/tokens.css");

    /// Exported tokens whose code value deliberately differs, with the value
    /// the export is expected to still have. When the export changes one of
    /// these, the test fails: adopt the exported value and remove the entry.
    const SUPERSEDED: &[(&str, &str)] = &[
        ("--font-weight-regular", "450"),
        ("--font-weight-medium", "550"),
        ("--font-weight-strong", "650"),
    ];

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

    /// Every exported token, formatted the way the export writes it.
    fn code_tokens() -> BTreeMap<&'static str, String> {
        BTreeMap::from([
            ("--color-chrome", hex(color::CHROME)),
            ("--color-canvas", hex(color::CANVAS)),
            ("--color-raised", hex(color::RAISED)),
            ("--color-field", hex(color::FIELD)),
            ("--color-hover", hex(color::HOVER)),
            ("--color-hairline", hex(color::HAIRLINE)),
            ("--color-text", hex(color::TEXT)),
            ("--color-text-muted", hex(color::TEXT_MUTED)),
            ("--color-text-faint", hex(color::TEXT_FAINT)),
            ("--color-molten", hex(color::MOLTEN)),
            ("--color-molten-soft", hex(color::MOLTEN_SOFT)),
            ("--color-moldavite", hex(color::MOLDAVITE)),
            ("--color-moldavite-soft", hex(color::MOLDAVITE_SOFT)),
            ("--font-ui", format!("\"{}\"", font::UI)),
            ("--font-mono", format!("\"{}\"", font::MONO)),
            ("--text-xs", pixels(text::XS)),
            ("--text-sm", pixels(text::SM)),
            ("--text-md", pixels(text::MD)),
            ("--text-lg", pixels(text::LG)),
            ("--tracking-tight", format!("{}em", tracking::TIGHT)),
            ("--tracking-caps", format!("{}em", tracking::CAPS)),
            ("--leading-ui", pixels(leading::UI)),
            (
                "--opacity-muted",
                format!("{}%", (opacity::MUTED * 100.).round()),
            ),
            ("--breakpoint-desktop", pixels(size::BREAKPOINT_DESKTOP)),
            ("--container-panel", pixels(size::PANEL)),
            ("--spacing-1", pixels(spacing::S1)),
            ("--spacing-2", pixels(spacing::S2)),
            ("--spacing-3", pixels(spacing::S3)),
            ("--spacing-4", pixels(spacing::S4)),
            ("--radius-sm", pixels(radius::SM)),
            ("--radius-md", pixels(radius::MD)),
            ("--radius-lg", pixels(radius::LG)),
        ])
    }

    fn json_tokens() -> BTreeMap<String, String> {
        let export: serde_json::Value =
            serde_json::from_str(TOKENS_JSON).expect("design/tokens.json is valid JSON");
        export["tokens"]
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
        let superseded = BTreeMap::from_iter(SUPERSEDED.iter().copied());

        for (name, exported_value) in &exported {
            if let Some(expected) = superseded.get(name.as_str()) {
                assert_eq!(
                    exported_value, expected,
                    "The export changed {name}, which the code overrides; adopt the exported value \
                     in tokens.rs and drop it from SUPERSEDED"
                );
            } else {
                let code_value = code.get(name.as_str()).unwrap_or_else(|| {
                    panic!("Exported token {name} has no constant in src/theme/tokens.rs")
                });
                assert_eq!(
                    code_value, exported_value,
                    "{name} drifted from design/tokens.json"
                );
            }
        }
        for name in code.keys().chain(superseded.keys()) {
            assert!(
                exported.contains_key(*name),
                "{name} is mapped in tokens.rs but missing from design/tokens.json"
            );
        }
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
}
