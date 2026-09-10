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
            let parsed_duration: Option<Duration> = match (indefinite, duration) {
                (true, _) | (_, None) => None,
                (false, Some(spec)) => Some(parse_duration_spec(&spec).ok_or_else(|| {
                    format!(
                        "Error: Invalid duration specification '{spec}'. Use format like 2h, 45m, 1d."
                    )
                })?),
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
        let parse = |args: &[&str]| Cli::try_parse_from(args).unwrap().command;
        assert!(matches!(parse(&["melaffeine", "status"]), Commands::Status));
        assert!(matches!(parse(&["melaffeine", "stop"]), Commands::Stop));
        assert!(matches!(parse(&["melaffeine", "toggle"]), Commands::Toggle));
        assert!(matches!(parse(&["melaffeine", "quit"]), Commands::Quit));

        for (args, expected) in [
            (
                &["melaffeine", "start", "2h", "--display"][..],
                (Some("2h"), true, false),
            ),
            (&["melaffeine", "start", "--indefinite"][..], (None, false, true)),
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            let Commands::Start {
                duration,
                display,
                indefinite,
            } = cli.command
            else {
                panic!("expected Start command")
            };
            assert_eq!(duration.as_deref(), expected.0);
            assert_eq!(display, expected.1);
            assert_eq!(indefinite, expected.2);
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
