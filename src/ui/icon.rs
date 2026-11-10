//! Coffer's icons (`docs/DESIGN.md` → "Iconography").
//!
//! The paths are copied from the Paper Components artboard, so they keep its
//! outline style: 1.2–1.3px strokes with round caps and joins, drawn at the
//! size of their `viewBox`. Only the agent's sparkle is filled. GPUI draws an
//! SVG as a mask in the element's text colour, so the colour inside the
//! markup is irrelevant (`currentColor`) and the caller sets it with
//! `text_color`.
//!
//! The icons are `svg()` elements rather than stock `Icon`s because a stock
//! icon fixes its colour when it renders, so a parent's hover style can't
//! change it. An `svg()` takes `group_hover`, which is how hovered rows and
//! icon buttons brighten their icon.

use gpui_kit::{Pixels, Styled as _, Svg, svg};

use crate::theme::tokens::size;

/// One of Coffer's icons.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Glyph {
    /// A bank account (card outline). 12px.
    Account,
    /// A savings pot under its parent account. 12px.
    Pot,
    /// Add. 12px.
    Plus,
    /// A transfer between own accounts, including pot moves (⇄). 12px.
    Transfer,
    /// The AI agent, and nothing else. Filled. 12px.
    Sparkle,
    /// Approve. 12px.
    Check,
    /// Reject or dismiss. 10px.
    Close,
    /// Sort direction and select chevron. 8px.
    ChevronDown,
    /// Show or hide a side panel. 16px.
    Panel,
    /// Search fields and the command bar. 12px.
    Search,
    /// Minimise the window. 10px.
    Minimize,
    /// Maximise or restore the window. 10px.
    Maximize,
}

impl Glyph {
    /// The size the icon is drawn at: its `viewBox`, so strokes stay at the
    /// width Paper draws them.
    pub fn size(self) -> Pixels {
        match self {
            Self::Account
            | Self::Pot
            | Self::Plus
            | Self::Transfer
            | Self::Sparkle
            | Self::Check
            | Self::Search => size::ICON_SM,
            Self::Close | Self::Minimize | Self::Maximize => size::ICON_XS,
            Self::ChevronDown => size::MARKER,
            Self::Panel => size::ICON_LG,
        }
    }

    /// The icon as SVG markup.
    pub fn markup(self) -> &'static str {
        match self {
            Self::Account => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<rect x="1.5" y="2.75" width="9" height="6.5" rx="1.25"/><path d="M1.5 5.25H10.5"/></svg>"#
            ),
            Self::Pot => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M2.75 4.5H9.25V8.75A2 2 0 0 1 7.25 10.75H4.75A2 2 0 0 1 2.75 8.75Z"/><path d="M4 2.25H8"/></svg>"#
            ),
            Self::Plus => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M6 2.5V9.5M2.5 6H9.5"/></svg>"#
            ),
            Self::Transfer => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M2 4.25H9.5M7.75 2.5L9.5 4.25L7.75 6M10 7.75H2.5M4.25 6L2.5 7.75L4.25 9.5"/></svg>"#
            ),
            Self::Sparkle => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12">"#,
                r#"<path d="M6 0.5L7.3 4.7L11.5 6L7.3 7.3L6 11.5L4.7 7.3L0.5 6L4.7 4.7Z" fill="currentColor"/></svg>"#
            ),
            Self::Check => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M2.5 6.25L5 8.75L9.5 3.75"/></svg>"#
            ),
            Self::Close => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M2.5 2.5L7.5 7.5M7.5 2.5L2.5 7.5"/></svg>"#
            ),
            Self::ChevronDown => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M1.5 2.75L4 5.25L6.5 2.75"/></svg>"#
            ),
            Self::Panel => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<rect x="2.25" y="3" width="11.5" height="10" rx="2"/><path d="M6.25 3V13"/></svg>"#
            ),
            Self::Search => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<circle cx="5.25" cy="5.25" r="3.75"/><path d="M8 8L10.5 10.5"/></svg>"#
            ),
            Self::Minimize => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M2 5H8"/></svg>"#
            ),
            Self::Maximize => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<rect x="2" y="2" width="6" height="6" rx="1"/></svg>"#
            ),
        }
    }

    /// The icon as an element at its own size, in the inherited text
    /// colour until the caller sets one.
    pub fn render(self) -> Svg {
        self.render_at(self.size())
    }

    /// The icon scaled to `side`, strokes included. Only where a spec draws
    /// an icon off its own size, such as the 10px sparkle before an
    /// AI-suggested category.
    pub fn render_at(self, side: Pixels) -> Svg {
        svg()
            .data(self.markup().as_bytes())
            .flex_none()
            .w(side)
            .h(side)
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::px;

    use super::*;

    const ALL: [Glyph; 12] = [
        Glyph::Account,
        Glyph::Pot,
        Glyph::Plus,
        Glyph::Transfer,
        Glyph::Sparkle,
        Glyph::Check,
        Glyph::Close,
        Glyph::ChevronDown,
        Glyph::Panel,
        Glyph::Search,
        Glyph::Minimize,
        Glyph::Maximize,
    ];

    /// The `viewBox` side in the markup, which `size` must match so strokes
    /// render at their designed width.
    fn view_box_side(markup: &str) -> f32 {
        let start = markup.find("viewBox=\"").expect("has a viewBox") + "viewBox=\"".len();
        let value = &markup[start..start + markup[start..].find('"').expect("closed")];
        let parts: Vec<f32> = value
            .split(' ')
            .map(|n| n.parse().expect("number"))
            .collect();
        assert_eq!(parts[..2], [0., 0.], "viewBox starts at the origin");
        assert_eq!(parts[2], parts[3], "icons are square");
        parts[2]
    }

    #[test]
    fn every_glyph_is_drawn_at_its_view_box() {
        for glyph in ALL {
            let markup = glyph.markup();
            assert_eq!(px(view_box_side(markup)), glyph.size(), "{glyph:?}");
        }
    }

    #[test]
    fn markup_carries_no_colour() {
        // The element's text colour paints the icon; a literal colour in the
        // markup would be a colour outside the theme.
        for glyph in ALL {
            let markup = glyph.markup();
            assert!(!markup.contains('#'), "{glyph:?}");
            for attribute in ["stroke=\"", "fill=\""] {
                for value in markup.split(attribute).skip(1) {
                    let value = &value[..value.find('"').expect("closed")];
                    assert!(
                        value == "currentColor" || value == "none",
                        "{glyph:?}: {attribute}{value}"
                    );
                }
            }
        }
    }

    #[test]
    fn only_the_sparkle_is_filled() {
        for glyph in ALL {
            let filled = glyph.markup().contains("fill=\"currentColor\"");
            assert_eq!(filled, glyph == Glyph::Sparkle, "{glyph:?}");
        }
    }
}
