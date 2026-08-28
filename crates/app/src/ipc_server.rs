//! Unix Domain Socket IPC server for remote CLI control.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::ptr::NonNull;

use app_core::ipc::{IpcCommand, socket_path};
use block2::RcBlock;
use objc2::rc::{Retained, Weak};
use objc2_foundation::NSTimer;

use crate::app_delegate::AppDelegate;

/// Interval for checking incoming IPC connections on the main runloop.
const IPC_POLL_INTERVAL_SECS: f64 = 0.05;

/// Active Unix Domain Socket IPC server.
pub struct IpcServer {
    listener: UnixListener,
    path: PathBuf,
    timer: Option<Retained<NSTimer>>,
}

impl IpcServer {
    /// Starts the IPC server and attaches the polling timer to the main run loop.
    pub fn start(delegate: &AppDelegate) -> Option<Self> {
        let path = socket_path();
        if path.exists() {
            let _ = fs::remove_file(&path);
        }

        let listener = UnixListener::bind(&path).ok()?;
        if listener.set_nonblocking(true).is_err() {
            let _ = fs::remove_file(&path);
            return None;
        }

        let weak_delegate: Weak<AppDelegate> = Weak::from_retained(&Retained::from(delegate));
        let block = RcBlock::new(move |_timer: NonNull<NSTimer>| {
            if let Some(strong_delegate) = weak_delegate.load() {
                strong_delegate.poll_ipc_connections();
            }
        });

        // SAFETY: scheduledTimerWithTimeInterval_repeats_block is called on the main thread.
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_repeats_block(
                IPC_POLL_INTERVAL_SECS,
                true,
                &block,
            )
        };

        Some(Self {
            listener,
            path,
            timer: Some(timer),
        })
    }

    /// Accepts all pending non-blocking connections and processes commands.
    pub fn poll_connections(&self, delegate: &AppDelegate) {
        while let Ok((stream, _)) = self.listener.accept() {
            Self::handle_connection(stream, delegate);
        }
    }

    fn handle_connection(mut stream: UnixStream, delegate: &AppDelegate) {
        let _ = stream.set_nonblocking(false);
        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
            let _ = stream.write_all(b"ERR empty command\n");
            return;
        }

        let Some(command) = IpcCommand::parse(&line) else {
            let _ = stream.write_all(b"ERR invalid command\n");
            return;
        };

        let response = delegate.execute_ipc_command(&command);
        let serialized = response.serialize();
        let _ = stream.write_all(serialized.as_bytes());
    }

    /// Stops the server, invalidates the runloop timer, and removes the socket file.
    pub fn stop(&mut self) {
        if let Some(timer) = self.timer.take() {
            timer.invalidate();
        }
        if self.path.exists() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        self.stop();
    }
}
