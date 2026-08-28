#![allow(
    clippy::clone_on_copy,
    clippy::unreadable_literal,
    clippy::duration_suboptimal_units
)]
use std::collections::HashSet;
use std::time::Duration;

use app_core::{
    DurationUnit, HOURS_PER_DAY, MAX_FINITE_DURATION_DAYS, MAX_FINITE_DURATION_SECONDS,
    MIN_DURATION_VALUE, MINUTES_PER_DAY, MINUTES_PER_HOUR, SECONDS_PER_DAY, SECONDS_PER_HOUR,
    SECONDS_PER_MINUTE, format_compact_duration, parse_duration,
};

#[test]
fn duration_unit_traits_and_constants() {
    // Debug representation
    assert_eq!(format!("{:?}", DurationUnit::Minutes), "Minutes");
    assert_eq!(format!("{:?}", DurationUnit::Hours), "Hours");
    assert_eq!(format!("{:?}", DurationUnit::Days), "Days");

    // Clone and Copy
    let unit = DurationUnit::Minutes;
    let copied = unit;
    let cloned = unit.clone();
    assert_eq!(unit, copied);
    assert_eq!(unit, cloned);

    // PartialEq and Eq
    assert_eq!(DurationUnit::Minutes, DurationUnit::Minutes);
    assert_eq!(DurationUnit::Hours, DurationUnit::Hours);
    assert_eq!(DurationUnit::Days, DurationUnit::Days);
    assert_ne!(DurationUnit::Minutes, DurationUnit::Hours);
    assert_ne!(DurationUnit::Hours, DurationUnit::Days);
    assert_ne!(DurationUnit::Days, DurationUnit::Minutes);

    // Hash support in HashSet
    let mut set = HashSet::new();
    assert!(set.insert(DurationUnit::Minutes));
    assert!(set.insert(DurationUnit::Hours));
    assert!(set.insert(DurationUnit::Days));
    assert!(!set.insert(DurationUnit::Minutes));
    assert_eq!(set.len(), 3);

    // Constants verification
    assert_eq!(MIN_DURATION_VALUE, 1);
    assert_eq!(MAX_FINITE_DURATION_DAYS, 365);
    assert_eq!(SECONDS_PER_MINUTE, 60);
    assert_eq!(SECONDS_PER_HOUR, 3600);
    assert_eq!(SECONDS_PER_DAY, 86400);
    assert_eq!(MAX_FINITE_DURATION_SECONDS, 31_536_000);
    assert_eq!(MINUTES_PER_DAY, 1440);
    assert_eq!(MINUTES_PER_HOUR, 60);
    assert_eq!(HOURS_PER_DAY, 24);

    // Mathematical consistency between units and limits
    assert_eq!(SECONDS_PER_MINUTE * 60, SECONDS_PER_HOUR);
    assert_eq!(SECONDS_PER_HOUR * 24, SECONDS_PER_DAY);
    assert_eq!(MINUTES_PER_HOUR * 24, MINUTES_PER_DAY);
    assert_eq!(
        SECONDS_PER_DAY * MAX_FINITE_DURATION_DAYS,
        MAX_FINITE_DURATION_SECONDS
    );

    // Maximum input values for each unit mapping to MAX_FINITE_DURATION_SECONDS (31_536_000s)
    let max_minutes = MAX_FINITE_DURATION_SECONDS / SECONDS_PER_MINUTE; // 525,600
    let max_hours = MAX_FINITE_DURATION_SECONDS / SECONDS_PER_HOUR; // 8,760
    let max_days = MAX_FINITE_DURATION_SECONDS / SECONDS_PER_DAY; // 365

    assert_eq!(max_minutes, 525_600);
    assert_eq!(max_hours, 8_760);
    assert_eq!(max_days, 365);
}

#[test]
fn parse_duration_rejects_invalid_inputs() {
    let invalid_inputs = [
        // Empty string
        "",
        // Whitespace cases
        " 1",
        "1 ",
        " 1 ",
        "\t1",
        "1\n",
        "\r\n1",
        " 100 ",
        "  ",
        "\t",
        "\n",
        // Sign cases
        "-1",
        "+1",
        "-0",
        "+0",
        "-525600",
        "+100",
        // Non-numeric strings
        "abc",
        "1a",
        "a1",
        "1.0",
        "0.5",
        "1.5",
        "1,000",
        "99_000",
        "!",
        "@#$",
        "1 0",
        "1e3",
        "NaN",
        "inf",
        "-inf",
        "0x10",
        "0b10",
        // Zero cases
        "0",
        "00",
        "000",
        "0000000000000",
        // Massive numeric strings and arithmetic overflow attempts
        "18446744073709551615",                 // u64::MAX
        "18446744073709551616",                 // u64::MAX + 1
        "999999999999999999999999999999999999", // huge number
        "184467440737095516150",
        "340282366920938463463374607431768211455", // u128::MAX
    ];

    for input in invalid_inputs {
        assert_eq!(
            parse_duration(input, DurationUnit::Minutes),
            None,
            "expected input {input:?} to be rejected for Minutes"
        );
        assert_eq!(
            parse_duration(input, DurationUnit::Hours),
            None,
            "expected input {input:?} to be rejected for Hours"
        );
        assert_eq!(
            parse_duration(input, DurationUnit::Days),
            None,
            "expected input {input:?} to be rejected for Days"
        );
    }

    // Limit boundary rejection (max limit is 365 days = 8760 hours = 525600 minutes)
    assert_eq!(
        parse_duration("366", DurationUnit::Days),
        None,
        "expected 366 days to be rejected"
    );
    assert_eq!(
        parse_duration("8761", DurationUnit::Hours),
        None,
        "expected 8761 hours to be rejected"
    );
    assert_eq!(
        parse_duration("525601", DurationUnit::Minutes),
        None,
        "expected 525601 minutes to be rejected"
    );
    assert_eq!(
        parse_duration("99000000", DurationUnit::Minutes),
        None,
        "expected huge minutes value to be rejected"
    );
}

#[test]
#[allow(clippy::duration_suboptimal_units)]
fn parse_duration_accepts_valid_inputs() {
    // 1 unit for each variant (multiplier test)
    assert_eq!(
        parse_duration("1", DurationUnit::Minutes),
        Some(Duration::from_secs(60))
    );
    assert_eq!(
        parse_duration("1", DurationUnit::Hours),
        Some(Duration::from_secs(3600))
    );
    assert_eq!(
        parse_duration("1", DurationUnit::Days),
        Some(Duration::from_secs(86400))
    );

    // Intermediate values
    assert_eq!(
        parse_duration("2", DurationUnit::Hours),
        Some(Duration::from_secs(7200))
    );
    assert_eq!(
        parse_duration("10", DurationUnit::Hours),
        Some(Duration::from_secs(36000))
    );
    assert_eq!(
        parse_duration("45", DurationUnit::Minutes),
        Some(Duration::from_secs(2700))
    );
    assert_eq!(
        parse_duration("7", DurationUnit::Days),
        Some(Duration::from_secs(604800))
    );

    // Leading zeroes on valid numbers
    assert_eq!(
        parse_duration("01", DurationUnit::Minutes),
        Some(Duration::from_secs(60))
    );
    assert_eq!(
        parse_duration("007", DurationUnit::Minutes),
        Some(Duration::from_secs(420))
    );
    assert_eq!(
        parse_duration("00000000000000000000000000001", DurationUnit::Minutes),
        Some(Duration::from_secs(60))
    );
    assert_eq!(
        parse_duration("00000000000000000000000000001", DurationUnit::Hours),
        Some(Duration::from_secs(3600))
    );
    assert_eq!(
        parse_duration("00000000000000000000000000001", DurationUnit::Days),
        Some(Duration::from_secs(86400))
    );
    assert_eq!(
        parse_duration("000365", DurationUnit::Days),
        Some(Duration::from_secs(31_536_000))
    );
    assert_eq!(
        parse_duration("0008760", DurationUnit::Hours),
        Some(Duration::from_secs(31_536_000))
    );
    assert_eq!(
        parse_duration("000525600", DurationUnit::Minutes),
        Some(Duration::from_secs(31_536_000))
    );

    // Exact max boundary inputs -> all produce 31_536_000s
    assert_eq!(
        parse_duration("525600", DurationUnit::Minutes),
        Some(Duration::from_secs(31_536_000))
    );
    assert_eq!(
        parse_duration("8760", DurationUnit::Hours),
        Some(Duration::from_secs(31_536_000))
    );
    assert_eq!(
        parse_duration("365", DurationUnit::Days),
        Some(Duration::from_secs(31_536_000))
    );
}

#[test]
#[allow(clippy::duration_suboptimal_units)]
fn format_compact_duration_cases() {
    // Zero duration
    assert_eq!(format_compact_duration(Duration::ZERO), "<1m");
    assert_eq!(format_compact_duration(Duration::from_secs(0)), "<1m");

    // Sub-second durations
    assert_eq!(format_compact_duration(Duration::from_nanos(1)), "1m");
    assert_eq!(format_compact_duration(Duration::from_micros(1)), "1m");
    assert_eq!(format_compact_duration(Duration::from_millis(1)), "1m");
    assert_eq!(format_compact_duration(Duration::from_millis(500)), "1m");
    assert_eq!(format_compact_duration(Duration::from_millis(999)), "1m");
    assert_eq!(
        format_compact_duration(Duration::from_nanos(59_999_999_999)),
        "1m"
    );

    // 1..59 seconds
    assert_eq!(format_compact_duration(Duration::from_secs(1)), "1m");
    assert_eq!(format_compact_duration(Duration::from_secs(30)), "1m");
    assert_eq!(format_compact_duration(Duration::from_secs(59)), "1m");

    // 60 seconds
    assert_eq!(format_compact_duration(Duration::from_secs(60)), "1m");
    assert_eq!(
        format_compact_duration(Duration::from_secs(60) + Duration::from_nanos(1)),
        "2m"
    );

    // 61..120 seconds
    assert_eq!(format_compact_duration(Duration::from_secs(61)), "2m");
    assert_eq!(format_compact_duration(Duration::from_secs(119)), "2m");
    assert_eq!(format_compact_duration(Duration::from_secs(120)), "2m");
    assert_eq!(format_compact_duration(Duration::from_secs(121)), "3m");

    // Minute and hour boundaries
    assert_eq!(format_compact_duration(Duration::from_secs(3540)), "59m"); // 59 minutes
    assert_eq!(format_compact_duration(Duration::from_secs(3541)), "1h 0m"); // 59m 1s -> ceil 60m = 1h 0m
    assert_eq!(format_compact_duration(Duration::from_secs(3599)), "1h 0m"); // 59m 59s -> ceil 60m = 1h 0m
    assert_eq!(format_compact_duration(Duration::from_secs(3600)), "1h 0m"); // exactly 1 hour
    assert_eq!(format_compact_duration(Duration::from_secs(3601)), "1h 1m"); // 1h 1s -> ceil 1h 1m
    assert_eq!(format_compact_duration(Duration::from_secs(7140)), "1h 59m"); // 1h 59m
    assert_eq!(format_compact_duration(Duration::from_secs(7141)), "2h 0m"); // 1h 59m 1s -> ceil 2h 0m
    assert_eq!(format_compact_duration(Duration::from_secs(7200)), "2h 0m"); // 2 hours
    assert_eq!(format_compact_duration(Duration::from_secs(7201)), "2h 1m"); // 2h 1s -> ceil 2h 1m
    assert_eq!(
        format_compact_duration(Duration::from_secs(86340)),
        "23h 59m"
    ); // 23h 59m
    assert_eq!(format_compact_duration(Duration::from_secs(86341)), "1d 0h"); // 23h 59m 1s -> ceil 24h 0m = 1d 0h

    // Day boundaries
    assert_eq!(format_compact_duration(Duration::from_secs(86400)), "1d 0h"); // exactly 1 day
    assert_eq!(format_compact_duration(Duration::from_secs(86401)), "1d 0h"); // 1d 1s -> 1441m -> 1d 0h
    assert_eq!(format_compact_duration(Duration::from_secs(89940)), "1d 0h"); // 1d 59m -> 1499m -> 1d 0h
    assert_eq!(format_compact_duration(Duration::from_secs(89941)), "1d 1h"); // 1d 59m 1s -> 1500m -> 1d 1h
    assert_eq!(format_compact_duration(Duration::from_secs(90000)), "1d 1h"); // 1 day 1 hour
    assert_eq!(format_compact_duration(Duration::from_secs(90001)), "1d 1h"); // 1d 1h 1s -> 1501m -> 1d 1h
    assert_eq!(
        format_compact_duration(Duration::from_secs(169200)),
        "1d 23h"
    ); // 1 day 23 hours
    assert_eq!(
        format_compact_duration(Duration::from_secs(882000)),
        "10d 5h"
    ); // 10 days 5 hours
    assert_eq!(
        format_compact_duration(Duration::from_secs(31_536_000)),
        "365d 0h"
    ); // 365 days 0 hours

    // Maximum duration bounds
    assert_eq!(
        format_compact_duration(Duration::MAX),
        "213503982334601d 7h"
    );
}
