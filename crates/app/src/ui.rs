//! Coffer's own controls: stock `gpui-component` elements wrapped, or built
//! from `div`, with every visible value taken from [`crate::theme::tokens`].
//!
//! [`crate::theme`] holds the values (tokens, the stock theme map, the focus
//! ring); this module holds the elements that use them. `docs/DESIGN.md` →
//! "Implementing in GPUI" → "Stock components" lists which components are
//! wrapped here and which are custom. Lives in the root crate until the M1
//! workspace split, then moves to `crates/app` with the theme.

pub mod agent_card;
pub mod agent_suggestion;
pub mod badge;
pub mod bulk_action_bar;
pub mod button;
pub mod dialog;
pub mod empty_state;
pub mod field;
pub mod icon;
pub mod inspector;
pub mod kbd;
pub mod list_row;
pub mod marker;
pub mod menu;
pub mod section_label;
pub mod status_bar;
pub mod text;
pub mod title_bar;
pub mod tracked;
pub mod transaction_row;
pub mod undo_bar;

use gpui_kit::{Div, Pixels, Rgba, Styled as _, div};

use crate::theme::tokens::{opacity, size};

/// An interaction state drawn without the input that causes it.
///
/// For the showcase's state matrices (`coffer --showcase`), which put every
/// state of a control side by side to compare with the Paper Components
/// artboard. Product UI never sets one: there the pointer and keyboard decide.
/// A previewed control still reacts to real input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewState {
    /// As if the pointer were over it.
    Hovered,
    /// As if it were being pressed.
    Pressed,
    /// As if it had keyboard focus: the focus ring shows.
    FocusVisible,
}

/// A vertical 1px rule `height` tall in `line`: between view tabs
/// (`hairline`, 14px), status bar items (`border-strong`, 12px) and the
/// bulk-action bar's groups (`border-strong`, 16px).
///
/// A filled box, not a border: GPUI paints nothing for a div whose only
/// extent is its own border.
pub fn separator(height: Pixels, line: Rgba) -> Div {
    div().flex_none().w(size::HAIRLINE).h(height).bg(line)
}

/// `color` as it shows on a disabled control, faded to [`opacity::DISABLED`]
/// as one group over the opaque `surface` it sits on: what CSS, and so
/// Paper, paints for a disabled control.
///
/// GPUI's `opacity` isn't group opacity: it multiplies into each thing the
/// element draws, so a label drawn over the control's own fill blends with
/// the already-faded fill and loses contrast (`DESIGN.md` → "Rendering
/// findings (D2.6)"). A disabled control with content on a fill paints every
/// colour with this instead and draws opaque. The blend works on
/// sRGB-encoded values, as GPUI's renderer and a browser blend, so the
/// result is the pixel Paper shows.
pub fn faded(color: Rgba, surface: Rgba) -> Rgba {
    debug_assert!(
        surface.a >= 1.,
        "a disabled control fades over an opaque surface"
    );
    let alpha = color.a * opacity::DISABLED;
    let mix = |channel: f32, under: f32| channel * alpha + under * (1. - alpha);
    Rgba {
        r: mix(color.r, surface.r),
        g: mix(color.g, surface.g),
        b: mix(color.b, surface.b),
        a: 1.,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::tokens::color;

    /// `color` as an 8-bit `#RRGGBB`, as a screen shows it.
    fn hex(color: Rgba) -> String {
        let byte = |channel: f32| (channel * 255.).round() as u8;
        format!(
            "#{:02X}{:02X}{:02X}",
            byte(color.r),
            byte(color.g),
            byte(color.b)
        )
    }

    #[test]
    fn faded_colours_are_the_css_group_composite() {
        // Paper's disabled neutral primary on `canvas` (button matrix `D4-0`,
        // colour-managed from its Display P3 export): `#6D6D70` fill,
        // `#141416` label. The fill's blue differs by one from P3 rounding.
        assert_eq!(hex(faded(color::TEXT, color::CANVAS)), "#6D6D6F");
        assert_eq!(hex(faded(color::ON_LIGHT, color::CANVAS)), "#141416");
        // A translucent colour fades by its own alpha times the group's:
        // `moldavite-soft` (12% moldavite) at 40% is 4.9% moldavite.
        assert_eq!(hex(faded(color::MOLDAVITE_SOFT, color::CANVAS)), "#1D201F");
        // Fading the surface over itself changes nothing.
        assert_eq!(hex(faded(color::CANVAS, color::CANVAS)), "#18181A");
    }
}
