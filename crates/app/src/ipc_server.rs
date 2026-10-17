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
        && let Err(error) = fs::remove_file(path)
        && error.kind() != ErrorKind::NotFound
    {
        tracing::warn!(%error, path = %path.display(), "failed to remove owned IPC socket");
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
    let listener = match UnixListener::bind(path) {
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
            if let Err(error) = fs::remove_file(path) {
                if error.kind() != ErrorKind::NotFound {
                    tracing::warn!(%error, path = %path.display(), "failed to remove stale IPC socket");
                }
                return None;
            }
            match UnixListener::bind(path) {
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
    let mut frame = Vec::new();
    let mut chunk = [0_u8; 512];
    loop {
        // Saturation turns an exhausted absolute budget into a timeout, never a fresh budget.
        let remaining = budget.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(io::Error::from(ErrorKind::TimedOut));
        }
        stream.set_read_timeout(Some(remaining))?;
        let capacity = MAX_FRAME_BYTES
            .saturating_add(1)
            .saturating_sub(frame.len());
        let limit = u64::try_from(capacity).unwrap_or(u64::MAX);
        let length = match (&mut *stream).take(limit).read(&mut chunk) {
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            result => result?,
        };
        if length == 0 || started.elapsed() >= budget {
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
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::Interrupted | ErrorKind::ConnectionAborted
                ) => {}
            Err(error) => {
                if last_error_kind != Some(error.kind()) {
                    tracing::error!(%error, "IPC accept error");
                    last_error_kind = Some(error.kind());
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
