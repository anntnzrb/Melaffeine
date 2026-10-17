//! Unix Domain Socket IPC server for remote CLI control.

use std::fs;
use std::io::{self, ErrorKind, Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex, MutexGuard,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use app_core::ipc::{IpcCommand, IpcResponse, MAX_FRAME_BYTES, effective_uid, send_command};
use dispatch2::MainThreadBound;
use objc2::MainThreadMarker;

/// Bound for each IPC write and live-owner probe.
const IPC_IO_TIMEOUT: Duration = Duration::from_secs(1);

/// Total monotonic budget for receiving one complete IPC frame.
const IPC_FRAME_BUDGET: Duration = Duration::from_secs(1);

/// Backoff delay after a non-transient `accept()` failure (e.g. `EMFILE`/`ENFILE`).
const IPC_ACCEPT_ERROR_BACKOFF: Duration = Duration::from_millis(200);

type Handler = Box<dyn Fn(&IpcCommand) -> IpcResponse>;

fn remove_socket_if_owned(path: &Path, identity: (u64, u64)) {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && (metadata.dev(), metadata.ino()) == identity
        && let Err(error) = remove_socket_file(path)
    {
        tracing::warn!(%error, path = %path.display(), "failed to remove owned IPC socket");
    }
}

#[derive(Debug, PartialEq, Eq)]
enum SocketRemoval {
    Removed,
    Missing,
}

fn remove_socket_file(path: &Path) -> io::Result<SocketRemoval> {
    match fs::remove_file(path) {
        Ok(()) => Ok(SocketRemoval::Removed),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(SocketRemoval::Missing),
        Err(error) => Err(error),
    }
}

/// Probes whether a live IPC owner still answers STATUS on `path`.
fn probe_live_owner(path: &Path) -> bool {
    send_command(path, &IpcCommand::Status, IPC_IO_TIMEOUT).is_ok()
}

fn socket_metadata(path: &Path) -> Option<fs::Metadata> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Some(metadata),
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "failed to inspect IPC socket");
            None
        }
    }
}

fn bind_listener(path: &Path, expected_uid: u32) -> Option<(UnixListener, (u64, u64))> {
    bind_listener_with(path, expected_uid, |path| UnixListener::bind(path))
}

fn bind_listener_with(
    path: &Path,
    expected_uid: u32,
    mut bind: impl FnMut(&Path) -> io::Result<UnixListener>,
) -> Option<(UnixListener, (u64, u64))> {
    let listener = match bind(path) {
        Ok(listener) => listener,
        Err(bind_error) if bind_error.kind() == ErrorKind::AddrInUse => {
            let metadata = socket_metadata(path)?;
            if !same_owner(metadata.uid(), expected_uid) {
                tracing::warn!(path = %path.display(), "foreign-owned IPC object; refusing to probe or remove it");
                return None;
            }
            if !metadata.file_type().is_socket() {
                return None;
            }
            if probe_live_owner(path) {
                return None;
            }
            match remove_socket_file(path) {
                Ok(SocketRemoval::Removed) => {}
                Ok(SocketRemoval::Missing) => return None,
                Err(error) => {
                    tracing::warn!(%error, path = %path.display(), "failed to remove stale IPC socket");
                    return None;
                }
            }
            match bind(path) {
                Ok(listener) => listener,
                Err(error) => {
                    tracing::warn!(%error, path = %path.display(), "failed to bind replacement IPC socket");
                    return None;
                }
            }
        }
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "failed to bind IPC socket");
            return None;
        }
    };
    let metadata = socket_metadata(path)?;
    let identity = (metadata.dev(), metadata.ino());
    if let Err(error) = fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
        tracing::warn!(%error, path = %path.display(), "failed to secure IPC socket");
        drop(listener);
        remove_socket_if_owned(path, identity);
        return None;
    }
    Some((listener, identity))
}

const fn same_owner(uid: u32, euid: u32) -> bool {
    uid == euid
}

fn read_frame(stream: &mut UnixStream, budget: Duration) -> io::Result<Vec<u8>> {
    let started = Instant::now();
    read_frame_with(
        |chunk, limit, remaining| {
            stream.set_read_timeout(Some(remaining))?;
            Ok((&mut *stream).take(limit).read(chunk))
        },
        || started.elapsed(),
        budget,
    )
}

fn read_frame_with(
    // The outer result configures the timeout; only the inner read may retry EINTR.
    mut read: impl FnMut(&mut [u8], u64, Duration) -> io::Result<io::Result<usize>>,
    mut elapsed: impl FnMut() -> Duration,
    budget: Duration,
) -> io::Result<Vec<u8>> {
    let mut frame = Vec::new();
    let mut chunk = [0_u8; 512];
    loop {
        // Saturation turns an exhausted absolute budget into a timeout, never a fresh budget.
        let remaining = budget.saturating_sub(elapsed());
        if remaining.is_zero() {
            return Err(io::Error::from(ErrorKind::TimedOut));
        }
        let capacity = MAX_FRAME_BYTES
            .saturating_add(1)
            .saturating_sub(frame.len());
        let limit = u64::try_from(capacity).unwrap_or(u64::MAX);
        let length = match read(&mut chunk, limit, remaining)? {
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            result => result?,
        };
        if length == 0 || elapsed() >= budget {
            return Err(io::Error::from(ErrorKind::TimedOut));
        }
        for byte in chunk.iter().take(length) {
            frame.push(*byte);
            if frame.len() > MAX_FRAME_BYTES {
                return Err(io::Error::from(ErrorKind::InvalidData));
            }
            if *byte == b'\n' {
                return Ok(frame);
            }
        }
    }
}

fn serve_connection_with(
    mut stream: UnixStream,
    handler: impl FnOnce(&IpcCommand) -> IpcResponse,
    budget: Duration,
    stopping: &AtomicBool,
) {
    if let Err(error) = stream.set_nonblocking(false) {
        tracing::warn!(%error, "failed to configure IPC connection; closing client");
        return;
    }
    if let Err(error) = stream.set_write_timeout(Some(IPC_IO_TIMEOUT)) {
        tracing::warn!(%error, "failed to configure IPC connection; closing client");
        return;
    }

    let Ok(frame) = read_frame(&mut stream, budget) else {
        if let Err(error) = stream.write_all(b"ERR invalid frame\n") {
            tracing::debug!(%error, "failed to write IPC response; peer may have disconnected");
        }
        return;
    };
    // Acquire observes stop's published cancellation before dispatching to the main queue.
    if stopping.load(Ordering::Acquire) {
        return;
    }
    let Some(command) = std::str::from_utf8(&frame).ok().and_then(IpcCommand::parse) else {
        if let Err(error) = stream.write_all(b"ERR invalid command\n") {
            tracing::debug!(%error, "failed to write IPC response; peer may have disconnected");
        }
        return;
    };

    let response = handler(&command);
    if let Err(error) = stream.write_all(response.serialize().as_bytes()) {
        tracing::debug!(%error, "failed to write IPC response; peer may have disconnected");
    }
}

#[cfg(test)]
fn serve_connection(stream: UnixStream, handler: impl FnOnce(&IpcCommand) -> IpcResponse) {
    serve_connection_with(stream, handler, IPC_FRAME_BUDGET, &AtomicBool::new(false));
}

type ActiveConnection = Arc<Mutex<Option<UnixStream>>>;

#[derive(Debug, PartialEq, Eq)]
enum AcceptErrorAction {
    Retry,
    Backoff,
    LogAndBackoff,
}

fn accept_error_action(error: &io::Error, last: Option<ErrorKind>) -> AcceptErrorAction {
    match error {
        error
            if matches!(
                error.kind(),
                ErrorKind::Interrupted | ErrorKind::ConnectionAborted
            ) =>
        {
            AcceptErrorAction::Retry
        }
        error if last != Some(error.kind()) => AcceptErrorAction::LogAndBackoff,
        _ => AcceptErrorAction::Backoff,
    }
}

fn active_slot(active: &ActiveConnection) -> MutexGuard<'_, Option<UnixStream>> {
    active.lock().unwrap_or_else(|poisoned| {
        tracing::warn!("recovering poisoned IPC active-connection slot");
        poisoned.into_inner()
    })
}

fn accept_connections(
    listener: &UnixListener,
    stopping: &AtomicBool,
    active: &ActiveConnection,
    handler: impl Fn(&IpcCommand) -> IpcResponse,
    budget: Duration,
) {
    let mut last_error_kind = None;
    loop {
        if stopping.load(Ordering::Acquire) {
            break;
        }
        let accept_result = listener.accept();
        if stopping.load(Ordering::Acquire) {
            break;
        }
        match accept_result {
            Ok((stream, _)) => {
                last_error_kind = None;
                let cancellation = match stream.try_clone() {
                    Ok(stream) => stream,
                    Err(error) => {
                        tracing::warn!(%error, "failed to register IPC cancellation; closing client");
                        continue;
                    }
                };
                *active_slot(active) = Some(cancellation);
                // Registration precedes this recheck so stop cannot miss a newly accepted client.
                if !stopping.load(Ordering::Acquire) {
                    serve_connection_with(stream, &handler, budget, stopping);
                }
                *active_slot(active) = None;
            }
            Err(error) => {
                match accept_error_action(&error, last_error_kind) {
                    AcceptErrorAction::Retry => continue,
                    AcceptErrorAction::Backoff => {}
                    AcceptErrorAction::LogAndBackoff => {
                        tracing::error!(%error, "IPC accept error");
                        last_error_kind = Some(error.kind());
                    }
                }
                #[expect(
                    clippy::disallowed_methods,
                    reason = "Dedicated non-async accept thread; bounded 200 ms backoff only after a non-transient accept error, with no clock to inject"
                )]
                thread::sleep(IPC_ACCEPT_ERROR_BACKOFF);
            }
        }
    }
}

/// Active Unix Domain Socket IPC server.
#[derive(Debug)]
pub struct IpcServer {
    path: PathBuf,
    socket_identity: (u64, u64),
    stopping: Arc<AtomicBool>,
    active: ActiveConnection,
    // Never join on the main thread: both handler dispatch and MainThreadBound::drop
    // synchronously use that thread and could deadlock. stop cancels socket I/O instead.
    worker: Option<JoinHandle<()>>,
}

impl IpcServer {
    /// Starts the IPC server with a background accept thread that dispatches commands to the main thread.
    pub fn start(
        mtm: MainThreadMarker,
        path: &Path,
        handler: impl Fn(&IpcCommand) -> IpcResponse + 'static,
    ) -> Option<Self> {
        let boxed_handler: Handler = Box::new(handler);
        let bound_handler = MainThreadBound::new(boxed_handler, mtm);
        Self::start_with(
            path,
            move |command| bound_handler.get_on_main(|h| h(command)),
            IPC_FRAME_BUDGET,
        )
    }

    fn start_with(
        path: &Path,
        handler: impl Fn(&IpcCommand) -> IpcResponse + Send + 'static,
        budget: Duration,
    ) -> Option<Self> {
        let (listener, socket_identity) = bind_listener(path, effective_uid())?;
        let stopping = Arc::new(AtomicBool::new(false));
        let active = Arc::new(Mutex::new(None));
        let worker_stopping = Arc::clone(&stopping);
        let worker_active = Arc::clone(&active);
        let worker = thread::spawn(move || {
            accept_connections(&listener, &worker_stopping, &worker_active, handler, budget);
        });
        Some(Self {
            path: path.to_path_buf(),
            socket_identity,
            stopping,
            active,
            worker: Some(worker),
        })
    }

    /// Stops the server, wakes the accept thread, and removes its socket file if owned.
    pub fn stop(&self) {
        // Publish cancellation before inspecting the slot; the worker rechecks after registration.
        if !self.stopping.swap(true, Ordering::AcqRel) {
            if let Some(stream) = active_slot(&self.active).as_ref()
                && let Err(error) = stream.shutdown(Shutdown::Both)
            {
                tracing::debug!(%error, "failed to shut down active IPC connection");
            }
            match UnixStream::connect(&self.path) {
                Ok(wakeup) => drop(wakeup),
                Err(error) => tracing::debug!(%error, "IPC stop wake-up connection failed"),
            }
        }
        remove_socket_if_owned(&self.path, self.socket_identity);
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        self.stop();
        tracing::debug!(
            worker_finished = self.worker.as_ref().is_some_and(JoinHandle::is_finished),
            "IPC worker cancellation requested"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn socket_removal_distinguishes_missing_removed_and_failed() {
        let path = temporary_path("removal");
        let listener = UnixListener::bind(&path).expect("bind removal fixture");
        assert_eq!(
            remove_socket_file(&path).expect("remove socket"),
            SocketRemoval::Removed,
            "existing socket must report removal"
        );
        assert_eq!(
            remove_socket_file(&path).expect("missing socket is harmless"),
            SocketRemoval::Missing,
            "already removed socket must not produce a warning error"
        );
        drop(listener);
        let directory = path.parent().expect("fixture directory");
        assert!(
            remove_socket_file(directory).is_err(),
            "real unlink failures must remain errors"
        );
        fs::remove_dir(directory).expect("clean removal fixture");
    }

    #[test]
    fn non_address_bind_error_never_removes_existing_socket() {
        let path = temporary_path("bind-error");
        let listener = UnixListener::bind(&path).expect("bind stale fixture");
        drop(listener);
        let before = fs::symlink_metadata(&path).expect("inspect stale fixture");
        let mut attempts = 0;
        let result = bind_listener_with(&path, effective_uid(), |_| {
            attempts += 1;
            Err(io::Error::from(ErrorKind::PermissionDenied))
        });
        let after = fs::symlink_metadata(&path).expect("non-address failure must preserve socket");
        assert!(
            result.is_none(),
            "unrelated bind failure cannot establish a listener"
        );
        assert_eq!(
            attempts, 1,
            "unrelated bind failure must not retry after stale cleanup"
        );
        assert_eq!(
            (after.dev(), after.ino()),
            (before.dev(), before.ino()),
            "unrelated bind failure must not unlink the existing object"
        );
        fs::remove_file(&path).expect("clean stale fixture");
        fs::remove_dir(path.parent().expect("fixture directory")).expect("clean bind fixture");
    }

    #[test]
    fn exact_frame_limit_is_accepted_and_one_extra_byte_is_rejected() {
        for (size, accepted) in [
            (MAX_FRAME_BYTES, true),
            (MAX_FRAME_BYTES.saturating_add(1), false),
        ] {
            let mut bytes = vec![b'x'; size.saturating_sub(1)];
            bytes.push(b'\n');
            let mut input = io::Cursor::new(&bytes);
            let result = read_frame_with(
                |chunk, limit, _| Ok((&mut input).take(limit).read(chunk)),
                || Duration::ZERO,
                Duration::from_secs(1),
            );
            if accepted {
                assert_eq!(
                    result.expect("exact limit frame must fit"),
                    bytes,
                    "frame limit includes its terminating newline"
                );
            } else {
                assert_eq!(
                    result.expect_err("oversized frame must fail").kind(),
                    ErrorKind::InvalidData,
                    "one byte over the frame limit must be rejected"
                );
            }
        }
    }

    #[test]
    fn interrupted_frame_read_retries_but_other_errors_propagate() {
        let mut calls = 0;
        let frame = read_frame_with(
            |chunk, _, _| {
                calls += 1;
                if calls == 1 {
                    return Ok(Err(io::Error::from(ErrorKind::Interrupted)));
                }
                let mut input = &b"STATUS\n"[..];
                Ok(input.read(chunk))
            },
            || Duration::ZERO,
            Duration::from_secs(1),
        );
        assert_eq!(
            frame.expect("retry interrupted read"),
            b"STATUS\n",
            "retry must preserve the frame"
        );
        assert_eq!(calls, 2, "interrupted read must be retried once");
        let mut errors = [ErrorKind::PermissionDenied, ErrorKind::BrokenPipe].into_iter();
        let error = read_frame_with(
            |_, _, _| {
                Ok(Err(io::Error::from(
                    errors.next().expect("fatal read must not be retried"),
                )))
            },
            || Duration::ZERO,
            Duration::from_secs(1),
        )
        .expect_err("fatal read must fail");
        assert_eq!(
            error.kind(),
            ErrorKind::PermissionDenied,
            "first fatal error must propagate"
        );
    }

    #[test]
    fn eof_before_deadline_and_data_at_deadline_both_time_out() {
        let mut reads = std::iter::once(Ok(0));
        let eof = read_frame_with(
            |_, _, _| Ok(reads.next().expect("EOF must not trigger another read")),
            || Duration::ZERO,
            Duration::from_secs(1),
        );
        assert_eq!(
            eof.expect_err("EOF is not a frame").kind(),
            ErrorKind::TimedOut,
            "EOF must terminate before the budget expires"
        );
        let mut clock = [Duration::ZERO, Duration::from_secs(1)].into_iter();
        let late = read_frame_with(
            |chunk, _, _| Ok((&b"STATUS\n"[..]).read(chunk)),
            || {
                clock
                    .next()
                    .expect("late data must be rejected immediately")
            },
            Duration::from_secs(1),
        );
        assert_eq!(
            late.expect_err("late frame must fail").kind(),
            ErrorKind::TimedOut,
            "a successful read must still respect the absolute deadline"
        );
    }

    #[test]
    fn frame_timeout_configuration_errors_are_not_retried() {
        let mut errors = std::iter::once(ErrorKind::Interrupted);
        let result = read_frame_with(
            |_, _, _| {
                Err(io::Error::from(
                    errors.next().expect("configuration must not retry"),
                ))
            },
            || Duration::ZERO,
            Duration::from_secs(1),
        );
        assert_eq!(
            result.expect_err("timeout configuration failed").kind(),
            ErrorKind::Interrupted,
            "configuration failures must propagate even when the error is Interrupted"
        );
    }

    #[test]
    fn accept_errors_retry_transient_failures_and_deduplicate_logs() {
        let action = |kind, last| accept_error_action(&io::Error::from(kind), last);
        for kind in [ErrorKind::Interrupted, ErrorKind::ConnectionAborted] {
            assert_eq!(
                action(kind, None),
                AcceptErrorAction::Retry,
                "transient accept errors must retry without backoff"
            );
        }
        assert_eq!(
            action(ErrorKind::PermissionDenied, None),
            AcceptErrorAction::LogAndBackoff,
            "first persistent error must be logged and backed off"
        );
        assert_eq!(
            action(
                ErrorKind::PermissionDenied,
                Some(ErrorKind::PermissionDenied)
            ),
            AcceptErrorAction::Backoff,
            "repeated persistent error must back off without log spam"
        );
        assert_eq!(
            action(ErrorKind::PermissionDenied, Some(ErrorKind::NotFound)),
            AcceptErrorAction::LogAndBackoff,
            "a changed persistent error must be logged again"
        );
    }
    static NEXT_PATH_ID: AtomicU64 = AtomicU64::new(0);

    fn temporary_path(label: &str) -> PathBuf {
        let unique = NEXT_PATH_ID.fetch_add(1, Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("mf-{label}-{}-{unique}", std::process::id()));
        fs::create_dir(&directory).expect("create unique IPC server test directory");
        directory.join("ipc.sock")
    }

    fn run_server(
        frame: &[u8],
        read_timeout: Duration,
        handler: impl FnOnce(&IpcCommand) -> IpcResponse + Send + 'static,
        expected: &[u8],
    ) {
        let (server, mut client) = UnixStream::pair().expect("create IPC test socket pair");
        client
            .set_read_timeout(Some(read_timeout))
            .expect("configure IPC client read timeout");
        let worker = thread::spawn(move || serve_connection(server, handler));
        client.write_all(frame).expect("send test frame");
        let mut response = Vec::new();
        client
            .read_to_end(&mut response)
            .expect("read server response");
        assert_eq!(response, expected, "Unexpected value for response");
        assert!(
            matches!(worker.join(), Ok(())),
            "IPC server worker must complete without panicking"
        );
    }

    #[test]
    fn complete_frame_gets_response() {
        run_server(
            b"STATUS\n",
            IPC_IO_TIMEOUT,
            |command| {
                assert_eq!(command, &IpcCommand::Status, "Unexpected value for command");
                IpcResponse::Ok(String::from("ready"))
            },
            b"OK ready\n",
        );
    }

    #[test]
    fn invalid_command_is_rejected() {
        run_server(
            b"INVALID\n",
            IPC_IO_TIMEOUT,
            |_| IpcResponse::Ok(String::from("unreachable")),
            b"ERR invalid command\n",
        );
    }

    #[test]
    fn newline_free_frame_times_out() {
        run_server(
            b"STATUS",
            IPC_IO_TIMEOUT.saturating_mul(2),
            |_| IpcResponse::Ok(String::from("unreachable")),
            b"ERR invalid frame\n",
        );
    }

    #[test]
    fn oversized_frame_is_rejected() {
        let frame = vec![b'x'; MAX_FRAME_BYTES.saturating_add(1)];
        run_server(
            &frame,
            IPC_IO_TIMEOUT,
            |_| IpcResponse::Ok(String::from("unreachable")),
            b"ERR invalid frame\n",
        );
    }

    #[test]
    fn ownership_cases_are_safe() {
        let dead_path = temporary_path("dead");
        let (dead_listener, dead_identity) =
            bind_listener(&dead_path, effective_uid()).expect("bind first listener");
        let first_metadata = fs::symlink_metadata(&dead_path)
            .expect("complete socket ownership test filesystem operation");
        assert_eq!(
            (first_metadata.dev(), first_metadata.ino()),
            dead_identity,
            "Unexpected value for (first_metadata.dev(), first_metadata.ino())"
        );

        let (new_listener, new_identity) =
            bind_listener(&dead_path, effective_uid()).expect("replace dead listener");
        assert_ne!(
            new_identity, dead_identity,
            "a bound-but-nonresponding listener is stale and must be replaced"
        );
        let new_metadata = fs::symlink_metadata(&dead_path)
            .expect("complete socket ownership test filesystem operation");
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
            bind_listener(&live_path, effective_uid()).expect("bind live listener");
        let responder = thread::spawn(move || {
            let (stream, _) = live_listener.accept()?;
            let mut reply = Vec::new();
            BufReader::new(&stream).read_until(b'\n', &mut reply)?;
            (&stream).write_all(b"STATUS inactive\n")?;
            io::Result::Ok(())
        });
        assert!(
            bind_listener(&live_path, effective_uid()).is_none(),
            "a live listener answering STATUS must not be replaced"
        );
        assert!(
            matches!(responder.join(), Ok(Ok(()))),
            "live responder must complete and serve STATUS without error"
        );
        let live_metadata = fs::symlink_metadata(&live_path)
            .expect("complete socket ownership test filesystem operation");
        assert_eq!(
            (live_metadata.dev(), live_metadata.ino()),
            live_identity,
            "the live listener must retain ownership"
        );
        remove_socket_if_owned(&live_path, live_identity);
        assert!(!live_path.exists(), "live socket cleanup must be safe");

        let stale_path = temporary_path("stale");
        let stale_listener = UnixListener::bind(&stale_path)
            .expect("complete socket ownership test filesystem operation");
        drop(stale_listener);
        let (stale_replacement, stale_identity) =
            bind_listener(&stale_path, effective_uid()).expect("replace stale socket");
        let stale_metadata = fs::symlink_metadata(&stale_path)
            .expect("complete socket ownership test filesystem operation");
        assert_eq!(
            (stale_metadata.dev(), stale_metadata.ino()),
            stale_identity,
            "Unexpected value for (stale_metadata.dev(), stale_metadata.ino())"
        );
        drop(stale_replacement);
        remove_socket_if_owned(&stale_path, stale_identity);
        assert!(!stale_path.exists(), "stale socket cleanup must be safe");

        for path in [&dead_path, &live_path, &stale_path] {
            fs::remove_dir(path.parent().expect("test socket directory"))
                .expect("clean IPC test directory");
        }
        check_regular_path();
    }

    fn check_regular_path() {
        let regular_path = temporary_path("regular");
        fs::write(&regular_path, b"not a socket")
            .expect("complete socket ownership test filesystem operation");
        assert!(
            bind_listener(&regular_path, effective_uid()).is_none(),
            "non-socket paths must not be removed"
        );
        assert!(
            regular_path.exists(),
            "Expected condition: regular_path.exists()"
        );
        fs::remove_file(&regular_path)
            .expect("complete socket ownership test filesystem operation");
        fs::remove_dir(regular_path.parent().expect("test directory"))
            .expect("clean regular path test directory");
    }

    #[test]
    fn bound_socket_has_mode_0600() {
        let path = temporary_path("mode");
        let (listener, identity) =
            bind_listener(&path, effective_uid()).expect("bind private socket");
        assert_eq!(
            fs::symlink_metadata(&path)
                .expect("inspect socket mode")
                .mode()
                & 0o777,
            0o600,
            "bound socket must explicitly restrict access"
        );
        drop(listener);
        remove_socket_if_owned(&path, identity);
        fs::remove_dir(path.parent().expect("test directory")).expect("clean mode test directory");
    }

    #[test]
    fn foreign_socket_is_never_probed_or_replaced() {
        let path = temporary_path("foreign");
        let listener = UnixListener::bind(&path).expect("bind live foreign fixture");
        listener.set_nonblocking(true).expect("nonblocking accept");
        let before = fs::symlink_metadata(&path).expect("inspect original socket");
        // Wrapping always selects a different identity without changing real ownership.
        assert!(
            bind_listener(&path, before.uid().wrapping_add(1)).is_none(),
            "foreign socket must be rejected"
        );
        let after = fs::symlink_metadata(&path).expect("foreign socket must remain");
        assert_eq!(
            (after.dev(), after.ino()),
            (before.dev(), before.ino()),
            "foreign socket must not be replaced"
        );
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == ErrorKind::WouldBlock),
            "foreign socket must receive no probe connections"
        );
        drop(listener);
        fs::remove_file(&path).expect("remove foreign fixture socket");
        fs::remove_dir(path.parent().expect("test directory"))
            .expect("remove foreign fixture directory");
    }

    #[test]
    fn socket_owner_predicate_rejects_foreign_uid() {
        assert!(
            same_owner(501, 501),
            "matching owner is eligible for probing"
        );
        assert!(
            !same_owner(502, 501),
            "foreign owner must never be probed or unlinked"
        );
        assert!(!same_owner(0, 501), "root-owned object is foreign too");
    }

    #[test]
    fn symlink_socket_path_is_preserved() {
        let path = temporary_path("link");
        let target = path.with_file_name("target");
        let listener = UnixListener::bind(&target).expect("bind symlink target");
        std::os::unix::fs::symlink(&target, &path).expect("create socket symlink");
        assert!(
            bind_listener(&path, effective_uid()).is_none(),
            "symlink must not be followed or replaced"
        );
        assert!(
            fs::symlink_metadata(&path)
                .expect("inspect preserved link")
                .file_type()
                .is_symlink(),
            "symlink must remain intact"
        );
        drop(listener);
        fs::remove_file(&path).expect("remove test link");
        fs::remove_file(target).expect("remove target socket");
        fs::remove_dir(path.parent().expect("test directory"))
            .expect("clean symlink test directory");
    }

    #[test]
    fn dribbling_frame_exceeds_total_budget() {
        let (server, mut client) = UnixStream::pair().expect("create dribbling socket pair");
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("bound response wait");
        let called = Arc::new(AtomicBool::new(false));
        let worker_called = Arc::clone(&called);
        let worker = thread::spawn(move || {
            serve_connection_with(
                server,
                |_| {
                    worker_called.store(true, Ordering::Relaxed);
                    IpcResponse::Status(None)
                },
                Duration::from_millis(100),
                &AtomicBool::new(false),
            );
        });
        let started = Instant::now();
        let mut sent = 0_usize;
        for byte in b"STATUS\n" {
            if client.write_all(&[*byte]).is_err() {
                break;
            }
            sent = sent.saturating_add(1);
            #[expect(
                clippy::disallowed_methods,
                reason = "Test-only pacing models a slow peer making progress within each inactivity timeout"
            )]
            thread::sleep(Duration::from_millis(30));
        }
        let mut response = Vec::new();
        client
            .read_to_end(&mut response)
            .expect("read rejected dribbling response");
        assert!(worker.join().is_ok(), "dribbling worker must terminate");
        assert_eq!(
            response, b"ERR invalid frame\n",
            "total budget must reject a progressing slow frame"
        );
        assert!(
            sent < b"STATUS\n".len(),
            "server must disconnect before the frame completes"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "deadline must close the connection promptly"
        );
        assert!(
            !called.load(Ordering::Relaxed),
            "slow frame must not dispatch a command"
        );
    }

    #[test]
    fn fast_frame_within_short_budget_succeeds() {
        let (server, mut client) = UnixStream::pair().expect("create fast socket pair");
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("bound fast response wait");
        let worker = thread::spawn(move || {
            serve_connection_with(
                server,
                |_| IpcResponse::Status(None),
                Duration::from_millis(100),
                &AtomicBool::new(false),
            );
        });
        client
            .write_all(b"STATUS\n")
            .expect("send complete fast frame");
        let mut response = Vec::new();
        client
            .read_to_end(&mut response)
            .expect("read fast response");
        assert!(worker.join().is_ok(), "fast frame worker must terminate");
        assert_eq!(
            response, b"STATUS inactive\n",
            "complete frame within budget must retain protocol"
        );
    }

    #[test]
    fn stop_cancels_silent_active_connection() {
        let path = temporary_path("stop");
        let mut server = IpcServer::start_with(
            &path,
            |_| panic!("silent client must never dispatch"),
            Duration::from_secs(10),
        )
        .expect("start cancellable test server");
        let mut client = UnixStream::connect(&path).expect("connect silent client");
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("bound cancellation wait");
        // A condition variable would add production synchronization only for a test.
        // yield_now lets the worker register its connection without a pacing sleep.
        let registration_started = Instant::now();
        while active_slot(&server.active).is_none()
            && registration_started.elapsed() < Duration::from_secs(2)
        {
            thread::yield_now();
        }
        let registered = active_slot(&server.active).is_some();
        let stopped = Instant::now();
        server.stop();
        let read = client.read(&mut [0_u8; 1]);
        let joined = server.worker.take().expect("retained worker handle").join();
        assert!(
            registered,
            "worker must register the silent connection before cancellation"
        );
        assert!(
            matches!(read, Ok(0))
                || matches!(read, Err(ref error) if matches!(error.kind(), ErrorKind::ConnectionReset | ErrorKind::BrokenPipe)),
            "stop must close the client rather than wait for its ten-second frame budget: {read:?}"
        );
        assert!(
            joined.is_ok(),
            "cancelled worker must finish without main-thread work"
        );
        assert!(
            stopped.elapsed() < Duration::from_secs(2),
            "shutdown must release silent connection promptly"
        );
        assert!(
            active_slot(&server.active).is_none(),
            "worker must clear active connection on exit"
        );
        fs::remove_dir(path.parent().expect("test directory"))
            .expect("clean cancellation test directory");
    }
}
