#![allow(clippy::duration_suboptimal_units, clippy::panic_in_result_fn)]

use std::path::PathBuf;
use std::time::Duration;

use app_core::ipc::{IpcCommand, IpcResponse, SessionStatus, socket_path};
use rustix::process::geteuid;

#[test]
fn socket_path_uses_os_identity() {
    let expected = PathBuf::from(format!("/tmp/melaffeine-{}.sock", geteuid().as_raw()));
    assert_eq!(socket_path(), expected);
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
        (
            IpcCommand::Start {
                duration: None,
                keep_display_awake: true,
            },
            "START indefinite display\n",
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

    // Case-insensitive and shorthand parse-only forms
    for (line, expected) in [
        ("stop\n", IpcCommand::Stop),
        ("status\n", IpcCommand::Status),
        (
            "START\n",
            IpcCommand::Start {
                duration: None,
                keep_display_awake: false,
            },
        ),
        (
            "START display\n",
            IpcCommand::Start {
                duration: None,
                keep_display_awake: true,
            },
        ),
    ] {
        assert_eq!(IpcCommand::parse(line), Some(expected));
    }

    for line in [
        "",
        "   \n",
        "START invalid\n",
        "STOP 1h\n",
        "TOGGLE typo\n",
        "STATUS typo\n",
        "QUIT typo\n",
        "START 1h indefinite\n",
        "START indefinite 1h\n",
        "START 1h 2h\n",
        "START display display\n",
        "START display 2h\n",
        "START display indefinite\n",
        "START 1h display extra\n",
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
            IpcResponse::Status(Some(SessionStatus {
                keep_display_awake: true,
                remaining_compact: Some(String::from("1h 30m")),
            })),
            "STATUS active display 1h 30m\n",
            "Melaffeine: ACTIVE [1h 30m] (display awake)",
        ),
        (
            IpcResponse::Status(Some(SessionStatus {
                keep_display_awake: false,
                remaining_compact: Some(String::from("45m")),
            })),
            "STATUS active nodisplay 45m\n",
            "Melaffeine: ACTIVE [45m]",
        ),
        (
            IpcResponse::Status(Some(SessionStatus {
                keep_display_awake: true,
                remaining_compact: None,
            })),
            "STATUS active display\n",
            "Melaffeine: ACTIVE [active] (display awake)",
        ),
        (
            IpcResponse::Status(Some(SessionStatus {
                keep_display_awake: false,
                remaining_compact: None,
            })),
            "STATUS active nodisplay\n",
            "Melaffeine: ACTIVE [active]",
        ),
        (
            IpcResponse::Status(None),
            "STATUS inactive\n",
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
        // Malformed status lines
        "STATUS",
        "STATUS active",
        "STATUS active maybe",
        "STATUS active maybe 1h 30m",
        "STATUS inactive display",
        "STATUS inactive nodisplay",
        "STATUS inactive 1h 30m",
        "STATUS unknown",
    ] {
        assert_eq!(IpcResponse::parse(line), None, "expected {line:?} rejected");
    }
}
