//! When a note was last edited, as a list says it (interface design,
//! sections 4.5 and 6.2): "now", "4m", "3h", "Thu", "Oct 3" on the row,
//! and "Edited Nov 13, 09:12" when the time is pointed at.
//!
//! Nothing here reads a clock or a time zone by itself: a [`Clock`] says
//! what time it is and how far local time is from UTC, so the same note
//! gives the same words in a test as on a device. Which layer of the list
//! a note lies in is `dirt_core::models::Layer`, by the same two figures.

use chrono::{DateTime, Datelike, NaiveDateTime};

const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 60 * MINUTE_MS;
const DAY_MS: i64 = 24 * HOUR_MS;

/// How many days before today a note is still named by its weekday. The
/// same six days that are "This week" in the list's layers, so no weekday
/// can mean two days.
const DAYS_BY_NAME: i64 = 6;

/// What time it is, and where.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    /// Now, in Unix milliseconds.
    pub now_ms: i64,
    /// How far local time is ahead of UTC, in seconds; behind, if negative.
    pub utc_offset_secs: i32,
}

impl Clock {
    /// The device's clock and time zone, now.
    #[must_use]
    pub fn now() -> Self {
        let now = chrono::Local::now();
        Self {
            now_ms: now.timestamp_millis(),
            utc_offset_secs: now.offset().local_minus_utc(),
        }
    }

    /// The local date and time at `at_ms`. A time too far off to be a date
    /// at all counts as the start of 1970.
    fn local(self, at_ms: i64) -> NaiveDateTime {
        let local = at_ms.saturating_add(i64::from(self.utc_offset_secs) * 1000);
        DateTime::from_timestamp_millis(local)
            .unwrap_or_default()
            .naive_utc()
    }
}

/// When a note was last edited, in a few characters.
///
/// "now" within the first minute, then minutes and hours, the weekday for
/// the six days before today, the month and day for this year, and the
/// month and year before that. A time later than now, from a device whose
/// clock is ahead, is "now".
#[must_use]
pub fn edited_short(edited_ms: i64, clock: Clock) -> String {
    let age = clock.now_ms.saturating_sub(edited_ms);
    if age < MINUTE_MS {
        return "now".to_owned();
    }
    if age < HOUR_MS {
        return format!("{}m", age / MINUTE_MS);
    }
    if age < DAY_MS {
        return format!("{}h", age / HOUR_MS);
    }
    let (then, today) = (clock.local(edited_ms), clock.local(clock.now_ms));
    let days = (today.date() - then.date()).num_days();
    let format = if days <= DAYS_BY_NAME {
        "%a"
    } else if then.year() == today.year() {
        "%b %-d"
    } else {
        "%b %Y"
    };
    then.format(format).to_string()
}

/// When a note was last edited, in full, for the time's tooltip: "Edited
/// Nov 13, 09:12", with the year when it is not this one.
#[must_use]
pub fn edited_full(edited_ms: i64, clock: Clock) -> String {
    let (then, today) = (clock.local(edited_ms), clock.local(clock.now_ms));
    let format = if then.year() == today.year() {
        "Edited %b %-d, %H:%M"
    } else {
        "Edited %b %-d, %Y, %H:%M"
    };
    then.format(format).to_string()
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    const HOUR_SECS: i32 = 3600;

    /// A local date and time as Unix milliseconds, where local time is
    /// `offset` seconds ahead of UTC.
    fn at(date: (i32, u32, u32), time: (u32, u32), offset: i32) -> i64 {
        let local = NaiveDate::from_ymd_opt(date.0, date.1, date.2)
            .unwrap()
            .and_hms_opt(time.0, time.1, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis();
        local - i64::from(offset) * 1000
    }

    /// Tuesday 17 November 2026, 10:30, eight hours ahead of UTC.
    fn clock() -> Clock {
        Clock {
            now_ms: at((2026, 11, 17), (10, 30), 8 * HOUR_SECS),
            utc_offset_secs: 8 * HOUR_SECS,
        }
    }

    fn short(date: (i32, u32, u32), time: (u32, u32)) -> String {
        edited_short(at(date, time, 8 * HOUR_SECS), clock())
    }

    #[test]
    fn the_last_day_is_counted_in_minutes_and_hours() {
        let now = clock().now_ms;
        assert_eq!(edited_short(now, clock()), "now");
        assert_eq!(edited_short(now - 59_999, clock()), "now");
        assert_eq!(edited_short(now - MINUTE_MS, clock()), "1m");
        assert_eq!(edited_short(now - 59 * MINUTE_MS, clock()), "59m");
        assert_eq!(edited_short(now - HOUR_MS, clock()), "1h");
        assert_eq!(edited_short(now - DAY_MS + 1, clock()), "23h");
        // A clock that is ahead somewhere else.
        assert_eq!(edited_short(now + HOUR_MS, clock()), "now");
    }

    #[test]
    fn the_six_days_before_today_go_by_their_names() {
        assert_eq!(short((2026, 11, 16), (9, 0)), "Mon");
        assert_eq!(short((2026, 11, 11), (23, 59)), "Wed");
        // A week ago today would be "Tue" again, so it is a date.
        assert_eq!(short((2026, 11, 10), (12, 0)), "Nov 10");
    }

    #[test]
    fn older_notes_have_a_date_and_those_of_other_years_the_year() {
        assert_eq!(short((2026, 10, 3), (8, 0)), "Oct 3");
        assert_eq!(short((2026, 1, 1), (0, 0)), "Jan 1");
        assert_eq!(short((2025, 12, 31), (23, 0)), "Dec 2025");
    }

    #[test]
    fn dates_are_the_devices_own() {
        // Half past five in the evening of the 15th, in UTC, is already
        // the 16th eight hours east: a Monday there, a Sunday at home.
        let edited = at((2026, 11, 15), (17, 30), 0);
        let east = clock();
        let home = Clock {
            now_ms: east.now_ms,
            utc_offset_secs: 0,
        };
        assert_eq!(edited_short(edited, east), "Mon");
        assert_eq!(edited_short(edited, home), "Sun");
    }

    #[test]
    fn the_tooltip_says_it_in_full() {
        let full = |date, time| edited_full(at(date, time, 8 * HOUR_SECS), clock());
        assert_eq!(full((2026, 11, 13), (9, 12)), "Edited Nov 13, 09:12");
        assert_eq!(full((2025, 3, 2), (18, 5)), "Edited Mar 2, 2025, 18:05");
    }
}
