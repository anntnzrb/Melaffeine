//! Behavioral coverage for the public ipc contract.

#![cfg(test)]

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use app_core::ipc::{
    IpcCommand, IpcResponse, RuntimeDirError, SessionStatus, ensure_private_dir, lock_path_in,
    socket_path, socket_path_in,
};

#[test]
fn socket_path_uses_os_identity() {
    let dir = Path::new("/Users/test/Library/Application Support/Melaffeine");
    assert_eq!(
        socket_path_in(dir).expect("valid test socket path"),
        dir.join("melaffeine.sock"),
        "socket path must live in the supplied private runtime directory"
    );
    assert_eq!(
        lock_path_in(dir),
        dir.join("melaffeine.lock"),
        "lock shares the runtime directory"
    );
    if let Some(home) = std::env::home_dir() {
        let expected = home.join("Library/Application Support/Melaffeine/melaffeine.sock");
        if expected.as_os_str().len() >= 104 {
            assert!(
                matches!(socket_path(), Err(RuntimeDirError::PathTooLong { .. })),
                "overlong home socket path must report the macOS length limit"
            );
        } else {
            assert_eq!(
                socket_path().expect("home socket path fits macOS limit"),
                expected,
                "real socket path must use exactly the OS identity's home"
            );
        }
    } else {
        assert!(
            matches!(socket_path(), Err(RuntimeDirError::NoHome)),
            "missing OS home must report NoHome"
        );
    }
}

#[test]
fn test_ipc_command_serialize_and_parse() {
    let round_trips = [
        (
            IpcCommand::Start {
                duration: Some(Duration::from_hours(1)),
                keep_display_awake: false,
            },
            "START 3600s\n",
        ),
        (
            IpcCommand::Start {
                duration: Some(Duration::from_hours(2)),
                keep_display_awake: true,
            },
            "START 7200s display\n",
        ),
        (
            IpcCommand::Start {
                duration: None,
                keep_display_awake: false,
            },
            "START indefinite\n",
        ),
        (
            IpcCommand::Start {
                duration: None,
                keep_display_awake: true,
            },
            "START indefinite display\n",
        ),
        (IpcCommand::Stop, "STOP\n"),
        (IpcCommand::Toggle, "TOGGLE\n"),
        (IpcCommand::Status, "STATUS\n"),
        (IpcCommand::Quit, "QUIT\n"),
    ];

    for (command, wire) in round_trips {
        check_command_round_trip(command, wire);
    }

    // Case-insensitive and shorthand parse-only forms
    for (line, expected) in [
        ("stop\n", IpcCommand::Stop),
        ("status\n", IpcCommand::Status),
        (
            "START\n",
            IpcCommand::Start {
                duration: None,
                keep_display_awake: false,
            },
        ),
        (
            "START display\n",
            IpcCommand::Start {
                duration: None,
                keep_display_awake: true,
            },
        ),
    ] {
        assert_eq!(
            IpcCommand::parse(line),
            Some(expected),
            "shorthand or lowercase command {line:?} must parse"
        );
    }

    for line in [
        "",
        "   \n",
        "START invalid\n",
        "STOP 1h\n",
        "TOGGLE typo\n",
        "STATUS typo\n",
        "QUIT typo\n",
        "START 1h indefinite\n",
        "START indefinite 1h\n",
        "START 1h 2h\n",
        "START display display\n",
        "START display 2h\n",
        "START display indefinite\n",
        "START 1h display extra\n",
    ] {
        assert_eq!(IpcCommand::parse(line), None, "expected {line:?} rejected");
    }
}

#[test]
fn test_ipc_response_serialize_parse_display() {
    let cases = [
        (
            IpcResponse::Ok(String::from("Session started")),
            "OK Session started\n",
            "Session started",
        ),
        (
            IpcResponse::Err(String::from("Something broke")),
            "ERR Something broke\n",
            "Error: Something broke",
        ),
        // Empty messages round-trip via bare "OK"/"ERR" tokens
        (IpcResponse::Ok(String::new()), "OK \n", ""),
        (IpcResponse::Err(String::new()), "ERR \n", "Error: "),
        (
            IpcResponse::Status(Some(SessionStatus {
                keep_display_awake: true,
                remaining_compact: Some(String::from("1h 30m")),
            })),
            "STATUS active display 1h 30m\n",
            "Melaffeine: ACTIVE [1h 30m] (display awake)",
        ),
        (
            IpcResponse::Status(Some(SessionStatus {
                keep_display_awake: false,
                remaining_compact: Some(String::from("45m")),
            })),
            "STATUS active nodisplay 45m\n",
            "Melaffeine: ACTIVE [45m]",
        ),
        (
            IpcResponse::Status(Some(SessionStatus {
                keep_display_awake: true,
                remaining_compact: None,
            })),
            "STATUS active display\n",
            "Melaffeine: ACTIVE [active] (display awake)",
        ),
        (
            IpcResponse::Status(Some(SessionStatus {
                keep_display_awake: false,
                remaining_compact: None,
            })),
            "STATUS active nodisplay\n",
            "Melaffeine: ACTIVE [active]",
        ),
        (
            IpcResponse::Status(None),
            "STATUS inactive\n",
            "Melaffeine: INACTIVE",
        ),
    ];

    for (response, wire, display) in cases {
        assert_eq!(
            response.serialize(),
            wire,
            "bad wire format for {response:?}"
        );
        assert_eq!(format!("{response}"), display, "bad display for {wire:?}");
        assert_eq!(
            IpcResponse::parse(wire),
            Some(response),
            "wire line {wire:?} did not round-trip"
        );
    }

    for line in [
        "",
        "UNKNOWN response",
        // Token boundaries: bare prefixes without separator are rejected
        "OKAY",
        "ERROR",
        // Malformed status lines
        "STATUS",
        "STATUS active",
        "STATUS active maybe",
        "STATUS active maybe 1h 30m",
        "STATUS inactive display",
        "STATUS inactive nodisplay",
        "STATUS inactive 1h 30m",
        "STATUS unknown",
    ] {
        assert_eq!(IpcResponse::parse(line), None, "expected {line:?} rejected");
    }
}

#[track_caller]
fn check_command_round_trip(command: IpcCommand, wire: &str) {
    assert_eq!(command.serialize(), wire, "bad wire format for {command:?}");
    assert_eq!(
        IpcCommand::parse(wire),
        Some(command),
        "wire line {wire:?} did not round-trip"
    );
}

static NEXT_DIR_ID: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let id = NEXT_DIR_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("mf-core-{}-{id}", std::process::id()));
        fs::create_dir(&path).expect("create unique IPC test directory");
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("clean unique IPC test directory");
    }
}

#[test]
fn socket_path_rejects_macos_length_limit() {
    let temp = TestDir::new();
    let prefix = temp.0.as_os_str().as_encoded_bytes().len();
    let suffix = Path::new("melaffeine.sock")
        .as_os_str()
        .as_encoded_bytes()
        .len();
    let dir = temp.0.join(
        "x".repeat(
            103_usize
                .saturating_sub(prefix)
                .saturating_sub(suffix)
                .saturating_sub(2),
        ),
    );
    let accepted = socket_path_in(&dir).expect("103-byte path must fit");
    assert_eq!(
        accepted.as_os_str().as_encoded_bytes().len(),
        103,
        "test exercises the exact accepted boundary"
    );
    let too_long = dir.with_file_name(format!(
        "{}x",
        dir.file_name().expect("test component").to_string_lossy()
    ));
    assert!(
        matches!(socket_path_in(&too_long), Err(RuntimeDirError::PathTooLong { path }) if path == too_long.join("melaffeine.sock")),
        "104-byte socket path must be rejected with its path"
    );
}

#[test]
fn private_runtime_directory_is_created_with_mode_0700() {
    let temp = TestDir::new();
    let dir = temp.0.join("parents/runtime");
    ensure_private_dir(&dir).expect("create private runtime directory");
    assert_eq!(
        fs::symlink_metadata(&dir)
            .expect("inspect private directory")
            .permissions()
            .mode()
            & 0o777,
        0o700,
        "new directory must be private"
    );
}

#[test]
fn private_runtime_directory_tightens_owned_mode_0755() {
    let temp = TestDir::new();
    let dir = temp.0.join("runtime");
    fs::create_dir(&dir).expect("create permissive test directory");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755))
        .expect("set permissive directory mode");
    ensure_private_dir(&dir).expect("tighten owned runtime directory");
    assert_eq!(
        fs::metadata(&dir)
            .expect("inspect tightened directory")
            .permissions()
            .mode()
            & 0o777,
        0o700,
        "owned directory permissions must be tightened"
    );
}

#[test]
fn private_runtime_directory_rejects_symlink() {
    let temp = TestDir::new();
    let target = temp.0.join("target");
    fs::create_dir(&target).expect("create symlink target");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755))
        .expect("set target permissions");
    let dir = temp.0.join("runtime");
    symlink(&target, &dir).expect("create runtime symlink");
    assert!(
        matches!(ensure_private_dir(&dir), Err(RuntimeDirError::NotPrivate { path }) if path == dir),
        "symlink runtime directory must be rejected"
    );
    assert_eq!(
        fs::metadata(target)
            .expect("inspect unchanged target")
            .permissions()
            .mode()
            & 0o777,
        0o755,
        "rejection must not chmod the symlink target"
    );
}

#[test]
fn private_runtime_directory_rejects_regular_file() {
    let temp = TestDir::new();
    let dir = temp.0.join("runtime");
    fs::write(&dir, b"keep").expect("create non-directory runtime object");
    assert!(
        matches!(ensure_private_dir(&dir), Err(RuntimeDirError::NotPrivate { path }) if path == dir),
        "regular runtime file must be rejected"
    );
    assert_eq!(
        fs::read(dir).expect("read unchanged file"),
        b"keep",
        "rejection must preserve existing file contents"
    );
}

#[test]
fn instance_lock_is_private_regular_file() {
    let temp = TestDir::new();
    let path = lock_path_in(&temp.0);
    let lock = app_core::ipc::open_instance_lock(&path).expect("open new instance lock");
    let metadata = lock.metadata().expect("inspect opened lock");
    assert!(metadata.is_file(), "lock must be a regular file");
    assert_eq!(
        metadata.permissions().mode() & 0o777,
        0o600,
        "new lock must be private"
    );
    lock.try_lock().expect("fresh lock must be lockable");
}

#[test]
fn instance_lock_rejects_symlink_without_touching_target() {
    let temp = TestDir::new();
    let target = temp.0.join("target");
    fs::write(&target, b"keep").expect("create target");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640))
        .expect("set target permissions");
    let path = lock_path_in(&temp.0);
    symlink(&target, &path).expect("plant lock symlink");
    let error = app_core::ipc::open_instance_lock(&path).expect_err("symlink lock must fail");
    assert!(
        error.to_string().contains(&path.display().to_string()),
        "error must name rejected lock"
    );
    assert_eq!(
        fs::read(&target).expect("read preserved target"),
        b"keep",
        "target contents must be preserved"
    );
    assert_eq!(
        fs::metadata(&target)
            .expect("inspect target")
            .permissions()
            .mode()
            & 0o777,
        0o640,
        "target permissions must be preserved"
    );
    assert!(
        fs::symlink_metadata(&path)
            .expect("inspect preserved link")
            .file_type()
            .is_symlink(),
        "lock link must remain"
    );
}

#[test]
fn instance_lock_rejects_directory() {
    let temp = TestDir::new();
    let path = lock_path_in(&temp.0);
    fs::create_dir(&path).expect("plant directory at lock path");
    let error = app_core::ipc::open_instance_lock(&path).expect_err("directory lock must fail");
    assert!(
        error.to_string().contains(&path.display().to_string()),
        "error must name rejected directory"
    );
    assert!(path.is_dir(), "directory must remain untouched");
}

#[test]
fn instance_lock_rejects_owned_fifo() {
    let temp = TestDir::new();
    let path = lock_path_in(&temp.0);
    let status = std::process::Command::new("/usr/bin/mkfifo")
        .arg(&path)
        .status()
        .expect("create owned FIFO at lock path");
    assert!(status.success(), "FIFO fixture creation must succeed");
    assert!(
        matches!(app_core::ipc::open_instance_lock(&path), Err(app_core::ipc::LockError::Invalid { path: rejected }) if rejected == path),
        "an owned non-regular descriptor must fail validation"
    );
}

#[test]
fn private_runtime_directory_rejects_parent_that_is_a_file() {
    let temp = TestDir::new();
    let parent = temp.0.join("file");
    fs::write(&parent, b"preserve me").expect("create non-directory parent");
    let dir = parent.join("runtime");
    assert!(
        matches!(
            ensure_private_dir(&dir),
            Err(RuntimeDirError::Filesystem { .. })
        ),
        "a non-directory parent must propagate a filesystem failure"
    );
    assert_eq!(
        fs::read(parent).expect("inspect preserved parent"),
        b"preserve me",
        "failed runtime preparation must not change the parent file"
    );
}
