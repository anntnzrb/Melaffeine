use std::time::Duration;

use app_core::{DurationUnit, format_compact_duration, parse_duration};

#[test]
fn parse_duration_rejects_invalid_inputs() {
    let invalid_general = ["", "0", "-1", "1.5", "abc", "99,000", " 1"];
    for input in invalid_general {
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
    assert_eq!(
        parse_duration("1", DurationUnit::Minutes),
        Some(Duration::from_secs(60))
    );
    assert_eq!(
        parse_duration("2", DurationUnit::Hours),
        Some(Duration::from_secs(7200))
    );
    assert_eq!(
        parse_duration("1", DurationUnit::Days),
        Some(Duration::from_secs(86400))
    );
    assert_eq!(
        parse_duration("365", DurationUnit::Days),
        Some(Duration::from_secs(31_536_000))
    );
    assert_eq!(
        parse_duration("525600", DurationUnit::Minutes),
        Some(Duration::from_secs(31_536_000))
    );
    assert_eq!(
        parse_duration("007", DurationUnit::Minutes),
        Some(Duration::from_secs(420))
    );
    assert_eq!(
        parse_duration("10", DurationUnit::Hours),
        Some(Duration::from_secs(36000))
    );
}

#[test]
#[allow(clippy::duration_suboptimal_units)]
fn format_compact_duration_cases() {
    assert_eq!(format_compact_duration(Duration::from_secs(0)), "<1m");
    assert_eq!(format_compact_duration(Duration::from_millis(500)), "1m");
    assert_eq!(format_compact_duration(Duration::from_secs(59)), "1m");
    assert_eq!(format_compact_duration(Duration::from_secs(60)), "1m");
    assert_eq!(format_compact_duration(Duration::from_secs(61)), "2m");
    assert_eq!(format_compact_duration(Duration::from_secs(3600)), "1h 0m");
    assert_eq!(format_compact_duration(Duration::from_secs(7200)), "2h 0m");
    assert_eq!(format_compact_duration(Duration::from_secs(86400)), "1d 0h");
    assert_eq!(format_compact_duration(Duration::from_secs(90000)), "1d 1h");
}
