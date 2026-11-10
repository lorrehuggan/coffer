//! Applying the type scale (`docs/DESIGN.md` → "Type scale") to elements.

use gpui_kit::{SharedString, Styled};

use crate::theme::tokens::typography::TextRole;

/// Sets a [`TextRole`] on an element.
pub trait TextRoleExt: Styled + Sized {
    /// Sets the role's family, size, line height and weight. Colour isn't
    /// part of a role; set it where the text sits.
    ///
    /// Tracking isn't applied: GPUI's text style has no letter spacing
    /// (`DESIGN.md` → "Implementing in GPUI" → "Letter spacing"). Uppercase
    /// isn't a style either; pass the text through [`CasedText::cased`].
    fn text_role(self, role: TextRole) -> Self {
        self.font_family(role.family)
            .text_size(role.size)
            .line_height(role.line_height)
            .font_weight(role.weight)
    }
}

impl<T: Styled + Sized> TextRoleExt for T {}

/// Text as a role shows it.
pub trait CasedText {
    /// `text` uppercased for an uppercase role (section labels), unchanged
    /// otherwise. Uppercasing is presentation: keep the source text in
    /// sentence case.
    fn cased(&self, text: impl Into<SharedString>) -> SharedString;
}

impl CasedText for TextRole {
    fn cased(&self, text: impl Into<SharedString>) -> SharedString {
        let text = text.into();
        if self.uppercase {
            text.to_uppercase().into()
        } else {
            text
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::theme::tokens::typography;

    use super::*;

    #[test]
    fn only_uppercase_roles_change_case() {
        assert_eq!(typography::SECTION_LABEL.cased("Accounts"), "ACCOUNTS");
        assert_eq!(typography::BODY.cased("Accounts"), "Accounts");
    }
}
