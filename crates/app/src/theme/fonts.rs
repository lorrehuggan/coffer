//! Bundled fonts: Inter for words, Geist Mono for figures.
//!
//! The faces are compiled into the binary, so text renders the same whatever
//! the system has installed. Following `docs/DESIGN.md` ("Implementing in
//! GPUI"), only static instances are bundled, one per weight the type scale
//! uses, so rendering never depends on variable-font support:
//!
//! | Family       | Weights            | Source                            |
//! |--------------|--------------------|-----------------------------------|
//! | Inter        | 400, 500, 600, 700 | Inter 4.1, `extras/ttf`           |
//! | Geist Mono   | 400, 600           | Geist 1.9.2, `GeistMono/ttf`      |
//!
//! Both are SIL OFL 1.1; the licence texts sit beside the files in
//! `assets/fonts/`. Adding a weight to [`super::tokens::weight`] or a role to
//! [`super::tokens::typography`] needs a matching face here; the tests fail
//! otherwise.

use std::borrow::Cow;
use std::collections::HashSet;
use std::error::Error;
use std::fmt;

use gpui_kit::component::Theme;
use gpui_kit::{App, Font, FontWeight};

use super::tokens::{font, weight};

/// One bundled font file and the family and weight it provides.
struct Face {
    family: &'static str,
    weight: FontWeight,
    data: &'static [u8],
}

const FACES: [Face; 6] = [
    Face {
        family: font::UI,
        weight: weight::REGULAR,
        data: include_bytes!("../../../../assets/fonts/inter/Inter-Regular.ttf"),
    },
    Face {
        family: font::UI,
        weight: weight::MEDIUM,
        data: include_bytes!("../../../../assets/fonts/inter/Inter-Medium.ttf"),
    },
    Face {
        family: font::UI,
        weight: weight::STRONG,
        data: include_bytes!("../../../../assets/fonts/inter/Inter-SemiBold.ttf"),
    },
    Face {
        family: font::UI,
        weight: weight::BOLD,
        data: include_bytes!("../../../../assets/fonts/inter/Inter-Bold.ttf"),
    },
    Face {
        family: font::MONO,
        weight: weight::REGULAR,
        data: include_bytes!("../../../../assets/fonts/geist-mono/GeistMono-Regular.ttf"),
    },
    Face {
        family: font::MONO,
        weight: weight::STRONG,
        data: include_bytes!("../../../../assets/fonts/geist-mono/GeistMono-SemiBold.ttf"),
    },
];

/// Why [`install`] failed.
#[derive(Debug)]
pub enum FontError {
    /// The text system rejected the bundled font data.
    Load(Box<dyn Error + Send + Sync>),
    /// A bundled face did not resolve to itself after loading: GPUI would
    /// silently draw that family and weight with a fallback font.
    Unresolved {
        family: &'static str,
        weight: FontWeight,
    },
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load(_) => f.write_str("the text system rejected the bundled fonts"),
            Self::Unresolved { family, weight } => write!(
                f,
                "bundled font {family:?} at weight {} did not resolve to its own face",
                weight.0
            ),
        }
    }
}

impl Error for FontError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Load(source) => Some(source.as_ref()),
            Self::Unresolved { .. } => None,
        }
    }
}

/// Registers the bundled faces with GPUI's text system and makes them the
/// theme's UI and mono families, so every component (and the window root's
/// default text style) uses them.
///
/// Call once at startup, after `gpui_kit::init` (which creates the theme)
/// and before opening a window.
///
/// # Errors
///
/// [`FontError::Load`] if the text system rejects a font file, and
/// [`FontError::Unresolved`] if a face loads but asking for its family and
/// weight lands on a different font (a fallback family, or a neighbouring
/// weight). The theme is left unchanged in both cases.
pub fn install(cx: &mut App) -> Result<(), FontError> {
    let text_system = cx.text_system();
    text_system
        .add_fonts(FACES.iter().map(|face| Cow::Borrowed(face.data)).collect())
        .map_err(|error| FontError::Load(error.into()))?;

    // `resolve_font` falls back silently; `get_font_for_id` only names the
    // requested font when it resolved directly. Distinct ids show that no
    // weight snapped to a neighbouring face.
    let mut resolved = HashSet::new();
    for face in &FACES {
        let requested = Font {
            weight: face.weight,
            ..gpui_kit::font(face.family)
        };
        let id = text_system.resolve_font(&requested);
        let direct = text_system
            .get_font_for_id(id)
            .is_some_and(|font| font.family.as_ref() == face.family);
        if !direct || !resolved.insert(id) {
            return Err(FontError::Unresolved {
                family: face.family,
                weight: face.weight,
            });
        }
    }

    Theme::update(cx, |theme| {
        theme.font_family = font::UI.into();
        theme.mono_font_family = font::MONO.into();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use ttf_parser::{Face as Parsed, name_id};

    use super::super::tokens::typography::{self, TextRole};
    use super::*;

    /// The family name font matchers look up: the typographic family when
    /// present (static Inter files name each weight "Inter Medium" etc. in
    /// the legacy family field), else the legacy family.
    fn family_name(face: &Parsed) -> String {
        let names = || face.names().into_iter().filter(|name| name.is_unicode());
        names()
            .find(|name| name.name_id == name_id::TYPOGRAPHIC_FAMILY)
            .or_else(|| names().find(|name| name.name_id == name_id::FAMILY))
            .and_then(|name| name.to_string())
            .expect("font has a family name")
    }

    fn bundles(family: &str, weight: FontWeight) -> bool {
        FACES
            .iter()
            .any(|face| face.family == family && face.weight == weight)
    }

    #[test]
    fn each_file_is_the_face_it_is_declared_as() {
        for face in &FACES {
            let parsed = Parsed::parse(face.data, 0).expect("bundled font parses");
            assert_eq!(family_name(&parsed), face.family);
            assert_eq!(f32::from(parsed.weight().to_number()), face.weight.0);
            assert!(
                !parsed.is_italic(),
                "{} {:?} is italic",
                face.family,
                face.weight
            );
        }
    }

    #[test]
    fn every_text_role_has_a_bundled_face() {
        let roles: [TextRole; 16] = [
            typography::DISPLAY,
            typography::DISPLAY_FIGURE,
            typography::FIGURE,
            typography::READING_HEADING,
            typography::READING_BODY,
            typography::TITLE,
            typography::CARD_TITLE,
            typography::SECTION_TITLE,
            typography::BODY,
            typography::BUTTON,
            typography::CAPTION,
            typography::SECTION_LABEL,
            typography::MONO_VALUE,
            typography::MONO_AMOUNT,
            typography::MONO_MICRO,
            typography::BADGE,
        ];
        for role in roles {
            assert!(
                bundles(role.family, role.weight),
                "missing face for {role:?}"
            );
        }
        // Variants the role docs name: emphasised body, agent and outline
        // buttons, selection labels.
        for weight in [
            weight::REGULAR,
            weight::MEDIUM,
            weight::STRONG,
            weight::BOLD,
        ] {
            assert!(bundles(font::UI, weight), "missing Inter {weight:?}");
        }
        assert!(bundles(font::MONO, weight::STRONG));
    }
}
