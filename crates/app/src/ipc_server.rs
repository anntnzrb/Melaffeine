//! Unix Domain Socket IPC server for remote CLI control.

use std::fs;
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::{
    Arc, Mutex,
    mpsc::{self, SyncSender, TryRecvError, TrySendError},
};
use std::thread;
use std::time::Duration;

use app_core::ipc::{IpcCommand, IpcResponse, socket_path};
use block2::RcBlock;
use objc2::rc::{Retained, Weak};
use objc2_foundation::NSTimer;

use crate::app_delegate::AppDelegate;

/// Interval for checking incoming IPC connections on the main runloop.
const IPC_POLL_INTERVAL_SECS: f64 = 0.05;
/// Maximum number of bytes allowed in one newline-terminated command frame.
const MAX_COMMAND_BYTES: usize = 4_096;
/// Maximum number of bytes read while looking for a command frame terminator.
const MAX_COMMAND_READ_BYTES: u64 = 4_097;
/// Timeout for each individual IPC read, write, and main-thread response.
const IPC_IO_TIMEOUT: Duration = Duration::from_secs(1);
/// Maximum number of accepted streams and parsed requests buffered at once.
const IPC_QUEUE_DEPTH: usize = 32;
/// Number of worker threads serving accepted IPC streams.
const IPC_WORKERS: usize = 4;

type Request = (IpcCommand, SyncSender<IpcResponse>);

fn remove_socket_if_owned(path: &Path, identity: (u64, u64)) {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && (metadata.dev(), metadata.ino()) == identity
    {
        let _ = fs::remove_file(path);
    }
}

fn bind_listener(path: &Path) -> Option<(UnixListener, (u64, u64))> {
    let listener = match UnixListener::bind(path) {
        Ok(listener) => listener,
        Err(error) if error.kind() == ErrorKind::AddrInUse => {
            if UnixStream::connect(path).is_ok() {
                return None;
            }
            let metadata = fs::symlink_metadata(path).ok()?;
            if !metadata.file_type().is_socket() {
                return None;
            }
            fs::remove_file(path).ok()?;
            UnixListener::bind(path).ok()?
        }
        Err(_) => return None,
    };
    let metadata = fs::symlink_metadata(path).ok()?;
    Some((listener, (metadata.dev(), metadata.ino())))
}

fn serve_connection(mut stream: UnixStream, requests: &SyncSender<Request>) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(IPC_IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IPC_IO_TIMEOUT));

    let mut frame = Vec::new();
    let read = BufReader::new((&stream).take(MAX_COMMAND_READ_BYTES)).read_until(b'\n', &mut frame);
    if !matches!(read, Ok(length) if length > 0)
        || frame.len() > MAX_COMMAND_BYTES
        || frame.last() != Some(&b'\n')
    {
        let _ = stream.write_all(b"ERR invalid frame\n");
        return;
    }

    let Ok(line) = std::str::from_utf8(&frame) else {
        let _ = stream.write_all(b"ERR invalid command\n");
        return;
    };
    let Some(command) = IpcCommand::parse(line) else {
        let _ = stream.write_all(b"ERR invalid command\n");
        return;
    };

    let (response_tx, response_rx) = mpsc::sync_channel(1);
    if requests.send((command, response_tx)).is_err() {
        let _ = stream.write_all(b"ERR server stopping\n");
        return;
    }

    match response_rx.recv_timeout(IPC_IO_TIMEOUT) {
        Ok(response) => {
            let _ = stream.write_all(response.serialize().as_bytes());
        }
        Err(_) => {
            let _ = stream.write_all(b"ERR server timeout\n");
        }
    }
}

/// Active Unix Domain Socket IPC server.
pub struct IpcServer {
    _listener: Rc<UnixListener>,
    path: PathBuf,
    socket_identity: (u64, u64),
    timer: Option<Retained<NSTimer>>,
}

impl IpcServer {
    /// Starts the IPC server and attaches the polling timer to the main run loop.
    pub fn start(delegate: &AppDelegate) -> Option<Self> {
        let path = socket_path();
        let (listener, socket_identity) = bind_listener(&path)?;
        if listener.set_nonblocking(true).is_err() {
            remove_socket_if_owned(&path, socket_identity);
            return None;
        }

        let (stream_tx, stream_rx) = mpsc::sync_channel::<UnixStream>(IPC_QUEUE_DEPTH);
        let (request_tx, request_rx) = mpsc::sync_channel::<Request>(IPC_QUEUE_DEPTH);
        let stream_rx = Arc::new(Mutex::new(stream_rx));

        for _ in 0..IPC_WORKERS {
            let worker_rx = Arc::clone(&stream_rx);
            let worker_requests = request_tx.clone();
            let _worker = thread::spawn(move || {
                loop {
                    let stream = {
                        let Ok(receiver) = worker_rx.lock() else {
                            return;
                        };
                        let Ok(stream) = receiver.recv() else {
                            return;
                        };
                        stream
                    };
                    serve_connection(stream, &worker_requests);
                }
            });
        }
        drop(request_tx);

        let listener_rc = Rc::new(listener);
        let listener_clone = Rc::clone(&listener_rc);
        let weak_delegate: Weak<AppDelegate> = Weak::from_retained(&Retained::from(delegate));
        let block = RcBlock::new(move |_timer: NonNull<NSTimer>| {
            for _ in 0..IPC_QUEUE_DEPTH {
                match listener_clone.accept() {
                    Ok((stream, _)) => match stream_tx.try_send(stream) {
                        Ok(()) | Err(TrySendError::Full(_)) => {}
                        Err(TrySendError::Disconnected(_)) => break,
                    },
                    Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                    Err(_) => break,
                }
            }

            for _ in 0..IPC_QUEUE_DEPTH {
                match request_rx.try_recv() {
                    Ok((command, response_tx)) => {
                        let response = weak_delegate.load().map_or_else(
                            || IpcResponse::Err(String::from("App unavailable")),
                            |delegate| delegate.execute_ipc_command(&command),
                        );
                        let _ = response_tx.try_send(response);
                    }
                    Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
                }
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
            _listener: listener_rc,
            path,
            socket_identity,
            timer: Some(timer),
        })
    }

    /// Stops the server, invalidates the runloop timer, and removes its socket file.
    pub fn stop(&mut self) {
        if let Some(timer) = self.timer.take() {
            timer.invalidate();
        }
        remove_socket_if_owned(&self.path, self.socket_identity);
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
#[allow(clippy::panic_in_result_fn)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_PATH_ID: AtomicU64 = AtomicU64::new(0);

    fn temporary_path(label: &str) -> PathBuf {
        let unique = NEXT_PATH_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "melaffeine-ipc-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn complete_frame_gets_response() -> Result<(), Box<dyn std::error::Error>> {
        let (server, mut client) = UnixStream::pair()?;
        client.set_read_timeout(Some(IPC_IO_TIMEOUT))?;
        let (request_tx, request_rx) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || serve_connection(server, &request_tx));

        client.write_all(b"STATUS\n")?;
        let (command, response_tx) = request_rx.recv_timeout(IPC_IO_TIMEOUT)?;
        assert_eq!(command, IpcCommand::Status);
        response_tx.send(IpcResponse::Ok(String::from("ready")))?;

        let mut response = Vec::new();
        client.read_to_end(&mut response)?;
        assert_eq!(response, b"OK ready\n");
        assert!(worker.join().is_ok());
        Ok(())
    }

    #[test]
    fn newline_free_frame_times_out() -> Result<(), Box<dyn std::error::Error>> {
        let (server, mut client) = UnixStream::pair()?;
        client.set_read_timeout(Some(IPC_IO_TIMEOUT.saturating_mul(2)))?;
        let (request_tx, _request_rx) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || serve_connection(server, &request_tx));

        client.write_all(b"STATUS")?;
        let mut response = Vec::new();
        client.read_to_end(&mut response)?;
        assert_eq!(response, b"ERR invalid frame\n");
        assert!(worker.join().is_ok());
        Ok(())
    }

    #[test]
    fn oversized_frame_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let (server, mut client) = UnixStream::pair()?;
        client.set_read_timeout(Some(IPC_IO_TIMEOUT))?;
        let (request_tx, _request_rx) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || serve_connection(server, &request_tx));

        let frame = vec![b'x'; MAX_COMMAND_BYTES + 1];
        client.write_all(&frame)?;
        let mut response = Vec::new();
        client.read_to_end(&mut response)?;
        assert_eq!(response, b"ERR invalid frame\n");
        assert!(worker.join().is_ok());
        Ok(())
    }

    #[test]
    fn ownership_cases_are_safe() -> Result<(), Box<dyn std::error::Error>> {
        let live_path = temporary_path("live");
        let (live_listener, live_identity) = bind_listener(&live_path)
            .ok_or_else(|| std::io::Error::other("bind first listener"))?;
        let first_metadata = fs::symlink_metadata(&live_path)?;
        assert_eq!((first_metadata.dev(), first_metadata.ino()), live_identity);

        assert!(
            bind_listener(&live_path).is_none(),
            "a live listener must not be replaced"
        );
        let live_metadata = fs::symlink_metadata(&live_path)?;
        assert_eq!(
            (live_metadata.dev(), live_metadata.ino()),
            live_identity,
            "the first listener must retain ownership"
        );

        remove_socket_if_owned(&live_path, (u64::MAX, u64::MAX));
        assert!(
            live_path.exists(),
            "wrong ownership must not unlink a socket"
        );
        drop(live_listener);
        remove_socket_if_owned(&live_path, live_identity);
        assert!(!live_path.exists(), "the owner must be able to clean up");

        let stale_path = temporary_path("stale");
        let stale_listener = UnixListener::bind(&stale_path)?;
        drop(stale_listener);
        let (new_listener, new_identity) = bind_listener(&stale_path)
            .ok_or_else(|| std::io::Error::other("replace stale socket"))?;
        let new_metadata = fs::symlink_metadata(&stale_path)?;
        assert_eq!((new_metadata.dev(), new_metadata.ino()), new_identity);
        drop(new_listener);
        remove_socket_if_owned(&stale_path, new_identity);
        assert!(!stale_path.exists(), "stale socket cleanup must be safe");

        let regular_path = temporary_path("regular");
        fs::write(&regular_path, b"not a socket")?;
        assert!(
            bind_listener(&regular_path).is_none(),
            "non-socket paths must not be removed"
        );
        assert!(regular_path.exists());
        fs::remove_file(regular_path)?;
        Ok(())
    }
}
