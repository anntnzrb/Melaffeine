//! Shared IPC command protocol and socket path definitions for Melaffeine.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fmt, fs};

use crate::duration::{MAX_FINITE_DURATION_SECONDS, parse_duration_spec};

/// Default socket filename prefix.
pub const SOCKET_NAME_PREFIX: &str = "melaffeine";

/// Maximum number of bytes allowed in one newline-terminated IPC frame.
pub const MAX_FRAME_BYTES: usize = 4_096;

/// Failure to locate or secure the per-user IPC runtime directory.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RuntimeDirError {
    /// Neither the environment nor the OS account database supplies a home directory.
    #[error("could not determine home directory")]
    NoHome,
    /// The socket path cannot fit macOS's 104-byte `sockaddr_un` field, including NUL.
    #[error("ipc socket path is too long: {path}", path = .path.display())]
    PathTooLong {
        /// Rejected socket path.
        path: PathBuf,
    },
    /// A filesystem operation failed while preparing or inspecting the directory.
    #[error("failed to {operation} {path}: {source}", path = .path.display())]
    Filesystem {
        /// Operation that failed.
        operation: &'static str,
        /// Affected filesystem path.
        path: PathBuf,
        /// Original filesystem failure.
        #[source]
        source: io::Error,
    },
    /// The final component is not a real directory owned exclusively by this user.
    #[error("runtime directory is not a private user-owned directory: {path}", path = .path.display())]
    NotPrivate {
        /// Rejected runtime directory.
        path: PathBuf,
    },
}

/// Returns the OS effective user ID used to validate runtime filesystem ownership.
#[must_use]
pub fn effective_uid() -> u32 {
    rustix::process::geteuid().as_raw()
}

/// Locates the per-user runtime directory without creating or modifying it.
///
/// # Errors
/// Returns [`RuntimeDirError::NoHome`] when no home directory can be determined.
pub fn runtime_dir() -> Result<PathBuf, RuntimeDirError> {
    std::env::home_dir()
        .map(|home| home.join("Library/Application Support/Melaffeine"))
        .ok_or(RuntimeDirError::NoHome)
}

/// Computes the default Unix socket path without touching the filesystem.
///
/// # Errors
/// Returns an error if home lookup fails or the path exceeds the macOS socket limit.
pub fn socket_path() -> Result<PathBuf, RuntimeDirError> {
    socket_path_in(&runtime_dir()?)
}

/// Computes a socket path in the supplied runtime directory without filesystem access.
///
/// # Errors
/// Returns [`RuntimeDirError::PathTooLong`] if the encoded path needs 104 bytes or more.
pub fn socket_path_in(dir: &Path) -> Result<PathBuf, RuntimeDirError> {
    let path = dir.join(format!("{SOCKET_NAME_PREFIX}.sock"));
    if path.as_os_str().as_encoded_bytes().len() >= 104 {
        return Err(RuntimeDirError::PathTooLong { path });
    }
    Ok(path)
}

/// Computes the default single-instance lock path without filesystem access.
///
/// # Errors
/// Returns [`RuntimeDirError::NoHome`] when home lookup fails.
pub fn lock_path() -> Result<PathBuf, RuntimeDirError> {
    Ok(lock_path_in(&runtime_dir()?))
}

/// Computes the stable single-instance lock path in the supplied runtime directory.
#[must_use]
pub fn lock_path_in(dir: &Path) -> PathBuf {
    dir.join(format!("{SOCKET_NAME_PREFIX}.lock"))
}

/// Failure to open a safe, user-owned single-instance lock file.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LockError {
    /// Opening or inspecting the lock failed.
    #[error("couldn't {operation} instance lock {path}: {source}", path = .path.display())]
    Io {
        /// Failed filesystem operation.
        operation: &'static str,
        /// Rejected lock path.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: io::Error,
    },
    /// The opened descriptor is not a regular file owned by this user.
    #[error("instance lock is not a user-owned regular file: {path}", path = .path.display())]
    Invalid {
        /// Rejected lock path.
        path: PathBuf,
    },
}

/// Opens a private instance lock without following a planted symlink.
///
/// # Errors
/// Returns a path-bearing error if opening fails or descriptor ownership/type is unsafe.
pub fn open_instance_lock(path: &Path) -> Result<fs::File, LockError> {
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits().cast_signed())
        .open(path)
        .map_err(|source| LockError::Io {
            operation: "open",
            path: path.to_path_buf(),
            source,
        })?;
    let metadata = file.metadata().map_err(|source| LockError::Io {
        operation: "inspect",
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() || metadata.uid() != effective_uid() {
        return Err(LockError::Invalid {
            path: path.to_path_buf(),
        });
    }
    Ok(file)
}

fn runtime_fs_error(operation: &'static str, path: &Path, source: io::Error) -> RuntimeDirError {
    RuntimeDirError::Filesystem {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

fn private_dir_metadata(dir: &Path) -> Result<fs::Metadata, RuntimeDirError> {
    let metadata =
        fs::symlink_metadata(dir).map_err(|source| runtime_fs_error("inspect", dir, source))?;
    if !metadata.file_type().is_dir() || metadata.uid() != effective_uid() {
        return Err(RuntimeDirError::NotPrivate {
            path: dir.to_path_buf(),
        });
    }
    Ok(metadata)
}

/// Creates or verifies a real, effective-user-owned directory with private permissions.
///
/// Existing directories owned by this user are tightened to 0700 if needed.
/// Symlinks, other filesystem objects, and foreign owners are rejected.
///
/// # Errors
/// Returns a path-bearing error on filesystem failure or an unsafe final component.
pub fn ensure_private_dir(dir: &Path) -> Result<(), RuntimeDirError> {
    if let Some(parent) = dir.parent() {
        fs::create_dir_all(parent)
            .map_err(|source| runtime_fs_error("create parent directory", parent, source))?;
    }
    match fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(source) => return Err(runtime_fs_error("create directory", dir, source)),
    }
    let metadata = private_dir_metadata(dir)?;
    if metadata.mode() & 0o077 != 0 {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
            .map_err(|source| runtime_fs_error("set private permissions on", dir, source))?;
        if private_dir_metadata(dir)?.mode() & 0o077 != 0 {
            return Err(RuntimeDirError::NotPrivate {
                path: dir.to_path_buf(),
            });
        }
    }
    Ok(())
}

/// Sends a command to the Melaffeine IPC socket at `path` and parses the single-line response.
///
/// # Errors
///
/// Returns connection, timeout-configuration, read, and write errors from
/// the Unix socket, including timeout failures. Returns
/// [`io::ErrorKind::InvalidData`] for oversized, unterminated, non-UTF-8,
/// or unparseable response frames.
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
                    debug_assert!(
                        secs <= MAX_FINITE_DURATION_SECONDS,
                        "serialized session duration must not exceed 365 days"
                    );
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
    pub fn parse(line: &str) -> Option<Self> {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("OK ") {
            return Some(Self::Ok(rest.to_string()));
        }
        if let Some(rest) = trimmed.strip_prefix("ERR ") {
            return Some(Self::Err(rest.to_string()));
        }
        match trimmed {
            "OK" => return Some(Self::Ok(String::new())),
            "ERR" => return Some(Self::Err(String::new())),
            "STATUS inactive" => return Some(Self::Status(None)),
            _ => {}
        }

        let active_rest = trimmed.strip_prefix("STATUS active ")?;
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
