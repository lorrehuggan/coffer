//! The 8×8 category marker (`docs/DESIGN.md` → "Data visualisation" and
//! "Data table (Transactions)"): a category's colour wherever the category
//! is named, in lists, table cells and selects.

use gpui_kit::{Div, Rgba, Styled as _, div};

use crate::theme::tokens::{color, radius, size};

/// Which marker a category shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Marker {
    /// A category with a palette colour ([`color::CHART`]): filled.
    Colour(Rgba),
    /// "Other", the categories outside the palette: `field` fill with a
    /// solid 1px `text-muted` outline.
    Other,
    /// No category ("Uncategorised", "Choose a category"): a dashed 1px
    /// `text-muted` outline and no fill, so it stays distinct from
    /// [`Other`](Self::Other).
    None,
}

impl Marker {
    /// The marker as an element: 8×8, radius 2.
    pub fn render(self) -> Div {
        let marker = div().flex_none().size(size::MARKER).rounded(radius::MARKER);
        match self {
            Self::Colour(fill) => marker.bg(fill),
            Self::Other => marker
                .bg(color::FIELD)
                .border(size::HAIRLINE)
                .border_color(color::TEXT_MUTED),
            Self::None => marker
                .border(size::HAIRLINE)
                .border_dashed()
                .border_color(color::TEXT_MUTED),
        }
    }
}
