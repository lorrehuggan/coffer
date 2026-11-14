//! The time now, and UTC times in the computer's time zone, read through
//! SQLite (as [`crate::schedule::local_today`] reads today): chrono is
//! built without its clock and time-zone features, so core has no other
//! dependency for them.
//!
//! For the app's Settings (M3.15, M3.16): the access log's "last 7 days"
//! and its times of day, and when each backup was taken.

use chrono::{DateTime, NaiveDateTime, Utc};
use rusqlite::Connection;

/// The format SQLite's `datetime()` writes.
const SQLITE_FORMAT: &str = "%Y-%m-%d %H:%M:%S%.f";

fn parse(text: &str) -> rusqlite::Result<NaiveDateTime> {
    NaiveDateTime::parse_from_str(text, SQLITE_FORMAT).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })
}

/// The time now, to the millisecond.
pub fn now() -> rusqlite::Result<DateTime<Utc>> {
    let conn = Connection::open_in_memory()?;
    let now: String = conn.query_row("SELECT strftime('%Y-%m-%d %H:%M:%f', 'now')", [], |row| {
        row.get(0)
    })?;
    Ok(parse(&now)?.and_utc())
}

/// Each of `times` in the computer's time zone, with the offset that
/// applied on its date (daylight saving included). One connection for all.
pub fn local(times: &[DateTime<Utc>]) -> rusqlite::Result<Vec<NaiveDateTime>> {
    let conn = Connection::open_in_memory()?;
    let mut statement = conn.prepare("SELECT strftime('%Y-%m-%d %H:%M:%f', ?1, 'localtime')")?;
    times
        .iter()
        .map(|time| {
            let utc = time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
            let local: String = statement.query_row([utc], |row| row.get(0))?;
            parse(&local)
        })
        .collect()
}

/// `time` as the database stores times: UTC, `YYYY-MM-DDTHH:MM:SS.SSSZ`,
/// so it compares with stored times as text.
pub fn stored(time: DateTime<Utc>) -> String {
    time.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// A time as the database stores it (`YYYY-MM-DDTHH:MM:SS.SSSZ`), `None`
/// when it isn't one.
pub fn parse_stored(text: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.fZ")
        .ok()
        .map(|time| time.and_utc())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_time_now() {
        let now = now().unwrap();
        // After this code was written, and before it's ancient.
        assert!(now > parse_stored("2026-11-14T00:00:00.000Z").unwrap());
        assert!(now < parse_stored("2200-01-01T00:00:00.000Z").unwrap());
    }

    #[test]
    fn converts_to_local_time_keeping_the_milliseconds() {
        let time = parse_stored("2026-11-10T10:42:07.123Z").unwrap();
        let local = local(&[time]).unwrap();
        assert_eq!(local.len(), 1);
        // Whatever the zone, it's within a day, with the same milliseconds
        // and seconds (offsets are whole minutes).
        let offset = local[0] - time.naive_utc();
        assert!(offset.num_hours().abs() <= 14);
        assert_eq!(local[0].format("%S%.3f").to_string(), "07.123");
        assert!(super::local(&[]).unwrap().is_empty());
    }

    #[test]
    fn parses_stored_times() {
        assert_eq!(
            parse_stored("2026-11-10T10:42:07.123Z")
                .unwrap()
                .format("%Y-%m-%d %H:%M:%S%.3f")
                .to_string(),
            "2026-11-10 10:42:07.123"
        );
        assert_eq!(parse_stored("yesterday"), None);
    }
}
