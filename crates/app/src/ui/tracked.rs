//! Single-line text with letter spacing (`docs/DESIGN.md` → "Implementing
//! in GPUI" → "Letter spacing").
//!
//! GPUI's text style has no letter spacing, so a role's tracking would be
//! lost on a plain text child. [`Tracked`] shapes the line as GPUI does and
//! paints each glyph itself, moved right by the tracking once per character
//! before it. As in CSS, the spacing also follows the last character, so
//! the element is `tracking × characters` wider than the shaped text.
//!
//! Use it for roles with tracking ([`TextRole::tracking`] non-zero): titles,
//! figures, uppercase labels. It doesn't wrap, truncate or select; colour
//! comes from the inherited text style, so set `text_color` on a parent.

use std::panic::Location;

use gpui_kit::{
    App, Bounds, Element, ElementId, Font, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, ShapedLine, SharedString, Style, TextRun, Window, point, px,
};

use crate::theme::tokens::typography::TextRole;
use crate::ui::text::CasedText as _;

/// One line of text in a [`TextRole`], tracking included.
pub struct Tracked {
    role: TextRole,
    text: SharedString,
}

impl Tracked {
    /// `text` in `role`, uppercased if the role is.
    pub fn new(role: TextRole, text: impl Into<SharedString>) -> Self {
        Self {
            text: role.cased(text),
            role,
        }
    }

    /// The tracking in pixels: the role's em value at its size.
    fn spacing(&self) -> Pixels {
        self.role.size * self.role.tracking
    }
}

/// The x offset tracking adds before the glyph at byte `index`.
fn offset_before(text: &str, index: usize, spacing: Pixels) -> Pixels {
    let chars = text.get(..index).map_or(0, |before| before.chars().count());
    spacing * chars as f32
}

impl IntoElement for Tracked {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Tracked {
    type RequestLayoutState = ShapedLine;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ShapedLine) {
        let inherited = window.text_style();
        let font = Font {
            family: self.role.family.into(),
            weight: self.role.weight,
            ..inherited.font()
        };
        let run = TextRun {
            len: self.text.len(),
            font,
            color: inherited.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = window
            .text_system()
            .shape_line(self.text.clone(), self.role.size, &[run], None);

        let tracking = self.spacing() * self.text.chars().count() as f32;
        let mut style = Style::default();
        style.size.width = (line.width + tracking).max(px(0.)).into();
        style.size.height = self.role.line_height.into();
        style.flex_shrink = 0.;
        (window.request_layout(style, [], cx), line)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut ShapedLine,
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        line: &mut ShapedLine,
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) {
        // The baseline as GPUI's own text puts it: the glyphs' extent
        // centred in the line height.
        let padding_top = (self.role.line_height - line.ascent - line.descent) / 2.;
        let baseline = bounds.origin.y + padding_top + line.ascent;
        let color = window.text_style().color;
        let spacing = self.spacing();
        for run in &line.runs {
            for glyph in &run.glyphs {
                let origin = point(
                    bounds.origin.x
                        + glyph.position.x
                        + offset_before(&self.text, glyph.index, spacing),
                    baseline + glyph.position.y,
                );
                // A glyph that fails to rasterise is left out, as GPUI's
                // own text does.
                let _ = if glyph.is_emoji {
                    window.paint_emoji(origin, run.font_id, glyph.id, self.role.size)
                } else {
                    window.paint_glyph(origin, run.font_id, glyph.id, self.role.size, color)
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::tokens::typography;

    #[test]
    fn spacing_is_the_roles_em_tracking() {
        // 11px at +0.06em.
        let label = Tracked::new(typography::SECTION_LABEL, "Merchant");
        assert!((f32::from(label.spacing()) - 0.66).abs() < 1e-4);
        assert_eq!(label.text, "MERCHANT");
        assert_eq!(Tracked::new(typography::BODY, "Body").spacing(), px(0.));
    }

    #[test]
    fn each_character_before_a_glyph_adds_one_step() {
        assert_eq!(offset_before("£23.99", 0, px(1.)), px(0.));
        // `£` is two bytes but one character.
        assert_eq!(offset_before("£23.99", 2, px(1.)), px(1.));
        assert_eq!(offset_before("£23.99", 3, px(1.)), px(2.));
    }
}
