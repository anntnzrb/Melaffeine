//! Duration validation, compact formatting, and the shared Melaffeine IPC protocol.

#![forbid(unsafe_code)]

/// Duration units, bounded parsing, and compact countdown formatting.
pub mod duration;
/// IPC commands, responses, framing limits, and the Unix socket client.
pub mod ipc;

pub use duration::{DurationUnit, format_compact_duration, parse_duration, parse_duration_spec};
pub use ipc::{
    IpcCommand, IpcResponse, RuntimeDirError, SessionStatus, ensure_private_dir, lock_path,
    runtime_dir, send_command, socket_path,
};
