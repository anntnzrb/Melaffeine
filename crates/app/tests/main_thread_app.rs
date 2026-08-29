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

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use app::app_delegate::AppDelegate;
use app::iokit::IOKitProvider;
use app::power::{AssertionKind, AssertionProvider, PowerController, PowerError};
use app::ui::{
    DEFAULT_DURATION_TEXT, ERROR_DURATION_INVALID, ICON_INACTIVE, TITLE_KEEP_DISPLAY_AWAKE,
    TITLE_RUN_INDEFINITELY, TITLE_START, TITLE_STOP, UNIT_DAYS_INDEX, UNIT_HOURS_INDEX,
    UNIT_MINUTES_INDEX, build_content_view, compute_ui_projection,
};
use app_core::duration::{DurationUnit, format_compact_duration, parse_duration};
use app_core::ipc::{IpcCommand, IpcResponse};
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, msg_send};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSControlStateValueOff, NSControlStateValueOn,
};
use objc2_foundation::{NSNotification, NSString, NSTimer};

#[derive(Clone, Default)]
struct MockProvider {
    fail: Rc<Cell<bool>>,
}

#[derive(Debug)]
struct MockHandle;

impl AssertionProvider for MockProvider {
    type Handle = MockHandle;
    fn acquire(&self, _kind: AssertionKind) -> Result<Self::Handle, PowerError> {
        if self.fail.get() {
            Err(PowerError::AcquisitionFailed(-1))
        } else {
            Ok(MockHandle)
        }
    }
}

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
    let duration_field = state.duration_field.clone();
    let indefinite_button = state.indefinite_button.clone();
    let display_awake_button = state.display_awake_button.clone();
    let unit_popup = state.unit_popup.clone();
    let start_stop_button = state.start_stop_button.clone();
    let status_item = state.status_item.clone();
    let time_label = state.time_label.clone();
    let error_label = state.error_label.clone();
    drop(state_opt);

    // UI sync and error display
    delegate.update_ui();
    delegate.show_error("Test Error Message");
    delegate.update_ui();
    assert_eq!(error_label.stringValue().to_string(), "");
    assert!(error_label.isHidden());

    // Action handler: controlChanged
    unsafe {
        let _: () = msg_send![&*delegate, controlChanged: &*duration_field];
    }

    // Finite session start / stop (minutes)
    duration_field.setStringValue(&NSString::from_str("15"));
    unit_popup.selectItemAtIndex(UNIT_MINUTES_INDEX);
    unsafe {
        let _: () = msg_send![&*delegate, startStopClicked: &*start_stop_button];
    }
    assert_eq!(error_label.stringValue().to_string(), "");
    assert!(error_label.isHidden());
    assert!(!time_label.isHidden());
    delegate.update_ui();

    let future_end = SystemTime::now() + Duration::from_secs(900);
    let countdown_res = delegate.format_countdown(Some(future_end));
    assert!(countdown_res.is_some());
    let past_end = SystemTime::now()
        .checked_sub(Duration::from_secs(10))
        .unwrap();
    assert!(delegate.format_countdown(Some(past_end)).is_none());
    assert!(delegate.format_countdown(None).is_none());
    delegate.show_error("stale active error");
    delegate.update_ui();
    assert_eq!(error_label.stringValue().to_string(), "");
    assert!(error_label.isHidden());

    // Stop session
    unsafe {
        let _: () = msg_send![&*delegate, startStopClicked: &*start_stop_button];
    }
    delegate.update_ui();

    // Indefinite session start / stop with display awake
    indefinite_button.setState(NSControlStateValueOn);
    display_awake_button.setState(NSControlStateValueOn);
    unsafe {
        let _: () = msg_send![&*delegate, startStopClicked: &*start_stop_button];
    }
    delegate.update_ui();
    unsafe {
        let _: () = msg_send![&*delegate, startStopClicked: &*start_stop_button];
    }

    // Invalid input error path
    indefinite_button.setState(NSControlStateValueOff);
    duration_field.setStringValue(&NSString::from_str("invalid_number"));
    unsafe {
        let _: () = msg_send![&*delegate, startStopClicked: &*start_stop_button];
    }
    assert_eq!(
        error_label.stringValue().to_string(),
        ERROR_DURATION_INVALID
    );
    assert!(!error_label.isHidden());

    // Hours and Days units
    duration_field.setStringValue(&NSString::from_str("3"));
    unit_popup.selectItemAtIndex(UNIT_HOURS_INDEX);
    assert!(delegate.handle_start_stop().is_ok());
    assert_eq!(error_label.stringValue().to_string(), "");
    assert!(error_label.isHidden());
    delegate.stop_power_and_expiry();

    duration_field.setStringValue(&NSString::from_str("2"));
    unit_popup.selectItemAtIndex(UNIT_DAYS_INDEX);
    assert!(delegate.handle_start_stop().is_ok());
    delegate.stop_power_and_expiry();

    // Popover toggle
    if let Some(button) = status_item.button(mtm) {
        delegate.toggle_popover_relative_to(&button);

        duration_field.setStringValue(&NSString::from_str("10"));
        unit_popup.selectItemAtIndex(UNIT_MINUTES_INDEX);
        unsafe {
            let _: () = msg_send![&*delegate, startStopClicked: &*start_stop_button];
        }
        delegate.update_ui();

        delegate.toggle_popover_relative_to(&button);
        delegate.close_popover();
    }

    // Outside click monitors and teardown
    delegate.install_outside_click_monitor();
    delegate.remove_outside_click_monitor();
    delegate.teardown();
    // 3. Test IPC command execution
    let status_resp = delegate.execute_ipc_command(&IpcCommand::Status);
    assert!(matches!(
        status_resp,
        IpcResponse::Status {
            is_active: false,
            ..
        }
    ));

    // Toggle when inactive with invalid input must report the validation error.
    indefinite_button.setState(NSControlStateValueOff);
    duration_field.setStringValue(&NSString::from_str("invalid_number"));
    let invalid_toggle = delegate.execute_ipc_command(&IpcCommand::Toggle);
    assert_eq!(
        invalid_toggle,
        IpcResponse::Err(String::from(ERROR_DURATION_INVALID))
    );
    let invalid_status = delegate.execute_ipc_command(&IpcCommand::Status);
    assert!(matches!(
        invalid_status,
        IpcResponse::Status {
            is_active: false,
            ..
        }
    ));

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
        IpcResponse::Status {
            is_active: true,
            keep_display_awake: true,
            ends_at_unix: Some(_),
            ..
        }
    ));

    // Current active sessions also clear stale errors.
    delegate.show_error("stale active error");
    delegate.update_ui();
    assert_eq!(error_label.stringValue().to_string(), "");
    assert!(error_label.isHidden());

    let toggle_resp = delegate.execute_ipc_command(&IpcCommand::Toggle);
    assert!(matches!(toggle_resp, IpcResponse::Ok(_)));

    let stop_resp = delegate.execute_ipc_command(&IpcCommand::Stop);
    assert!(matches!(stop_resp, IpcResponse::Ok(_)));

    // A no-conflict inactive projection clears stale errors as well.
    if app::conflicts::detect_external_conflict().is_none() {
        delegate.show_error("stale inactive error");
        delegate.update_ui();
        assert_eq!(error_label.stringValue().to_string(), "");
        assert!(error_label.isHidden());
    }

    // Toggle when inactive -> starts session
    let toggle_inactive = delegate.execute_ipc_command(&IpcCommand::Toggle);
    assert!(matches!(toggle_inactive, IpcResponse::Ok(_)));
    delegate.stop_power_and_expiry();

    // Start indefinite via IPC
    let start_indef_resp = delegate.execute_ipc_command(&IpcCommand::Start {
        duration: None,
        keep_display_awake: false,
    });
    assert!(matches!(start_indef_resp, IpcResponse::Ok(_)));
    delegate.stop_power_and_expiry();

    let quit_resp = delegate.execute_ipc_command(&IpcCommand::Quit);
    assert!(matches!(quit_resp, IpcResponse::Ok(_)));

    // Popover, Menu & Timer tests
    if let Some(btn) = status_item.button(mtm) {
        delegate.toggle_popover_relative_to(&btn);
        delegate.toggle_popover_relative_to(&btn);
    }
    delegate.install_outside_click_monitor();
    delegate.remove_outside_click_monitor();
    delegate.start_countdown_timer_if_needed();
    // Manually install and invalidate countdown timer to cover branch
    let test_timer = unsafe {
        NSTimer::scheduledTimerWithTimeInterval_repeats_block(
            10.0,
            false,
            &block2::RcBlock::new(|_| {}),
        )
    };
    delegate
        .ivars()
        .borrow_mut()
        .as_mut()
        .unwrap()
        .countdown_timer = Some(test_timer);
    delegate.stop_countdown_timer();
    assert!(
        delegate
            .ivars()
            .borrow()
            .as_ref()
            .unwrap()
            .countdown_timer
            .is_none()
    );
    delegate.close_popover();

    // Timer expiry via NSRunLoop
    delegate
        .start_session(Some(Duration::from_secs(60)), false)
        .unwrap();
    delegate.start_countdown_timer_if_needed();
    delegate.stop_power_and_expiry();

    // Termination lifecycle
    unsafe {
        let _: () = msg_send![&*delegate, applicationWillTerminate: &*notif];
    }
    // 4. Test IOKitProvider
    let provider = IOKitProvider;
    assert_eq!(format!("{provider:?}"), "IOKitProvider");
    let sys_handle = provider.acquire(AssertionKind::PreventSystemSleep);
    if let Ok(h) = sys_handle {
        assert!(format!("{h:?}").contains("IOKitAssertion"));
        drop(h);
    }
    let disp_handle = provider.acquire(AssertionKind::PreventDisplaySleep);
    if let Ok(h) = disp_handle {
        drop(h);
    }

    // 5. Test PowerController with IOKitProvider and MockProvider
    let mut iokit_power = PowerController::new(IOKitProvider);
    assert!(!iokit_power.is_active());
    assert!(!iokit_power.keep_display_awake());
    assert_eq!(iokit_power.started_at(), None);
    assert_eq!(iokit_power.ends_at(), None);
    let _ = format!("{iokit_power:?}");
    let now = SystemTime::now();
    let _ = iokit_power.start(Some(Duration::from_secs(60)), true, now);
    let _ = iokit_power.keep_display_awake();
    let _ = iokit_power.started_at();
    let _ = iokit_power.ends_at();
    let _ = format!("{iokit_power:?}");
    iokit_power.stop();

    let mock = MockProvider::default();
    let mut power = PowerController::new(mock);
    assert!(!power.is_active());
    assert!(!power.keep_display_awake());
    assert_eq!(power.started_at(), None);
    assert_eq!(power.ends_at(), None);
    assert!(
        power
            .start(Some(Duration::from_secs(60)), true, now)
            .is_ok()
    );
    assert!(power.is_active());
    assert!(power.keep_display_awake());
    assert_eq!(power.started_at(), Some(now));
    assert_eq!(power.ends_at(), Some(now + Duration::from_secs(60)));
    let dbg = format!("{power:?}");
    assert!(dbg.contains("PowerController"));
    power.stop();
    assert!(!power.is_active());

    // Replacement acquisition errors preserve the platform error and reset projection state.
    let replacement_provider = MockProvider::default();
    let mut replacement_power = PowerController::new(replacement_provider.clone());
    assert!(replacement_power.start(None, false, now).is_ok());
    replacement_provider.fail.set(true);
    let replacement_error = replacement_power
        .start(None, false, now)
        .expect_err("replacement acquisition should fail");
    assert_eq!(
        replacement_error.to_string(),
        "Failed to acquire power assertion (code: -1)"
    );
    assert!(!replacement_power.is_active());
    assert_eq!(replacement_power.ends_at(), None);
    let replacement_projection = compute_ui_projection(
        replacement_power.is_active(),
        replacement_power.ends_at().is_none(),
        None,
    );
    assert_eq!(replacement_projection.status_icon, ICON_INACTIVE);
    assert_eq!(replacement_projection.start_stop_title, TITLE_START);

    let fail_provider = MockProvider::default();
    fail_provider.fail.set(true);
    let mut fail_power = PowerController::new(fail_provider);
    assert!(fail_power.start(None, false, now).is_err());
    assert!(!fail_power.is_active());

    // 6. Test AppCore duration functions
    assert_eq!(
        parse_duration("10", DurationUnit::Minutes),
        Some(Duration::from_secs(600))
    );
    assert_eq!(
        parse_duration("1", DurationUnit::Hours),
        Some(Duration::from_secs(3600))
    );
    assert_eq!(
        parse_duration("1", DurationUnit::Days),
        Some(Duration::from_secs(86400))
    );
    assert_eq!(parse_duration("0", DurationUnit::Minutes), None);
    assert_eq!(parse_duration("-5", DurationUnit::Minutes), None);
    assert_eq!(parse_duration("abc", DurationUnit::Minutes), None);
    assert_eq!(parse_duration("", DurationUnit::Minutes), None);
    assert_eq!(parse_duration("525601", DurationUnit::Minutes), None);

    assert_eq!(format_compact_duration(Duration::from_nanos(0)), "<1m");
    assert_eq!(format_compact_duration(Duration::from_secs(30)), "1m");
    assert_eq!(format_compact_duration(Duration::from_secs(60)), "1m");
    assert_eq!(format_compact_duration(Duration::from_secs(3600)), "1h 0m");
    assert_eq!(format_compact_duration(Duration::from_secs(3660)), "1h 1m");
    assert_eq!(format_compact_duration(Duration::from_secs(86400)), "1d 0h");
    assert_eq!(
        format_compact_duration(Duration::from_secs(86400 + 7200)),
        "1d 2h"
    );

    // 7. Test UiProjection combinations
    let proj1 = compute_ui_projection(false, false, None);
    assert_eq!(proj1.start_stop_title, TITLE_START);
    let proj2 = compute_ui_projection(true, true, None);
    assert_eq!(proj2.start_stop_title, TITLE_STOP);
    let proj3 = compute_ui_projection(true, false, Some("5m".to_string()));
    assert_eq!(proj3.countdown_text, Some("5m".to_string()));

    println!("All main thread integration tests PASSED!");
}
