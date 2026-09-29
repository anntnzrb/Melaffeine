//! Unix Domain Socket IPC server for remote CLI control.

use std::fs;
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

use app_core::ipc::{IpcCommand, IpcResponse, MAX_FRAME_BYTES, send_command};
use dispatch2::MainThreadBound;
use objc2::MainThreadMarker;

/// Timeout for each individual IPC read and write operation.
const IPC_IO_TIMEOUT: Duration = Duration::from_secs(1);

/// Backoff delay after a non-transient `accept()` failure (e.g. `EMFILE`/`ENFILE`).
const IPC_ACCEPT_ERROR_BACKOFF: Duration = Duration::from_millis(200);

type Handler = Box<dyn Fn(&IpcCommand) -> IpcResponse>;

fn remove_socket_if_owned(path: &Path, identity: (u64, u64)) {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && (metadata.dev(), metadata.ino()) == identity
    {
        let _ = fs::remove_file(path);
    }
}

/// Probes whether a live IPC owner still answers STATUS on `path`.
fn probe_live_owner(path: &Path) -> bool {
    send_command(path, &IpcCommand::Status, IPC_IO_TIMEOUT).is_ok()
}

fn bind_listener(path: &Path) -> Option<(UnixListener, (u64, u64))> {
    let listener = match UnixListener::bind(path) {
        Ok(listener) => listener,
        Err(error) if error.kind() == ErrorKind::AddrInUse => {
            if probe_live_owner(path) {
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

fn serve_connection(mut stream: UnixStream, handler: impl FnOnce(&IpcCommand) -> IpcResponse) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(IPC_IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IPC_IO_TIMEOUT));

    let limit = u64::try_from(MAX_FRAME_BYTES.saturating_add(1)).unwrap_or(u64::MAX);
    let mut frame = Vec::new();
    let read = BufReader::new((&stream).take(limit)).read_until(b'\n', &mut frame);
    if !matches!(read, Ok(length) if length > 0)
        || frame.len() > MAX_FRAME_BYTES
        || frame.last() != Some(&b'\n')
    {
        let _ = stream.write_all(b"ERR invalid frame\n");
        return;
    }

    let Some(command) = std::str::from_utf8(&frame).ok().and_then(IpcCommand::parse) else {
        let _ = stream.write_all(b"ERR invalid command\n");
        return;
    };

    let response = handler(&command);
    let _ = stream.write_all(response.serialize().as_bytes());
}

/// Active Unix Domain Socket IPC server.
pub struct IpcServer {
    path: PathBuf,
    socket_identity: (u64, u64),
    stopping: Arc<AtomicBool>,
}

impl IpcServer {
    /// Starts the IPC server with a background accept thread that dispatches commands to the main thread.
    pub fn start(
        mtm: MainThreadMarker,
        path: &Path,
        handler: impl Fn(&IpcCommand) -> IpcResponse + 'static,
    ) -> Option<Self> {
        let (listener, socket_identity) = bind_listener(path)?;
        let stopping = Arc::new(AtomicBool::new(false));
        let boxed_handler: Handler = Box::new(handler);
        let bound_handler = MainThreadBound::new(boxed_handler, mtm);

        let worker_stopping = Arc::clone(&stopping);
        drop(thread::spawn(move || {
            let mut last_error_kind = None;
            loop {
                if worker_stopping.load(Ordering::Acquire) {
                    break;
                }
                let accept_result = listener.accept();
                if worker_stopping.load(Ordering::Acquire) {
                    break;
                }
                match accept_result {
                    Ok((stream, _)) => {
                        last_error_kind = None;
                        serve_connection(stream, |command| {
                            bound_handler.get_on_main(|h| h(command))
                        });
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            ErrorKind::Interrupted | ErrorKind::ConnectionAborted
                        ) => {}
                    Err(error) => {
                        if last_error_kind != Some(error.kind()) {
                            eprintln!("Melaffeine: IPC accept error: {error}");
                            last_error_kind = Some(error.kind());
                        }
                        thread::sleep(IPC_ACCEPT_ERROR_BACKOFF);
                    }
                }
            }
        }));

        Some(Self {
            path: path.to_path_buf(),
            socket_identity,
            stopping,
        })
    }

    /// Stops the server, wakes the accept thread, and removes its socket file if owned.
    pub fn stop(&mut self) {
        if !self.stopping.swap(true, Ordering::AcqRel) {
            let _ = UnixStream::connect(&self.path);
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

    fn run_server(
        frame: &[u8],
        read_timeout: Duration,
        handler: impl FnOnce(&IpcCommand) -> IpcResponse + Send + 'static,
        expected: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (server, mut client) = UnixStream::pair()?;
        client.set_read_timeout(Some(read_timeout))?;
        let worker = thread::spawn(move || serve_connection(server, handler));
        client.write_all(frame)?;
        let mut response = Vec::new();
        client.read_to_end(&mut response)?;
        assert_eq!(response, expected);
        assert!(worker.join().is_ok());
        Ok(())
    }

    #[test]
    fn complete_frame_gets_response() -> Result<(), Box<dyn std::error::Error>> {
        run_server(
            b"STATUS\n",
            IPC_IO_TIMEOUT,
            |command| {
                assert_eq!(command, &IpcCommand::Status);
                IpcResponse::Ok(String::from("ready"))
            },
            b"OK ready\n",
        )
    }

    #[test]
    fn invalid_command_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        run_server(
            b"INVALID\n",
            IPC_IO_TIMEOUT,
            |_| IpcResponse::Ok(String::from("unreachable")),
            b"ERR invalid command\n",
        )
    }

    #[test]
    fn newline_free_frame_times_out() -> Result<(), Box<dyn std::error::Error>> {
        run_server(
            b"STATUS",
            IPC_IO_TIMEOUT.saturating_mul(2),
            |_| IpcResponse::Ok(String::from("unreachable")),
            b"ERR invalid frame\n",
        )
    }

    #[test]
    fn oversized_frame_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let frame = vec![b'x'; MAX_FRAME_BYTES.saturating_add(1)];
        run_server(
            &frame,
            IPC_IO_TIMEOUT,
            |_| IpcResponse::Ok(String::from("unreachable")),
            b"ERR invalid frame\n",
        )
    }

    #[test]
    fn ownership_cases_are_safe() -> Result<(), Box<dyn std::error::Error>> {
        let dead_path = temporary_path("dead");
        let (dead_listener, dead_identity) = bind_listener(&dead_path)
            .ok_or_else(|| std::io::Error::other("bind first listener"))?;
        let first_metadata = fs::symlink_metadata(&dead_path)?;
        assert_eq!((first_metadata.dev(), first_metadata.ino()), dead_identity);

        let (new_listener, new_identity) = bind_listener(&dead_path)
            .ok_or_else(|| std::io::Error::other("replace dead listener"))?;
        assert_ne!(
            new_identity, dead_identity,
            "a bound-but-nonresponding listener is stale and must be replaced"
        );
        let new_metadata = fs::symlink_metadata(&dead_path)?;
        assert_eq!(
            (new_metadata.dev(), new_metadata.ino()),
            new_identity,
            "the new listener must own the path"
        );

        remove_socket_if_owned(&dead_path, (u64::MAX, u64::MAX));
        assert!(
            dead_path.exists(),
            "wrong ownership must not unlink a socket"
        );
        drop(dead_listener);
        drop(new_listener);
        remove_socket_if_owned(&dead_path, new_identity);
        assert!(!dead_path.exists(), "the owner must be able to clean up");

        let live_path = temporary_path("live");
        let (live_listener, live_identity) =
            bind_listener(&live_path).ok_or_else(|| std::io::Error::other("bind live listener"))?;
        let responder = thread::spawn(move || {
            let (stream, _) = live_listener.accept()?;
            let mut reply = Vec::new();
            BufReader::new(&stream).read_until(b'\n', &mut reply)?;
            (&stream).write_all(b"STATUS inactive\n")?;
            std::io::Result::Ok(())
        });
        assert!(
            bind_listener(&live_path).is_none(),
            "a live listener answering STATUS must not be replaced"
        );
        assert!(matches!(responder.join(), Ok(Ok(()))));
        let live_metadata = fs::symlink_metadata(&live_path)?;
        assert_eq!(
            (live_metadata.dev(), live_metadata.ino()),
            live_identity,
            "the live listener must retain ownership"
        );
        remove_socket_if_owned(&live_path, live_identity);
        assert!(!live_path.exists(), "live socket cleanup must be safe");

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
