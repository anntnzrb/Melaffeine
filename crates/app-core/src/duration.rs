use std::time::Duration;

const MIN_DURATION_VALUE: u64 = 1;
const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 3600;
const SECONDS_PER_DAY: u64 = 86400;
pub const MAX_FINITE_DURATION_SECONDS: u64 = 31_536_000;
const MINUTES_PER_DAY: u64 = 1440;
const MINUTES_PER_HOUR: u64 = 60;
const HOURS_PER_DAY: u64 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DurationUnit {
    Seconds,
    Minutes,
    Hours,
    Days,
}

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

/// Helper to parse duration string with flexible suffixes (`m`, `h`, `d`, `s` or bare minutes).
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
