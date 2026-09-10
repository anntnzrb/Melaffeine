#![allow(clippy::duration_suboptimal_units, clippy::panic_in_result_fn)]

use std::process::Command;
use std::time::Duration;

use app_core::ipc::{IpcCommand, IpcResponse, parse_duration_spec, socket_path};
use rustix::process::geteuid;

#[test]
fn socket_path_uses_os_identity() -> Result<(), Box<dyn std::error::Error>> {
    let expected = format!("/tmp/melaffeine-{}.sock", geteuid().as_raw());
    assert_eq!(socket_path().to_string_lossy().into_owned(), expected);

    let executable = std::env::current_exe()?;
    let output = Command::new(executable)
        .args(["--exact", "socket_path_reports_child_value", "--nocapture"])
        .env("UID", "not-the-effective-uid")
        .env("USER", "not-the-current-user")
        .env("TMPDIR", "/var/tmp")
        .output()?;
    assert!(
        output.status.success(),
        "child test process failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let child_stdout = String::from_utf8_lossy(&output.stdout);
    let child_path = child_stdout
        .lines()
        .find_map(|line| line.split_once("SOCKET_PATH=").map(|(_, path)| path));
    assert_eq!(child_path, Some(expected.as_str()));
    Ok(())
}

#[test]
fn socket_path_reports_child_value() {
    println!("SOCKET_PATH={}", socket_path().display());
}

#[test]
fn test_parse_duration_spec() {
    let cases = [
        ("30s", Some(30)),
        ("30S", Some(30)),
        ("15m", Some(900)),
        ("15M", Some(900)),
        ("2h", Some(7200)),
        ("2H", Some(7200)),
        ("1d", Some(86_400)),
        ("1D", Some(86_400)),
        ("10", Some(600)), // Bare number defaults to minutes
        ("", None),
        ("0s", None),
        ("0m", None),
        ("0h", None),
        ("0d", None),
        ("31536001s", None),
        ("+30s", None),
        ("+30m", None),
        ("invalid", None),
    ];

    for (spec, expected_secs) in cases {
        assert_eq!(
            parse_duration_spec(spec),
            expected_secs.map(Duration::from_secs),
            "unexpected result for spec {spec:?}"
        );
    }
}

#[test]
fn test_ipc_command_serialize_and_parse() {
    let round_trips = [
        (
            IpcCommand::Start {
                duration: Some(Duration::from_secs(3600)),
                keep_display_awake: false,
            },
            "START 3600s\n",
        ),
        (
            IpcCommand::Start {
                duration: Some(Duration::from_secs(7200)),
                keep_display_awake: true,
            },
            "START 7200s display\n",
        ),
        (
            IpcCommand::Start {
                duration: None,
                keep_display_awake: false,
            },
            "START indefinite\n",
        ),
        (IpcCommand::Stop, "STOP\n"),
        (IpcCommand::Toggle, "TOGGLE\n"),
        (IpcCommand::Status, "STATUS\n"),
        (IpcCommand::Quit, "QUIT\n"),
    ];

    for (command, wire) in round_trips {
        assert_eq!(command.serialize(), wire, "bad wire format for {command:?}");
        assert_eq!(
            IpcCommand::parse(wire),
            Some(command),
            "wire line {wire:?} did not round-trip"
        );
    }

    // Case-insensitive parse-only forms
    for (line, expected) in [
        ("stop\n", IpcCommand::Stop),
        ("status\n", IpcCommand::Status),
    ] {
        assert_eq!(IpcCommand::parse(line), Some(expected));
    }

    for line in [
        "START invalid\n",
        "STOP 1h\n",
        "TOGGLE typo\n",
        "STATUS typo\n",
        "QUIT typo\n",
        "START 1h indefinite\n",
        "START indefinite 1h\n",
        "START 1h 2h\n",
        "START display display\n",
    ] {
        assert_eq!(IpcCommand::parse(line), None, "expected {line:?} rejected");
    }
}

#[test]
fn test_ipc_response_serialize_parse_display() {
    let cases = [
        (
            IpcResponse::Ok(String::from("Session started")),
            "OK Session started\n",
            "Session started",
        ),
        (
            IpcResponse::Err(String::from("Something broke")),
            "ERR Something broke\n",
            "Error: Something broke",
        ),
        // Empty messages round-trip via bare "OK"/"ERR" tokens
        (IpcResponse::Ok(String::new()), "OK \n", ""),
        (IpcResponse::Err(String::new()), "ERR \n", "Error: "),
        (
            IpcResponse::Status {
                is_active: true,
                keep_display_awake: true,
                ends_at_unix: Some(1_724_850_000),
                remaining_compact: Some(String::from("1h 30m")),
            },
            "STATUS active=true display=true ends_at=1724850000 remaining=1h_30m\n",
            "Melaffeine: ACTIVE [1h 30m] (display awake)",
        ),
        (
            IpcResponse::Status {
                is_active: true,
                keep_display_awake: false,
                ends_at_unix: None,
                remaining_compact: None,
            },
            "STATUS active=true display=false ends_at=none remaining=none\n",
            "Melaffeine: ACTIVE [active]",
        ),
        (
            IpcResponse::Status {
                is_active: false,
                keep_display_awake: false,
                ends_at_unix: None,
                remaining_compact: None,
            },
            "STATUS active=false display=false ends_at=none remaining=none\n",
            "Melaffeine: INACTIVE",
        ),
    ];

    for (response, wire, display) in cases {
        assert_eq!(
            response.serialize(),
            wire,
            "bad wire format for {response:?}"
        );
        assert_eq!(format!("{response}"), display, "bad display for {wire:?}");
        assert_eq!(
            IpcResponse::parse(wire),
            Some(response),
            "wire line {wire:?} did not round-trip"
        );
    }

    for line in [
        "",
        "UNKNOWN response",
        // Token boundaries: bare prefixes without separator are rejected
        "OKAY",
        "ERROR",
        // Invalid field values
        "STATUS active=invalid display=false ends_at=123 remaining=1h_30m",
        "STATUS active=true display=invalid ends_at=123 remaining=1h_30m",
        "STATUS active=true display=false ends_at=notanumber remaining=none",
        // Missing required status fields
        "STATUS display=false ends_at=123 remaining=1h_30m",
        "STATUS active=true ends_at=123 remaining=1h_30m",
        "STATUS active=true display=false remaining=1h_30m",
        "STATUS active=true display=false ends_at=123",
        // Duplicate status fields
        "STATUS active=true active=false display=false ends_at=123 remaining=none",
        "STATUS active=true display=false display=true ends_at=123 remaining=none",
        "STATUS active=true display=false ends_at=123 ends_at=456 remaining=none",
        "STATUS active=true display=false ends_at=123 remaining=none remaining=1h",
        // Unknown field
        "STATUS active=true display=false ends_at=123 remaining=none unexpected=value",
    ] {
        assert_eq!(IpcResponse::parse(line), None, "expected {line:?} rejected");
    }
}
