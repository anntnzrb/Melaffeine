//! Melaffeine CLI controller for managing sleep prevention sessions.

use std::io::{self, ErrorKind};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use app_core::duration::parse_duration_spec;
use app_core::ipc::{IpcCommand, IpcResponse, send_command, socket_path};
use clap::{Parser, Subcommand};

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
        #[arg(short, long, conflicts_with = "duration")]
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
    match run_cli(Cli::parse()) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_cli(cli: Cli) -> Result<String, String> {
    let command = match cli.command {
        Commands::Start {
            duration,
            display,
            indefinite: _,
        } => {
            let parsed_duration = duration
                .as_deref()
                .map(|spec| {
                    parse_duration_spec(spec).ok_or_else(|| {
                        format!(
                            "Invalid duration specification '{spec}'. Use format like 2h, 45m, 1d."
                        )
                    })
                })
                .transpose()?;
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

    let path = socket_path();
    let response = send_command(&path, &command, Duration::from_secs(5))
        .map_err(|error| format_ipc_error(&path, &error))?;

    match response {
        IpcResponse::Err(error) => Err(error),
        other => Ok(other.to_string()),
    }
}

fn format_ipc_error(path: &Path, error: &io::Error) -> String {
    match error.kind() {
        ErrorKind::NotFound | ErrorKind::ConnectionRefused => format!(
            "Melaffeine is not running (no listener at {}). Open Melaffeine.app first.",
            path.display()
        ),
        ErrorKind::TimedOut | ErrorKind::WouldBlock => String::from(
            "Melaffeine is running but did not respond in time. Check that its password prompt or popover isn't waiting for you.",
        ),
        ErrorKind::InvalidData => format!("Unexpected response from Melaffeine: {error}"),
        _ => format!(
            "Could not talk to Melaffeine at {}: {error}",
            path.display()
        ),
    }
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
            (
                &["melaffeine", "start", "--indefinite"][..],
                (None, false, true),
            ),
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
        assert!(Cli::try_parse_from(["melaffeine", "start", "2h", "--indefinite"]).is_err());
    }

    #[test]
    fn test_run_cli_invalid_duration() {
        let cli = Cli::try_parse_from(["melaffeine", "start", "invalid_duration"]).unwrap();
        assert!(run_cli(cli).is_err());
    }

    #[test]
    fn test_format_ipc_error_categories() {
        let path = Path::new("/tmp/melaffeine-test.sock");
        for (kind, expected_substring) in [
            (ErrorKind::NotFound, "not running"),
            (ErrorKind::ConnectionRefused, "not running"),
            (ErrorKind::TimedOut, "did not respond in time"),
            (ErrorKind::WouldBlock, "did not respond in time"),
            (ErrorKind::InvalidData, "Unexpected response"),
            (ErrorKind::PermissionDenied, "Could not talk to Melaffeine"),
        ] {
            let error = io::Error::new(kind, "synthetic detail");
            let message = format_ipc_error(path, &error);
            assert!(
                message.contains(expected_substring),
                "kind {kind:?} produced {message:?}, expected substring {expected_substring:?}"
            );
        }
    }
}
