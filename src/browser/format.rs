//! Turning file metadata into something readable at a glance.

use std::time::{Duration, SystemTime};

/// One minute, in seconds.
const MINUTE: u64 = 60;
const HOUR: u64 = 60 * MINUTE;
const DAY: u64 = 24 * HOUR;
const WEEK: u64 = 7 * DAY;
/// The average length of a month, which is what makes "3mo" mean anything.
const MONTH: u64 = 30 * DAY + 10 * HOUR + 30 * MINUTE;
const YEAR: u64 = 12 * MONTH;

/// How long ago `then` was, in the fewest characters that still say something.
///
/// Deliberately terse: this sits in a column beside a filename, and a reader
/// scanning a list wants the magnitude, not the precision.
#[must_use]
pub fn relative_time(then: SystemTime, now: SystemTime) -> String {
    let elapsed = now
        .duration_since(then)
        // A file stamped in the future is a clock problem, not a file that
        // will be modified later; show it as new rather than as nonsense.
        .unwrap_or(Duration::ZERO)
        .as_secs();
    match elapsed {
        0..MINUTE => "just now".to_owned(),
        secs @ MINUTE..HOUR => format!("{}m ago", secs / MINUTE),
        secs @ HOUR..DAY => format!("{}h ago", secs / HOUR),
        secs @ DAY..WEEK => format!("{}d ago", secs / DAY),
        secs @ WEEK..MONTH => format!("{}w ago", secs / WEEK),
        secs @ MONTH..YEAR => format!("{}mo ago", secs / MONTH),
        secs => format!("{}y ago", secs / YEAR),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ago(secs: u64) -> String {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10 * YEAR);
        relative_time(now - Duration::from_secs(secs), now)
    }

    #[test]
    fn recent_edits_read_as_recent() {
        assert_eq!(ago(0), "just now");
        assert_eq!(ago(59), "just now");
        assert_eq!(ago(60), "1m ago");
        assert_eq!(ago(59 * MINUTE), "59m ago");
    }

    #[test]
    fn each_unit_takes_over_from_the_one_below_it() {
        assert_eq!(ago(HOUR), "1h ago");
        assert_eq!(ago(DAY), "1d ago");
        assert_eq!(ago(WEEK), "1w ago");
        assert_eq!(ago(MONTH), "1mo ago");
        assert_eq!(ago(YEAR), "1y ago");
    }

    #[test]
    fn the_last_moment_of_a_unit_is_still_that_unit() {
        assert_eq!(ago(DAY - 1), "23h ago");
        assert_eq!(ago(WEEK - 1), "6d ago");
        assert_eq!(ago(YEAR - 1), "11mo ago");
    }

    #[test]
    fn a_file_stamped_in_the_future_reads_as_new() {
        // Clock skew across a network mount should not print something absurd.
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        assert_eq!(
            relative_time(now + Duration::from_secs(500), now),
            "just now"
        );
    }
}
