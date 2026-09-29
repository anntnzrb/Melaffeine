use std::time::Duration;

use app::app_delegate::AppDelegate;
use app_core::ipc::{IpcCommand, send_command, socket_path};
use objc2::MainThreadMarker;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

fn main() {
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("must run on main thread");
        return;
    };

    // Single instance: if another Melaffeine answers on the socket, exit before
    // touching AppKit, pmset, or the menu bar.
    if send_command(
        &socket_path(),
        &IpcCommand::Status,
        Duration::from_millis(500),
    )
    .is_ok()
    {
        eprintln!("Melaffeine is already running.");
        return;
    }

    let app = NSApplication::sharedApplication(mtm);
    let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let delegate = AppDelegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
}
