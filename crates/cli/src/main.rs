//! Melaffeine CLI controller for managing sleep prevention sessions.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;
use std::time::Duration;

use app_core::ipc::{IpcCommand, IpcResponse, parse_duration_spec, socket_path};
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
    let cli = Cli::parse();
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
                    eprintln!(
                        "Error: Invalid duration specification '{spec}'. Use format like 2h, 45m, 1d."
                    );
                    return ExitCode::FAILURE;
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

    match send_ipc_command(&command) {
        Ok(response) => {
            println!("{response}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
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

    let line = command.serialize();
    stream
        .write_all(line.as_bytes())
        .map_err(|e| format!("Failed to send command: {e}"))?;

    let mut reader = BufReader::new(&stream);
    let mut response_line = String::new();
    reader
        .read_line(&mut response_line)
        .map_err(|e| format!("Failed to read response: {e}"))?;

    IpcResponse::parse(&response_line).ok_or_else(|| {
        format!("Received invalid response from Melaffeine: {response_line}")
    })
}
