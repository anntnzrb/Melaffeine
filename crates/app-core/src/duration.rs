//! Bounded duration parsing and compact, minute-rounded countdown text.

use std::time::Duration;

const MIN_DURATION_VALUE: u64 = 1;
const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 3600;
const SECONDS_PER_DAY: u64 = 86400;
/// Maximum finite session length in seconds: 365 days.
pub const MAX_FINITE_DURATION_SECONDS: u64 = 31_536_000;
const MINUTES_PER_DAY: u64 = 1440;
const MINUTES_PER_HOUR: u64 = 60;
const HOURS_PER_DAY: u64 = 24;

/// Unit applied to an unsigned integer by [`parse_duration`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DurationUnit {
    /// Seconds, with no conversion multiplier.
    Seconds,
    /// Minutes of 60 seconds each.
    Minutes,
    /// Hours of 3,600 seconds each.
    Hours,
    /// Days of 86,400 seconds each.
    Days,
}

/// Parses a positive ASCII integer in the given unit, bounded to 365 days.
///
/// Returns `None` for empty input, whitespace, signs, non-digits, zero,
/// integer overflow, or a result exceeding [`MAX_FINITE_DURATION_SECONDS`].
#[must_use]
pub fn parse_duration(input: &str, unit: DurationUnit) -> Option<Duration> {
    if input.is_empty() || !input.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }

    let value = input.parse::<u64>().ok()?;

    if value < MIN_DURATION_VALUE {
        return None;
    }

    let multiplier = match unit {
        DurationUnit::Seconds => 1,
        DurationUnit::Minutes => SECONDS_PER_MINUTE,
        DurationUnit::Hours => SECONDS_PER_HOUR,
        DurationUnit::Days => SECONDS_PER_DAY,
    };

    let total_seconds = value.checked_mul(multiplier)?;
    if total_seconds > MAX_FINITE_DURATION_SECONDS {
        return None;
    }

    Some(Duration::from_secs(total_seconds))
}

/// Formats a countdown using minutes, hours and minutes, or days and hours.
///
/// Positive durations round up to the next minute before choosing units.
/// Zero is shown as `<1m`; day-based output omits remaining minutes.
/// The conversion saturates at the maximum whole-second duration.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use app_core::format_compact_duration;
///
/// assert_eq!(format_compact_duration(Duration::from_secs(61)), "2m",
///     "partial minutes round up");
/// assert_eq!(format_compact_duration(Duration::from_secs(3660)), "1h 1m",
///     "hour-based countdowns include remaining minutes");
/// ```
#[must_use]
pub fn format_compact_duration(remaining: Duration) -> String {
    let seconds = remaining
        .as_secs()
        .saturating_add(u64::from(remaining.subsec_nanos() != 0));
    let minutes = seconds.div_ceil(SECONDS_PER_MINUTE);
    if minutes == 0 {
        return String::from("<1m");
    }

    let days = minutes / MINUTES_PER_DAY;
    if days > 0 {
        let hours = (minutes / MINUTES_PER_HOUR) % HOURS_PER_DAY;
        format!("{days}d {hours}h")
    } else {
        let hours = minutes / MINUTES_PER_HOUR;
        if hours > 0 {
            let rem_minutes = minutes % MINUTES_PER_HOUR;
            format!("{hours}h {rem_minutes}m")
        } else {
            format!("{minutes}m")
        }
    }
}

/// Parses a duration with a seconds, minutes, hours, or days suffix.
///
/// Trims surrounding whitespace and accepts case-insensitive `s`, `m`, `h`,
/// and `d` suffixes; bare integers mean minutes. The numeric part follows
/// [`parse_duration`]: invalid, zero, overflowing, or over-365-day values
/// return `None`.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use app_core::parse_duration_spec;
///
/// assert_eq!(parse_duration_spec(" 2H "), Some(Duration::from_hours(2)),
///     "suffixes ignore case and surrounding whitespace");
/// assert_eq!(parse_duration_spec("15"), Some(Duration::from_mins(15)),
///     "bare integers mean minutes");
/// assert_eq!(parse_duration_spec("366d"), None,
///     "finite sessions cannot exceed 365 days");
/// ```
#[must_use]
pub fn parse_duration_spec(spec: &str) -> Option<Duration> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (num, unit) = match trimmed.chars().last() {
        Some('s' | 'S') => (trimmed.strip_suffix(['s', 'S'])?, DurationUnit::Seconds),
        Some('m' | 'M') => (trimmed.strip_suffix(['m', 'M'])?, DurationUnit::Minutes),
        Some('h' | 'H') => (trimmed.strip_suffix(['h', 'H'])?, DurationUnit::Hours),
        Some('d' | 'D') => (trimmed.strip_suffix(['d', 'D'])?, DurationUnit::Days),
        _ => (trimmed, DurationUnit::Minutes),
    };

    parse_duration(num, unit)
}
