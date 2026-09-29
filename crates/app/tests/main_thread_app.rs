#![allow(
    deprecated,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::arithmetic_side_effects,
    clippy::duration_suboptimal_units,
    clippy::as_conversions,
    clippy::unnecessary_cast,
    dead_code
)]

use std::time::{Duration, Instant, SystemTime};

use app::app_delegate::AppDelegate;
use app::ui::{
    DEFAULT_DURATION_TEXT, ERROR_DURATION_INVALID, TITLE_KEEP_DISPLAY_AWAKE,
    TITLE_KEEP_RUNNING_LID_CLOSED, TITLE_RUN_INDEFINITELY, TITLE_START, TITLE_STOP,
    UNIT_DAYS_INDEX, UNIT_HOURS_INDEX, UNIT_MINUTES_INDEX, build_content_view,
};
use app_core::ipc::{IpcCommand, IpcResponse, SessionStatus};
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, msg_send};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSControlStateValueOff, NSControlStateValueOn,
};
use objc2_foundation::{NSDate, NSNotification, NSRunLoop, NSString};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--list") {
        println!("main_thread_app: test");
        return;
    }

    println!("Running comprehensive main thread app integration tests...");
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("Test runner must execute on the main thread!");
        std::process::exit(1);
    };

    let app = NSApplication::sharedApplication(mtm);
    let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    // 1. Test build_content_view
    let controls = build_content_view(mtm);
    assert_eq!(
        controls.indefinite_button.title().to_string(),
        TITLE_RUN_INDEFINITELY
    );
    assert_eq!(controls.start_stop_button.title().to_string(), TITLE_START);
    assert_eq!(
        controls.keep_display_awake_button.title().to_string(),
        TITLE_KEEP_DISPLAY_AWAKE
    );
    assert_eq!(
        controls.lid_closed_button.title().to_string(),
        TITLE_KEEP_RUNNING_LID_CLOSED
    );
    assert_eq!(controls.lid_closed_button.state(), NSControlStateValueOff);
    assert_eq!(
        controls.duration_field.stringValue().to_string(),
        DEFAULT_DURATION_TEXT
    );

    // 2. Test AppDelegate allocation and full lifecycle
    let delegate = AppDelegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    // Test delegate init_app_state on main thread
    delegate.init_app_state(mtm);

    // Test delegate notifications
    let notif = unsafe {
        NSNotification::notificationWithName_object(&NSString::from_str("TestNotification"), None)
    };
    unsafe {
        let _: () = msg_send![&*delegate, applicationDidFinishLaunching: &*notif];
        let _: () = msg_send![&*delegate, applicationDidResignActive: &*notif];
    }

    // Inspect ivars
    let state_opt = delegate.ivars().borrow();
    let state = state_opt.as_ref().expect("AppState must be initialized");
    let duration_field = state.controls.duration_field.clone();
    let indefinite_button = state.controls.indefinite_button.clone();
    let display_awake_button = state.controls.keep_display_awake_button.clone();
    let lid_closed_button = state.controls.lid_closed_button.clone();
    let unit_popup = state.controls.unit_popup.clone();
    let start_stop_button = state.controls.start_stop_button.clone();
    let status_item = state.status_item.clone();
    let time_label = state.controls.time_label.clone();
    let error_label = state.controls.error_label.clone();
    drop(state_opt);

    // NEVER tick `lid_closed_button` before a Start in tests: that would trigger
    // the macOS administrator password prompt / sudo.
    assert_eq!(
        lid_closed_button.title().to_string(),
        TITLE_KEEP_RUNNING_LID_CLOSED
    );
    assert_eq!(lid_closed_button.state(), NSControlStateValueOff);
    assert!(lid_closed_button.isEnabled());

    // Validation error display and persistence across update_ui
    delegate.show_error("Test Error Message");
    assert_eq!(error_label.stringValue().to_string(), "Test Error Message");
    assert!(!error_label.isHidden());
    delegate.update_ui();
    assert_eq!(error_label.stringValue().to_string(), "Test Error Message");
    assert!(!error_label.isHidden());

    // Action handler: controlChanged clears error
    unsafe {
        let _: () = msg_send![&*delegate, controlChanged: &*duration_field];
    }
    assert_eq!(error_label.stringValue().to_string(), "");
    assert!(error_label.isHidden());

    // Finite session start / stop (minutes)
    duration_field.setStringValue(&NSString::from_str("15"));
    unit_popup.selectItemAtIndex(UNIT_MINUTES_INDEX);
    assert_eq!(delegate.handle_start_stop(), Ok(true));
    assert_eq!(error_label.stringValue().to_string(), "");
    assert!(error_label.isHidden());
    assert!(!time_label.isHidden());
    assert_eq!(start_stop_button.title().to_string(), TITLE_STOP);
    assert!(!lid_closed_button.isEnabled());

    let future_end = SystemTime::now() + Duration::from_secs(900);
    let countdown_res = delegate.format_countdown(Some(future_end));
    assert!(countdown_res.is_some());
    let past_end = SystemTime::now()
        .checked_sub(Duration::from_secs(10))
        .unwrap();
    assert!(delegate.format_countdown(Some(past_end)).is_none());
    assert!(delegate.format_countdown(None).is_none());

    // Stop session
    assert_eq!(delegate.handle_start_stop(), Ok(false));
    assert_eq!(start_stop_button.title().to_string(), TITLE_START);
    assert!(lid_closed_button.isEnabled());

    // Indefinite session start / stop with display awake
    indefinite_button.setState(NSControlStateValueOn);
    delegate.update_ui();
    assert!(!lid_closed_button.isEnabled());
    display_awake_button.setState(NSControlStateValueOn);
    assert_eq!(delegate.handle_start_stop(), Ok(true));
    assert_eq!(start_stop_button.title().to_string(), TITLE_STOP);
    assert!(!lid_closed_button.isEnabled());
    assert_eq!(delegate.handle_start_stop(), Ok(false));
    assert_eq!(start_stop_button.title().to_string(), TITLE_START);

    // Invalid input error path
    indefinite_button.setState(NSControlStateValueOff);
    delegate.update_ui();
    assert!(lid_closed_button.isEnabled());
    duration_field.setStringValue(&NSString::from_str("invalid_number"));
    assert_eq!(
        delegate.handle_start_stop(),
        Err(String::from(ERROR_DURATION_INVALID))
    );
    delegate.show_error(ERROR_DURATION_INVALID);
    assert_eq!(
        error_label.stringValue().to_string(),
        ERROR_DURATION_INVALID
    );
    assert!(!error_label.isHidden());

    // Hours and Days units
    duration_field.setStringValue(&NSString::from_str("3"));
    unit_popup.selectItemAtIndex(UNIT_HOURS_INDEX);
    assert_eq!(delegate.handle_start_stop(), Ok(true));
    assert_eq!(error_label.stringValue().to_string(), "");
    assert!(error_label.isHidden());
    delegate.stop_session();

    duration_field.setStringValue(&NSString::from_str("2"));
    unit_popup.selectItemAtIndex(UNIT_DAYS_INDEX);
    assert_eq!(delegate.handle_start_stop(), Ok(true));
    delegate.stop_session();

    // Popover toggle
    if let Some(button) = status_item.button(mtm) {
        delegate.toggle_popover_relative_to(&button);

        duration_field.setStringValue(&NSString::from_str("10"));
        unit_popup.selectItemAtIndex(UNIT_MINUTES_INDEX);
        assert_eq!(delegate.handle_start_stop(), Ok(true));
        assert_eq!(start_stop_button.title().to_string(), TITLE_STOP);

        delegate.toggle_popover_relative_to(&button);
        delegate.close_popover();
        delegate.stop_session();
    }

    // Outside click monitors and teardown
    delegate.install_outside_click_monitor();
    delegate.remove_outside_click_monitor();
    delegate.teardown();
    let status_resp = delegate.execute_ipc_command(&IpcCommand::Status);
    assert_eq!(status_resp, IpcResponse::Status(None));

    // Toggle when inactive with invalid input must report the validation error.
    indefinite_button.setState(NSControlStateValueOff);
    duration_field.setStringValue(&NSString::from_str("invalid_number"));
    let invalid_toggle = delegate.execute_ipc_command(&IpcCommand::Toggle);
    assert_eq!(
        invalid_toggle,
        IpcResponse::Err(String::from(ERROR_DURATION_INVALID))
    );
    let invalid_status = delegate.execute_ipc_command(&IpcCommand::Status);
    assert_eq!(invalid_status, IpcResponse::Status(None));

    // A finite IPC start remains finite even when the editable checkbox is checked.
    indefinite_button.setState(NSControlStateValueOn);
    delegate.show_error("stale successful start error");
    let start_resp = delegate.execute_ipc_command(&IpcCommand::Start {
        duration: Some(Duration::from_secs(120)),
        keep_display_awake: true,
    });
    assert!(matches!(start_resp, IpcResponse::Ok(_)));
    delegate.update_ui();
    assert!(!time_label.isHidden());
    assert_ne!(time_label.stringValue().to_string(), "");
    assert_eq!(error_label.stringValue().to_string(), "");
    assert!(error_label.isHidden());

    let status_resp2 = delegate.execute_ipc_command(&IpcCommand::Status);
    assert!(matches!(
        status_resp2,
        IpcResponse::Status(Some(SessionStatus {
            keep_display_awake: true,
            remaining_compact: Some(_),
        }))
    ));

    // Toggle while active stops sleep prevention.
    let toggle_resp = delegate.execute_ipc_command(&IpcCommand::Toggle);
    assert_eq!(
        toggle_resp,
        IpcResponse::Ok(String::from("Stopped sleep prevention"))
    );
    assert_eq!(
        delegate.execute_ipc_command(&IpcCommand::Status),
        IpcResponse::Status(None)
    );

    let stop_resp = delegate.execute_ipc_command(&IpcCommand::Stop);
    assert_eq!(
        stop_resp,
        IpcResponse::Ok(String::from("Stopped sleep prevention"))
    );

    // Toggle when inactive with valid input starts session
    duration_field.setStringValue(&NSString::from_str("2"));
    unit_popup.selectItemAtIndex(UNIT_HOURS_INDEX);
    let toggle_inactive = delegate.execute_ipc_command(&IpcCommand::Toggle);
    assert_eq!(
        toggle_inactive,
        IpcResponse::Ok(String::from("Started sleep prevention"))
    );
    delegate.stop_session();

    // Start indefinite via IPC
    let start_indef_resp = delegate.execute_ipc_command(&IpcCommand::Start {
        duration: None,
        keep_display_awake: false,
    });
    assert!(matches!(start_indef_resp, IpcResponse::Ok(_)));
    assert_eq!(
        delegate.execute_ipc_command(&IpcCommand::Status),
        IpcResponse::Status(Some(SessionStatus {
            keep_display_awake: false,
            remaining_compact: None,
        }))
    );
    delegate.stop_session();
    // Popover toggle open and close
    if let Some(btn) = status_item.button(mtm) {
        delegate.toggle_popover_relative_to(&btn);
        delegate.toggle_popover_relative_to(&btn);
    }
    delegate.close_popover();
    // Real timer expiry via NSRunLoop
    let start_finite = delegate.execute_ipc_command(&IpcCommand::Start {
        duration: Some(Duration::from_secs(1)),
        keep_display_awake: false,
    });
    assert!(matches!(start_finite, IpcResponse::Ok(_)));
    assert!(
        delegate
            .ivars()
            .borrow()
            .as_ref()
            .unwrap()
            .power
            .is_active()
    );

    let run_loop = NSRunLoop::currentRunLoop();
    let start_time = Instant::now();
    while delegate
        .ivars()
        .borrow()
        .as_ref()
        .unwrap()
        .power
        .is_active()
        && start_time.elapsed() < Duration::from_secs(3)
    {
        let until = NSDate::dateWithTimeIntervalSinceNow(0.1);
        run_loop.runUntilDate(&until);
    }

    let is_active = delegate
        .ivars()
        .borrow()
        .as_ref()
        .unwrap()
        .power
        .is_active();
    assert!(
        !is_active,
        "PowerController must be inactive after expiry timer fires"
    );
    assert_eq!(
        delegate.execute_ipc_command(&IpcCommand::Status),
        IpcResponse::Status(None)
    );
    assert_eq!(start_stop_button.title().to_string(), TITLE_START);
    // Termination lifecycle
    unsafe {
        let _: () = msg_send![&*delegate, applicationWillTerminate: &*notif];
    }
    println!("All main thread integration tests PASSED!");

    // Quit IPC command is tested last so the 0.05s termination timer does not
    // fire during NSRunLoop processing in earlier tests.
    let quit_resp = delegate.execute_ipc_command(&IpcCommand::Quit);
    assert_eq!(quit_resp, IpcResponse::Ok(String::from("Terminating")));
}
