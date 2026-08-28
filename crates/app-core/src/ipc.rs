//! Shared IPC command protocol and socket path definitions for Melaffeine.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use crate::duration::{DurationUnit, parse_duration};

/// Default socket filename prefix.
pub const SOCKET_NAME_PREFIX: &str = "melaffeine";

/// Computes the default Unix Domain Socket path for the current user.
#[must_use]
pub fn socket_path() -> PathBuf {
    let uid = std::env::var("UID").unwrap_or_else(|_| {
        std::env::var("USER").unwrap_or_else(|_| String::from("default"))
    });
    let tmp = std::env::temp_dir();
    tmp.join(format!("{SOCKET_NAME_PREFIX}-{uid}.sock"))
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
    #[allow(clippy::option_if_let_else, clippy::question_mark)]
    pub fn parse(line: &str) -> Option<Self> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }

        let mut parts = trimmed.split_whitespace();
        let action = parts.next()?;

        if action.eq_ignore_ascii_case("STOP") {
            Some(Self::Stop)
        } else if action.eq_ignore_ascii_case("TOGGLE") {
            Some(Self::Toggle)
        } else if action.eq_ignore_ascii_case("STATUS") {
            Some(Self::Status)
        } else if action.eq_ignore_ascii_case("QUIT") {
            Some(Self::Quit)
        } else if action.eq_ignore_ascii_case("START") {
            let mut duration: Option<Duration> = None;
            let mut keep_display_awake = false;

            for part in parts {
                if part.eq_ignore_ascii_case("display") {
                    keep_display_awake = true;
                } else if part.eq_ignore_ascii_case("indefinite") {
                    duration = None;
                } else if let Some(dur) = parse_duration_spec(part) {
                    duration = Some(dur);
                } else {
                    return None;
                }
            }

            Some(Self::Start {
                duration,
                keep_display_awake,
            })
        } else {
            None
        }
    }
}

/// Helper to parse duration string with flexible suffixes (`m`, `h`, `d`, `s` or bare minutes).
#[must_use]
pub fn parse_duration_spec(spec: &str) -> Option<Duration> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(num) = trimmed.strip_suffix('s').or_else(|| trimmed.strip_suffix('S')) {
        let secs = num.parse::<u64>().ok()?;
        if secs == 0 || secs > 31_536_000 {
            return None;
        }
        return Some(Duration::from_secs(secs));
    }

    if let Some(num) = trimmed.strip_suffix('m').or_else(|| trimmed.strip_suffix('M')) {
        return parse_duration(num, DurationUnit::Minutes);
    }

    if let Some(num) = trimmed.strip_suffix('h').or_else(|| trimmed.strip_suffix('H')) {
        return parse_duration(num, DurationUnit::Hours);
    }

    if let Some(num) = trimmed.strip_suffix('d').or_else(|| trimmed.strip_suffix('D')) {
        return parse_duration(num, DurationUnit::Days);
    }

    // Default to minutes if bare number
    parse_duration(trimmed, DurationUnit::Minutes)
}

/// Structured response sent from the application back to the CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcResponse {
    /// Generic success message.
    Ok(String),
    /// Detailed status snapshot.
    Status {
        is_active: bool,
        keep_display_awake: bool,
        ends_at_unix: Option<u64>,
        remaining_compact: Option<String>,
    },
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
            Self::Status {
                is_active,
                keep_display_awake,
                ends_at_unix,
                remaining_compact,
            } => {
                let ends_str = ends_at_unix.map_or_else(|| String::from("none"), |u| u.to_string());
                let rem_str = remaining_compact
                    .as_deref()
                    .unwrap_or("none")
                    .replace(' ', "_");
                format!(
                    "STATUS active={is_active} display={keep_display_awake} ends_at={ends_str} remaining={rem_str}\n"
                )
            }
        }
    }

    /// Parses an incoming protocol line into an `IpcResponse`.
    #[must_use]
    #[allow(clippy::option_if_let_else, clippy::collapsible_if)]
    pub fn parse(line: &str) -> Option<Self> {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("OK ") {
            Some(Self::Ok(rest.to_string()))
        } else if let Some(rest) = trimmed.strip_prefix("ERR ") {
            Some(Self::Err(rest.to_string()))
        } else if let Some(rest) = trimmed.strip_prefix("STATUS ") {
            let mut is_active = false;
            let mut keep_display_awake = false;
            let mut ends_at_unix: Option<u64> = None;
            let mut remaining_compact: Option<String> = None;

            for part in rest.split_whitespace() {
                if let Some(v) = part.strip_prefix("active=") {
                    is_active = v.eq_ignore_ascii_case("true");
                } else if let Some(v) = part.strip_prefix("display=") {
                    keep_display_awake = v.eq_ignore_ascii_case("true");
                } else if let Some(v) = part.strip_prefix("ends_at=") {
                    if v != "none" {
                        ends_at_unix = v.parse::<u64>().ok();
                    }
                } else if let Some(v) = part.strip_prefix("remaining=") {
                    if v != "none" {
                        remaining_compact = Some(v.replace('_', " "));
                    }
                }
            }

            Some(Self::Status {
                is_active,
                keep_display_awake,
                ends_at_unix,
                remaining_compact,
            })
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
            Self::Status {
                is_active,
                keep_display_awake,
                ends_at_unix: _,
                remaining_compact,
            } => {
                if *is_active {
                    let rem = remaining_compact.as_deref().unwrap_or("active");
                    let disp = if *keep_display_awake { " (display awake)" } else { "" };
                    write!(f, "Melaffeine: ACTIVE [{rem}]{disp}")
                } else {
                    write!(f, "Melaffeine: INACTIVE")
                }
            }
        }
    }
}
