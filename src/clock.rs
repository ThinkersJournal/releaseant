//! The real clock.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::registry::Clock;

/// Seconds since the Unix epoch now (0 if the system clock is before it).
#[must_use]
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Inverse of [`days_from_civil`] (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `2026-10-24T01:27:14Z` for Unix seconds.
#[must_use]
pub fn format_utc(unix: i64) -> String {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

/// Sleeps for real and reads the system clock.
#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn sleep(&mut self, d: Duration) {
        std::thread::sleep(d);
    }

    fn now_utc(&mut self) -> String {
        format_utc(now_unix())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::days_from_civil;

    #[test]
    fn formats_known_instants() {
        // Values computed independently with Python's datetime.
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(1_792_805_234), "2026-10-24T01:27:14Z");
        assert_eq!(format_utc(1_582_977_600), "2020-02-29T12:00:00Z");
        assert_eq!(format_utc(946_684_799), "1999-12-31T23:59:59Z");
    }

    #[test]
    fn round_trips_through_days_from_civil() {
        for days in [-1, 0, 1, 365, 10_000, 20_000, 30_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
    }
}
