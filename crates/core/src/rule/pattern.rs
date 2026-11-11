//! A rule's pattern, validated for its match kind.

use std::fmt;

use regex::{Regex, RegexBuilder};

use super::MatchKind;

/// The longest pattern accepted, in bytes, for any match kind.
///
/// Rules match short bank text; anything longer is a mistake or an attack,
/// and for `regex` it is refused before it is parsed.
pub const MAX_PATTERN_LEN: usize = 1_000;

/// The most memory a compiled `regex` pattern may take, in bytes.
///
/// The `regex` crate matches in linear time, but a small pattern can still
/// compile to a large program (`\w{1000}`, nested repetition). Past this
/// limit the pattern is refused rather than compiled.
pub const REGEX_SIZE_LIMIT: usize = 1 << 20;

/// How deeply a `regex` pattern may nest groups and repetitions.
pub const REGEX_NEST_LIMIT: u32 = 32;

/// A rule's pattern, checked for its [`MatchKind`].
///
/// Never empty and at most [`MAX_PATTERN_LEN`] bytes. A `regex` pattern
/// compiles case-insensitively (matching ignores case for every kind) within
/// [`REGEX_SIZE_LIMIT`] and [`REGEX_NEST_LIMIT`], and the compiled regex is
/// kept for matching. The text is stored as given: case folding for the
/// other kinds is the matcher's job.
///
/// `Debug` shows the kind and length only, since the text is user input.
#[derive(Clone)]
pub struct Pattern {
    kind: MatchKind,
    text: String,
    regex: Option<Regex>,
}

/// Why a pattern was refused.
///
/// The pattern itself is never part of the message: it is user (or agent)
/// text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PatternError {
    /// The pattern is empty.
    #[error("the pattern is empty")]
    Empty,

    /// The pattern is longer than [`MAX_PATTERN_LEN`].
    #[error("the pattern is longer than {max} bytes")]
    TooLong {
        /// The limit, [`MAX_PATTERN_LEN`].
        max: usize,
    },

    /// A `regex` pattern doesn't parse. `reason` is the parser's short
    /// description of the fault (`unclosed group`), without the pattern.
    #[error("the pattern isn't a valid regular expression: {reason}")]
    InvalidRegex {
        /// What is wrong, e.g. `unclosed group`.
        reason: String,
    },

    /// A `regex` pattern would compile past [`REGEX_SIZE_LIMIT`].
    #[error("the pattern is too complex: simplify it or use fewer repetitions")]
    RegexTooBig,
}

impl Pattern {
    /// Checks `text` as a pattern of `kind`.
    pub fn new(kind: MatchKind, text: &str) -> Result<Pattern, PatternError> {
        if text.is_empty() {
            return Err(PatternError::Empty);
        }
        if text.len() > MAX_PATTERN_LEN {
            return Err(PatternError::TooLong {
                max: MAX_PATTERN_LEN,
            });
        }
        let regex = match kind {
            MatchKind::Regex => Some(compile(text)?),
            MatchKind::Is | MatchKind::StartsWith | MatchKind::Contains => None,
        };
        Ok(Pattern {
            kind,
            text: text.to_owned(),
            regex,
        })
    }

    /// How the pattern is tested.
    pub const fn kind(&self) -> MatchKind {
        self.kind
    }

    /// The pattern as stored in `rule.pattern`.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The compiled, case-insensitive regex of a `regex` pattern; `None` for
    /// the other kinds.
    pub fn regex(&self) -> Option<&Regex> {
        self.regex.as_ref()
    }
}

fn compile(text: &str) -> Result<Regex, PatternError> {
    RegexBuilder::new(text)
        .case_insensitive(true)
        .size_limit(REGEX_SIZE_LIMIT)
        .dfa_size_limit(REGEX_SIZE_LIMIT)
        .nest_limit(REGEX_NEST_LIMIT)
        .build()
        .map_err(|err| match err {
            regex::Error::CompiledTooBig(_) => PatternError::RegexTooBig,
            regex::Error::Syntax(message) => PatternError::InvalidRegex {
                reason: syntax_reason(&message),
            },
            _ => PatternError::InvalidRegex {
                reason: "it can't be compiled".to_owned(),
            },
        })
}

/// The `error: …` line of a `regex` syntax error. The full message repeats
/// the pattern with a caret under the fault, which mustn't reach a log.
fn syntax_reason(message: &str) -> String {
    message
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix("error: "))
        .unwrap_or("it doesn't parse")
        .to_owned()
}

impl PartialEq for Pattern {
    fn eq(&self, other: &Pattern) -> bool {
        self.kind == other.kind && self.text == other.text
    }
}

impl Eq for Pattern {}

impl fmt::Debug for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pattern")
            .field("kind", &self.kind)
            .field("len", &self.text.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_kinds_keep_the_text_as_given() {
        for kind in [MatchKind::Is, MatchKind::StartsWith, MatchKind::Contains] {
            for text in ["UBER", "Tesco Stores 1234", "(", "a*b", " ", "café"] {
                let pattern = Pattern::new(kind, text).unwrap();
                assert_eq!((pattern.kind(), pattern.as_str()), (kind, text));
                assert!(pattern.regex().is_none());
            }
        }
    }

    #[test]
    fn regex_compiles_case_insensitively() {
        let pattern = Pattern::new(MatchKind::Regex, r"^tfl\s+travel").unwrap();
        assert_eq!(pattern.as_str(), r"^tfl\s+travel");
        let regex = pattern.regex().unwrap();
        assert!(regex.is_match("TFL TRAVEL CH"));
        assert!(regex.is_match("Tfl  travel"));
        assert!(!regex.is_match("OYSTER TFL TRAVEL"));
    }

    #[test]
    fn empty_is_refused_for_every_kind() {
        for kind in MatchKind::ALL {
            assert_eq!(Pattern::new(*kind, ""), Err(PatternError::Empty));
        }
    }

    #[test]
    fn too_long_is_refused_for_every_kind() {
        let longest = "a".repeat(MAX_PATTERN_LEN);
        let too_long = "a".repeat(MAX_PATTERN_LEN + 1);
        for kind in MatchKind::ALL {
            assert!(Pattern::new(*kind, &longest).is_ok());
            assert_eq!(
                Pattern::new(*kind, &too_long),
                Err(PatternError::TooLong {
                    max: MAX_PATTERN_LEN
                })
            );
        }
    }

    #[test]
    fn invalid_regex_is_refused_without_echoing_it() {
        for text in ["(", "SECRET[", "a{2,1}", r"\q", "*SECRET"] {
            let err = Pattern::new(MatchKind::Regex, text).unwrap_err();
            let PatternError::InvalidRegex { reason } = &err else {
                panic!("{text:?}: {err:?}");
            };
            assert!(!reason.is_empty() && !reason.contains('\n'), "{reason:?}");
            assert!(!err.to_string().contains("SECRET"), "{err}");
        }
        assert_eq!(
            Pattern::new(MatchKind::Regex, "("),
            Err(PatternError::InvalidRegex {
                reason: "unclosed group".to_owned()
            })
        );
    }

    #[test]
    fn huge_regex_is_refused_not_compiled() {
        // Short text, but each repetition multiplies the compiled program.
        for text in [r"\w{1000}", r"(\w{50}){50}", r"[\p{L}\p{N}]{500}"] {
            assert_eq!(
                Pattern::new(MatchKind::Regex, text),
                Err(PatternError::RegexTooBig),
                "{text}"
            );
        }
    }

    #[test]
    fn deep_nesting_is_refused() {
        let deep = format!("{}a{}", "(".repeat(40), ")".repeat(40));
        assert!(matches!(
            Pattern::new(MatchKind::Regex, &deep),
            Err(PatternError::InvalidRegex { .. })
        ));
        let shallow = format!("{}a{}", "(".repeat(10), ")".repeat(10));
        assert!(Pattern::new(MatchKind::Regex, &shallow).is_ok());
    }

    #[test]
    fn equality_is_kind_and_text() {
        let a = Pattern::new(MatchKind::Contains, "uber").unwrap();
        assert_eq!(a, Pattern::new(MatchKind::Contains, "uber").unwrap());
        assert_ne!(a, Pattern::new(MatchKind::Is, "uber").unwrap());
        assert_ne!(a, Pattern::new(MatchKind::Contains, "UBER").unwrap());
    }

    #[test]
    fn debug_hides_the_text() {
        let pattern = Pattern::new(MatchKind::Regex, "SECRET").unwrap();
        assert_eq!(format!("{pattern:?}"), "Pattern { kind: Regex, len: 6 }");
    }
}
