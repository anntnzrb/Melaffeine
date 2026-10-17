//! Melaffeine CLI controller for managing sleep prevention sessions.

use std::io::{self, ErrorKind, Write};
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
    write_output(
        &run_cli(Cli::parse()),
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    )
}

fn write_output(
    result: &Result<String, String>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> ExitCode {
    let (write_result, exit_code) = match result {
        Ok(output) => (writeln!(stdout, "{output}"), ExitCode::SUCCESS),
        Err(error) => (writeln!(stderr, "Error: {error}"), ExitCode::FAILURE),
    };
    if let Err(error) = write_result
        && error.kind() != ErrorKind::BrokenPipe
    {
        return ExitCode::FAILURE;
    }
    exit_code
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

    let path = socket_path()
        .map_err(|error| format!("Could not locate Melaffeine IPC socket: {error}"))?;
    let response = send_command(&path, &command, Duration::from_secs(5))
        .map_err(|error| format_ipc_error(&path, &error))?;

    match response {
        IpcResponse::Err(error) => Err(error),
        other @ (IpcResponse::Ok(_) | IpcResponse::Status(_)) => Ok(other.to_string()),
    }
}

fn format_ipc_error(path: &Path, error: &io::Error) -> String {
    // `ErrorKind` is non-exhaustive, so every unlisted kind shares the generic message.
    let kind = error.kind();
    if matches!(kind, ErrorKind::NotFound | ErrorKind::ConnectionRefused) {
        format!(
            "Melaffeine is not running (no listener at {}). Open Melaffeine.app first.",
            path.display()
        )
    } else if matches!(kind, ErrorKind::TimedOut | ErrorKind::WouldBlock) {
        String::from(
            "Melaffeine is running but did not respond in time. Check that its password prompt or popover isn't waiting for you.",
        )
    } else if kind == ErrorKind::InvalidData {
        format!("Unexpected response from Melaffeine: {error}")
    } else {
        format!(
            "Could not talk to Melaffeine at {}: {error}",
            path.display()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing() {
        let parse = |args: &[&str]| Cli::try_parse_from(args).unwrap().command;
        assert!(
            matches!(parse(&["melaffeine", "status"]), Commands::Status),
            "status must select its command variant"
        );
        assert!(
            matches!(parse(&["melaffeine", "stop"]), Commands::Stop),
            "stop must select its command variant"
        );
        assert!(
            matches!(parse(&["melaffeine", "toggle"]), Commands::Toggle),
            "toggle must select its command variant"
        );
        assert!(
            matches!(parse(&["melaffeine", "quit"]), Commands::Quit),
            "quit must select its command variant"
        );

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
            assert_eq!(
                duration.as_deref(),
                expected.0,
                "duration must match {args:?}"
            );
            assert_eq!(display, expected.1, "display flag must match {args:?}");
            assert_eq!(
                indefinite, expected.2,
                "indefinite flag must match {args:?}"
            );
        }
        assert_eq!(
            Cli::try_parse_from(["melaffeine", "start", "2h", "--indefinite"])
                .expect_err("duration and indefinite must conflict")
                .kind(),
            clap::error::ErrorKind::ArgumentConflict,
            "a finite duration must conflict with --indefinite"
        );
    }

    #[test]
    fn test_run_cli_invalid_duration() {
        let cli = Cli::try_parse_from(["melaffeine", "start", "invalid_duration"]).unwrap();
        assert_eq!(
            run_cli(cli),
            Err(String::from(
                "Invalid duration specification 'invalid_duration'. Use format like 2h, 45m, 1d."
            )),
            "invalid duration must fail before contacting the application"
        );
    }

    #[test]
    fn test_format_ipc_error_categories() {
        let path = Path::new("/test/runtime/melaffeine.sock");
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

    struct FailingWriter(ErrorKind);

    impl Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::from(self.0))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn output_success_writes_trailing_newline() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            write_output(
                &Ok(String::from("Session started")),
                &mut stdout,
                &mut stderr
            ),
            ExitCode::SUCCESS,
            "writing successful command output must succeed"
        );
        assert_eq!(
            stdout, b"Session started\n",
            "stdout must preserve text and append a newline"
        );
        assert!(
            stderr.is_empty(),
            "successful output must leave stderr empty"
        );
    }

    #[test]
    fn output_error_writes_prefix_and_trailing_newline() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            write_output(
                &Err(String::from("Something broke")),
                &mut stdout,
                &mut stderr
            ),
            ExitCode::FAILURE,
            "an application error must return failure"
        );
        assert_eq!(
            stderr, b"Error: Something broke\n",
            "stderr must preserve the error prefix and newline"
        );
        assert!(stdout.is_empty(), "errors must leave stdout empty");
    }

    #[test]
    fn output_broken_pipe_is_success() {
        let mut stderr = Vec::new();
        assert_eq!(
            write_output(
                &Ok(String::from("Session started")),
                &mut FailingWriter(ErrorKind::BrokenPipe),
                &mut stderr
            ),
            ExitCode::SUCCESS,
            "a closed stdout pipe must not turn success into failure"
        );
        assert!(
            stderr.is_empty(),
            "a closed stdout pipe must not emit an error"
        );
    }

    #[test]
    fn output_other_stdout_error_is_failure() {
        let mut stderr = Vec::new();
        assert_eq!(
            write_output(
                &Ok(String::from("Session started")),
                &mut FailingWriter(ErrorKind::PermissionDenied),
                &mut stderr
            ),
            ExitCode::FAILURE,
            "stdout failures other than BrokenPipe must fail"
        );
        assert!(
            stderr.is_empty(),
            "stdout write failure must not add new error text"
        );
    }

    #[test]
    fn output_stderr_error_is_failure() {
        for kind in [ErrorKind::BrokenPipe, ErrorKind::PermissionDenied] {
            let mut stdout = Vec::new();
            assert_eq!(
                write_output(
                    &Err(String::from("Something broke")),
                    &mut stdout,
                    &mut FailingWriter(kind)
                ),
                ExitCode::FAILURE,
                "stderr failure {kind:?} must preserve failure status"
            );
            assert!(stdout.is_empty(), "stderr failure must leave stdout empty");
        }
    }
}
