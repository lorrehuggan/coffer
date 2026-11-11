//! ISO 4217 currency codes and their minor units.

use std::fmt;
use std::str::FromStr;

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};

/// An ISO 4217 currency, e.g. `GBP`.
///
/// Only codes in Coffer's table ([`Currency::all`]) can be built, so every
/// `Currency` knows its minor units: the number of decimal places an amount
/// has (`GBP` 2, `JPY` 0, `KWD` 3).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Currency {
    code: [u8; 3],
    minor_units: u8,
}

/// The table: every code Coffer accepts and its ISO 4217 minor units, sorted
/// by code.
///
/// Hand-written and frozen by the tests below: changing a value changes
/// what every stored amount in that currency means, so it needs a decision
/// in `docs/ARCHITECTURE.md`.
///
/// It holds the active ISO 4217 list (as of 2026-11) apart from the codes
/// with no minor unit (precious metals, `XDR`, `XSU`, `XUA`, the test and
/// "no currency" codes), plus codes withdrawn since 2014: older statements
/// and card payments abroad still name them. Where ISO and everyday use
/// differ (`MGA`, `MRU` are divided by 5), ISO's value is the one used.
const TABLE: &[(&str, u8)] = &[
    ("AED", 2),
    ("AFN", 2),
    ("ALL", 2),
    ("AMD", 2),
    ("ANG", 2), // being replaced by XCG (2025)
    ("AOA", 2),
    ("ARS", 2),
    ("AUD", 2),
    ("AWG", 2),
    ("AZN", 2),
    ("BAM", 2),
    ("BBD", 2),
    ("BDT", 2),
    ("BGN", 2), // Bulgaria adopted the euro in 2026
    ("BHD", 3),
    ("BIF", 0),
    ("BMD", 2),
    ("BND", 2),
    ("BOB", 2),
    ("BOV", 2),
    ("BRL", 2),
    ("BSD", 2),
    ("BTN", 2),
    ("BWP", 2),
    ("BYN", 2),
    ("BYR", 0), // withdrawn 2017, replaced by BYN
    ("BZD", 2),
    ("CAD", 2),
    ("CDF", 2),
    ("CHE", 2),
    ("CHF", 2),
    ("CHW", 2),
    ("CLF", 4),
    ("CLP", 0),
    ("CNY", 2),
    ("COP", 2),
    ("COU", 2),
    ("CRC", 2),
    ("CUC", 2), // withdrawn after Cuba's 2021 unification
    ("CUP", 2),
    ("CVE", 2),
    ("CZK", 2),
    ("DJF", 0),
    ("DKK", 2),
    ("DOP", 2),
    ("DZD", 2),
    ("EGP", 2),
    ("ERN", 2),
    ("ETB", 2),
    ("EUR", 2),
    ("FJD", 2),
    ("FKP", 2),
    ("GBP", 2),
    ("GEL", 2),
    ("GHS", 2),
    ("GIP", 2),
    ("GMD", 2),
    ("GNF", 0),
    ("GTQ", 2),
    ("GYD", 2),
    ("HKD", 2),
    ("HNL", 2),
    ("HRK", 2), // withdrawn 2023, Croatia adopted the euro
    ("HTG", 2),
    ("HUF", 2),
    ("IDR", 2),
    ("ILS", 2),
    ("INR", 2),
    ("IQD", 3),
    ("IRR", 2),
    ("ISK", 0),
    ("JMD", 2),
    ("JOD", 3),
    ("JPY", 0),
    ("KES", 2),
    ("KGS", 2),
    ("KHR", 2),
    ("KMF", 0),
    ("KPW", 2),
    ("KRW", 0),
    ("KWD", 3),
    ("KYD", 2),
    ("KZT", 2),
    ("LAK", 2),
    ("LBP", 2),
    ("LKR", 2),
    ("LRD", 2),
    ("LSL", 2),
    ("LTL", 2), // withdrawn 2015, Lithuania adopted the euro
    ("LVL", 2), // withdrawn 2014, Latvia adopted the euro
    ("LYD", 3),
    ("MAD", 2),
    ("MDL", 2),
    ("MGA", 2),
    ("MKD", 2),
    ("MMK", 2),
    ("MNT", 2),
    ("MOP", 2),
    ("MRO", 2), // withdrawn 2018, replaced by MRU
    ("MRU", 2),
    ("MUR", 2),
    ("MVR", 2),
    ("MWK", 2),
    ("MXN", 2),
    ("MXV", 2),
    ("MYR", 2),
    ("MZN", 2),
    ("NAD", 2),
    ("NGN", 2),
    ("NIO", 2),
    ("NOK", 2),
    ("NPR", 2),
    ("NZD", 2),
    ("OMR", 3),
    ("PAB", 2),
    ("PEN", 2),
    ("PGK", 2),
    ("PHP", 2),
    ("PKR", 2),
    ("PLN", 2),
    ("PYG", 0),
    ("QAR", 2),
    ("RON", 2),
    ("RSD", 2),
    ("RUB", 2),
    ("RWF", 0),
    ("SAR", 2),
    ("SBD", 2),
    ("SCR", 2),
    ("SDG", 2),
    ("SEK", 2),
    ("SGD", 2),
    ("SHP", 2),
    ("SLE", 2),
    ("SLL", 2), // withdrawn 2024, replaced by SLE
    ("SOS", 2),
    ("SRD", 2),
    ("SSP", 2),
    ("STD", 2), // withdrawn 2018, replaced by STN
    ("STN", 2),
    ("SVC", 2),
    ("SYP", 2),
    ("SZL", 2),
    ("THB", 2),
    ("TJS", 2),
    ("TMT", 2),
    ("TND", 3),
    ("TOP", 2),
    ("TRY", 2),
    ("TTD", 2),
    ("TWD", 2),
    ("TZS", 2),
    ("UAH", 2),
    ("UGX", 0),
    ("USD", 2),
    ("USN", 2),
    ("UYI", 0),
    ("UYU", 2),
    ("UYW", 4),
    ("UZS", 2),
    ("VED", 2),
    ("VEF", 2), // withdrawn 2018, replaced by VES
    ("VES", 2),
    ("VND", 0),
    ("VUV", 0),
    ("WST", 2),
    ("XAF", 0),
    ("XCD", 2),
    ("XCG", 2),
    ("XOF", 0),
    ("XPF", 0),
    ("YER", 2),
    ("ZAR", 2),
    ("ZMW", 2),
    ("ZWG", 2),
    ("ZWL", 2), // withdrawn 2024, replaced by ZWG
];

/// A code that isn't in Coffer's currency table.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code:?} isn't an ISO 4217 currency code Coffer knows")]
pub struct UnknownCurrency {
    /// The text that was given, truncated to 16 characters.
    pub code: String,
}

impl Currency {
    /// Pound sterling.
    pub const GBP: Currency = Currency::known(*b"GBP", 2);
    /// Euro.
    pub const EUR: Currency = Currency::known(*b"EUR", 2);
    /// US dollar.
    pub const USD: Currency = Currency::known(*b"USD", 2);

    /// Looks up an ISO 4217 code. Exact match only: three capital letters,
    /// no surrounding spaces.
    pub fn new(code: &str) -> Result<Currency, UnknownCurrency> {
        TABLE
            .binary_search_by(|(known, _)| (*known).cmp(code))
            .map(|index| {
                let (known, minor_units) = TABLE[index];
                let bytes = known.as_bytes();
                Currency::known([bytes[0], bytes[1], bytes[2]], minor_units)
            })
            .map_err(|_| UnknownCurrency {
                code: code.chars().take(16).collect(),
            })
    }

    const fn known(code: [u8; 3], minor_units: u8) -> Currency {
        Currency { code, minor_units }
    }

    /// Every currency Coffer knows, in code order.
    pub fn all() -> impl ExactSizeIterator<Item = Currency> {
        TABLE
            .iter()
            .map(|&(code, _)| Currency::new(code).expect("every code in the table looks itself up"))
    }

    /// The three-letter code, e.g. `"GBP"`.
    pub fn code(&self) -> &str {
        std::str::from_utf8(&self.code).expect("currency codes are ASCII")
    }

    /// Decimal places in an amount: 2 for `GBP`, 0 for `JPY`, 3 for `KWD`.
    pub fn minor_units(&self) -> u8 {
        self.minor_units
    }

    /// Minor units per major unit: 100 for `GBP`, 1 for `JPY`.
    pub fn minor_per_major(&self) -> i64 {
        10_i64.pow(u32::from(self.minor_units))
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl fmt::Debug for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Currency({})", self.code())
    }
}

impl FromStr for Currency {
    type Err = UnknownCurrency;

    fn from_str(code: &str) -> Result<Currency, UnknownCurrency> {
        Currency::new(code)
    }
}

impl serde::Serialize for Currency {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.code())
    }
}

impl<'de> serde::Deserialize<'de> for Currency {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Currency, D::Error> {
        let code = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        Currency::new(&code).map_err(serde::de::Error::custom)
    }
}

/// Stored as its code in a `TEXT` column (`currency`, `original_currency`).
impl ToSql for Currency {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(ValueRef::Text(&self.code)))
    }
}

/// A stored code the table doesn't know is an error, not a guess.
impl FromSql for Currency {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Currency> {
        let text = value.as_str()?;
        Currency::new(text).map_err(|error| FromSqlError::Other(Box::new(error)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_unique_and_well_formed() {
        for pair in TABLE.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
        }
        for &(code, minor_units) in TABLE {
            assert!(
                code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_uppercase()),
                "{code:?}"
            );
            assert!(minor_units <= 4, "{code} has {minor_units} minor units");
        }
    }

    /// Freezes the table: any change to it must change this test too.
    #[test]
    fn table_is_frozen() {
        let by_units = |units: u8| -> Vec<&str> {
            TABLE
                .iter()
                .filter(|&&(_, minor_units)| minor_units == units)
                .map(|&(code, _)| code)
                .collect()
        };
        assert_eq!(
            by_units(0),
            [
                "BIF", "BYR", "CLP", "DJF", "GNF", "ISK", "JPY", "KMF", "KRW", "PYG", "RWF", "UGX",
                "UYI", "VND", "VUV", "XAF", "XOF", "XPF",
            ]
        );
        assert_eq!(
            by_units(3),
            ["BHD", "IQD", "JOD", "KWD", "LYD", "OMR", "TND"]
        );
        assert_eq!(by_units(4), ["CLF", "UYW"]);
        assert_eq!(by_units(1), Vec::<&str>::new());
        assert_eq!(TABLE.len(), 176);
    }

    #[test]
    fn looks_up_codes_and_minor_units() {
        assert_eq!(Currency::new("GBP"), Ok(Currency::GBP));
        assert_eq!(Currency::new("EUR"), Ok(Currency::EUR));
        assert_eq!(Currency::new("USD"), Ok(Currency::USD));
        assert_eq!(Currency::GBP.minor_units(), 2);
        assert_eq!(Currency::GBP.minor_per_major(), 100);
        let jpy = Currency::new("JPY").unwrap();
        assert_eq!((jpy.minor_units(), jpy.minor_per_major()), (0, 1));
        let kwd = Currency::new("KWD").unwrap();
        assert_eq!((kwd.minor_units(), kwd.minor_per_major()), (3, 1000));
        assert_eq!(Currency::new("CLF").unwrap().minor_per_major(), 10_000);
        assert_eq!(Currency::new("HRK").unwrap().code(), "HRK");
    }

    #[test]
    fn named_constants_match_the_table() {
        for currency in [Currency::GBP, Currency::EUR, Currency::USD] {
            assert_eq!(Currency::new(currency.code()), Ok(currency));
        }
    }

    #[test]
    fn refuses_anything_but_an_exact_known_code() {
        for code in [
            "", "gbp", "Gbp", " GBP", "GBP ", "GB", "GBPX", "XAU", "XXX", "XTS", "ZZZ", "£",
        ] {
            assert!(Currency::new(code).is_err(), "{code:?} was accepted");
        }
        let error = Currency::new(&"A".repeat(100)).unwrap_err();
        assert_eq!(error.code.len(), 16);
    }

    #[test]
    fn every_currency_round_trips_through_its_code() {
        assert_eq!(Currency::all().len(), TABLE.len());
        for currency in Currency::all() {
            assert_eq!(currency.code().parse(), Ok(currency));
            assert_eq!(currency.to_string(), currency.code());
        }
    }

    #[test]
    fn serde_uses_the_code_and_validates_it() {
        #[derive(serde::Deserialize, serde::Serialize, PartialEq, Debug)]
        struct Preset {
            currency: Currency,
        }
        let preset: Preset = serde_json::from_str(r#"{"currency":"EUR"}"#).unwrap();
        assert_eq!(preset.currency, Currency::EUR);
        assert_eq!(
            serde_json::to_string(&preset).unwrap(),
            r#"{"currency":"EUR"}"#
        );
        assert!(serde_json::from_str::<Preset>(r#"{"currency":"eur"}"#).is_err());
    }

    #[test]
    fn sqlite_stores_the_code_and_refuses_unknown_ones() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let stored: String = conn
            .query_row("SELECT ?1", [Currency::GBP], |row| row.get(0))
            .unwrap();
        assert_eq!(stored, "GBP");
        let read: Currency = conn
            .query_row("SELECT 'JPY'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(read.minor_units(), 0);
        let unknown = conn.query_row("SELECT 'ABC'", [], |row| row.get::<_, Currency>(0));
        assert!(unknown.is_err());
        let not_text = conn.query_row("SELECT 826", [], |row| row.get::<_, Currency>(0));
        assert!(not_text.is_err());
    }
}
