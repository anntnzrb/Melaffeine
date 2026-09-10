//! Melaffeine CLI controller for managing sleep prevention sessions.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;
use std::time::Duration;

use app_core::ipc::{IpcCommand, IpcResponse, parse_duration_spec, socket_path};
use clap::{Parser, Subcommand};

/// Maximum bytes read for a single IPC response line.
const MAX_RESPONSE_BYTES: u64 = 4_097;

#[derive(Parser, Debug)]
#[command(
    name = "melaffeine",
    author,
    version,
    about = "Control Melaffeine sleep prevention from the command line"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start sleep prevention
    Start {
        /// Duration (e.g. 2h, 45m, 1d, 3600s). Defaults to indefinite if omitted.
        duration: Option<String>,

        /// Keep display awake too
        #[arg(short, long)]
        display: bool,

        /// Run indefinitely
        #[arg(short, long)]
        indefinite: bool,
    },
    /// Stop active sleep prevention session
    Stop,
    /// Toggle active sleep prevention state
    Toggle,
    /// Query status of the running Melaffeine application
    Status,
    /// Request Melaffeine application termination
    Quit,
}

fn main() -> ExitCode {
    report_result(run_cli(Cli::parse()))
}

fn report_result(result: Result<IpcResponse, String>) -> ExitCode {
    match result {
        Ok(IpcResponse::Err(error)) => {
            eprintln!("Error: {error}");
            ExitCode::FAILURE
        }
        Ok(response) => {
            println!("{response}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run_cli(cli: Cli) -> Result<IpcResponse, String> {
    let command = match cli.command {
        Commands::Start {
            duration,
            display,
            indefinite,
        } => {
            let parsed_duration: Option<Duration> = if indefinite {
                None
            } else if let Some(spec) = duration {
                let Some(dur) = parse_duration_spec(&spec) else {
                    return Err(format!(
                        "Error: Invalid duration specification '{spec}'. Use format like 2h, 45m, 1d."
                    ));
                };
                Some(dur)
            } else {
                None
            };
            IpcCommand::Start {
                duration: parsed_duration,
                keep_display_awake: display,
            }
        }
        Commands::Stop => IpcCommand::Stop,
        Commands::Toggle => IpcCommand::Toggle,
        Commands::Status => IpcCommand::Status,
        Commands::Quit => IpcCommand::Quit,
    };

    send_ipc_command(&command)
}

/// Sends an IPC command to the running Melaffeine application over Unix domain socket.
fn send_ipc_command(command: &IpcCommand) -> Result<IpcResponse, String> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path).map_err(|_| {
        format!(
            "Error: Could not connect to Melaffeine at {}.\nIs Melaffeine.app running?",
            path.display()
        )
    })?;

    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("Failed to configure socket: {e}"))?;

    let line = command.serialize();
    stream
        .write_all(line.as_bytes())
        .map_err(|e| format!("Failed to send command: {e}"))?;

    let mut reader = BufReader::new((&stream).take(MAX_RESPONSE_BYTES));
    let mut response_line = String::new();
    reader
        .read_line(&mut response_line)
        .map_err(|e| format!("Failed to read response: {e}"))?;

    IpcResponse::parse(&response_line)
        .ok_or_else(|| format!("Received invalid response from Melaffeine: {response_line}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing() {
        let status = Cli::try_parse_from(["melaffeine", "status"]).unwrap();
        assert!(matches!(status.command, Commands::Status));

        let stop = Cli::try_parse_from(["melaffeine", "stop"]).unwrap();
        assert!(matches!(stop.command, Commands::Stop));

        let toggle = Cli::try_parse_from(["melaffeine", "toggle"]).unwrap();
        assert!(matches!(toggle.command, Commands::Toggle));

        let quit = Cli::try_parse_from(["melaffeine", "quit"]).unwrap();
        assert!(matches!(quit.command, Commands::Quit));

        let start_dur = Cli::try_parse_from(["melaffeine", "start", "2h", "--display"]).unwrap();
        if let Commands::Start {
            duration,
            display,
            indefinite,
        } = start_dur.command
        {
            assert_eq!(duration, Some(String::from("2h")));
            assert!(display);
            assert!(!indefinite);
        } else {
            panic!("expected Start command");
        }

        let start_indef = Cli::try_parse_from(["melaffeine", "start", "--indefinite"]).unwrap();
        if let Commands::Start {
            duration,
            display,
            indefinite,
        } = start_indef.command
        {
            assert_eq!(duration, None);
            assert!(!display);
            assert!(indefinite);
        } else {
            panic!("expected Start command");
        }
    }

    #[test]
    fn test_send_ipc_command_not_running() {
        let res = send_ipc_command(&IpcCommand::Status);
        if let Err(e) = res {
            assert!(e.contains("Could not connect to Melaffeine"));
        }
    }

    #[test]
    fn application_error_returns_failure() {
        assert_eq!(
            report_result(Ok(IpcResponse::Err(String::from("rejected")))),
            ExitCode::FAILURE
        );
    }

    #[test]
    fn test_run_cli_invalid_duration() {
        let cli = Cli::try_parse_from(["melaffeine", "start", "invalid_duration"]).unwrap();
        let err = run_cli(cli).unwrap_err();
        assert!(err.contains("Invalid duration specification"));
    }
}
