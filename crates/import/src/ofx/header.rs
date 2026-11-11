//! The OFX header and the file's text encoding.
//!
//! OFX 1.x starts with `KEY:VALUE` lines (`OFXHEADER:100`, `VERSION:102`,
//! `ENCODING:USASCII`, `CHARSET:1252`, …) before the SGML body. OFX 2.x is
//! XML: an optional `<?xml … encoding="…"?>` declaration, then an
//! `<?OFX OFXHEADER="200" VERSION="220" …?>` processing instruction. Some
//! banks leave the header out and start at `<OFX>`.

use std::borrow::Cow;

use coffer_core::statement::{SourceFormat, SourceLocation};

use crate::ImportError;
use crate::csv::TextEncoding;

/// How much of the file detection and the encoding sniff look at.
const PREFIX: usize = 4096;

/// What the header says, and where the body starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Header {
    /// `VERSION` as a number (`102`, `220`); `None` without a header or
    /// when it isn't a number.
    pub version: Option<u32>,
    /// Byte offset of the body in the decoded text.
    pub body_start: usize,
    /// The 1-based line the body starts on.
    pub body_line: u64,
}

fn malformed(location: Option<u64>, reason: &'static str) -> ImportError {
    ImportError::Malformed {
        format: SourceFormat::Ofx,
        location: location.map(SourceLocation::Line),
        reason,
    }
}

/// The start of `bytes` as text, for sniffing only: UTF-16 by its byte
/// order mark, anything else as lossy UTF-8 (headers are ASCII).
fn prefix(bytes: &[u8]) -> Cow<'_, str> {
    let head = &bytes[..bytes.len().min(PREFIX)];
    match encoding_rs::Encoding::for_bom(head) {
        Some((encoding, len)) => encoding.decode_without_bom_handling(&head[len..]).0,
        None => String::from_utf8_lossy(head),
    }
}

fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}

/// Whether the file starts like OFX: an OFX 1.x header, an XML declaration
/// or processing instruction followed by OFX, or a bare `<OFX>`.
pub(super) fn looks_like_ofx(bytes: &[u8]) -> bool {
    let prefix = prefix(bytes);
    let start = prefix.trim_start();
    if starts_with_ignore_case(start, "OFXHEADER") || starts_with_ignore_case(start, "<OFX>") {
        return true;
    }
    start.starts_with("<?") && (start.contains("<?OFX") || start.contains("<OFX>"))
}

/// Decodes the file: a byte order mark wins, otherwise the encoding the
/// header states (see [`stated_encoding`]). Decoding is strict, as for CSV:
/// a byte that isn't valid in the encoding fails the file.
pub(super) fn decode(bytes: &[u8]) -> Result<(Cow<'_, str>, TextEncoding), ImportError> {
    let (encoding, body) = match encoding_rs::Encoding::for_bom(bytes) {
        Some((encoding, len)) => {
            let known = [
                TextEncoding::Utf8,
                TextEncoding::Utf16Le,
                TextEncoding::Utf16Be,
            ]
            .into_iter()
            .find(|known| known.encoding() == encoding)
            .expect("every BOM names UTF-8 or UTF-16");
            (known, &bytes[len..])
        }
        None => (stated_encoding(bytes)?, bytes),
    };
    let text = encoding
        .encoding()
        .decode_without_bom_handling_and_without_replacement(body)
        .ok_or(ImportError::Encoding {
            encoding: encoding.name(),
        })?;
    if text.contains('\0') {
        return Err(malformed(
            None,
            "the text contains NUL characters; it may be UTF-16 without a byte order mark",
        ));
    }
    Ok((text, encoding))
}

/// The encoding the header names, never guessed:
///
/// - OFX 1.x: `ENCODING:UTF-8` (or `UNICODE`) is UTF-8. `ENCODING:USASCII`
///   takes `CHARSET`: `1252` and Latin-1 are Windows-1252, `ISO-8859-15`
///   is Latin-9, and `NONE` or no charset is ASCII, read as UTF-8 so that
///   other bytes fail the file.
/// - OFX 2.x: the XML declaration's `encoding`, else UTF-8.
/// - No header: UTF-8.
///
/// An encoding Coffer doesn't read is [`ImportError::Unsupported`].
fn stated_encoding(bytes: &[u8]) -> Result<TextEncoding, ImportError> {
    let unsupported = ImportError::Unsupported {
        what: "the file's character encoding",
    };
    let prefix = prefix(bytes);
    let start = prefix.trim_start();
    if starts_with_ignore_case(start, "OFXHEADER") {
        // Malformed lines are reported by `read`, with their line numbers.
        let header = &start[..start.find('<').unwrap_or(start.len())];
        let field = |key: &str| {
            header
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.trim().eq_ignore_ascii_case(key))
                .map(|(_, value)| value.trim().to_ascii_uppercase())
        };
        match field("ENCODING").as_deref() {
            Some("UTF-8" | "UTF8" | "UNICODE") => Ok(TextEncoding::Utf8),
            None | Some("USASCII") => match field("CHARSET").as_deref() {
                None | Some("NONE") => Ok(TextEncoding::Utf8),
                Some(charset) => charset_encoding(charset).ok_or(unsupported),
            },
            Some(_) => Err(unsupported),
        }
    } else if start.starts_with("<?xml") {
        let declaration = &start[..start.find("?>").unwrap_or(start.len())];
        match attribute(declaration, "encoding") {
            None => Ok(TextEncoding::Utf8),
            Some(label) => charset_encoding(label).ok_or(unsupported),
        }
    } else {
        Ok(TextEncoding::Utf8)
    }
}

/// An OFX `CHARSET` or XML encoding label: a WHATWG label (`utf-8`,
/// `windows-1252`, `iso-8859-1`, `iso-8859-15`), or a bare code page or
/// ISO number (`1252`, `8859-1`).
fn charset_encoding(label: &str) -> Option<TextEncoding> {
    let label = label.trim();
    [
        label.to_owned(),
        format!("windows-{label}"),
        format!("iso-{label}"),
    ]
    .iter()
    .find_map(|label| TextEncoding::from_label(label))
    .filter(|encoding| *encoding != TextEncoding::Auto)
}

/// The `KEY:VALUE` lines of an OFX 1.x header, starting on `first_line`.
/// Blank lines are skipped; any other line without a `:` fails the file.
fn sgml_fields(header: &str, first_line: u64) -> Result<Vec<(&str, &str)>, ImportError> {
    let mut fields = Vec::new();
    for (line, text) in (first_line..).zip(header.lines()) {
        if text.trim().is_empty() {
            continue;
        }
        let (key, value) = text
            .split_once(':')
            .ok_or_else(|| malformed(Some(line), "a header line isn't KEY:VALUE"))?;
        fields.push((key.trim(), value.trim()));
    }
    Ok(fields)
}

/// The value of `name="value"` (or single quotes) in an XML declaration or
/// processing instruction.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = tag;
    while let Some(at) = rest.find(name) {
        let before_ok = rest[..at]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace);
        let after = rest[at + name.len()..].trim_start();
        if before_ok && let Some(value) = after.strip_prefix('=') {
            let value = value.trim_start();
            let quote = value.chars().next().filter(|c| matches!(c, '"' | '\''))?;
            let value = &value[1..];
            return value.find(quote).map(|end| &value[..end]);
        }
        rest = &rest[at + name.len()..];
    }
    None
}

/// Reads the header at the start of the decoded `text`.
pub(super) fn read(text: &str) -> Result<Header, ImportError> {
    let start = text.len() - text.trim_start().len();
    let first_line = 1 + newlines(&text[..start]);
    let trimmed = &text[start..];

    if starts_with_ignore_case(trimmed, "OFXHEADER") {
        let header_len = trimmed
            .find('<')
            .ok_or_else(|| malformed(None, "the file has an OFX header but no body"))?;
        let header = &trimmed[..header_len];
        let fields = sgml_fields(header, first_line)?;
        let field = |key: &str| {
            fields
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(key))
                .map(|(_, value)| *value)
        };
        if field("COMPRESSION").is_some_and(|value| !value.eq_ignore_ascii_case("NONE")) {
            return Err(ImportError::Unsupported {
                what: "compressed OFX files",
            });
        }
        return Ok(Header {
            version: field("VERSION").and_then(|version| version.parse().ok()),
            body_start: start + header_len,
            body_line: first_line + newlines(header),
        });
    }

    if trimmed.starts_with("<?") || starts_with_ignore_case(trimmed, "<OFX") {
        // The tokenizer skips the processing instructions; only the
        // version is read here.
        let version = trimmed.find("<?OFX").and_then(|at| {
            let instruction = &trimmed[at..];
            let instruction = &instruction[..instruction.find("?>")?];
            attribute(instruction, "VERSION")?.parse().ok()
        });
        return Ok(Header {
            version,
            body_start: start,
            body_line: first_line,
        });
    }

    Err(malformed(None, "the file doesn't start with an OFX header"))
}

fn newlines(text: &str) -> u64 {
    text.bytes().filter(|&b| b == b'\n').count() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoding_of(bytes: &[u8]) -> Result<TextEncoding, String> {
        decode(bytes)
            .map(|(_, encoding)| encoding)
            .map_err(|error| error.to_string())
    }

    fn sgml(encoding: &str, charset: &str, body: &[u8]) -> Vec<u8> {
        let mut file = format!(
            "OFXHEADER:100\r\nDATA:OFXSGML\r\nVERSION:102\r\nSECURITY:NONE\r\n\
             ENCODING:{encoding}\r\nCHARSET:{charset}\r\nCOMPRESSION:NONE\r\n\
             OLDFILEUID:NONE\r\nNEWFILEUID:NONE\r\n\r\n<OFX>"
        )
        .into_bytes();
        file.extend_from_slice(body);
        file.extend_from_slice(b"</OFX>");
        file
    }

    #[test]
    fn detection() {
        assert!(looks_like_ofx(b"OFXHEADER:100\nDATA:OFXSGML\n"));
        assert!(looks_like_ofx(b"\r\n  ofxheader:100"));
        assert!(looks_like_ofx(
            b"<?xml version=\"1.0\"?>\n<?OFX OFXHEADER=\"200\"?>"
        ));
        assert!(looks_like_ofx(b"<?OFX OFXHEADER=\"200\"?><OFX>"));
        assert!(looks_like_ofx(b"<OFX>\n<SIGNONMSGSRSV1>"));
        assert!(looks_like_ofx(b"\xEF\xBB\xBFOFXHEADER:100"));
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("<OFX>".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert!(looks_like_ofx(&utf16));

        assert!(!looks_like_ofx(b"Date,Amount,Description\n"));
        assert!(!looks_like_ofx(b"<?xml version=\"1.0\"?><Document>"));
        assert!(!looks_like_ofx(b""));
    }

    #[test]
    fn sgml_encodings() {
        assert_eq!(
            encoding_of(&sgml("USASCII", "1252", b"Caf\xe9 \xa3")),
            Ok(TextEncoding::Windows1252)
        );
        assert_eq!(
            encoding_of(&sgml("USASCII", "ISO-8859-1", b"Caf\xe9")),
            Ok(TextEncoding::Windows1252)
        );
        assert_eq!(
            encoding_of(&sgml("USASCII", "8859-15", b"\xa4")),
            Ok(TextEncoding::Iso8859_15)
        );
        assert_eq!(
            encoding_of(&sgml("UTF-8", "NONE", "Café".as_bytes())),
            Ok(TextEncoding::Utf8)
        );
        assert_eq!(
            encoding_of(&sgml("UNICODE", "1252", "Café".as_bytes())),
            Ok(TextEncoding::Utf8),
            "CHARSET only applies to USASCII"
        );
        assert_eq!(
            encoding_of(&sgml("USASCII", "NONE", b"Plain")),
            Ok(TextEncoding::Utf8)
        );
    }

    #[test]
    fn encodings_are_never_guessed() {
        assert_eq!(
            encoding_of(&sgml("USASCII", "NONE", b"Caf\xe9")),
            Err("the file isn't valid UTF-8 text".to_owned()),
            "ASCII with a stray Latin-1 byte"
        );
        assert_eq!(
            encoding_of(&sgml("USASCII", "437", b"x")),
            Err("Coffer can't import the file's character encoding".to_owned())
        );
        assert_eq!(
            encoding_of(&sgml("EBCDIC", "NONE", b"x")),
            Err("Coffer can't import the file's character encoding".to_owned())
        );
    }

    #[test]
    fn xml_encodings() {
        let xml = |declaration: &str, body: &[u8]| {
            let mut file =
                format!("{declaration}\n<?OFX OFXHEADER=\"200\" VERSION=\"220\"?>\n<OFX>")
                    .into_bytes();
            file.extend_from_slice(body);
            file
        };
        assert_eq!(
            encoding_of(&xml("<?xml version=\"1.0\"?>", "é".as_bytes())),
            Ok(TextEncoding::Utf8)
        );
        assert_eq!(
            encoding_of(&xml("<?xml version='1.0' encoding='ISO-8859-1'?>", b"\xe9")),
            Ok(TextEncoding::Windows1252)
        );
        assert_eq!(
            encoding_of(&xml("<?xml version=\"1.0\" encoding=\"Shift_JIS\"?>", b"")),
            Err("Coffer can't import the file's character encoding".to_owned())
        );
    }

    #[test]
    fn a_bom_wins_over_the_header() {
        let mut file = b"\xEF\xBB\xBF".to_vec();
        file.extend(sgml("USASCII", "1252", "Café".as_bytes()));
        let (text, encoding) = decode(&file).unwrap();
        assert_eq!(encoding, TextEncoding::Utf8);
        assert!(text.starts_with("OFXHEADER"));
        assert!(text.contains("Café"));
    }

    #[test]
    fn nul_characters_fail_the_file() {
        let utf16: Vec<u8> = "OFXHEADER:100"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let error = encoding_of(&utf16).unwrap_err();
        assert!(error.contains("NUL"), "{error}");
    }

    #[test]
    fn sgml_header() {
        let text = "\r\nOFXHEADER:100\r\nDATA:OFXSGML\r\nVERSION:102\r\n\r\n<OFX></OFX>";
        let header = read(text).unwrap();
        assert_eq!(header.version, Some(102));
        assert!(text[header.body_start..].starts_with("<OFX>"));
        assert_eq!(header.body_line, 6);
    }

    #[test]
    fn xml_header() {
        let text = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                    <?OFX OFXHEADER=\"200\" VERSION=\"211\" SECURITY=\"NONE\"?>\n<OFX></OFX>";
        let header = read(text).unwrap();
        assert_eq!(header.version, Some(211));
        assert_eq!(header.body_start, 0);
        assert_eq!(header.body_line, 1);
    }

    #[test]
    fn no_header() {
        let header = read("\n<OFX></OFX>").unwrap();
        assert_eq!(header.version, None);
        assert_eq!(header.body_start, 1);
        assert_eq!(header.body_line, 2);
    }

    #[test]
    fn header_errors() {
        let error = |text: &str| read(text).unwrap_err().to_string();
        assert_eq!(
            error("OFXHEADER:100\nCOMPRESSION:ZLIB\n<OFX>"),
            "Coffer can't import compressed OFX files"
        );
        assert_eq!(
            error("OFXHEADER:100\nsecret text\n<OFX>"),
            "the file isn't a readable ofx file (line 2): a header line isn't KEY:VALUE"
        );
        assert_eq!(
            error("OFXHEADER:100\nVERSION:102\n"),
            "the file isn't a readable ofx file: the file has an OFX header but no body"
        );
        assert_eq!(
            error("Date,Amount\n"),
            "the file isn't a readable ofx file: the file doesn't start with an OFX header"
        );
    }

    #[test]
    fn attributes() {
        let tag = "<?OFX OFXHEADER=\"200\" VERSION = '220' OLDVERSION=\"1\"";
        assert_eq!(attribute(tag, "VERSION"), Some("220"));
        assert_eq!(attribute(tag, "OFXHEADER"), Some("200"));
        assert_eq!(attribute(tag, "SECURITY"), None);
    }
}
