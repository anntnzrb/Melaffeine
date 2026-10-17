//! Menu-bar application entry point and single-instance bootstrap.

use std::fs::TryLockError;

use app::app_delegate::AppDelegate;
use app_core::ipc::{ensure_private_dir, lock_path, open_instance_lock, runtime_dir};
use objc2::MainThreadMarker;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

fn main() {
    if let Err(error) = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .try_init()
    {
        tracing::warn!(%error, "failed to initialize diagnostic subscriber");
    }
    let Some(mtm) = MainThreadMarker::new() else {
        tracing::error!("must run on main thread");
        return;
    };

    // Single instance: acquire an exclusive advisory file lock. An IPC ping
    // can false-negative if the existing instance is busy (e.g. main thread
    // blocked on an administrator password dialog), spawning duplicate instances.
    // The kernel releases the lock automatically on process exit or crash.
    let runtime_dir = match runtime_dir() {
        Ok(path) => path,
        Err(error) => {
            tracing::error!(%error, "failed to locate runtime directory");
            return;
        }
    };
    if let Err(error) = ensure_private_dir(&runtime_dir) {
        tracing::error!(%error, path = %runtime_dir.display(), "failed to secure runtime directory");
        return;
    }
    let lock_path = match lock_path() {
        Ok(path) => path,
        Err(error) => {
            tracing::error!(%error, path = %runtime_dir.display(), "failed to locate lock file");
            return;
        }
    };
    let _lock_file = match open_instance_lock(&lock_path) {
        Ok(file) => match file.try_lock() {
            Ok(()) => file,
            Err(TryLockError::WouldBlock) => {
                tracing::info!("Melaffeine is already running.");
                return;
            }
            Err(err) => {
                // Refuse to create UI or change system state without instance ownership.
                tracing::error!(error = %err, path = %lock_path.display(), "failed to acquire lock");
                return;
            }
        },
        Err(err) => {
            // Refuse to create UI or change system state without instance ownership.
            tracing::error!(error = %err, path = %lock_path.display(), "failed to open lock file");
            return;
        }
    };

    let app = NSApplication::sharedApplication(mtm);
    if !app.setActivationPolicy(NSApplicationActivationPolicy::Accessory) {
        tracing::warn!("failed to set accessory activation policy");
    }
    let delegate = AppDelegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
}
