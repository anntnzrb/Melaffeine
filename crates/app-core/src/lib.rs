#![forbid(unsafe_code)]

pub mod duration;

pub use duration::{
    DurationUnit, HOURS_PER_DAY, MAX_FINITE_DURATION_DAYS, MAX_FINITE_DURATION_SECONDS,
    MIN_DURATION_VALUE, MINUTES_PER_DAY, MINUTES_PER_HOUR, SECONDS_PER_DAY, SECONDS_PER_HOUR,
    SECONDS_PER_MINUTE, format_compact_duration, parse_duration,
};
