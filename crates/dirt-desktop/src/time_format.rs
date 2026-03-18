//! Shared relative-time formatting utilities.

const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 60 * MINUTE_MS;
const DAY_MS: i64 = 24 * HOUR_MS;
const WEEK_MS: i64 = 7 * DAY_MS;

/// Format a timestamp as a short relative label (e.g. "now", "5m", "3h", "2d", "1w").
#[must_use]
pub fn format_short_time(timestamp_ms: i64) -> String {
    let delta = delta_ms(timestamp_ms);

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

/// Format a timestamp as a relative label with suffix (e.g. "just now", "5m ago").
#[must_use]
pub fn format_relative_time(timestamp_ms: i64) -> String {
    let delta = delta_ms(timestamp_ms);

    if delta < MINUTE_MS {
        return "just now".to_string();
    }
    if delta < HOUR_MS {
        return format!("{}m ago", delta / MINUTE_MS);
    }
    if delta < DAY_MS {
        return format!("{}h ago", delta / HOUR_MS);
    }
    if delta < WEEK_MS {
        return format!("{}d ago", delta / DAY_MS);
    }
    format!("{}w ago", delta / WEEK_MS)
}

fn delta_ms(timestamp_ms: i64) -> i64 {
    let now_ms = chrono::Utc::now().timestamp_millis();
    (now_ms - timestamp_ms).max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now_ms() -> i64 {
        chrono::Utc::now().timestamp_millis()
    }

    #[test]
    fn short_time_just_now() {
        assert_eq!(format_short_time(now_ms()), "now");
    }

    #[test]
    fn short_time_boundary_under_one_minute() {
        let ts = now_ms() - (MINUTE_MS - 1);
        assert_eq!(format_short_time(ts), "now");
    }

    #[test]
    fn short_time_exactly_one_minute() {
        let ts = now_ms() - MINUTE_MS;
        assert_eq!(format_short_time(ts), "1m");
    }

    #[test]
    fn short_time_hours() {
        let ts = now_ms() - (3 * HOUR_MS);
        assert_eq!(format_short_time(ts), "3h");
    }

    #[test]
    fn short_time_days() {
        let ts = now_ms() - (2 * DAY_MS);
        assert_eq!(format_short_time(ts), "2d");
    }

    #[test]
    fn short_time_weeks() {
        let ts = now_ms() - (3 * WEEK_MS);
        assert_eq!(format_short_time(ts), "3w");
    }

    #[test]
    fn short_time_future_timestamp_clamps_to_now() {
        let ts = now_ms() + 60_000;
        assert_eq!(format_short_time(ts), "now");
    }

    #[test]
    fn relative_time_just_now() {
        assert_eq!(format_relative_time(now_ms()), "just now");
    }

    #[test]
    fn relative_time_minutes() {
        let ts = now_ms() - MINUTE_MS;
        assert_eq!(format_relative_time(ts), "1m ago");
    }

    #[test]
    fn relative_time_weeks() {
        let ts = now_ms() - (3 * WEEK_MS);
        assert_eq!(format_relative_time(ts), "3w ago");
    }
}
