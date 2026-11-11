//! The dedupe keys: a row's source ID, else its fingerprint.
//!
//! A fingerprint is `v1|<date>|<amount minor>|<currency>|<occurrence>|<description>`:
//!
//! - `date` is `YYYY-MM-DD`; the time is left out, since not every export of
//!   an account has one;
//! - the amount is signed integer minor units, then the ISO 4217 code;
//! - `occurrence` counts the earlier rows in the same file with the same
//!   date, amount, currency and normalized description, from 0. Rows with and without
//!   a source ID are counted apart, so one kind's presence never shifts the
//!   other's numbers. Flagged rows have no fields to compare and take no
//!   number;
//! - `description` is [`normalize_description`] of the raw description
//!   (whitespace collapsed, trimmed, Unicode uppercase). It comes last, so
//!   a `|` inside it can't make two fingerprints meet.
//!
//! The account isn't in the text: the unique index is on
//! `(account_id, fingerprint)`. The text is kept readable rather than
//! hashed, so two different rows can never share one by collision, and it
//! adds nothing the row doesn't already hold. The `v1` prefix names this
//! recipe; a different recipe needs a new prefix and a decision about the
//! rows already stored.
//!
//! So two identical payments on one day in one file are occurrences 0 and
//! 1 and both import, and importing the file again finds both. An
//! overlapping export with all of that day's rows numbers them the same
//! way. One that holds only some of a day's identical rows (cut off
//! mid-day) can number them differently; such an export would have to start
//! or end part-way through the day.

use std::collections::HashMap;

use chrono::NaiveDate;

use crate::money::Currency;
use crate::statement::{StatementLine, normalize_description};

/// The fingerprint recipe's version, the first part of every fingerprint.
pub const FINGERPRINT_VERSION: &str = "v1";

/// The fingerprint of `line`, the `occurrence`-th of its kind in its file.
pub fn fingerprint(line: &StatementLine, occurrence: u32) -> String {
    format!(
        "{FINGERPRINT_VERSION}|{}|{}|{}|{occurrence}|{}",
        line.date.format("%Y-%m-%d"),
        line.amount.minor(),
        line.amount.currency().code(),
        normalize_description(&line.description_raw),
    )
}

/// Numbers identical rows in file order, as [`fingerprint`] needs.
#[derive(Debug, Default)]
pub(crate) struct Occurrences {
    seen: HashMap<(bool, NaiveDate, i64, Currency, String), u32>,
}

impl Occurrences {
    /// The occurrence index of the next row, `line`.
    pub(crate) fn next(&mut self, line: &StatementLine) -> u32 {
        let key = (
            line.external_id.is_some(),
            line.date,
            line.amount.minor(),
            line.amount.currency(),
            normalize_description(&line.description_raw),
        );
        let count = self.seen.entry(key).or_default();
        let index = *count;
        *count += 1;
        index
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::money::Money;

    fn line(d: u32, minor: i64, description: &str) -> StatementLine {
        StatementLine::new(
            NaiveDate::from_ymd_opt(2026, 11, d).unwrap(),
            Money::new(minor, Currency::GBP),
            description,
        )
    }

    #[test]
    fn the_recipe_is_frozen() {
        // Changing this changes which stored rows count as already imported.
        assert_eq!(
            fingerprint(&line(2, -1240, "  Tesco  Stores\t1234 "), 0),
            "v1|2026-11-02|-1240|GBP|0|TESCO STORES 1234"
        );
        assert_eq!(
            fingerprint(&line(2, 50_000, "a|b"), 3),
            "v1|2026-11-02|50000|GBP|3|A|B"
        );
    }

    #[test]
    fn the_time_and_other_fields_are_left_out() {
        let plain = line(2, -500, "Coffee");
        let mut detailed = plain.clone();
        detailed.time = chrono::NaiveTime::from_hms_opt(9, 30, 0);
        detailed.merchant = Some("Coffee Co".into());
        detailed.note = Some("note".into());
        detailed.balance_after = Some(Money::new(1, Currency::GBP));
        assert_eq!(fingerprint(&plain, 0), fingerprint(&detailed, 0));
    }

    #[test]
    fn identical_rows_are_numbered_in_order() {
        let mut occurrences = Occurrences::default();
        let coffee = line(2, -500, "Coffee");
        assert_eq!(occurrences.next(&coffee), 0);
        assert_eq!(occurrences.next(&line(2, -500, "Tea")), 0);
        assert_eq!(occurrences.next(&line(3, -500, "Coffee")), 0);
        assert_eq!(occurrences.next(&line(2, -501, "Coffee")), 0);
        let mut euros = line(2, -500, "Coffee");
        euros.amount = Money::new(-500, Currency::EUR);
        assert_eq!(
            occurrences.next(&euros),
            0,
            "another currency is another row"
        );
        // Normalization makes these the same description.
        assert_eq!(occurrences.next(&line(2, -500, " coffee ")), 1);
        assert_eq!(occurrences.next(&coffee), 2);
    }

    #[test]
    fn rows_with_and_without_a_source_id_are_numbered_apart() {
        let mut occurrences = Occurrences::default();
        let mut with_id = line(2, -500, "Coffee");
        with_id.external_id = Some("tx_1".into());
        assert_eq!(occurrences.next(&with_id), 0);
        assert_eq!(occurrences.next(&line(2, -500, "Coffee")), 0);
        with_id.external_id = Some("tx_2".into());
        assert_eq!(occurrences.next(&with_id), 1);
    }
}
