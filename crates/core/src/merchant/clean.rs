//! The deterministic clean-up of bank merchant text ([`clean`]).

use std::sync::LazyLock;

use regex::Regex;

use crate::rule::collapse_whitespace;

/// Bank text cleaned into a merchant name.
///
/// Removes the noise card payments add around a merchant's name, and keeps
/// everything else as the bank wrote it:
///
/// - **processor prefixes** at the start: `SQ *`, `SUMUP *`, `ZETTLE_*`,
///   `IZ *`, `PAYPAL *`, `PP*`, `CRV*`, `TST*`, `SP *` and `SP ` (the space
///   or `*` after the name is what marks a prefix, so `SPAR` stays);
/// - **payment-method words** at the start or end: `CONTACTLESS`,
///   `CONTACTLESS PAYMENT`, `CARD PAYMENT` (`TO`), `CARD PURCHASE`,
///   `DEBIT CARD`, `APPLE PAY`, `GOOGLE PAY`, `CHIP AND PIN`, and at the
///   start only `POS` and `VIS`;
/// - **card references** anywhere: `CARD 1234`, `CARD ENDING 1234`,
///   `CARD NO. XXXX1234`, and masked card numbers (`XXXX XXXX XXXX 1234`,
///   `****1234`, `4929********1234`);
/// - **trailing dates, times and references**, after the first word only:
///   `12/11`, `12/11/26`, `2026-11-12`, `12NOV`, `12 NOV 2026`, each
///   optionally after `ON`; `10:42`; `REF 123456`; a run of four or more
///   digits (`#1234` too: store numbers, phone numbers, references); a code
///   of six or more letters and digits with at least four digits; and a
///   `*` followed by an order code (`*AB12CD34`);
/// - separators left at either end (`*`, `-`, `,`, `:`, `;`, `/`, `#`,
///   `_`, `|`), and runs of whitespace, which collapse to one space.
///
/// It repeats these until nothing changes, so cleaning a cleaned name
/// returns it unchanged. If nothing would be left (`CONTACTLESS` alone,
/// `XXXX1234`), the result is the input with its whitespace collapsed:
/// non-blank input never gives an empty name. Blank input gives `""`.
///
/// **Case is kept as the bank wrote it.** Title-casing would get many names
/// wrong (`McDonald's`, `IKEA`, `eBay`) and can't be undone, and every
/// comparison that uses the cleaned text (aliases, rules) ignores case
/// anyway. A user who wants another spelling adds a merchant name (alias).
///
/// The input is untrusted bank text: it is only ever transformed, never
/// interpreted, and never logged.
///
/// ```
/// use coffer_core::merchant::clean;
///
/// assert_eq!(clean("SQ *ACME COFFEE"), "ACME COFFEE");
/// assert_eq!(clean("CONTACTLESS  Acme Bakery 12/11"), "Acme Bakery");
/// assert_eq!(clean("ACME STORES 3297 CARD 1234"), "ACME STORES");
/// assert_eq!(clean("CONTACTLESS"), "CONTACTLESS");
/// ```
pub fn clean(text: &str) -> String {
    let original = collapse_whitespace(text);
    let mut current = original.clone();
    loop {
        let next = step(&current);
        // Every change removes text, so this ends.
        if next == current {
            break;
        }
        current = next;
    }
    if current.is_empty() {
        original
    } else {
        current
    }
}

/// One pass of every clean-up, on whitespace-collapsed text.
fn step(text: &str) -> String {
    let mut text = PROCESSOR_PREFIX.replace(text, "").into_owned();
    text = LEADING_PAYMENT.replace(&text, "").into_owned();
    text = TRAILING_PAYMENT.replace(&text, "").into_owned();
    text = CARD_REFERENCE.replace_all(&text, " ").into_owned();
    text = remove_masked_numbers(&text);
    text = strip_trailing_noise(&text);
    collapse_whitespace(text.trim_matches(is_edge_separator))
}

fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("a built-in clean-up pattern compiles")
}

/// A payment processor's prefix before the merchant's own name.
static PROCESSOR_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    regex(r"(?i)^(?:(?:SQ|SQU|SUMUP|IZ|ZETTLE|PAYPAL|PP|CRV|TST|SP)\s*\*\s*|ZETTLE_\*?\s*|SP\s+)")
});

/// Payment-method words, as the alternatives of a regex group.
const PAYMENT_WORDS: &str = r"CONTACTLESS(?:\s+PAYMENT)?|CARD\s+(?:PAYMENT|PURCHASE)|DEBIT\s+CARD(?:\s+(?:PAYMENT|PURCHASE))?|APPLE\s+PAY|GOOGLE\s+PAY|CHIP\s+AND\s+PIN";

/// Payment-method words at the start, followed by a separator (so a word
/// that merely starts with one, like `POSTAGE`, stays).
static LEADING_PAYMENT: LazyLock<Regex> = LazyLock::new(|| {
    regex(&format!(
        r"(?i)^(?:CARD\s+PAYMENT\s+TO|{PAYMENT_WORDS}|POS|VIS)(?:\s*[:,-]\s*|\s+)"
    ))
});

/// Payment-method words at the end, after a separator.
static TRAILING_PAYMENT: LazyLock<Regex> =
    LazyLock::new(|| regex(&format!(r"(?i)(?:\s*[,-]\s*|\s+)(?:{PAYMENT_WORDS})$")));

/// `CARD 1234`, `CARD ENDING IN 1234`, `CARD NO. XXXX1234`: a card named
/// by its last four digits.
static CARD_REFERENCE: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"(?i)\bCARD(?:\s+(?:NO\.?|NUMBER|ENDING(?:\s+IN)?))?\s*[:#]?\s*(?:[X*]+[\s-]?)*[0-9]{4}\b",
    )
});

/// Month names and their abbreviations, longest first.
const MONTHS: &str = "JANUARY|JAN|FEBRUARY|FEB|MARCH|MAR|APRIL|APR|MAY|JUNE|JUN|JULY|JUL|AUGUST|AUG|SEPTEMBER|SEPT|SEP|OCTOBER|OCT|NOVEMBER|NOV|DECEMBER|DEC";

/// A trailing date or time, optionally after `ON`. A two-part numeric date
/// needs two digits each (`12/11`, not `24/7`).
static TRAILING_DATE: LazyLock<Regex> = LazyLock::new(|| {
    regex(&format!(
        r"(?i)\s+(?:ON\s+)?(?:[0-9]{{2}}[/.-][0-9]{{2}}|[0-9]{{1,2}}[/.-][0-9]{{1,2}}[/.-](?:[0-9]{{4}}|[0-9]{{2}})|[0-9]{{4}}-[0-9]{{2}}-[0-9]{{2}}|[0-9]{{1,2}}\s*(?:{MONTHS})(?:\s*(?:[0-9]{{4}}|[0-9]{{2}}))?|[0-9]{{1,2}}:[0-9]{{2}}(?::[0-9]{{2}})?)$"
    ))
});

/// A trailing reference: `REF 123456`, `REF: AB1234`. It must contain a
/// digit.
static TRAILING_REFERENCE: LazyLock<Regex> =
    LazyLock::new(|| regex(r"(?i)\s+REF(?:ERENCE)?(?:[.:#]\s*|\s+)\S*[0-9]\S*$"));

/// A trailing run of four or more digits, optionally after `#`.
static TRAILING_DIGITS: LazyLock<Regex> = LazyLock::new(|| regex(r"\s+#?[0-9]{4,}$"));

/// The trailing date, time, reference, digit run or code, after the first
/// word (each needs whitespace before it, so the first word always stays).
fn strip_trailing_noise(text: &str) -> String {
    let mut text = TRAILING_REFERENCE.replace(text, "").into_owned();
    text = TRAILING_DATE.replace(&text, "").into_owned();
    text = TRAILING_DIGITS.replace(&text, "").into_owned();

    // A code like `AB123456` as the last word.
    if let Some((head, last)) = text.rsplit_once(' ')
        && is_code(last, 4)
    {
        text = head.to_owned();
    }
    // An order code after `*` (`ACME.CO.UK*AB12CD34`).
    if let Some((head, code)) = text.rsplit_once('*')
        && !head.trim().is_empty()
        && is_code(code, 2)
    {
        text = head.to_owned();
    }
    text
}

/// Six or more ASCII letters and digits, at least `min_digits` of them
/// digits.
fn is_code(word: &str, min_digits: usize) -> bool {
    word.len() >= 6
        && word.chars().all(|c| c.is_ascii_alphanumeric())
        && word.chars().filter(char::is_ascii_digit).count() >= min_digits
}

/// Removes masked card numbers: words of digits and mask characters (`X`,
/// `*`, `•`) with at least two mask characters and either a digit or four
/// mask characters (`XXXX`, `****1234`, `4929********1234`), and the four
/// digits that follow one (`XXXX XXXX XXXX 1234`).
fn remove_masked_numbers(text: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut after_mask = false;
    for word in text.split(' ') {
        if is_masked(word) {
            after_mask = true;
            continue;
        }
        if after_mask && word.len() == 4 && word.chars().all(|c| c.is_ascii_digit()) {
            after_mask = false;
            continue;
        }
        after_mask = false;
        kept.push(word);
    }
    kept.join(" ")
}

fn is_masked(word: &str) -> bool {
    let is_mask = |c: char| matches!(c, 'X' | 'x' | '*' | '•');
    let masks = word.chars().filter(|c| is_mask(*c)).count();
    let digits = word.chars().filter(char::is_ascii_digit).count();
    word.chars().all(|c| is_mask(c) || c.is_ascii_digit())
        && masks >= 2
        && (digits > 0 || masks >= 4)
}

fn is_edge_separator(c: char) -> bool {
    c.is_whitespace() || matches!(c, '*' | '-' | ',' | ':' | ';' | '/' | '#' | '_' | '|')
}
