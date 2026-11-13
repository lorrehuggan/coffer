//! Checks for `pseudonymous` output (ROADMAP M2.11, M2.13, M2.5): no
//! merchant display name may reach a client at that level, in any field.
//!
//! - [`display_merchants`]: the fixture database's display names, the
//!   strings to look for.
//! - [`assert_no_display_merchants`]: fails if any of them appears in a
//!   response.
//! - [`assert_pseudonymous_ok`]: that, plus [`assert_no_pii`]; run it on
//!   every `pseudonymous` response.
//! - [`is_merchant_token`]: whether a string is a merchant token
//!   (`M-` and 12 lowercase hex digits).

use serde_json::Value;

use super::{Fixture, assert_no_pii, visit_strings};

/// Display names shorter than this, in characters, aren't looked for: a
/// two-letter name would match ordinary words and field names.
pub const MIN_CHECKED_NAME_CHARS: usize = 3;

/// The distinct non-empty `merchant_display` values in the fixture
/// database, trimmed, sorted. Read straight from the file, since core has
/// no query for them yet.
pub fn display_merchants(fixture: &Fixture) -> Vec<String> {
    let conn = rusqlite::Connection::open(fixture.path()).unwrap();
    let mut statement = conn
        .prepare(
            "SELECT DISTINCT trim(merchant_display) FROM \"transaction\" \
             WHERE merchant_display IS NOT NULL AND trim(merchant_display) <> '' \
             ORDER BY 1",
        )
        .unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Fails if a display name from the fixture database appears in any string
/// of `value`, keys included.
///
/// A name matches ignoring case, as a whole token: the characters either
/// side of it (if any) aren't letters or digits, so `Bean There` is found
/// in `BEAN THERE TESTVILLE` but `Tesco` isn't found in `Tescobar`, and a
/// merchant token's hex can't spell a name by accident. Names shorter than
/// [`MIN_CHECKED_NAME_CHARS`] are skipped. A display name that is also an
/// ordinary word in Coffer's own output (`Description`, `Instructions`)
/// would be a false positive; the fixtures have none.
pub fn assert_no_display_merchants(fixture: &Fixture, value: &Value) {
    let names: Vec<String> = display_merchants(fixture)
        .into_iter()
        .filter(|name| name.chars().count() >= MIN_CHECKED_NAME_CHARS)
        .map(|name| name.to_lowercase())
        .collect();
    assert!(
        !names.is_empty(),
        "the fixture database has no display names to look for"
    );
    let mut found = Vec::new();
    visit_strings(value, &mut |text| {
        let lower = text.to_lowercase();
        for name in &names {
            if contains_token(&lower, name) {
                found.push(format!("{name:?} in {text:?}"));
            }
        }
    });
    assert!(
        found.is_empty(),
        "merchant names in pseudonymous output: {found:#?}"
    );
}

/// [`assert_no_pii`] and [`assert_no_display_merchants`]: what every
/// `pseudonymous` response must pass.
pub fn assert_pseudonymous_ok(fixture: &Fixture, value: &Value) {
    assert_no_pii(value);
    assert_no_display_merchants(fixture, value);
}

/// Whether `text` is a merchant token: `M-` and 12 lowercase hex digits.
pub fn is_merchant_token(text: &str) -> bool {
    text.strip_prefix("M-").is_some_and(|hex| {
        hex.len() == 12
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

/// Whether `needle` occurs in `haystack` with no letter or digit right
/// before or after it.
fn contains_token(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(start, _)| {
        let before = haystack[..start].chars().next_back();
        let after = haystack[start + needle.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}
