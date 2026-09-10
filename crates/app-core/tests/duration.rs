#![allow(clippy::duration_suboptimal_units)]
use std::time::Duration;

use app_core::{DurationUnit, format_compact_duration, parse_duration};

const ALL_UNITS: [DurationUnit; 4] = [
    DurationUnit::Seconds,
    DurationUnit::Minutes,
    DurationUnit::Hours,
    DurationUnit::Days,
];

#[test]
fn parse_duration_rejects_invalid_inputs() {
    let invalid_inputs = [
        // Empty string and whitespace cases
        "",
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
        for unit in ALL_UNITS {
            assert_eq!(
                parse_duration(input, unit),
                None,
                "expected input {input:?} to be rejected for {unit:?}"
            );
        }
    }

    // Limit boundary rejection (max limit is 365d = 8760h = 525600m = 31536000s)
    for (input, unit) in [
        ("366", DurationUnit::Days),
        ("8761", DurationUnit::Hours),
        ("525601", DurationUnit::Minutes),
        ("31536001", DurationUnit::Seconds),
        ("99000000", DurationUnit::Minutes),
    ] {
        assert_eq!(
            parse_duration(input, unit),
            None,
            "expected {input:?} to be rejected for {unit:?}"
        );
    }
}

#[test]
fn parse_duration_accepts_valid_inputs() {
    let cases = [
        // 1 unit for each variant (multiplier test)
        ("1", DurationUnit::Seconds, 1),
        ("1", DurationUnit::Minutes, 60),
        ("1", DurationUnit::Hours, 3600),
        ("1", DurationUnit::Days, 86_400),
        // Intermediate values
        ("2", DurationUnit::Hours, 7200),
        ("10", DurationUnit::Hours, 36_000),
        ("45", DurationUnit::Minutes, 2700),
        ("7", DurationUnit::Days, 604_800),
        // Leading zeroes on valid numbers
        ("01", DurationUnit::Minutes, 60),
        ("007", DurationUnit::Minutes, 420),
        ("00000000000000000000000000001", DurationUnit::Seconds, 1),
        ("00000000000000000000000000001", DurationUnit::Minutes, 60),
        ("00000000000000000000000000001", DurationUnit::Hours, 3600),
        ("00000000000000000000000000001", DurationUnit::Days, 86_400),
        ("000365", DurationUnit::Days, 31_536_000),
        ("0008760", DurationUnit::Hours, 31_536_000),
        ("000525600", DurationUnit::Minutes, 31_536_000),
        // Exact max boundary inputs -> all produce 31_536_000s
        ("31536000", DurationUnit::Seconds, 31_536_000),
        ("525600", DurationUnit::Minutes, 31_536_000),
        ("8760", DurationUnit::Hours, 31_536_000),
        ("365", DurationUnit::Days, 31_536_000),
    ];

    for (input, unit, secs) in cases {
        assert_eq!(
            parse_duration(input, unit),
            Some(Duration::from_secs(secs)),
            "expected {input:?} to parse as {secs}s for {unit:?}"
        );
    }
}

#[test]
fn format_compact_duration_cases() {
    let cases = [
        // Zero duration
        (Duration::ZERO, "<1m"),
        (Duration::from_secs(0), "<1m"),
        // Positive sub-minute durations round up to 1m
        (Duration::from_nanos(1), "1m"),
        (Duration::from_micros(1), "1m"),
        (Duration::from_millis(1), "1m"),
        (Duration::from_millis(500), "1m"),
        (Duration::from_millis(999), "1m"),
        (Duration::from_nanos(59_999_999_999), "1m"),
        // 1..=59 seconds round up to 1m; 60s is exact
        (Duration::from_secs(1), "1m"),
        (Duration::from_secs(30), "1m"),
        (Duration::from_secs(59), "1m"),
        (Duration::from_secs(60), "1m"),
        // Positive partial minutes round up
        (Duration::from_secs(60) + Duration::from_nanos(1), "2m"),
        (Duration::from_secs(61), "2m"),
        (Duration::from_secs(119), "2m"),
        (Duration::from_secs(120), "2m"),
        (Duration::from_secs(121), "3m"),
        (Duration::from_secs(179), "3m"),
        // Minute and hour boundaries
        (Duration::from_secs(3540), "59m"),   // 59 minutes
        (Duration::from_secs(3541), "1h 0m"), // 59m 1s rounds up
        (Duration::from_secs(3599), "1h 0m"), // 59m 59s rounds up
        (Duration::from_secs(3600), "1h 0m"), // exactly 1 hour
        (Duration::from_secs(3601), "1h 1m"), // 1h 0m 1s rounds up
        (Duration::from_secs(3659), "1h 1m"), // 1h 0m 59s rounds up
        (Duration::from_secs(3660), "1h 1m"), // 1h 1m 0s
        (Duration::from_secs(7140), "1h 59m"),
        (Duration::from_secs(7141), "2h 0m"), // 1h 59m 1s rounds up
        (Duration::from_secs(7199), "2h 0m"), // 1h 59m 59s rounds up
        (Duration::from_secs(7200), "2h 0m"), // 2 hours
        (Duration::from_secs(7201), "2h 1m"), // 2h 0m 1s rounds up
        (Duration::from_secs(7259), "2h 1m"), // 2h 0m 59s rounds up
        (Duration::from_secs(7260), "2h 1m"), // 2h 1m 0s
        (Duration::from_secs(86_340), "23h 59m"),
        (Duration::from_secs(86_399), "1d 0h"), // 23h 59m 59s rounds up
        // Day boundaries
        (Duration::from_secs(86_400), "1d 0h"), // exactly 1 day
        (Duration::from_secs(86_401), "1d 0h"), // 1d 1s -> 1d 0h
        (Duration::from_secs(89_940), "1d 0h"), // 1d 59m -> 1d 0h
        (Duration::from_secs(89_999), "1d 1h"), // 1d 59m 59s rounds up
        (Duration::from_secs(90_000), "1d 1h"), // 1 day 1 hour
        (Duration::from_secs(90_001), "1d 1h"), // 1d 1h 1s -> 1d 1h
        (Duration::from_secs(169_200), "1d 23h"),
        (Duration::from_secs(882_000), "10d 5h"),
        (Duration::from_secs(31_536_000), "365d 0h"),
        // Maximum duration bounds
        (Duration::MAX, "213503982334601d 7h"),
    ];

    for (input, expected) in cases {
        assert_eq!(
            format_compact_duration(input),
            expected,
            "unexpected format for {input:?}"
        );
    }
}
