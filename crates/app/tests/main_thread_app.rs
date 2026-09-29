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
    AwakeMode, DEFAULT_DURATION_TEXT, ERROR_DURATION_INVALID, TITLE_MODE_DISPLAY, TITLE_MODE_LID,
    TITLE_MODE_SYSTEM, TITLE_NO_TIME_LIMIT, TITLE_START, TITLE_STOP, UNIT_DAYS_INDEX,
    UNIT_HOURS_INDEX, UNIT_MINUTES_INDEX, build_content_view,
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
        controls.no_time_limit_button.title().to_string(),
        TITLE_NO_TIME_LIMIT
    );
    assert_eq!(controls.start_stop_button.title().to_string(), TITLE_START);
    assert_eq!(
        controls.mode_system_button.title().to_string(),
        TITLE_MODE_SYSTEM
    );
    assert_eq!(
        controls.mode_display_button.title().to_string(),
        TITLE_MODE_DISPLAY
    );
    assert_eq!(controls.mode_lid_button.title().to_string(), TITLE_MODE_LID);
    assert_eq!(controls.mode_system_button.state(), NSControlStateValueOn);
    assert_eq!(controls.mode_display_button.state(), NSControlStateValueOff);
    assert_eq!(controls.mode_lid_button.state(), NSControlStateValueOff);
    assert_eq!(controls.awake_mode(), AwakeMode::SystemOnly);

    // Test exclusive radio selection updates
    controls.set_awake_mode(AwakeMode::Display);
    assert_eq!(controls.mode_system_button.state(), NSControlStateValueOff);
    assert_eq!(controls.mode_display_button.state(), NSControlStateValueOn);
    assert_eq!(controls.mode_lid_button.state(), NSControlStateValueOff);
    assert_eq!(controls.awake_mode(), AwakeMode::Display);

    controls.set_awake_mode(AwakeMode::LidClosed);
    assert_eq!(controls.mode_system_button.state(), NSControlStateValueOff);
    assert_eq!(controls.mode_display_button.state(), NSControlStateValueOff);
    assert_eq!(controls.mode_lid_button.state(), NSControlStateValueOn);
    assert_eq!(controls.awake_mode(), AwakeMode::LidClosed);

    controls.set_awake_mode(AwakeMode::SystemOnly);
    assert_eq!(controls.mode_system_button.state(), NSControlStateValueOn);
    assert_eq!(controls.mode_display_button.state(), NSControlStateValueOff);
    assert_eq!(controls.mode_lid_button.state(), NSControlStateValueOff);
    assert_eq!(controls.awake_mode(), AwakeMode::SystemOnly);

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
    let no_time_limit_button = state.controls.no_time_limit_button.clone();
    let mode_system_button = state.controls.mode_system_button.clone();
    let mode_display_button = state.controls.mode_display_button.clone();
    let mode_lid_button = state.controls.mode_lid_button.clone();
    let unit_popup = state.controls.unit_popup.clone();
    let start_stop_button = state.controls.start_stop_button.clone();
    let status_item = state.status_item.clone();
    let time_label = state.controls.time_label.clone();
    let error_label = state.controls.error_label.clone();
    drop(state_opt);

    assert_eq!(mode_system_button.title().to_string(), TITLE_MODE_SYSTEM);
    assert_eq!(mode_system_button.state(), NSControlStateValueOn);
    assert!(mode_system_button.isEnabled());
    assert_eq!(mode_display_button.title().to_string(), TITLE_MODE_DISPLAY);
    assert_eq!(mode_display_button.state(), NSControlStateValueOff);
    assert!(mode_display_button.isEnabled());
    assert_eq!(mode_lid_button.title().to_string(), TITLE_MODE_LID);
    assert_eq!(mode_lid_button.state(), NSControlStateValueOff);
    assert!(mode_lid_button.isEnabled());
    // Ticking No time limit while LidClosed is selected switches to SystemOnly and disables lid radio
    delegate
        .ivars()
        .borrow()
        .as_ref()
        .unwrap()
        .controls
        .set_awake_mode(AwakeMode::LidClosed);
    assert_eq!(mode_lid_button.state(), NSControlStateValueOn);
    assert_eq!(
        delegate
            .ivars()
            .borrow()
            .as_ref()
            .unwrap()
            .controls
            .awake_mode(),
        AwakeMode::LidClosed
    );

    no_time_limit_button.setState(NSControlStateValueOn);
    unsafe {
        let _: () = msg_send![&*delegate, noTimeLimitChanged: &*no_time_limit_button];
    }
    assert_eq!(mode_system_button.state(), NSControlStateValueOn);
    assert_eq!(mode_lid_button.state(), NSControlStateValueOff);
    assert!(!mode_lid_button.isEnabled());
    assert_eq!(
        delegate
            .ivars()
            .borrow()
            .as_ref()
            .unwrap()
            .controls
            .awake_mode(),
        AwakeMode::SystemOnly
    );

    // Untick No time limit
    no_time_limit_button.setState(NSControlStateValueOff);
    unsafe {
        let _: () = msg_send![&*delegate, noTimeLimitChanged: &*no_time_limit_button];
    }
    assert!(mode_lid_button.isEnabled());

    // Non-interactive IPC Toggle with LidClosed selected must never show an admin authorization prompt.
    // If the machine already has passwordless sudo configured for pmset, LidSession::start()
    // will succeed without prompting. Otherwise, it fails and returns an Err containing "set up".
    // Either outcome is acceptable; neither prompts. We immediately stop the session if started.
    // NEVER drive the Start BUTTON with LidClosed selected (it could open the admin prompt).
    delegate
        .ivars()
        .borrow()
        .as_ref()
        .unwrap()
        .controls
        .set_awake_mode(AwakeMode::LidClosed);
    let toggle_lid_resp = delegate.execute_ipc_command(&IpcCommand::Toggle);
    assert!(
        !matches!(toggle_lid_resp, IpcResponse::Status(_)),
        "Unexpected status response to Toggle"
    );
    if let IpcResponse::Err(err) = &toggle_lid_resp {
        assert!(
            err.contains("set up"),
            "Expected error message instructing user to set up lid mode, got: {err}"
        );
    } else if matches!(toggle_lid_resp, IpcResponse::Ok(_)) {
        // Sudoers rule was already installed on this machine; ensure we clean up immediately.
        delegate.stop_session();
    }
    delegate
        .ivars()
        .borrow()
        .as_ref()
        .unwrap()
        .controls
        .set_awake_mode(AwakeMode::SystemOnly);
    delegate.update_ui();
    // Validation error display and persistence across update_ui
    let idle_height = delegate
        .ivars()
        .borrow()
        .as_ref()
        .unwrap()
        .controls
        .layout_footer();
    delegate.show_error("Test Error Message");
    assert_eq!(error_label.stringValue().to_string(), "Test Error Message");
    assert!(!error_label.isHidden());
    let error_height = delegate
        .ivars()
        .borrow()
        .as_ref()
        .unwrap()
        .controls
        .layout_footer();
    assert!(
        idle_height < error_height,
        "Expected idle content height ({idle_height}) to be less than height with error shown ({error_height})"
    );
    delegate.update_ui();
    assert_eq!(error_label.stringValue().to_string(), "Test Error Message");
    assert!(!error_label.isHidden());
    // Action handler: noTimeLimitChanged clears error
    unsafe {
        let _: () = msg_send![&*delegate, noTimeLimitChanged: &*no_time_limit_button];
    }
    assert_eq!(error_label.stringValue().to_string(), "");
    assert!(error_label.isHidden());

    // Finite session start / stop (minutes)
    duration_field.setStringValue(&NSString::from_str("15"));
    unit_popup.selectItemAtIndex(UNIT_MINUTES_INDEX);
    assert_eq!(delegate.handle_start_stop(true), Ok(true));
    assert_eq!(error_label.stringValue().to_string(), "");
    assert!(error_label.isHidden());
    assert!(!time_label.isHidden());
    assert_eq!(start_stop_button.title().to_string(), TITLE_STOP);
    assert!(!mode_lid_button.isEnabled());

    let future_end = SystemTime::now() + Duration::from_secs(900);
    let countdown_res = delegate.format_countdown(Some(future_end));
    assert!(countdown_res.is_some());
    let countdown_str = countdown_res.unwrap();
    assert!(countdown_str.starts_with("Ends at "));
    assert!(countdown_str.contains(" · "));
    assert!(countdown_str.ends_with(" left"));
    let past_end = SystemTime::now()
        .checked_sub(Duration::from_secs(10))
        .unwrap();
    assert!(delegate.format_countdown(Some(past_end)).is_none());
    assert!(delegate.format_countdown(None).is_none());

    // Stop session
    assert_eq!(delegate.handle_start_stop(true), Ok(false));
    assert_eq!(start_stop_button.title().to_string(), TITLE_START);
    assert!(mode_lid_button.isEnabled());

    // Indefinite session start / stop with display awake
    no_time_limit_button.setState(NSControlStateValueOn);
    delegate.update_ui();
    assert!(!mode_lid_button.isEnabled());
    delegate
        .ivars()
        .borrow()
        .as_ref()
        .unwrap()
        .controls
        .set_awake_mode(AwakeMode::Display);
    assert_eq!(delegate.handle_start_stop(true), Ok(true));
    assert_eq!(start_stop_button.title().to_string(), TITLE_STOP);
    assert!(!mode_lid_button.isEnabled());
    assert_eq!(delegate.handle_start_stop(true), Ok(false));
    assert_eq!(start_stop_button.title().to_string(), TITLE_START);

    // Invalid input error path
    no_time_limit_button.setState(NSControlStateValueOff);
    delegate.update_ui();
    assert!(mode_lid_button.isEnabled());
    duration_field.setStringValue(&NSString::from_str("invalid_number"));
    assert_eq!(
        delegate.handle_start_stop(true),
        Err(String::from(ERROR_DURATION_INVALID))
    );
    assert_eq!(
        error_label.stringValue().to_string(),
        ERROR_DURATION_INVALID
    );
    assert!(!error_label.isHidden());

    // Hours and Days units
    duration_field.setStringValue(&NSString::from_str("3"));
    unit_popup.selectItemAtIndex(UNIT_HOURS_INDEX);
    assert_eq!(delegate.handle_start_stop(true), Ok(true));
    assert_eq!(error_label.stringValue().to_string(), "");
    assert!(error_label.isHidden());
    delegate.stop_session();

    duration_field.setStringValue(&NSString::from_str("2"));
    unit_popup.selectItemAtIndex(UNIT_DAYS_INDEX);
    assert_eq!(delegate.handle_start_stop(true), Ok(true));
    delegate.stop_session();

    // Popover toggle
    if let Some(button) = status_item.button(mtm) {
        delegate.toggle_popover_relative_to(&button);

        duration_field.setStringValue(&NSString::from_str("10"));
        unit_popup.selectItemAtIndex(UNIT_MINUTES_INDEX);
        assert_eq!(delegate.handle_start_stop(true), Ok(true));
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
    no_time_limit_button.setState(NSControlStateValueOff);
    duration_field.setStringValue(&NSString::from_str("invalid_number"));
    let invalid_toggle = delegate.execute_ipc_command(&IpcCommand::Toggle);
    assert_eq!(
        invalid_toggle,
        IpcResponse::Err(String::from(ERROR_DURATION_INVALID))
    );
    let invalid_status = delegate.execute_ipc_command(&IpcCommand::Status);
    assert_eq!(invalid_status, IpcResponse::Status(None));

    // A finite IPC start remains finite even when the editable checkbox is checked.
    no_time_limit_button.setState(NSControlStateValueOn);
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

    if let Ok(render_prefix) = std::env::var("MELAFFEINE_RENDER_POPOVER") {
        use objc2_app_kit::{
            NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSBackingStoreType,
            NSWindow, NSWindowStyleMask,
        };
        use objc2_core_foundation::{CGPoint, CGRect, CGSize};

        let initial_h = delegate
            .ivars()
            .borrow()
            .as_ref()
            .unwrap()
            .controls
            .layout_footer();
        let win_frame = CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(app::ui::MENU_WIDTH, initial_h),
        );
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                win_frame,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        if let Some(aqua) = unsafe { NSAppearance::appearanceNamed(NSAppearanceNameAqua) } {
            window.setAppearance(Some(&aqua));
        }

        let render_view = |path: &str| {
            let state = delegate.ivars().borrow();
            let controls = &state.as_ref().unwrap().controls;
            let total_h = controls.layout_footer();
            let view = &controls.view;
            window.setContentSize(CGSize::new(app::ui::MENU_WIDTH, total_h));
            window.setContentView(Some(view));
            view.layoutSubtreeIfNeeded();
            view.displayIfNeeded();

            let bounds = view.bounds();
            let rep = view
                .bitmapImageRepForCachingDisplayInRect(bounds)
                .expect("bitmap rep");
            view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
            let dict = objc2_foundation::NSDictionary::new();
            let data = unsafe {
                rep.representationUsingType_properties(
                    objc2_app_kit::NSBitmapImageFileType::PNG,
                    &dict,
                )
            }
            .expect("png data");
            let bytes = unsafe { data.as_bytes_unchecked() };
            std::fs::write(path, bytes).expect("write png");
            println!("Rendered popover view to {path} (height={total_h})");
        };
        // Render idle state
        delegate.stop_session();
        let state = delegate.ivars().borrow();
        let controls = &state.as_ref().unwrap().controls;
        controls
            .no_time_limit_button
            .setState(NSControlStateValueOff);
        controls.set_awake_mode(AwakeMode::SystemOnly);
        drop(state);
        delegate.update_ui();
        render_view(&format!("{render_prefix}-idle.png"));

        // Render active state with countdown visible (started via IPC Start with 2h)
        let start_resp = delegate.execute_ipc_command(&IpcCommand::Start {
            duration: Some(Duration::from_secs(7200)),
            keep_display_awake: false,
        });
        assert!(matches!(start_resp, IpcResponse::Ok(_)));
        delegate.update_ui();
        render_view(&format!("{render_prefix}-active.png"));
        delegate.stop_session();

        // Render error state while idle
        delegate.show_error("Open Melaffeine and click Start once to set up lid-closed mode.");
        delegate.update_ui();
        render_view(&format!("{render_prefix}-error.png"));
        delegate.clear_error();
    }

    // Quit IPC command is tested last so the 0.05s termination timer does not
    // fire during NSRunLoop processing in earlier tests.
    let quit_resp = delegate.execute_ipc_command(&IpcCommand::Quit);
    assert_eq!(quit_resp, IpcResponse::Ok(String::from("Terminating")));
}
