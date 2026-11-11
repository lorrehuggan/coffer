//! A tolerant reader for OFX markup: SGML (OFX 1.x, where values are left
//! unclosed) and XML (OFX 2.x) through the same code.
//!
//! OFX has two kinds of element. An *aggregate* holds other elements and
//! is always closed (`<STMTTRN> … </STMTTRN>`). A *value* element holds
//! text; SGML leaves it unclosed (`<TRNAMT>-12.40`), XML closes it
//! (`<TRNAMT>-12.40</TRNAMT>`), and some SGML files close some of them. A
//! start tag followed by text is a value either way. A start tag followed by
//! another tag is an aggregate or an empty value; the known OFX names
//! decide, and for any other name, whether the file ever closes it.
//!
//! The structure is read strictly: an aggregate that isn't closed, a
//! closing tag for an aggregate that isn't open, text where only elements
//! belong, or a file cut short fails the whole file, since guessing would
//! move transactions between statements. Errors name a line, never the
//! file's text.

use std::collections::HashSet;
use std::fmt;

use coffer_core::statement::{SourceFormat, SourceLocation};

use crate::ImportError;

/// Aggregates in the parts of OFX that Coffer reads, and their usual
/// neighbours. Any of these opens an aggregate even when the file never
/// closes it, which then fails the file instead of swallowing siblings.
const AGGREGATES: &[&str] = &[
    "OFX",
    "SIGNONMSGSRSV1",
    "SONRS",
    "STATUS",
    "FI",
    "SIGNUPMSGSRSV1",
    "BANKMSGSRSV1",
    "STMTTRNRS",
    "STMTRS",
    "STMTENDTRNRS",
    "STMTENDRS",
    "CLOSING",
    "BANKACCTFROM",
    "BANKACCTTO",
    "BANKTRANLIST",
    "STMTTRN",
    "PAYEE",
    "CURRENCY",
    "ORIGCURRENCY",
    "LEDGERBAL",
    "AVAILBAL",
    "BALLIST",
    "BAL",
    "CREDITCARDMSGSRSV1",
    "CCSTMTTRNRS",
    "CCSTMTRS",
    "CCSTMTENDTRNRS",
    "CCSTMTENDRS",
    "CCACCTFROM",
    "CCACCTTO",
    "INVSTMTMSGSRSV1",
    "INVSTMTTRNRS",
    "INVSTMTRS",
    "INVACCTFROM",
    "INVTRANLIST",
    "INVBANKTRAN",
    "INVPOSLIST",
    "INVBAL",
    "SECLISTMSGSRSV1",
    "SECLIST",
];

/// Values that may appear empty in SGML (`<MEMO>` straight before the next
/// tag). Without this list, a file that closes them elsewhere would make
/// an empty one look like an aggregate.
const VALUES: &[&str] = &[
    "CODE",
    "SEVERITY",
    "MESSAGE",
    "DTSERVER",
    "USERKEY",
    "LANGUAGE",
    "DTPROFUP",
    "DTACCTUP",
    "ORG",
    "FID",
    "SESSCOOKIE",
    "TRNUID",
    "CLTCOOKIE",
    "CURDEF",
    "BANKID",
    "BRANCHID",
    "ACCTID",
    "ACCTTYPE",
    "ACCTKEY",
    "DTSTART",
    "DTEND",
    "TRNTYPE",
    "DTPOSTED",
    "DTUSER",
    "DTAVAIL",
    "TRNAMT",
    "FITID",
    "CORRECTFITID",
    "CORRECTACTION",
    "SRVRTID",
    "CHECKNUM",
    "REFNUM",
    "SIC",
    "PAYEEID",
    "NAME",
    "EXTDNAME",
    "MEMO",
    "CURRATE",
    "CURSYM",
    "BALAMT",
    "DTASOF",
    "ADDR1",
    "ADDR2",
    "ADDR3",
    "CITY",
    "STATE",
    "POSTALCODE",
    "COUNTRY",
    "PHONE",
    "MKTGINFO",
    "INV401KSOURCE",
    "INTU.BID",
    "INTU.USERID",
];

/// One element of the document.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct Element {
    /// The tag name in ASCII uppercase.
    pub name: String,
    /// The line its start tag is on.
    pub line: u64,
    pub content: Content,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) enum Content {
    /// A value element's text, entities decoded and trimmed. Empty when the
    /// element is.
    Value(String),
    /// An aggregate's elements, in file order.
    Children(Vec<Element>),
}

impl Element {
    /// The value, if this is a value element.
    pub fn value(&self) -> Option<&str> {
        match &self.content {
            Content::Value(value) => Some(value),
            Content::Children(_) => None,
        }
    }

    /// The elements inside, if this is an aggregate; none otherwise.
    pub fn children(&self) -> &[Element] {
        match &self.content {
            Content::Children(children) => children,
            Content::Value(_) => &[],
        }
    }

    /// The direct children called `name`, in order.
    pub fn children_named<'a, 'n>(
        &'a self,
        name: &'n str,
    ) -> impl Iterator<Item = &'a Element> + use<'a, 'n> {
        self.children()
            .iter()
            .filter(move |child| child.name == name)
    }
}

/// Shows the name, line and shape only: values are transaction data.
impl fmt::Debug for Element {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("Element");
        debug.field("name", &self.name).field("line", &self.line);
        match &self.content {
            Content::Value(value) => debug.field("value_len", &value.len()),
            Content::Children(children) => debug.field("children", children),
        };
        debug.finish()
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Start {
        name: String,
        line: u64,
    },
    End {
        name: String,
        line: u64,
    },
    /// Text between tags, entities decoded, untrimmed.
    Text {
        text: String,
        line: u64,
    },
}

fn malformed(line: u64, reason: &'static str) -> ImportError {
    ImportError::Malformed {
        format: SourceFormat::Ofx,
        location: Some(SourceLocation::Line(line)),
        reason,
    }
}

/// Reads the OFX body (`text` starts on line `first_line`) into its root
/// `<OFX>` element.
pub(super) fn read(text: &str, first_line: u64) -> Result<Element, ImportError> {
    let tokens = tokenize(text, first_line)?;
    build(&tokens)
}

/// Splits `text` into tags and text. Comments, processing instructions and
/// declarations are skipped; CDATA is text taken as written.
fn tokenize(text: &str, first_line: u64) -> Result<Vec<Token>, ImportError> {
    let mut tokens = Vec::new();
    let mut line = first_line;
    let mut pending = String::new();
    let mut pending_line = line;
    let mut rest = text;

    let flush = |tokens: &mut Vec<Token>, pending: &mut String, pending_line: u64| {
        if !pending.is_empty() {
            tokens.push(Token::Text {
                text: std::mem::take(pending),
                line: pending_line,
            });
        }
    };

    while !rest.is_empty() {
        let Some(at) = rest.find('<') else {
            if pending.is_empty() {
                pending_line = line;
            }
            decode_entities(rest, &mut pending);
            break;
        };
        let (before, markup) = rest.split_at(at);
        if !before.is_empty() {
            if pending.is_empty() {
                pending_line = line;
            }
            decode_entities(before, &mut pending);
            line += newlines(before);
        }

        let (consumed, token) = if let Some(after) = markup.strip_prefix("<!--") {
            let end = after
                .find("-->")
                .ok_or_else(|| malformed(line, "a comment isn't closed"))?;
            (4 + end + 3, None)
        } else if let Some(after) = markup.strip_prefix("<![CDATA[") {
            let end = after
                .find("]]>")
                .ok_or_else(|| malformed(line, "a CDATA section isn't closed"))?;
            if pending.is_empty() {
                pending_line = line;
            }
            pending.push_str(&after[..end]);
            (9 + end + 3, None)
        } else if markup.starts_with("<?") {
            let end = markup
                .find("?>")
                .ok_or_else(|| malformed(line, "a processing instruction isn't closed"))?;
            (end + 2, None)
        } else if markup.starts_with("<!") {
            let end = markup
                .find('>')
                .ok_or_else(|| malformed(line, "a declaration isn't closed"))?;
            (end + 1, None)
        } else if let Some(after) = markup.strip_prefix("</") {
            let end = after
                .find(['>', '<'])
                .filter(|&end| after.as_bytes()[end] == b'>')
                .ok_or_else(|| malformed(line, "a closing tag isn't closed with '>'"))?;
            let name = after[..end].trim();
            if name.is_empty() {
                return Err(malformed(line, "a closing tag has no name"));
            }
            let token = Token::End {
                name: name.to_ascii_uppercase(),
                line,
            };
            (2 + end + 1, Some(vec![token]))
        } else if markup[1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
            let (end, self_closing) =
                tag_end(markup).ok_or_else(|| malformed(line, "a tag isn't closed with '>'"))?;
            let inside = &markup[1..if self_closing { end - 1 } else { end }];
            let name = inside
                .split(|c: char| c.is_whitespace())
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase();
            let mut tokens = vec![Token::Start {
                name: name.clone(),
                line,
            }];
            if self_closing {
                tokens.push(Token::End { name, line });
            }
            (end + 1, Some(tokens))
        } else {
            // A `<` that doesn't start markup ("A < B") is text.
            if pending.is_empty() {
                pending_line = line;
            }
            pending.push('<');
            (1, None)
        };

        if let Some(new) = token {
            flush(&mut tokens, &mut pending, pending_line);
            tokens.extend(new);
        }
        line += newlines(&markup[..consumed]);
        rest = &markup[consumed..];
    }
    flush(&mut tokens, &mut pending, pending_line);
    Ok(tokens)
}

/// The index of the `>` ending the start tag at the beginning of `markup`,
/// skipping quoted attribute values, and whether it's self-closing (`/>`).
/// `None` if a `<` or the end of the text comes first.
fn tag_end(markup: &str) -> Option<(usize, bool)> {
    let mut quote = None;
    for (index, c) in markup.char_indices().skip(1) {
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '<') => return None,
            (None, '>') => return Some((index, markup[..index].ends_with('/'))),
            (None, _) => {}
        }
    }
    None
}

fn newlines(text: &str) -> u64 {
    text.bytes().filter(|&b| b == b'\n').count() as u64
}

/// Appends `raw` to `out`, decoding `&amp;`, `&lt;`, `&gt;`, `&quot;`,
/// `&apos;` and numeric references. Anything else after a `&` is kept as
/// written: SGML files often have a bare `&` ("M&S").
fn decode_entities(raw: &str, out: &mut String) {
    let mut rest = raw;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let decoded = after.find(';').filter(|&end| end <= 10).and_then(|end| {
            let entity = &after[..end];
            let c = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => {
                    let number = entity.strip_prefix('#')?;
                    let code = match number.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => number.parse().ok()?,
                    };
                    char::from_u32(code).filter(|&c| c != '\0')
                }
            }?;
            Some((c, end + 1))
        });
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &after[len..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
}

fn is_blank(text: &str) -> bool {
    text.trim().is_empty()
}

/// Builds the element tree from the tokens. See the module documentation
/// for how values and aggregates are told apart.
fn build(tokens: &[Token]) -> Result<Element, ImportError> {
    let closed: HashSet<&str> = tokens
        .iter()
        .filter_map(|token| match token {
            Token::End { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let opens_aggregate = |name: &str| {
        AGGREGATES.contains(&name) || (!VALUES.contains(&name) && closed.contains(name))
    };

    let mut stack: Vec<Element> = Vec::new();
    let mut roots: Vec<Element> = Vec::new();
    let attach = |stack: &mut Vec<Element>, roots: &mut Vec<Element>, element: Element| match stack
        .last_mut()
    {
        Some(Element {
            content: Content::Children(children),
            ..
        }) => children.push(element),
        _ => roots.push(element),
    };
    let is_end_of = |token: Option<&Token>, name: &str| matches!(token, Some(Token::End { name: end, .. }) if end == name);

    let mut i = 0;
    while let Some(token) = tokens.get(i) {
        i += 1;
        match token {
            Token::Start { name, line } => {
                let value = |text: &str| Element {
                    name: name.clone(),
                    line: *line,
                    content: Content::Value(text.trim().to_owned()),
                };
                match tokens.get(i) {
                    // `<NAME>text`, closed or not.
                    Some(Token::Text { text, .. }) if !is_blank(text) => {
                        if AGGREGATES.contains(&name.as_str()) {
                            return Err(malformed(*line, "an aggregate element holds text"));
                        }
                        i += 1;
                        if is_end_of(tokens.get(i), name) {
                            i += 1;
                        }
                        attach(&mut stack, &mut roots, value(text));
                        continue;
                    }
                    _ => {}
                }
                // Skip blank text to see what follows.
                let next = match tokens.get(i) {
                    Some(Token::Text { .. }) => i + 1,
                    _ => i,
                };
                if is_end_of(tokens.get(next), name) {
                    // `<MEMO></MEMO>`, or an empty aggregate.
                    i = next + 1;
                    let element = if AGGREGATES.contains(&name.as_str()) {
                        Element {
                            name: name.clone(),
                            line: *line,
                            content: Content::Children(Vec::new()),
                        }
                    } else {
                        value("")
                    };
                    attach(&mut stack, &mut roots, element);
                } else if opens_aggregate(name) {
                    stack.push(Element {
                        name: name.clone(),
                        line: *line,
                        content: Content::Children(Vec::new()),
                    });
                } else {
                    // `<MEMO>` straight before the next tag (SGML).
                    attach(&mut stack, &mut roots, value(""));
                }
            }
            Token::End { name, line } => match stack.last() {
                Some(open) if open.name == *name => {
                    let element = stack.pop().expect("the stack has an element");
                    attach(&mut stack, &mut roots, element);
                }
                _ if stack.iter().any(|open| open.name == *name) => {
                    return Err(malformed(*line, "an element inside this one isn't closed"));
                }
                _ if AGGREGATES.contains(&name.as_str()) => {
                    return Err(malformed(*line, "a closing tag has no opening tag"));
                }
                // A value's closing tag after it was read as empty, or an
                // unknown element's: nothing is open that it could close.
                _ => {}
            },
            Token::Text { text, line } => {
                if !is_blank(text) {
                    let leading = &text[..text.len() - text.trim_start().len()];
                    return Err(malformed(
                        line + newlines(leading),
                        "text outside an element's value",
                    ));
                }
            }
        }
    }
    if let Some(open) = stack.last() {
        return Err(malformed(
            open.line,
            "the file ends before this element is closed",
        ));
    }

    let mut roots = roots.into_iter();
    match (roots.next(), roots.next()) {
        (
            Some(
                root @ Element {
                    content: Content::Children(_),
                    ..
                },
            ),
            None,
        ) if root.name == "OFX" => Ok(root),
        (Some(_), Some(second)) => Err(malformed(
            second.line,
            "the file has more than one top-level element",
        )),
        (Some(root), None) => Err(malformed(root.line, "the top-level element isn't <OFX>")),
        (None, _) => Err(ImportError::Malformed {
            format: SourceFormat::Ofx,
            location: None,
            reason: "the file has no <OFX> element",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tree as `NAME=value` and `NAME{…}`, for compact assertions.
    fn shape(element: &Element) -> String {
        match &element.content {
            Content::Value(value) => format!("{}={value}", element.name),
            Content::Children(children) => {
                let inner: Vec<_> = children.iter().map(shape).collect();
                format!("{}{{{}}}", element.name, inner.join(" "))
            }
        }
    }

    fn read_shape(text: &str) -> String {
        shape(&read(text, 1).unwrap())
    }

    fn error(text: &str) -> String {
        read(text, 1).unwrap_err().to_string()
    }

    #[test]
    fn sgml_values_are_unclosed() {
        let text = "<OFX>\n<STMTTRN>\n<TRNTYPE>DEBIT\n<TRNAMT>-12.40\n<NAME>Corner Shop\n</STMTTRN>\n</OFX>\n";
        assert_eq!(
            read_shape(text),
            "OFX{STMTTRN{TRNTYPE=DEBIT TRNAMT=-12.40 NAME=Corner Shop}}"
        );
    }

    #[test]
    fn xml_values_are_closed() {
        let text = "<?xml version=\"1.0\"?><?OFX OFXHEADER=\"200\"?>\
                    <OFX><STMTTRN><TRNAMT>-12.40</TRNAMT><NAME>Corner Shop</NAME></STMTTRN></OFX>";
        assert_eq!(
            read_shape(text),
            "OFX{STMTTRN{TRNAMT=-12.40 NAME=Corner Shop}}"
        );
    }

    #[test]
    fn some_sgml_files_close_some_values() {
        let text =
            "<OFX><STMTTRN><TRNAMT>-1.00</TRNAMT>\n<NAME>A\n<MEMO>B</MEMO>\n</STMTTRN></OFX>";
        assert_eq!(read_shape(text), "OFX{STMTTRN{TRNAMT=-1.00 NAME=A MEMO=B}}");
    }

    #[test]
    fn empty_values() {
        // SGML: an empty value straight before the next tag, even when the
        // same name is closed elsewhere in the file.
        let text = "<OFX><STMTTRN><NAME>\n<MEMO>B</MEMO></STMTTRN><STMTTRN><NAME>X</NAME><MEMO>\n</STMTTRN></OFX>";
        assert_eq!(
            read_shape(text),
            "OFX{STMTTRN{NAME= MEMO=B} STMTTRN{NAME=X MEMO=}}"
        );
        // XML: closed straight away, with or without blank text, or
        // self-closing.
        let text = "<OFX><STMTTRN><NAME></NAME><MEMO> </MEMO><SIC/></STMTTRN></OFX>";
        assert_eq!(read_shape(text), "OFX{STMTTRN{NAME= MEMO= SIC=}}");
    }

    #[test]
    fn unknown_elements_are_aggregates_only_if_closed() {
        // Intuit's values are unclosed in SGML; an unknown aggregate is
        // closed.
        let text = "<OFX><SONRS><INTU.BID>00000<X.NEW><X.ITEM>1</X.NEW><X.EMPTY>\n</SONRS></OFX>";
        assert_eq!(
            read_shape(text),
            "OFX{SONRS{INTU.BID=00000 X.NEW{X.ITEM=1} X.EMPTY=}}"
        );
    }

    #[test]
    fn values_are_trimmed_and_entities_decoded() {
        let text = "<OFX><NAME>  M&amp;S &lt;Food&gt; &#163;&#x20AC; &quot;x&apos; \r\n<MEMO>Tom & Jerry &nbsp; &#0; &</OFX>";
        assert_eq!(
            read_shape(text),
            "OFX{NAME=M&S <Food> £€ \"x' MEMO=Tom & Jerry &nbsp; &#0; &}"
        );
    }

    #[test]
    fn comments_cdata_and_stray_angle_brackets() {
        let text = "<!DOCTYPE ofx><OFX><!-- a <comment> --><NAME><![CDATA[A &amp; <B>]]></NAME><MEMO>1 < 2</OFX>";
        assert_eq!(read_shape(text), "OFX{NAME=A &amp; <B> MEMO=1 < 2}");
    }

    #[test]
    fn tag_names_are_uppercased_and_attributes_ignored() {
        let text = "<ofx><stmttrn kind=\"a>b\"><name>A</NAME></Stmttrn></OFX>";
        assert_eq!(read_shape(text), "OFX{STMTTRN{NAME=A}}");
    }

    #[test]
    fn structure_errors_name_the_line() {
        assert_eq!(
            error("<OFX>\n<STMTTRN>\n<NAME>A\n</OFX>"),
            "the file isn't a readable ofx file (line 4): an element inside this one isn't closed"
        );
        assert_eq!(
            error("<OFX>\n<STMTTRN>\n<NAME>A\n"),
            "the file isn't a readable ofx file (line 2): the file ends before this element is closed"
        );
        assert_eq!(
            error("<OFX>\n</STMTTRN>\n</OFX>"),
            "the file isn't a readable ofx file (line 2): a closing tag has no opening tag"
        );
        assert_eq!(
            error("<OFX>\n<STMTTRN>secret\n</STMTTRN></OFX>"),
            "the file isn't a readable ofx file (line 2): an aggregate element holds text"
        );
        assert_eq!(
            error("<OFX>\n<STMTTRN><NAME>A</NAME>\nsecret</STMTTRN></OFX>"),
            "the file isn't a readable ofx file (line 3): text outside an element's value"
        );
        assert_eq!(
            error("<OFX>\n<NAME>A\n<MEMO"),
            "the file isn't a readable ofx file (line 3): a tag isn't closed with '>'"
        );
        assert_eq!(
            error("<OFX></OFX>\n<OFX></OFX>"),
            "the file isn't a readable ofx file (line 2): the file has more than one top-level element"
        );
        assert_eq!(
            error("<FOO></FOO>"),
            "the file isn't a readable ofx file (line 1): the top-level element isn't <OFX>"
        );
        assert_eq!(
            error(""),
            "the file isn't a readable ofx file: the file has no <OFX> element"
        );
    }

    #[test]
    fn empty_aggregates() {
        assert_eq!(
            read_shape("<OFX><BALLIST></BALLIST></OFX>"),
            "OFX{BALLIST{}}"
        );
        assert_eq!(read_shape("<OFX></OFX>"), "OFX{}");
    }

    #[test]
    fn a_stray_closing_tag_of_a_value_is_ignored() {
        assert_eq!(read_shape("<OFX><MEMO>\n</MEMO></OFX>"), "OFX{MEMO=}");
        assert_eq!(
            read_shape("<OFX><NAME>A\n</X.UNKNOWN></OFX>"),
            "OFX{NAME=A}"
        );
    }

    #[test]
    fn lines_count_from_the_first_line() {
        let root = read("<OFX>\r\n<STMTTRN>\r\n<NAME>A\r\n</STMTTRN>\r\n</OFX>", 10).unwrap();
        assert_eq!(root.line, 10);
        let transaction = &root.children()[0];
        assert_eq!(transaction.line, 11);
        assert_eq!(transaction.children()[0].line, 12);
    }

    #[test]
    fn debug_leaves_values_out() {
        let root = read("<OFX><NAME>Secret Merchant</OFX>", 1).unwrap();
        let debug = format!("{root:?}");
        assert!(!debug.contains("Secret"), "{debug}");
        assert!(debug.contains("value_len: 15"), "{debug}");
    }
}
