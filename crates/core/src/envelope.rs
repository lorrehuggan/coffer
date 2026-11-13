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
    fn the_marker_names_the_count() {
        assert_eq!(
            withheld_marker(42),
            "[withheld: 42 characters that read like instructions]"
        );
    }
}
