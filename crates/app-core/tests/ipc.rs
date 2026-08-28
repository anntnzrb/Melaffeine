#![allow(
    clippy::unreadable_literal,
    clippy::duration_suboptimal_units,
    clippy::similar_names
)]

use std::time::Duration;

use app_core::ipc::{IpcCommand, IpcResponse, parse_duration_spec, socket_path};

#[test]
fn test_socket_path() {
    let path = socket_path();
    assert!(path.to_string_lossy().contains("melaffeine"));
    assert!(path.to_string_lossy().ends_with(".sock"));
}

#[test]
fn test_parse_duration_spec() {
    assert_eq!(parse_duration_spec("30s"), Some(Duration::from_secs(30)));
    assert_eq!(parse_duration_spec("30S"), Some(Duration::from_secs(30)));
    assert_eq!(parse_duration_spec("15m"), Some(Duration::from_secs(900)));
    assert_eq!(parse_duration_spec("15M"), Some(Duration::from_secs(900)));
    assert_eq!(parse_duration_spec("2h"), Some(Duration::from_secs(7200)));
    assert_eq!(parse_duration_spec("2H"), Some(Duration::from_secs(7200)));
    assert_eq!(parse_duration_spec("1d"), Some(Duration::from_secs(86400)));
    assert_eq!(parse_duration_spec("1D"), Some(Duration::from_secs(86400)));
    assert_eq!(parse_duration_spec("10"), Some(Duration::from_secs(600))); // Bare number defaults to minutes

    assert_eq!(parse_duration_spec(""), None);
    assert_eq!(parse_duration_spec("0s"), None);
    assert_eq!(parse_duration_spec("0m"), None);
    assert_eq!(parse_duration_spec("0h"), None);
    assert_eq!(parse_duration_spec("0d"), None);
    assert_eq!(parse_duration_spec("31536001s"), None);
    assert_eq!(parse_duration_spec("invalid"), None);
}
#[test]
fn test_ipc_command_serialize_and_parse() {
    // Start finite
    let cmd = IpcCommand::Start {
        duration: Some(Duration::from_secs(3600)),
        keep_display_awake: false,
    };
    let s = cmd.serialize();
    assert_eq!(s, "START 3600s\n");
    assert_eq!(IpcCommand::parse(&s), Some(cmd));

    // Start with display
    let cmd_disp = IpcCommand::Start {
        duration: Some(Duration::from_secs(7200)),
        keep_display_awake: true,
    };
    let s_disp = cmd_disp.serialize();
    assert_eq!(s_disp, "START 7200s display\n");
    assert_eq!(IpcCommand::parse(&s_disp), Some(cmd_disp));

    // Start indefinite
    let cmd_indef = IpcCommand::Start {
        duration: None,
        keep_display_awake: false,
    };
    let s_indef = cmd_indef.serialize();
    assert_eq!(s_indef, "START indefinite\n");
    assert_eq!(IpcCommand::parse(&s_indef), Some(cmd_indef));

    // Stop, Toggle, Status, Quit serialization & parse
    let stop = IpcCommand::Stop;
    assert_eq!(stop.serialize(), "STOP\n");
    assert_eq!(IpcCommand::parse(&stop.serialize()), Some(stop));

    let toggle = IpcCommand::Toggle;
    assert_eq!(toggle.serialize(), "TOGGLE\n");
    assert_eq!(IpcCommand::parse(&toggle.serialize()), Some(toggle));

    let status = IpcCommand::Status;
    assert_eq!(status.serialize(), "STATUS\n");
    assert_eq!(IpcCommand::parse(&status.serialize()), Some(status));

    let quit = IpcCommand::Quit;
    assert_eq!(quit.serialize(), "QUIT\n");
    assert_eq!(IpcCommand::parse(&quit.serialize()), Some(quit));
    // Case insensitivity
    assert_eq!(IpcCommand::parse("stop\n"), Some(IpcCommand::Stop));
    assert_eq!(IpcCommand::parse("status\n"), Some(IpcCommand::Status));

    // Invalid commands
    assert_eq!(IpcCommand::parse(""), None);
    assert_eq!(IpcCommand::parse("UNKNOWN\n"), None);
    assert_eq!(IpcCommand::parse("START invalid\n"), None);
}

#[test]
fn test_ipc_response_serialize_parse_display() {
    let ok = IpcResponse::Ok(String::from("Session started"));
    let s_ok = ok.serialize();
    assert_eq!(s_ok, "OK Session started\n");
    assert_eq!(IpcResponse::parse(&s_ok), Some(ok.clone()));
    assert_eq!(format!("{ok}"), "Session started");

    let err = IpcResponse::Err(String::from("Something broke"));
    let s_err = err.serialize();
    assert_eq!(s_err, "ERR Something broke\n");
    assert_eq!(IpcResponse::parse(&s_err), Some(err.clone()));
    assert_eq!(format!("{err}"), "Error: Something broke");

    let status_active = IpcResponse::Status {
        is_active: true,
        keep_display_awake: true,
        ends_at_unix: Some(1724850000),
        remaining_compact: Some(String::from("1h 30m")),
    };
    let s_status = status_active.serialize();
    assert!(s_status.contains("active=true"));
    assert!(s_status.contains("display=true"));
    assert!(s_status.contains("remaining=1h_30m"));
    assert_eq!(IpcResponse::parse(&s_status), Some(status_active.clone()));
    assert!(format!("{status_active}").contains("ACTIVE [1h 30m] (display awake)"));

    // Active status without display awake and without remaining
    let status_active_no_disp = IpcResponse::Status {
        is_active: true,
        keep_display_awake: false,
        ends_at_unix: None,
        remaining_compact: None,
    };
    let s_no_disp = status_active_no_disp.serialize();
    assert!(s_no_disp.contains("ends_at=none"));
    assert!(s_no_disp.contains("remaining=none"));
    assert_eq!(
        IpcResponse::parse(&s_no_disp),
        Some(status_active_no_disp.clone())
    );
    assert_eq!(
        format!("{status_active_no_disp}"),
        "Melaffeine: ACTIVE [active]"
    );

    let status_inactive = IpcResponse::Status {
        is_active: false,
        keep_display_awake: false,
        ends_at_unix: None,
        remaining_compact: None,
    };
    assert_eq!(format!("{status_inactive}"), "Melaffeine: INACTIVE");

    // Invalid response lines
    assert_eq!(IpcResponse::parse(""), None);
    assert_eq!(IpcResponse::parse("UNKNOWN response"), None);
    assert_eq!(
        IpcResponse::parse("STATUS active=invalid ends_at=notanumber remaining=none"),
        Some(IpcResponse::Status {
            is_active: false,
            keep_display_awake: false,
            ends_at_unix: None,
            remaining_compact: None,
        })
    );
}
