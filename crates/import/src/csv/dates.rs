//! Date and time formats for CSV columns.
//!
//! Formats are `strftime` patterns (`%d/%m/%Y`). chrono's parser is lenient
//! (`%Y` reads `26` as the year 26, `%d` reads `1`), so every value must
//! also round-trip: formatted again, it has to give back the text it came
//! from, ignoring ASCII case. A value that doesn't is flagged, which keeps a
//! wrong format from being read as plausible but wrong dates.

use std::fmt::{self, Write as _};

use chrono::format::{Item, Parsed, StrftimeItems};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime, Timelike};

/// A pattern that can't be used as a date or time format.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{pattern:?} isn't a usable {kind} format: {reason}")]
pub struct FormatError {
    pub pattern: String,
    pub kind: &'static str,
    pub reason: &'static str,
}

/// How a date column is written: `%d/%m/%Y` (UK, EU), `%m/%d/%Y` (US),
/// `%Y-%m-%d`, `%d %b %Y`, …
///
/// It may include a time of day (`%Y-%m-%dT%H:%M:%S`), which is then the
/// row's time unless a separate time column is mapped. Time zones aren't
/// supported: a bank's local date is the date that counts.
#[derive(Clone)]
pub struct DateFormat {
    pattern: String,
    items: Vec<Item<'static>>,
    with_time: bool,
}

/// The formats [`DateFormat::candidates`] tries, most common first.
const COMMON_DATE_FORMATS: &[&str] = &[
    "%d/%m/%Y",
    "%m/%d/%Y",
    "%Y-%m-%d",
    "%d.%m.%Y",
    "%d-%m-%Y",
    "%m-%d-%Y",
    "%Y/%m/%d",
    "%-d/%-m/%Y",
    "%-m/%-d/%Y",
    "%d/%m/%y",
    "%m/%d/%y",
    "%d.%m.%y",
    "%d %b %Y",
    "%d-%b-%Y",
    "%d %b %y",
    "%d-%b-%y",
    "%Y-%m-%dT%H:%M:%S",
    "%Y-%m-%d %H:%M:%S",
    "%Y-%m-%d %H:%M",
    "%d/%m/%Y %H:%M:%S",
    "%d/%m/%Y %H:%M",
    "%m/%d/%Y %H:%M:%S",
    "%m/%d/%Y %H:%M",
    "%d.%m.%Y %H:%M:%S",
    "%d.%m.%Y %H:%M",
];

fn items(pattern: &str, kind: &'static str) -> Result<Vec<Item<'static>>, FormatError> {
    StrftimeItems::new(pattern)
        .parse_to_owned()
        .map_err(|_| FormatError {
            pattern: pattern.to_owned(),
            kind,
            reason: "it has an unknown % specifier",
        })
}

/// What `write` produces, or `None` when the format asks for something the
/// value doesn't have (a time from a date): chrono reports that as a
/// formatting error, which `to_string` would turn into a panic.
fn render(write: impl FnOnce(&mut String) -> fmt::Result) -> Option<String> {
    let mut out = String::new();
    write(&mut out).ok()?;
    Some(out)
}

fn parse_with(text: &str, items: &[Item<'static>]) -> Option<Parsed> {
    let mut parsed = Parsed::new();
    chrono::format::parse(&mut parsed, text, items.iter()).ok()?;
    Some(parsed)
}

impl DateFormat {
    pub fn new(pattern: &str) -> Result<DateFormat, FormatError> {
        let error = |reason| FormatError {
            pattern: pattern.to_owned(),
            kind: "date",
            reason,
        };
        let items = items(pattern, "date")?;

        // A date late in the month and year, so day, month and year can't
        // stand in for each other.
        let date = NaiveDate::from_ymd_opt(2026, 11, 23).expect("valid date");
        let datetime = date.and_hms_opt(13, 45, 0).expect("valid time");
        let reads_back = |format: &DateFormat, expected: NaiveDateTime, with_time| {
            let text = format.format(expected, with_time)?;
            let (read_date, read_time) = format.parse(&text)?;
            (read_date == expected.date() && read_time.is_none_or(|t| t == expected.time()))
                .then_some(())
        };

        let mut format = DateFormat {
            pattern: pattern.to_owned(),
            items,
            with_time: false,
        };
        if render(|out| write!(out, "{}", date.format_with_items(format.items.iter()))).is_some() {
            reads_back(&format, datetime.date().into(), false)
                .ok_or_else(|| error("it needs a year, a month and a day"))?;
            return Ok(format);
        }
        format.with_time = true;
        if render(|out| write!(out, "{}", datetime.format_with_items(format.items.iter())))
            .is_none()
        {
            return Err(error("time zones aren't supported"));
        }
        reads_back(&format, datetime, true)
            .ok_or_else(|| error("it needs a year, a month, a day, an hour and a minute"))?;
        Ok(format)
    }

    /// The pattern, e.g. `%d/%m/%Y`.
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Whether the format includes a time of day.
    pub fn has_time(&self) -> bool {
        self.with_time
    }

    fn format(&self, value: NaiveDateTime, with_time: bool) -> Option<String> {
        if with_time {
            render(|out| write!(out, "{}", value.format_with_items(self.items.iter())))
        } else {
            render(|out| write!(out, "{}", value.date().format_with_items(self.items.iter())))
        }
    }

    /// Reads `text` (surrounding whitespace ignored). `None` when it doesn't
    /// match the format exactly.
    pub fn parse(&self, text: &str) -> Option<(NaiveDate, Option<NaiveTime>)> {
        let text = text.trim();
        let parsed = parse_with(text, &self.items)?;
        let (date, time) = if self.with_time {
            let datetime = parsed.to_naive_datetime_with_offset(0).ok()?;
            (datetime.date(), Some(datetime.time()))
        } else {
            (parsed.to_naive_date().ok()?, None)
        };
        let again = self.format(date.and_time(time.unwrap_or_default()), self.with_time)?;
        again.eq_ignore_ascii_case(text).then_some((date, time))
    }

    /// The common formats that read every one of `values` (empty ones are
    /// skipped), most common first. Empty if no value is given.
    ///
    /// More than one candidate means the values are ambiguous (`01/02/2026`
    /// is 1 February or 2 January): ask the user rather than pick.
    pub fn candidates<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<DateFormat> {
        let values: Vec<&str> = values
            .into_iter()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect();
        if values.is_empty() {
            return Vec::new();
        }
        COMMON_DATE_FORMATS
            .iter()
            .map(|pattern| DateFormat::new(pattern).expect("common formats are valid"))
            .filter(|format| values.iter().all(|value| format.parse(value).is_some()))
            .collect()
    }
}

impl PartialEq for DateFormat {
    fn eq(&self, other: &Self) -> bool {
        self.pattern == other.pattern
    }
}

impl Eq for DateFormat {}

impl fmt::Debug for DateFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DateFormat({:?})", self.pattern)
    }
}

/// How a separate time column is written: `%H:%M:%S`, `%H:%M`, `%I:%M %p`.
#[derive(Clone)]
pub struct TimeFormat {
    pattern: String,
    items: Vec<Item<'static>>,
}

impl TimeFormat {
    pub fn new(pattern: &str) -> Result<TimeFormat, FormatError> {
        let error = |reason| FormatError {
            pattern: pattern.to_owned(),
            kind: "time",
            reason,
        };
        let format = TimeFormat {
            pattern: pattern.to_owned(),
            items: items(pattern, "time")?,
        };
        let sample = NaiveTime::from_hms_opt(13, 45, 0).expect("valid time");
        let text = render(|out| write!(out, "{}", sample.format_with_items(format.items.iter())))
            .ok_or_else(|| error("it can only contain a time of day"))?;
        match format.parse(&text) {
            Some(time) if time.hour() == 13 && time.minute() == 45 => Ok(format),
            _ => Err(error("it needs an hour and a minute")),
        }
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Reads `text` (surrounding whitespace ignored). `None` when it doesn't
    /// match the format exactly.
    pub fn parse(&self, text: &str) -> Option<NaiveTime> {
        let text = text.trim();
        let time = parse_with(text, &self.items)?.to_naive_time().ok()?;
        let again = render(|out| write!(out, "{}", time.format_with_items(self.items.iter())))?;
        again.eq_ignore_ascii_case(text).then_some(time)
    }
}

impl PartialEq for TimeFormat {
    fn eq(&self, other: &Self) -> bool {
        self.pattern == other.pattern
    }
}

impl Eq for TimeFormat {}

impl fmt::Debug for TimeFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TimeFormat({:?})", self.pattern)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn time(h: u32, m: u32, s: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, s).unwrap()
    }

    fn format(pattern: &str) -> DateFormat {
        DateFormat::new(pattern).unwrap()
    }

    #[test]
    fn day_month_order_comes_from_the_format() {
        assert_eq!(
            format("%d/%m/%Y").parse("01/02/2026"),
            Some((date(2026, 2, 1), None))
        );
        assert_eq!(
            format("%m/%d/%Y").parse("01/02/2026"),
            Some((date(2026, 1, 2), None))
        );
        assert_eq!(format("%m/%d/%Y").parse("13/02/2026"), None);
        assert_eq!(
            format("%Y-%m-%d").parse(" 2026-11-01 "),
            Some((date(2026, 11, 1), None)),
            "surrounding whitespace is ignored"
        );
    }

    #[test]
    fn values_must_round_trip() {
        let uk = format("%d/%m/%Y");
        assert_eq!(uk.parse("26/11/26"), None, "a two-digit year isn't %Y");
        assert_eq!(uk.parse("1/11/2026"), None, "%d is zero-padded");
        assert_eq!(uk.parse("01/11/2026 extra"), None);
        assert_eq!(
            format("%-d/%-m/%Y").parse("1/11/2026"),
            Some((date(2026, 11, 1), None))
        );
        assert_eq!(
            format("%d/%m/%y").parse("26/11/26"),
            Some((date(2026, 11, 26), None))
        );
        assert_eq!(
            format("%d %b %Y").parse("01 NOV 2026"),
            Some((date(2026, 11, 1), None)),
            "month names in any case"
        );
        assert_eq!(uk.parse("31/02/2026"), None, "no such day");
    }

    #[test]
    fn a_date_format_can_carry_the_time() {
        let iso = format("%Y-%m-%dT%H:%M:%S");
        assert!(iso.has_time());
        assert_eq!(
            iso.parse("2026-11-01T09:05:33"),
            Some((date(2026, 11, 1), Some(time(9, 5, 33))))
        );
        assert_eq!(iso.parse("2026-11-01"), None);
        assert!(!format("%Y-%m-%d").has_time());
    }

    #[test]
    fn unusable_date_formats_are_refused() {
        let reason = |pattern| DateFormat::new(pattern).unwrap_err().reason;
        assert_eq!(reason("%d/%m"), "it needs a year, a month and a day");
        assert_eq!(reason("%Q"), "it has an unknown % specifier");
        assert_eq!(reason("%Y-%m-%dT%H:%M:%S%z"), "time zones aren't supported");
        assert_eq!(
            reason("%Y-%m-%d %S"),
            "it needs a year, a month, a day, an hour and a minute"
        );
        assert_eq!(
            DateFormat::new("%d/%m").unwrap_err().to_string(),
            "\"%d/%m\" isn't a usable date format: it needs a year, a month and a day"
        );
    }

    #[test]
    fn time_formats() {
        let hms = TimeFormat::new("%H:%M:%S").unwrap();
        assert_eq!(hms.parse("09:05:33"), Some(time(9, 5, 33)));
        assert_eq!(hms.parse("9:05:33"), None);
        assert_eq!(hms.parse("09:05"), None);
        let hm = TimeFormat::new("%H:%M").unwrap();
        assert_eq!(hm.parse("23:59"), Some(time(23, 59, 0)));
        let twelve = TimeFormat::new("%I:%M %p").unwrap();
        assert_eq!(twelve.parse("01:30 pm"), Some(time(13, 30, 0)));

        let reason = |pattern| TimeFormat::new(pattern).unwrap_err().reason;
        assert_eq!(reason("%d/%m/%Y"), "it can only contain a time of day");
        assert_eq!(reason("%M:%S"), "it needs an hour and a minute");
    }

    fn patterns(values: &[&str]) -> Vec<String> {
        DateFormat::candidates(values.iter().copied())
            .iter()
            .map(|format| format.pattern().to_owned())
            .collect()
    }

    #[test]
    fn candidates_show_ambiguity() {
        assert_eq!(
            patterns(&["01/02/2026", "03/04/2026"]),
            ["%d/%m/%Y", "%m/%d/%Y"],
            "ambiguous: the caller asks"
        );
        assert_eq!(patterns(&["01/02/2026", "13/02/2026", ""]), ["%d/%m/%Y"]);
        assert_eq!(patterns(&["02/13/2026"]), ["%m/%d/%Y"]);
        assert_eq!(patterns(&["2026-11-01"]), ["%Y-%m-%d"]);
        assert_eq!(patterns(&["2026-11-01T10:00:00"]), ["%Y-%m-%dT%H:%M:%S"]);
        assert!(patterns(&["", " "]).is_empty());
        assert!(patterns(&["yesterday"]).is_empty());
    }

    #[test]
    fn every_common_format_is_valid() {
        for pattern in COMMON_DATE_FORMATS {
            DateFormat::new(pattern).unwrap();
        }
    }
}
