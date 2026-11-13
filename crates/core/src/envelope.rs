//! The untrusted-text envelope (ROADMAP M2.13; `docs/ARCHITECTURE.md` →
//! "MCP surface" and "Decision record: safe report rendering and withheld
//! text (2026-11-10)").
//!
//! Bank text (descriptions, imported merchant names) can carry
//! prompt-injection text. Before any of it leaves Coffer as
//! `description_untrusted`, it goes through [`sanitize`]: a pure function,
//! so the app's "Withheld from AI" filter can re-run the same check over
//! raw descriptions and agree with what was sent.
//!
//! Wave 0 only caps the length. Block B implements the rest here, keeping
//! the signature: strip control, zero-width and bidi characters; make URLs
//! inert; replace text matching injection patterns ("ignore previous…",
//! base64 blobs, URLs) with [`withheld_marker`] and report how many
//! characters were withheld.

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::setting::PseudonymKey;

/// How many bytes of the HMAC a pseudonym keeps: 6 bytes, 12 hex digits.
/// At 10,000 merchants the chance that any two share a token is about
/// n²/2⁴⁹ ≈ 1.8 × 10⁻⁷.
pub const PSEUDONYM_BYTES: usize = 6;

/// A merchant's pseudonym at `pseudonymous` access: `M-` and 12 lowercase
/// hex digits of HMAC-SHA256(key, display name). The same name always gives
/// the same token in one database, and a different one in another (each has
/// its own key). The name is trimmed and lowercased first, so names that
/// differ only in case or surrounding spaces share a token.
pub fn pseudonym(key: &PseudonymKey, display: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key.bytes()).expect("HMAC takes any key length");
    mac.update(display.trim().to_lowercase().as_bytes());
    let digest = mac.finalize().into_bytes();
    let mut token = String::with_capacity(2 + PSEUDONYM_BYTES * 2);
    token.push_str("M-");
    for byte in &digest[..PSEUDONYM_BYTES] {
        token.push_str(&format!("{byte:02x}"));
    }
    token
}

/// The longest `description_untrusted`, in characters.
pub const MAX_UNTRUSTED_CHARS: usize = 280;

/// Text that is safe to hand to an AI client as untrusted text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sanitized {
    /// The text to send: cleaned, capped, or the withheld marker.
    pub text: String,
    /// How many characters of the input were withheld (0 when none were).
    /// A capped tail isn't withheld.
    pub withheld_chars: usize,
}

impl Sanitized {
    /// Whether the text was withheld.
    pub fn is_withheld(&self) -> bool {
        self.withheld_chars > 0
    }
}

/// The marker that replaces withheld text. It reads as Coffer's words, not
/// the bank's, so a client can't mistake it for a description.
pub fn withheld_marker(chars: usize) -> String {
    format!("[withheld: {chars} characters that read like instructions]")
}

/// Removes the characters that change how text reads without being seen:
/// C0 controls except tab and line feed, DEL, C1 controls, zero-width and
/// other invisible format characters, bidi embeddings, overrides, isolates
/// and marks, line and paragraph separators, interlinear annotation
/// characters and Unicode tag characters (U+E0000–U+E007F, which can spell
/// out hidden ASCII). Nothing else changes: no length cap, nothing
/// withheld. For saved reports read back (`list_reports`, decision record
/// 2026-11-10, point 4) and the access log's `purpose`; [`sanitize`] builds
/// on it.
pub fn strip_invisible(text: &str) -> String {
    text.chars().filter(|&c| !is_invisible(c)).collect()
}

/// Whether [`strip_invisible`] removes `c`.
pub fn is_invisible(c: char) -> bool {
    matches!(c,
        // C0 controls but tab and line feed, DEL, C1 controls.
        '\u{0}'..='\u{8}' | '\u{B}'..='\u{1F}' | '\u{7F}'..='\u{9F}'
        // Soft hyphen, Arabic letter mark, Mongolian vowel separator.
        | '\u{AD}' | '\u{61C}' | '\u{180E}'
        // Zero-width space, non-joiner, joiner; LRM, RLM.
        | '\u{200B}'..='\u{200F}'
        // Line and paragraph separators; bidi embeddings and overrides.
        | '\u{2028}'..='\u{202E}'
        // Word joiner, invisible operators, bidi isolates, deprecated
        // format characters.
        | '\u{2060}'..='\u{206F}'
        // Byte order mark / zero-width no-break space.
        | '\u{FEFF}'
        // Interlinear annotation.
        | '\u{FFF9}'..='\u{FFFB}'
        // Tag characters.
        | '\u{E0000}'..='\u{E007F}'
    )
}

/// Cleans `raw` for an AI client.
pub fn sanitize(raw: &str) -> Sanitized {
    // Block B: strip control, zero-width and bidi characters; make URLs
    // inert; withhold text that matches injection patterns, returning
    // `withheld_marker(n)` with `withheld_chars: n`.
    Sanitized {
        text: raw.chars().take(MAX_UNTRUSTED_CHARS).collect(),
        withheld_chars: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_the_length() {
        let long = "é".repeat(MAX_UNTRUSTED_CHARS + 5);
        let sanitized = sanitize(&long);
        assert_eq!(sanitized.text.chars().count(), MAX_UNTRUSTED_CHARS);
        assert!(!sanitized.is_withheld());
        assert_eq!(sanitize("CARD PAYMENT").text, "CARD PAYMENT");
    }

    #[test]
    fn strip_invisible_keeps_lines_and_visible_text() {
        assert_eq!(
            strip_invisible("a\u{202E}b\u{200B}c\u{0}d\u{85}e\u{E0041}f\u{2066}g\u{FEFF}"),
            "abcdefg"
        );
        assert_eq!(
            strip_invisible("# Title\n\n\tcafé £3 ü"),
            "# Title\n\n\tcafé £3 ü"
        );
    }

    #[test]
    fn the_marker_names_the_count() {
        assert_eq!(
            withheld_marker(42),
            "[withheld: 42 characters that read like instructions]"
        );
    }
}
