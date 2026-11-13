//! Months ([`Period`], `YYYY-MM`) and inclusive date ranges ([`DateRange`]).

use std::fmt;
use std::str::FromStr;

use chrono::{Datelike, Months, NaiveDate};

use super::Error;

/// A calendar month, written `YYYY-MM` (years 1–9999).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Period {
    /// The month's first day.
    first: NaiveDate,
}

impl Period {
    /// Month `month` (1–12) of `year` (1–9999).
    pub fn new(year: i32, month: u32) -> Result<Period, Error> {
        if !(1..=9999).contains(&year) {
            return Err(Error::InvalidPeriod);
        }
        NaiveDate::from_ymd_opt(year, month, 1)
            .map(|first| Period { first })
            .ok_or(Error::InvalidPeriod)
    }

    /// The month `date` is in.
    pub fn containing(date: NaiveDate) -> Period {
        Period {
            first: date.with_day(1).expect("every month has a first day"),
        }
    }

    pub fn year(&self) -> i32 {
        self.first.year()
    }

    /// 1–12.
    pub fn month(&self) -> u32 {
        self.first.month()
    }

    pub fn first_day(&self) -> NaiveDate {
        self.first
    }

    pub fn last_day(&self) -> NaiveDate {
        self.next_first()
            .and_then(|next| next.pred_opt())
            .unwrap_or(NaiveDate::MAX)
    }

    /// Every day of the month.
    pub fn range(&self) -> DateRange {
        DateRange {
            from: self.first_day(),
            to: self.last_day(),
        }
    }

    /// The month after, `None` after 9999-12.
    pub fn next(&self) -> Option<Period> {
        self.next_first()
            .filter(|first| first.year() <= 9999)
            .map(|first| Period { first })
    }

    /// The month before, `None` before 0001-01.
    pub fn previous(&self) -> Option<Period> {
        self.first
            .checked_sub_months(Months::new(1))
            .filter(|first| first.year() >= 1)
            .map(|first| Period { first })
    }

    /// Every month from this one through `last`, both included; empty when
    /// `last` is earlier.
    pub fn through(self, last: Period) -> impl Iterator<Item = Period> {
        std::iter::successors(Some(self), |period| period.next())
            .take_while(move |period| *period <= last)
    }

    /// `YYYY-MM`, as stored in `budget.month`.
    pub fn key(&self) -> String {
        self.to_string()
    }

    fn next_first(&self) -> Option<NaiveDate> {
        self.first.checked_add_months(Months::new(1))
    }
}

impl fmt::Display for Period {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}", self.year(), self.month())
    }
}

impl FromStr for Period {
    type Err = Error;

    /// Exactly `YYYY-MM`: four digits, a hyphen, two digits.
    fn from_str(text: &str) -> Result<Period, Error> {
        let bytes = text.as_bytes();
        let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
        if bytes.len() != 7 || bytes[4] != b'-' || !digits(0..4) || !digits(5..7) {
            return Err(Error::InvalidPeriod);
        }
        let year = text[0..4].parse().map_err(|_| Error::InvalidPeriod)?;
        let month = text[5..7].parse().map_err(|_| Error::InvalidPeriod)?;
        Period::new(year, month)
    }
}

/// Reads a date written exactly `YYYY-MM-DD`.
pub fn parse_date(text: &str) -> Result<NaiveDate, Error> {
    let bytes = text.as_bytes();
    let shape = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, byte)| i == 4 || i == 7 || byte.is_ascii_digit());
    if !shape {
        return Err(Error::InvalidDate);
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .ok()
        .filter(|date| date.year() >= 1)
        .ok_or(Error::InvalidDate)
}

/// Days from `from` through `to`, both included; never empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DateRange {
    from: NaiveDate,
    to: NaiveDate,
}

impl DateRange {
    /// `from` through `to`; refused when `from` is after `to`.
    pub fn new(from: NaiveDate, to: NaiveDate) -> Result<DateRange, Error> {
        if from > to {
            return Err(Error::EmptyRange { from, to });
        }
        Ok(DateRange { from, to })
    }

    /// Reads two `YYYY-MM-DD` dates.
    pub fn parse(from: &str, to: &str) -> Result<DateRange, Error> {
        DateRange::new(parse_date(from)?, parse_date(to)?)
    }

    /// The first day of `first` through the last day of `last`.
    pub fn months(first: Period, last: Period) -> Result<DateRange, Error> {
        DateRange::new(first.first_day(), last.last_day())
    }

    pub fn from(&self) -> NaiveDate {
        self.from
    }

    pub fn to(&self) -> NaiveDate {
        self.to
    }

    pub fn contains(&self, date: NaiveDate) -> bool {
        self.from <= date && date <= self.to
    }

    /// Every month the range touches, in order.
    pub fn periods(&self) -> impl Iterator<Item = Period> {
        Period::containing(self.from).through(Period::containing(self.to))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> NaiveDate {
        parse_date(text).unwrap()
    }

    #[test]
    fn periods_read_and_write_yyyy_mm() {
        let period: Period = "2026-02".parse().unwrap();
        assert_eq!((period.year(), period.month()), (2026, 2));
        assert_eq!(period.to_string(), "2026-02");
        assert_eq!(period.key(), "2026-02");
        assert_eq!(period.first_day(), date("2026-02-01"));
        assert_eq!(period.last_day(), date("2026-02-28"));
        assert_eq!(
            "2024-02".parse::<Period>().unwrap().last_day(),
            date("2024-02-29")
        );
        for bad in [
            "",
            "2026-2",
            "2026-13",
            "2026-00",
            "0000-01",
            "26-11",
            "2026/11",
            "2026-11-01",
            " 2026-11",
            "+202-11",
            "２０２６-11",
        ] {
            assert!(
                matches!(bad.parse::<Period>(), Err(Error::InvalidPeriod)),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn periods_step_and_iterate() {
        let nov = Period::new(2026, 11).unwrap();
        assert_eq!(nov.next(), Some(Period::new(2026, 12).unwrap()));
        assert_eq!(nov.next().unwrap().next(), Period::new(2027, 1).ok());
        assert_eq!(
            Period::new(2026, 1).unwrap().previous(),
            Period::new(2025, 12).ok()
        );
        assert_eq!(Period::new(9999, 12).unwrap().next(), None);
        assert_eq!(Period::new(1, 1).unwrap().previous(), None);
        let months: Vec<String> = Period::new(2026, 11)
            .unwrap()
            .through(Period::new(2027, 2).unwrap())
            .map(|p| p.key())
            .collect();
        assert_eq!(months, ["2026-11", "2026-12", "2027-01", "2027-02"]);
        assert_eq!(nov.through(Period::new(2026, 10).unwrap()).count(), 0);
        assert_eq!(Period::containing(date("2026-11-30")), nov);
    }

    #[test]
    fn ranges_are_inclusive_and_never_empty() {
        let range = DateRange::parse("2026-10-15", "2026-12-01").unwrap();
        assert!(range.contains(date("2026-10-15")));
        assert!(range.contains(date("2026-12-01")));
        assert!(!range.contains(date("2026-12-02")));
        let months: Vec<String> = range.periods().map(|p| p.key()).collect();
        assert_eq!(months, ["2026-10", "2026-11", "2026-12"]);
        assert!(DateRange::parse("2026-11-01", "2026-11-01").is_ok());
        assert!(matches!(
            DateRange::parse("2026-11-02", "2026-11-01"),
            Err(Error::EmptyRange { .. })
        ));
        let year = DateRange::months(
            Period::new(2026, 1).unwrap(),
            Period::new(2026, 12).unwrap(),
        )
        .unwrap();
        assert_eq!(
            (year.from(), year.to()),
            (date("2026-01-01"), date("2026-12-31"))
        );
        for bad in [
            "2026-11-31",
            "2026-1-01",
            "13/11/2026",
            "2026-11-01T00:00",
            "",
        ] {
            assert!(
                matches!(parse_date(bad), Err(Error::InvalidDate)),
                "{bad:?}"
            );
        }
    }
}
