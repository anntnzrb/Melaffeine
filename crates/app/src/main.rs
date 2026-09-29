use std::fs::{File, TryLockError};

use app::app_delegate::AppDelegate;
use app_core::ipc::socket_path;
use objc2::MainThreadMarker;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

fn main() {
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("must run on main thread");
        return;
    };

    // Single instance: acquire an exclusive advisory file lock. An IPC ping
    // can false-negative if the existing instance is busy (e.g. main thread
    // blocked on an administrator password dialog), spawning duplicate instances.
    // The kernel releases the lock automatically on process exit or crash.
    let lock_path = socket_path().with_extension("lock");
    let _lock_file = match File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
    {
        Ok(file) => match file.try_lock() {
            Ok(()) => Some(file),
            Err(TryLockError::WouldBlock) => {
                eprintln!("Melaffeine is already running.");
                return;
            }
            Err(err) => {
                // Fail open rather than refuse to launch if locking fails unexpectedly.
                eprintln!(
                    "Warning: failed to acquire lock on {}: {err}",
                    lock_path.display()
                );
                None
            }
        },
        Err(err) => {
            // Fail open rather than refuse to launch if open fails unexpectedly.
            eprintln!(
                "Warning: failed to open lock file {}: {err}",
                lock_path.display()
            );
            None
        }
    };

    let app = NSApplication::sharedApplication(mtm);
    let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let delegate = AppDelegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
}
