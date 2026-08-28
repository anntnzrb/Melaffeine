use std::time::Duration;

pub const MIN_DURATION_VALUE: u64 = 1;
pub const MAX_FINITE_DURATION_DAYS: u64 = 365;
pub const SECONDS_PER_MINUTE: u64 = 60;
pub const SECONDS_PER_HOUR: u64 = 3600;
pub const SECONDS_PER_DAY: u64 = 86400;
pub const MAX_FINITE_DURATION_SECONDS: u64 = 31_536_000;
pub const MINUTES_PER_DAY: u64 = 1440;
pub const MINUTES_PER_HOUR: u64 = 60;
pub const HOURS_PER_DAY: u64 = 24;

const NANOS_PER_SECOND: u128 = 1_000_000_000;
const NANOS_PER_MINUTE: u128 = 60 * NANOS_PER_SECOND;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DurationUnit {
    Minutes,
    Hours,
    Days,
}

#[must_use]
pub fn parse_duration(input: &str, unit: DurationUnit) -> Option<Duration> {
    if input.is_empty() {
        return None;
    }

    let mut value: u64 = 0;
    for b in input.bytes() {
        let digit = match b {
            b'0' => 0_u64,
            b'1' => 1_u64,
            b'2' => 2_u64,
            b'3' => 3_u64,
            b'4' => 4_u64,
            b'5' => 5_u64,
            b'6' => 6_u64,
            b'7' => 7_u64,
            b'8' => 8_u64,
            b'9' => 9_u64,
            _ => return None,
        };
        value = value.checked_mul(10)?.checked_add(digit)?;
    }

    if value < MIN_DURATION_VALUE {
        return None;
    }

    let multiplier = match unit {
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
    let nanos = remaining.as_nanos();
    if nanos == 0 {
        return String::from("<1m");
    }

    let div = nanos.checked_div(NANOS_PER_MINUTE).unwrap_or(0);
    let rem = nanos.checked_rem(NANOS_PER_MINUTE).unwrap_or(0);
    let add_ceil = u64::from(rem > 0);

    let minutes = u64::try_from(div).map_or(u64::MAX, |m| m.saturating_add(add_ceil));

    if minutes == 0 {
        return String::from("<1m");
    }

    let days = minutes.checked_div(MINUTES_PER_DAY).unwrap_or(0);
    let hours = minutes
        .checked_div(MINUTES_PER_HOUR)
        .map_or(0, |h| h.checked_rem(HOURS_PER_DAY).unwrap_or(0));
    let rem_minutes = minutes.checked_rem(MINUTES_PER_HOUR).unwrap_or(0);

    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {rem_minutes}m")
    } else {
        format!("{rem_minutes}m")
    }
}
