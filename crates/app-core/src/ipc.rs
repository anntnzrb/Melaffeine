//! Shared IPC command protocol and socket path definitions for Melaffeine.

use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::duration::{MAX_FINITE_DURATION_SECONDS, parse_duration_spec};

/// Default socket filename prefix.
pub const SOCKET_NAME_PREFIX: &str = "melaffeine";

/// Maximum number of bytes allowed in one newline-terminated IPC frame.
pub const MAX_FRAME_BYTES: usize = 4_096;

/// Computes the default Unix Domain Socket path for the current user.
#[must_use]
pub fn socket_path() -> PathBuf {
    PathBuf::from("/tmp").join(format!(
        "{SOCKET_NAME_PREFIX}-{}.sock",
        rustix::process::geteuid().as_raw()
    ))
}

/// Sends a command to the Melaffeine IPC socket at `path` and parses the single-line response.
pub fn send_command(
    path: &Path,
    command: &IpcCommand,
    timeout: Duration,
) -> io::Result<IpcResponse> {
    let mut stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(command.serialize().as_bytes())?;

    let limit = u64::try_from(MAX_FRAME_BYTES.saturating_add(1)).unwrap_or(u64::MAX);
    let mut line = String::new();
    BufReader::new(stream.take(limit)).read_line(&mut line)?;
    if line.len() > MAX_FRAME_BYTES || !line.ends_with('\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid IPC response frame",
        ));
    }

    IpcResponse::parse(&line)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid IPC response"))
}

/// Commands sent from the CLI client to the running application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcCommand {
    /// Start a sleep prevention session.
    Start {
        /// Optional finite duration. If None, runs indefinitely.
        duration: Option<Duration>,
        /// Whether to keep display awake as well.
        keep_display_awake: bool,
    },
    /// Stop the active sleep prevention session.
    Stop,
    /// Toggle active sleep prevention state.
    Toggle,
    /// Query status of the running application.
    Status,
    /// Request application termination.
    Quit,
}

impl IpcCommand {
    /// Serializes command into a newline-terminated ASCII protocol string.
    #[must_use]
    pub fn serialize(&self) -> String {
        match self {
            Self::Start {
                duration,
                keep_display_awake,
            } => {
                let mut s = String::from("START");
                if let Some(dur) = duration {
                    let secs = dur.as_secs();
                    debug_assert!(secs <= MAX_FINITE_DURATION_SECONDS);
                    s.push(' ');
                    s.push_str(&secs.to_string());
                    s.push('s');
                } else {
                    s.push_str(" indefinite");
                }
                if *keep_display_awake {
                    s.push_str(" display");
                }
                s.push('\n');
                s
            }
            Self::Stop => String::from("STOP\n"),
            Self::Toggle => String::from("TOGGLE\n"),
            Self::Status => String::from("STATUS\n"),
            Self::Quit => String::from("QUIT\n"),
        }
    }

    /// Parses an incoming protocol line into an `IpcCommand`.
    #[must_use]
    pub fn parse(line: &str) -> Option<Self> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }

        let mut parts = trimmed.split_whitespace();
        let action = parts.next()?;

        match action.to_ascii_uppercase().as_str() {
            "STOP" if parts.next().is_none() => Some(Self::Stop),
            "TOGGLE" if parts.next().is_none() => Some(Self::Toggle),
            "STATUS" if parts.next().is_none() => Some(Self::Status),
            "QUIT" if parts.next().is_none() => Some(Self::Quit),
            "START" => {
                let (duration, next) = match parts.next() {
                    None => (None, None),
                    Some(tok) if tok.eq_ignore_ascii_case("display") => {
                        return parts.next().is_none().then_some(Self::Start {
                            duration: None,
                            keep_display_awake: true,
                        });
                    }
                    Some(tok) if tok.eq_ignore_ascii_case("indefinite") => (None, parts.next()),
                    Some(tok) => (Some(parse_duration_spec(tok)?), parts.next()),
                };
                let keep_display_awake = match next {
                    None => false,
                    Some(tok) if tok.eq_ignore_ascii_case("display") && parts.next().is_none() => {
                        true
                    }
                    Some(_) => return None,
                };
                Some(Self::Start {
                    duration,
                    keep_display_awake,
                })
            }
            _ => None,
        }
    }
}

/// Active session status details returned by `STATUS`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionStatus {
    /// Whether display sleep is also being prevented.
    pub keep_display_awake: bool,
    /// Compact remaining duration text for finite sessions, or `None` for indefinite sessions.
    pub remaining_compact: Option<String>,
}

/// Structured response sent from the application back to the CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcResponse {
    /// Generic success message.
    Ok(String),
    /// Detailed status snapshot (`None` when inactive).
    Status(Option<SessionStatus>),
    /// Error message.
    Err(String),
}

impl IpcResponse {
    /// Serializes response to newline-terminated protocol text.
    #[must_use]
    pub fn serialize(&self) -> String {
        match self {
            Self::Ok(msg) => format!("OK {msg}\n"),
            Self::Err(msg) => format!("ERR {msg}\n"),
            Self::Status(None) => String::from("STATUS inactive\n"),
            Self::Status(Some(SessionStatus {
                keep_display_awake,
                remaining_compact,
            })) => {
                let display = if *keep_display_awake {
                    "display"
                } else {
                    "nodisplay"
                };
                remaining_compact.as_deref().map_or_else(
                    || format!("STATUS active {display}\n"),
                    |remaining| format!("STATUS active {display} {remaining}\n"),
                )
            }
        }
    }

    /// Parses an incoming protocol line into an `IpcResponse`.
    #[must_use]
    #[allow(clippy::option_if_let_else)]
    pub fn parse(line: &str) -> Option<Self> {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("OK ") {
            Some(Self::Ok(rest.to_string()))
        } else if let Some(rest) = trimmed.strip_prefix("ERR ") {
            Some(Self::Err(rest.to_string()))
        } else if trimmed == "OK" {
            Some(Self::Ok(String::new()))
        } else if trimmed == "ERR" {
            Some(Self::Err(String::new()))
        } else if trimmed == "STATUS inactive" {
            Some(Self::Status(None))
        } else if let Some(active_rest) = trimmed.strip_prefix("STATUS active ") {
            let (display_tok, remaining_compact) = match active_rest.split_once(' ') {
                Some((display_tok, rem)) => {
                    let rem = rem.trim();
                    if rem.is_empty() {
                        return None;
                    }
                    (display_tok, Some(rem.to_string()))
                }
                None => (active_rest, None),
            };
            let keep_display_awake = match display_tok {
                "display" => true,
                "nodisplay" => false,
                _ => return None,
            };
            Some(Self::Status(Some(SessionStatus {
                keep_display_awake,
                remaining_compact,
            })))
        } else {
            None
        }
    }
}

impl fmt::Display for IpcResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok(msg) => write!(f, "{msg}"),
            Self::Err(msg) => write!(f, "Error: {msg}"),
            Self::Status(Some(SessionStatus {
                keep_display_awake,
                remaining_compact,
            })) => {
                let rem = remaining_compact.as_deref().unwrap_or("active");
                let disp = if *keep_display_awake {
                    " (display awake)"
                } else {
                    ""
                };
                write!(f, "Melaffeine: ACTIVE [{rem}]{disp}")
            }
            Self::Status(None) => write!(f, "Melaffeine: INACTIVE"),
        }
    }
}
