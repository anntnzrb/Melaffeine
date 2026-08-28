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

use std::time::{Duration, SystemTime};

use app::app_delegate::AppDelegate;
use app::iokit::IOKitProvider;
use app::login::LoginItemService;
use app::power::{AssertionKind, AssertionProvider, PowerController, PowerError};
use app::ui::{
    DEFAULT_DURATION_TEXT, TITLE_KEEP_DISPLAY_AWAKE, TITLE_LAUNCH_AT_LOGIN, TITLE_RUN_INDEFINITELY,
    TITLE_START, TITLE_STOP, UNIT_DAYS_INDEX, UNIT_HOURS_INDEX, UNIT_MINUTES_INDEX,
    build_content_view, compute_ui_projection,
};
use app_core::duration::{DurationUnit, format_compact_duration, parse_duration};
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, msg_send};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSControlStateValueOff, NSControlStateValueOn,
};
use objc2_foundation::{NSNotification, NSString};

#[derive(Clone, Default)]
struct MockProvider {
    fail: bool,
}

struct MockHandle;

impl AssertionProvider for MockProvider {
    type Handle = MockHandle;
    fn acquire(&self, _kind: AssertionKind) -> Result<Self::Handle, PowerError> {
        if self.fail {
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
        controls.launch_at_login_button.title().to_string(),
        TITLE_LAUNCH_AT_LOGIN
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
    let launch_at_login_button = state.launch_at_login_button.clone();
    let status_item = state.status_item.clone();
    drop(state_opt);

    // UI sync and error display
    delegate.update_ui();
    delegate.show_error("Test Error Message");

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
    delegate.update_ui();

    let future_end = SystemTime::now() + Duration::from_secs(900);
    let countdown_res = delegate.format_countdown(Some(future_end));
    assert!(countdown_res.is_some());
    let past_end = SystemTime::now()
        .checked_sub(Duration::from_secs(10))
        .unwrap();
    assert!(delegate.format_countdown(Some(past_end)).is_none());
    assert!(delegate.format_countdown(None).is_none());

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

    // Hours and Days units
    duration_field.setStringValue(&NSString::from_str("3"));
    unit_popup.selectItemAtIndex(UNIT_HOURS_INDEX);
    delegate.handle_start_stop();
    delegate.stop_power_and_expiry();

    duration_field.setStringValue(&NSString::from_str("2"));
    unit_popup.selectItemAtIndex(UNIT_DAYS_INDEX);
    delegate.handle_start_stop();
    delegate.stop_power_and_expiry();

    // Launch at login handler via msg_send (safe unregister/noop path only)
    launch_at_login_button.setState(NSControlStateValueOff);
    unsafe {
        let _: () = msg_send![&*delegate, launchAtLoginChanged: &*launch_at_login_button];
    }

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

    // 3. Test LoginItemService
    let _ = LoginItemService::is_enabled();
    let _ = LoginItemService::set_enabled(false);

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

    let mut fail_power = PowerController::new(MockProvider { fail: true });
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
    assert_eq!(format_compact_duration(Duration::from_secs(30)), "<1m");
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
