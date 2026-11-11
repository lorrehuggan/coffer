//! Decoding a CSV file's bytes into text.

use std::borrow::Cow;
use std::fmt;

use coffer_core::statement::SourceFormat;

use crate::ImportError;

/// The character encoding a CSV file is in.
///
/// Decoding is strict: a byte sequence that isn't valid in the encoding
/// fails the whole file ([`ImportError::Encoding`]) rather than being
/// replaced, since a replaced character would change the description and so
/// the dedupe fingerprint. A byte order mark at the start always wins over
/// the stated encoding, as in browsers, and is removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextEncoding {
    /// A byte order mark if the file starts with one, otherwise UTF-8.
    ///
    /// Nothing else is tried: a file that isn't valid UTF-8 needs its
    /// encoding stated (by the preset or the user) rather than guessed,
    /// because single-byte encodings accept any bytes and a wrong one
    /// silently changes accented letters and symbols.
    #[default]
    Auto,
    Utf8,
    Utf16Le,
    Utf16Be,
    /// Windows-1252, which also reads Latin-1 (ISO-8859-1) files, as the
    /// WHATWG Encoding Standard does. Common in older UK and US exports.
    Windows1252,
    /// Latin-9 (ISO-8859-15): Latin-1 with the euro sign. Some EU banks.
    Iso8859_15,
}

impl TextEncoding {
    pub(crate) fn encoding(self) -> &'static encoding_rs::Encoding {
        match self {
            TextEncoding::Auto | TextEncoding::Utf8 => encoding_rs::UTF_8,
            TextEncoding::Utf16Le => encoding_rs::UTF_16LE,
            TextEncoding::Utf16Be => encoding_rs::UTF_16BE,
            TextEncoding::Windows1252 => encoding_rs::WINDOWS_1252,
            TextEncoding::Iso8859_15 => encoding_rs::ISO_8859_15,
        }
    }

    fn from_encoding(encoding: &'static encoding_rs::Encoding) -> Option<TextEncoding> {
        [
            TextEncoding::Utf8,
            TextEncoding::Utf16Le,
            TextEncoding::Utf16Be,
            TextEncoding::Windows1252,
            TextEncoding::Iso8859_15,
        ]
        .into_iter()
        .find(|known| known.encoding() == encoding)
    }

    /// The encoding a preset names: `auto`, or a WHATWG label such as
    /// `utf-8`, `utf-16le`, `windows-1252` (also `latin1`, `iso-8859-1`)
    /// or `iso-8859-15`, ignoring ASCII case. `None` for anything else,
    /// including real encodings Coffer doesn't read.
    pub fn from_label(label: &str) -> Option<TextEncoding> {
        if label.trim().eq_ignore_ascii_case("auto") {
            return Some(TextEncoding::Auto);
        }
        encoding_rs::Encoding::for_label(label.as_bytes()).and_then(TextEncoding::from_encoding)
    }

    /// The encoding's name, e.g. `UTF-8`.
    pub fn name(self) -> &'static str {
        match self {
            TextEncoding::Auto => "automatic",
            other => other.encoding().name(),
        }
    }
}

impl fmt::Display for TextEncoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Decodes `bytes` as `stated`, or as the encoding its byte order mark
/// names. Returns the text without the mark and the encoding actually used
/// (never [`TextEncoding::Auto`]).
pub(super) fn decode(
    bytes: &[u8],
    stated: TextEncoding,
) -> Result<(Cow<'_, str>, TextEncoding), ImportError> {
    let (used, body) = match encoding_rs::Encoding::for_bom(bytes) {
        Some((encoding, bom_len)) => (
            TextEncoding::from_encoding(encoding).expect("every BOM names a known encoding"),
            &bytes[bom_len..],
        ),
        None if stated == TextEncoding::Auto => (TextEncoding::Utf8, bytes),
        None => (stated, bytes),
    };
    let text = used
        .encoding()
        .decode_without_bom_handling_and_without_replacement(body)
        .ok_or(ImportError::Encoding {
            encoding: used.name(),
        })?;
    // Valid UTF-8 can still be UTF-16 without a byte order mark (every
    // other byte of ASCII text is NUL). No bank file has NULs otherwise.
    if text.contains('\0') {
        return Err(ImportError::Malformed {
            format: SourceFormat::Csv,
            location: None,
            reason: "the text contains NUL characters; it may be UTF-16 without a byte order mark",
        });
    }
    Ok((text, used))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded(bytes: &[u8], stated: TextEncoding) -> Result<(String, TextEncoding), String> {
        decode(bytes, stated)
            .map(|(text, used)| (text.into_owned(), used))
            .map_err(|error| error.to_string())
    }

    #[test]
    fn labels() {
        for (label, encoding) in [
            ("auto", TextEncoding::Auto),
            ("AUTO", TextEncoding::Auto),
            ("utf-8", TextEncoding::Utf8),
            ("UTF8", TextEncoding::Utf8),
            ("utf-16le", TextEncoding::Utf16Le),
            ("utf-16be", TextEncoding::Utf16Be),
            ("windows-1252", TextEncoding::Windows1252),
            ("latin1", TextEncoding::Windows1252),
            ("iso-8859-1", TextEncoding::Windows1252),
            ("iso-8859-15", TextEncoding::Iso8859_15),
        ] {
            assert_eq!(TextEncoding::from_label(label), Some(encoding), "{label}");
        }
        for label in ["", "utf-7", "shift_jis", "ebcdic"] {
            assert_eq!(TextEncoding::from_label(label), None, "{label}");
        }
    }

    #[test]
    fn auto_reads_utf8_and_strips_its_bom() {
        assert_eq!(
            decoded("Café,1".as_bytes(), TextEncoding::Auto),
            Ok(("Café,1".to_owned(), TextEncoding::Utf8))
        );
        assert_eq!(
            decoded(b"\xEF\xBB\xBFDate,Amount", TextEncoding::Auto),
            Ok(("Date,Amount".to_owned(), TextEncoding::Utf8))
        );
    }

    #[test]
    fn auto_refuses_other_encodings_instead_of_guessing() {
        assert_eq!(
            decoded(b"Caf\xe9,1", TextEncoding::Auto),
            Err("the file isn't valid UTF-8 text".to_owned())
        );
    }

    #[test]
    fn stated_single_byte_encodings() {
        assert_eq!(
            decoded(b"Caf\xe9 \x80 \xa3", TextEncoding::Windows1252),
            Ok(("Café € £".to_owned(), TextEncoding::Windows1252))
        );
        assert_eq!(
            decoded(b"Caf\xe9 \xa4", TextEncoding::Iso8859_15),
            Ok(("Café €".to_owned(), TextEncoding::Iso8859_15))
        );
    }

    #[test]
    fn stated_utf8_is_strict() {
        assert_eq!(
            decoded(b"Caf\xe9", TextEncoding::Utf8),
            Err("the file isn't valid UTF-8 text".to_owned())
        );
    }

    #[test]
    fn utf16_by_bom_or_stated() {
        let le: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("A,£".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(
            decoded(&le, TextEncoding::Auto),
            Ok(("A,£".to_owned(), TextEncoding::Utf16Le))
        );
        let be: Vec<u8> = "A,£".encode_utf16().flat_map(u16::to_be_bytes).collect();
        assert_eq!(
            decoded(&be, TextEncoding::Utf16Be),
            Ok(("A,£".to_owned(), TextEncoding::Utf16Be))
        );
    }

    #[test]
    fn a_bom_wins_over_the_stated_encoding() {
        assert_eq!(
            decoded("\u{FEFF}é".as_bytes(), TextEncoding::Windows1252),
            Ok(("é".to_owned(), TextEncoding::Utf8))
        );
    }

    #[test]
    fn utf16_without_a_bom_is_caught() {
        let le: Vec<u8> = "Date,Amount"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let error = decoded(&le, TextEncoding::Auto).unwrap_err();
        assert!(
            error.contains("UTF-16 without a byte order mark"),
            "{error}"
        );
    }

    #[test]
    fn names() {
        assert_eq!(TextEncoding::Utf8.name(), "UTF-8");
        assert_eq!(TextEncoding::Windows1252.to_string(), "windows-1252");
        assert_eq!(TextEncoding::Iso8859_15.name(), "ISO-8859-15");
        assert_eq!(TextEncoding::Auto.name(), "automatic");
    }
}
