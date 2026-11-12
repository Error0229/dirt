//! Shared relative-time formatting utilities.

const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 60 * MINUTE_MS;
const DAY_MS: i64 = 24 * HOUR_MS;
const WEEK_MS: i64 = 7 * DAY_MS;

/// Format a timestamp as a short relative label (e.g. "now", "5m", "3h", "2d", "1w").
#[must_use]
pub fn format_short_time(timestamp_ms: i64) -> String {
    format_short_time_at(timestamp_ms, chrono::Utc::now().timestamp_millis())
}

/// The label for `timestamp_ms` as seen at `now_ms`. Taking the current time
/// as a value keeps the boundaries testable: a test that read the clock
/// itself would disagree with this function whenever a millisecond passed
/// between the two readings.
fn format_short_time_at(timestamp_ms: i64, now_ms: i64) -> String {
    let delta = (now_ms - timestamp_ms).max(0);

    if delta < MINUTE_MS {
        return "now".to_string();
    }
    if delta < HOUR_MS {
        return format!("{}m", delta / MINUTE_MS);
    }
    if delta < DAY_MS {
        return format!("{}h", delta / HOUR_MS);
    }
    if delta < WEEK_MS {
        return format!("{}d", delta / DAY_MS);
    }
    format!("{}w", delta / WEEK_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed "now", so that no test depends on the clock.
    const NOW_MS: i64 = 1_800_000_000_000;

    fn label(age_ms: i64) -> String {
        format_short_time_at(NOW_MS - age_ms, NOW_MS)
    }

    #[test]
    fn short_time_just_now() {
        assert_eq!(label(0), "now");
        assert_eq!(
            format_short_time(chrono::Utc::now().timestamp_millis()),
            "now"
        );
    }

    #[test]
    fn short_time_boundary_under_one_minute() {
        assert_eq!(label(MINUTE_MS - 1), "now");
    }

    #[test]
    fn short_time_exactly_one_minute() {
        assert_eq!(label(MINUTE_MS), "1m");
    }

    #[test]
    fn short_time_hours() {
        assert_eq!(label(3 * HOUR_MS), "3h");
    }

    #[test]
    fn short_time_days() {
        assert_eq!(label(2 * DAY_MS), "2d");
    }

    #[test]
    fn short_time_weeks() {
        assert_eq!(label(3 * WEEK_MS), "3w");
    }

    #[test]
    fn short_time_future_timestamp_clamps_to_now() {
        assert_eq!(label(-60_000), "now");
    }
}
