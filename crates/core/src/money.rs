//! Money: integer minor units and an ISO 4217 currency, never floats.
//!
//! An amount is a signed count of its currency's minor units (pence, cents;
//! whole yen for `JPY`), the same `*_minor` integer the database stores next
//! to a `currency` code. Negative is money out of an account.
//!
//! - Arithmetic is checked: adding two currencies or overflowing `i64` is
//!   an error, never a panic or a wrapped value. Conversion between
//!   currencies happens only in reporting, with a stored rate (M4.6).
//! - [`Money::parse_decimal`] is the import boundary: bank text becomes
//!   minor units exactly or not at all. It never rounds, so an amount with
//!   more decimals than its currency has is refused.
//!
//! Display formatting for the app (currency symbols, the locale's
//! separators, `−` and `+`) belongs to the UI; [`Money::to_decimal_string`]
//! is the plain, locale-free form for exports and the CLI.

mod currency;

use std::cmp::Ordering;
use std::fmt;

pub use currency::{Currency, UnknownCurrency};

/// An amount of money in one currency.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Money {
    minor: i64,
    currency: Currency,
}

/// Why an arithmetic operation on [`Money`] failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MoneyError {
    #[error("can't combine {left} and {right} amounts without an exchange rate")]
    CurrencyMismatch { left: Currency, right: Currency },

    #[error("the amount is too large to represent")]
    Overflow,
}

/// How an amount in bank text separates its decimals: `1,234.56`
/// ([`Point`](DecimalMark::Point)) or `1.234,56` ([`Comma`](DecimalMark::Comma)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DecimalMark {
    /// `1234.56`; thousands may be grouped with `,`.
    #[serde(rename = ".")]
    Point,
    /// `1234,56`; thousands may be grouped with `.`.
    #[serde(rename = ",")]
    Comma,
}

impl DecimalMark {
    fn mark(self) -> char {
        match self {
            DecimalMark::Point => '.',
            DecimalMark::Comma => ',',
        }
    }

    /// Characters accepted between groups of thousands. Spaces and the
    /// apostrophe are unambiguous in both styles (`1 234,56`, `1'234.56`).
    fn is_group_separator(self, c: char) -> bool {
        let other = match self {
            DecimalMark::Point => ',',
            DecimalMark::Comma => '.',
        };
        c == other || matches!(c, ' ' | '\u{A0}' | '\u{202F}' | '\'')
    }
}

/// Why bank text couldn't be read as an amount.
///
/// The text itself is left out: it's transaction data and errors get logged.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseAmountError {
    #[error("the amount is empty")]
    Empty,

    #[error("the amount isn't a number: {0}")]
    Malformed(&'static str),

    #[error(
        "the amount has more decimal places than {currency} has ({}); \
         Coffer doesn't round imported amounts",
        currency.minor_units()
    )]
    TooPrecise { currency: Currency },

    #[error("the amount is too large to represent")]
    Overflow,
}

impl Money {
    /// `minor` units of `currency`: `Money::new(-1240, Currency::GBP)` is −£12.40.
    pub const fn new(minor: i64, currency: Currency) -> Money {
        Money { minor, currency }
    }

    /// Nothing, in `currency`.
    pub const fn zero(currency: Currency) -> Money {
        Money::new(0, currency)
    }

    /// The amount in minor units, as stored in a `*_minor` column.
    pub const fn minor(&self) -> i64 {
        self.minor
    }

    pub const fn currency(&self) -> Currency {
        self.currency
    }

    pub const fn is_zero(&self) -> bool {
        self.minor == 0
    }

    /// Below zero: money out of an account, or a debt.
    pub const fn is_negative(&self) -> bool {
        self.minor < 0
    }

    pub const fn is_positive(&self) -> bool {
        self.minor > 0
    }

    fn same_currency(&self, other: &Money) -> Result<(), MoneyError> {
        if self.currency == other.currency {
            Ok(())
        } else {
            Err(MoneyError::CurrencyMismatch {
                left: self.currency,
                right: other.currency,
            })
        }
    }

    fn with_minor(&self, minor: Option<i64>) -> Result<Money, MoneyError> {
        minor
            .map(|minor| Money::new(minor, self.currency))
            .ok_or(MoneyError::Overflow)
    }

    pub fn checked_add(&self, other: Money) -> Result<Money, MoneyError> {
        self.same_currency(&other)?;
        self.with_minor(self.minor.checked_add(other.minor))
    }

    pub fn checked_sub(&self, other: Money) -> Result<Money, MoneyError> {
        self.same_currency(&other)?;
        self.with_minor(self.minor.checked_sub(other.minor))
    }

    /// The amount with its sign flipped.
    pub fn checked_neg(&self) -> Result<Money, MoneyError> {
        self.with_minor(self.minor.checked_neg())
    }

    /// The amount without its sign.
    pub fn checked_abs(&self) -> Result<Money, MoneyError> {
        self.with_minor(self.minor.checked_abs())
    }

    /// Orders two amounts in the same currency.
    pub fn checked_cmp(&self, other: &Money) -> Result<Ordering, MoneyError> {
        self.same_currency(other)?;
        Ok(self.minor.cmp(&other.minor))
    }

    /// The total of `amounts`, all in `currency`; zero when there are none.
    pub fn sum(
        currency: Currency,
        amounts: impl IntoIterator<Item = Money>,
    ) -> Result<Money, MoneyError> {
        amounts
            .into_iter()
            .try_fold(Money::zero(currency), |total, amount| {
                total.checked_add(amount)
            })
    }

    /// Reads an amount from bank text, exactly.
    ///
    /// Accepts digits with one decimal mark, thousands grouped by the other
    /// mark, a space or an apostrophe (`1,234.56`, `1.234,56`, `1 234,56`),
    /// and a sign: a leading `-`, `−` (U+2212) or `+`, or parentheses
    /// (`(12.34)` is −12.34). Surrounding whitespace is ignored. Fewer
    /// decimals than the currency has are fine (`12.5` GBP is 1250); more
    /// are allowed only when they are zeros, since anything else would mean
    /// rounding. Currency symbols and codes are the importer's to remove.
    pub fn parse_decimal(
        text: &str,
        currency: Currency,
        mark: DecimalMark,
    ) -> Result<Money, ParseAmountError> {
        use ParseAmountError::{Empty, Malformed, Overflow, TooPrecise};

        let text = text.trim();
        if text.is_empty() {
            return Err(Empty);
        }

        let (negative, unsigned) = if let Some(inner) = text
            .strip_prefix('(')
            .and_then(|rest| rest.strip_suffix(')'))
        {
            (true, inner)
        } else if let Some(rest) = text.strip_prefix(['-', '\u{2212}']) {
            (true, rest)
        } else if let Some(rest) = text.strip_prefix('+') {
            (false, rest)
        } else {
            (false, text)
        };

        let (whole, fraction) = match unsigned.split_once(mark.mark()) {
            Some((_, fraction)) if fraction.contains(mark.mark()) => {
                return Err(Malformed("more than one decimal mark"));
            }
            Some((_, "")) => return Err(Malformed("no digits after the decimal mark")),
            Some((whole, fraction)) => (whole, fraction),
            None => (unsigned, ""),
        };
        if whole.is_empty() && fraction.is_empty() {
            return Err(Malformed("no digits"));
        }
        if !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(Malformed("unexpected character after the decimal mark"));
        }
        let whole = whole_digits(whole, mark)?;

        let places = usize::from(currency.minor_units());
        let (kept, dropped) = fraction.split_at(fraction.len().min(places));
        if dropped.bytes().any(|byte| byte != b'0') {
            return Err(TooPrecise { currency });
        }

        // Accumulate in i128 so every digit string that fits i64 once
        // scaled, including i64::MIN, is read without intermediate overflow.
        let mut minor: i128 = 0;
        let padding = places - kept.len();
        let digits = whole.iter().copied().chain(kept.bytes());
        for digit in digits.chain(std::iter::repeat_n(b'0', padding)) {
            minor = minor
                .checked_mul(10)
                .and_then(|value| value.checked_add(i128::from(digit - b'0')))
                .ok_or(Overflow)?;
        }
        if negative {
            minor = -minor;
        }
        i64::try_from(minor)
            .map(|minor| Money::new(minor, currency))
            .map_err(|_| Overflow)
    }

    /// The amount as plain decimal text in major units, without grouping or
    /// currency: `-1234.56` for −£1,234.56, `-500` for −¥500. Reads back
    /// with [`Money::parse_decimal`] and [`DecimalMark::Point`].
    pub fn to_decimal_string(&self) -> String {
        let sign = if self.minor < 0 { "-" } else { "" };
        let magnitude = self.minor.unsigned_abs();
        let places = u32::from(self.currency.minor_units());
        if places == 0 {
            return format!("{sign}{magnitude}");
        }
        let scale = 10_u64.pow(places);
        let width = places as usize;
        format!("{sign}{}.{:0width$}", magnitude / scale, magnitude % scale)
    }
}

/// The digits of the whole part of an amount, with any thousands grouping
/// checked and removed: the first group has 1–3 digits, every later one 3,
/// all separated by the same character.
fn whole_digits(whole: &str, mark: DecimalMark) -> Result<Vec<u8>, ParseAmountError> {
    use ParseAmountError::Malformed;

    let mut separator = None;
    let mut groups = Vec::new();
    let mut start = 0;
    for (index, c) in whole.char_indices() {
        if c.is_ascii_digit() {
            continue;
        }
        if !mark.is_group_separator(c) {
            return Err(Malformed("unexpected character"));
        }
        if *separator.get_or_insert(c) != c {
            return Err(Malformed("mixed thousands separators"));
        }
        groups.push(&whole[start..index]);
        start = index + c.len_utf8();
    }
    groups.push(&whole[start..]);

    if separator.is_some() {
        let (first, rest) = groups.split_first().expect("split yields one group");
        if !(1..=3).contains(&first.len()) || rest.iter().any(|group| group.len() != 3) {
            return Err(Malformed("thousands separators in the wrong place"));
        }
    }
    Ok(groups.concat().into_bytes())
}

/// `-1234.56 GBP`: the plain decimal form and the code. For logs and the CLI;
/// the app formats amounts itself.
impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.to_decimal_string(), self.currency)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn currency(code: &str) -> Currency {
        Currency::new(code).unwrap()
    }

    fn gbp(minor: i64) -> Money {
        Money::new(minor, Currency::GBP)
    }

    fn parse(text: &str, code: &str, mark: DecimalMark) -> Result<i64, ParseAmountError> {
        Money::parse_decimal(text, currency(code), mark).map(|money| money.minor())
    }

    #[test]
    fn adds_and_subtracts_in_one_currency() {
        assert_eq!(gbp(1250).checked_add(gbp(-1240)), Ok(gbp(10)));
        assert_eq!(gbp(1250).checked_sub(gbp(1300)), Ok(gbp(-50)));
        assert_eq!(gbp(-645).checked_neg(), Ok(gbp(645)));
        assert_eq!(gbp(-645).checked_abs(), Ok(gbp(645)));
        assert_eq!(gbp(5).checked_cmp(&gbp(-5)), Ok(Ordering::Greater));
        assert!(gbp(-1).is_negative() && gbp(1).is_positive() && gbp(0).is_zero());
    }

    #[test]
    fn refuses_to_mix_currencies() {
        let mismatch = Err(MoneyError::CurrencyMismatch {
            left: Currency::GBP,
            right: Currency::EUR,
        });
        let euros = Money::new(100, Currency::EUR);
        assert_eq!(gbp(100).checked_add(euros), mismatch);
        assert_eq!(gbp(100).checked_sub(euros), mismatch);
        assert_eq!(
            gbp(100).checked_cmp(&euros),
            mismatch.map(|_| Ordering::Equal)
        );
        assert_eq!(Money::sum(Currency::GBP, [gbp(1), euros]), mismatch);
    }

    #[test]
    fn overflow_is_an_error_not_a_wrap() {
        let max = gbp(i64::MAX);
        let min = gbp(i64::MIN);
        assert_eq!(max.checked_add(gbp(1)), Err(MoneyError::Overflow));
        assert_eq!(min.checked_sub(gbp(1)), Err(MoneyError::Overflow));
        assert_eq!(min.checked_neg(), Err(MoneyError::Overflow));
        assert_eq!(min.checked_abs(), Err(MoneyError::Overflow));
        assert_eq!(
            Money::sum(Currency::GBP, [max, gbp(1)]),
            Err(MoneyError::Overflow)
        );
    }

    #[test]
    fn sums_amounts() {
        assert_eq!(Money::sum(Currency::GBP, []), Ok(gbp(0)));
        assert_eq!(
            Money::sum(Currency::GBP, [gbp(-3774), gbp(-6824), gbp(125_000)]),
            Ok(gbp(114_402))
        );
    }

    #[test]
    fn parses_plain_and_signed_amounts() {
        use DecimalMark::Point;
        assert_eq!(parse("12.34", "GBP", Point), Ok(1234));
        assert_eq!(parse("-12.34", "GBP", Point), Ok(-1234));
        assert_eq!(parse("\u{2212}12.34", "GBP", Point), Ok(-1234));
        assert_eq!(parse("+12.34", "GBP", Point), Ok(1234));
        assert_eq!(parse("(12.34)", "GBP", Point), Ok(-1234));
        assert_eq!(parse("  -0.05\t", "GBP", Point), Ok(-5));
        assert_eq!(parse("-0", "GBP", Point), Ok(0));
        assert_eq!(parse("12", "GBP", Point), Ok(1200));
        assert_eq!(parse("12.5", "GBP", Point), Ok(1250));
        assert_eq!(parse(".5", "GBP", Point), Ok(50));
        assert_eq!(parse("007.10", "GBP", Point), Ok(710));
    }

    #[test]
    fn parses_thousands_in_both_styles() {
        use DecimalMark::{Comma, Point};
        assert_eq!(parse("1,234.56", "GBP", Point), Ok(123_456));
        assert_eq!(parse("1,234,567.89", "USD", Point), Ok(123_456_789));
        assert_eq!(parse("1.234,56", "EUR", Comma), Ok(123_456));
        assert_eq!(parse("-1.234.567,89", "EUR", Comma), Ok(-123_456_789));
        assert_eq!(parse("1234,56", "EUR", Comma), Ok(123_456));
        assert_eq!(parse("1 234,56", "EUR", Comma), Ok(123_456));
        assert_eq!(parse("1\u{A0}234,56", "EUR", Comma), Ok(123_456));
        assert_eq!(parse("1\u{202F}234,56", "EUR", Comma), Ok(123_456));
        assert_eq!(parse("1'234.56", "CHF", Point), Ok(123_456));
        // With a point mark, a lone comma is grouping, not decimals.
        assert_eq!(parse("1,234", "GBP", Point), Ok(123_400));
        assert_eq!(parse("1.234", "EUR", Comma), Ok(123_400));
    }

    #[test]
    fn uses_each_currencys_minor_units() {
        use DecimalMark::Point;
        assert_eq!(parse("1,500", "JPY", Point), Ok(1500));
        assert_eq!(parse("1500.00", "JPY", Point), Ok(1500));
        assert_eq!(parse("12.345", "KWD", Point), Ok(12_345));
        assert_eq!(parse("12.3", "KWD", Point), Ok(12_300));
        assert_eq!(parse("1.2345", "CLF", Point), Ok(12_345));
        assert_eq!(parse("12.340", "GBP", Point), Ok(1234));
    }

    #[test]
    fn never_rounds() {
        use DecimalMark::Point;
        let too_precise = |code: &str| {
            Err(ParseAmountError::TooPrecise {
                currency: currency(code),
            })
        };
        assert_eq!(parse("12.345", "GBP", Point), too_precise("GBP"));
        assert_eq!(parse("0.001", "GBP", Point), too_precise("GBP"));
        assert_eq!(parse("1500.5", "JPY", Point), too_precise("JPY"));
        assert_eq!(parse("1.2345", "KWD", Point), too_precise("KWD"));
    }

    #[test]
    fn refuses_malformed_text() {
        use DecimalMark::{Comma, Point};
        assert_eq!(parse("", "GBP", Point), Err(ParseAmountError::Empty));
        assert_eq!(parse("   ", "GBP", Point), Err(ParseAmountError::Empty));
        for (text, mark) in [
            ("abc", Point),
            ("£12.34", Point),
            ("12.34 GBP", Point),
            ("12.34-", Point),
            ("--12", Point),
            ("-+12", Point),
            ("-(12)", Point),
            ("(12", Point),
            ("- 12", Point),
            ("-", Point),
            ("+", Point),
            ("()", Point),
            (".", Point),
            ("12.", Point),
            ("1.2.3", Point),
            ("12.3a", Point),
            ("12.-3", Point),
            ("1e5", Point),
            ("0x10", Point),
            ("1,23.45", Point),
            ("1234,567.89", Point),
            (",123.45", Point),
            ("123,.45", Point),
            ("1,234 567.89", Point),
            ("12.34,5", Comma),
            ("1,234,56", Comma),
            ("١٢", Point),
        ] {
            assert!(
                matches!(
                    parse(text, "GBP", mark),
                    Err(ParseAmountError::Malformed(_))
                ),
                "{text:?} with {mark:?} gave {:?}",
                parse(text, "GBP", mark)
            );
        }
    }

    #[test]
    fn refuses_amounts_beyond_i64() {
        use DecimalMark::Point;
        assert_eq!(parse("92233720368547758.07", "GBP", Point), Ok(i64::MAX));
        assert_eq!(parse("-92233720368547758.08", "GBP", Point), Ok(i64::MIN));
        assert_eq!(
            parse("92233720368547758.08", "GBP", Point),
            Err(ParseAmountError::Overflow)
        );
        assert_eq!(
            parse(&"9".repeat(60), "GBP", Point),
            Err(ParseAmountError::Overflow)
        );
    }

    #[test]
    fn formats_plain_decimals() {
        assert_eq!(gbp(-123_456).to_decimal_string(), "-1234.56");
        assert_eq!(gbp(5).to_decimal_string(), "0.05");
        assert_eq!(gbp(-5).to_decimal_string(), "-0.05");
        assert_eq!(gbp(0).to_decimal_string(), "0.00");
        assert_eq!(
            Money::new(-500, currency("JPY")).to_decimal_string(),
            "-500"
        );
        assert_eq!(Money::new(1, currency("KWD")).to_decimal_string(), "0.001");
        assert_eq!(
            Money::new(12_345, currency("CLF")).to_decimal_string(),
            "1.2345"
        );
        assert_eq!(gbp(i64::MIN).to_decimal_string(), "-92233720368547758.08");
        assert_eq!(gbp(-1240).to_string(), "-12.40 GBP");
    }

    #[test]
    fn decimal_strings_round_trip_in_every_currency() {
        for currency in Currency::all() {
            for minor in [
                0,
                1,
                -1,
                7,
                -99,
                100,
                123_456_789,
                -987_654_321,
                i64::MAX,
                i64::MIN,
            ] {
                let money = Money::new(minor, currency);
                let text = money.to_decimal_string();
                assert_eq!(
                    Money::parse_decimal(&text, currency, DecimalMark::Point),
                    Ok(money),
                    "{text} {currency}"
                );
            }
        }
    }

    #[test]
    fn decimal_mark_reads_from_presets() {
        #[derive(serde::Deserialize)]
        struct Preset {
            decimal: DecimalMark,
        }
        let point: Preset = serde_json::from_str(r#"{"decimal":"."}"#).unwrap();
        let comma: Preset = serde_json::from_str(r#"{"decimal":","}"#).unwrap();
        assert_eq!(
            (point.decimal, comma.decimal),
            (DecimalMark::Point, DecimalMark::Comma)
        );
        assert!(serde_json::from_str::<Preset>(r#"{"decimal":"Point"}"#).is_err());
    }
}
