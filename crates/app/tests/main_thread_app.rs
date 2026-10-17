//! Custom main-thread `AppKit` lifecycle and rendering integration harness.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant, SystemTime};

use app::app_delegate::AppDelegate;
use app::environment::{AppEnvironment, ThermalSource};
use app::ui::{
    AwakeMode, DEFAULT_DURATION_TEXT, ERROR_DURATION_INVALID, TITLE_MODE_DISPLAY, TITLE_MODE_LID,
    TITLE_MODE_SYSTEM, TITLE_NO_TIME_LIMIT, TITLE_START, TITLE_STOP, UNIT_DAYS_INDEX,
    UNIT_HOURS_INDEX, UNIT_MINUTES_INDEX, build_content_view,
};
use app_core::ipc::{IpcCommand, IpcResponse, SessionStatus};
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSControlStateValueOff, NSControlStateValueOn,
    NSEventMask, NSEventTrackingRunLoopMode,
};
use objc2_foundation::{NSDate, NSNotification, NSRunLoop, NSString};
// The custom harness exit status is its single test result.
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ignored = args.iter().any(|arg| arg == "--ignored");
    let exact = args.iter().any(|arg| arg == "--exact");
    let mut args_iter = args.iter();
    let mut selected = true;
    while let Some(arg) = args_iter.next() {
        match arg.as_str() {
            "--format" => {
                args_iter.next();
            }
            flag if flag.starts_with('-') => {}
            filter => {
                selected &= if exact {
                    filter == "main_thread_app"
                } else {
                    "main_thread_app".contains(filter)
                };
            }
        }
    }
    if ignored || !selected {
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|arg| arg == "--list") {
        harness_output("main_thread_app: test");
        return ExitCode::SUCCESS;
    }

    if std::env::var("MELAFFEINE_HARNESS_SANDBOXED").as_deref() != Ok("1") {
        return sandbox_reexec();
    }
    if sandbox_canary().is_none() {
        harness_error("Sandbox canary failed; refusing to initialize AppKit.");
        return ExitCode::FAILURE;
    }

    harness_output("Running comprehensive main thread app integration tests...");
    if run_harness().is_none() {
        harness_error("Main-thread integration harness failed to obtain a required value.");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn sbpl_escape(path: &str) -> String {
    path.replace('\\', "\\\\").replace('"', "\\\"")
}

fn runtime_dir() -> Option<PathBuf> {
    if std::env::var_os("HOME").is_none() {
        harness_error("HOME is required for the sandbox boundary.");
        return None;
    }
    let Some(home) = std::env::home_dir() else {
        harness_error("Cannot locate home directory for the sandbox boundary.");
        return None;
    };
    Some(home.join("Library/Application Support/Melaffeine"))
}

fn sandbox_reexec() -> ExitCode {
    let Some(runtime_dir) = runtime_dir() else {
        return ExitCode::FAILURE;
    };
    let Some(path) = runtime_dir.to_str() else {
        harness_error("Sandbox path must be valid UTF-8.");
        return ExitCode::FAILURE;
    };
    let profile = format!(
        "(version 1)\n(allow default)\n\
         (deny process-exec (literal \"/usr/bin/sudo\") (literal \"/usr/bin/osascript\") (literal \"/usr/bin/pmset\"))\n\
         (deny file-read* file-write* (subpath \"{}\"))",
        sbpl_escape(path)
    );
    let Some(exe) = harness_value(std::env::current_exe()) else {
        return ExitCode::FAILURE;
    };
    let Some(status) = harness_value(
        Command::new("/usr/bin/sandbox-exec")
            .args(["-p", &profile])
            .arg(exe)
            .args(std::env::args_os().skip(1))
            .env("MELAFFEINE_HARNESS_SANDBOXED", "1")
            .status(),
    ) else {
        return ExitCode::FAILURE;
    };
    match status.code().map(u8::try_from) {
        Some(Ok(code)) => ExitCode::from(code),
        _ => ExitCode::FAILURE,
    }
}

fn sandbox_canary() -> Option<()> {
    assert_eq!(
        sbpl_escape("a\\b\"c"),
        "a\\\\b\\\"c",
        "SBPL paths must escape backslashes and quotes"
    );
    match Command::new("/usr/bin/pmset").arg("-g").output() {
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            harness_output(&format!("Sandbox canary: pmset spawn denied: {error}"));
        }
        Ok(output) if !output.status.success() => {
            harness_output(&format!(
                "Sandbox canary: pmset rejected: {}",
                output.status
            ));
        }
        other => {
            harness_error(&format!("Sandbox failed to deny pmset: {other:?}"));
            return None;
        }
    }
    match std::fs::metadata(runtime_dir()?) {
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::NotFound
            ) =>
        {
            harness_output(&format!(
                "Sandbox canary: runtime directory inaccessible: {error}"
            ));
            Some(())
        }
        other => {
            harness_error(&format!(
                "Sandbox failed to protect runtime directory: {other:?}"
            ));
            None
        }
    }
}

/// Owns every filesystem effect and outlives the delegate's guardian and IPC worker.
struct IsolatedFiles {
    dir: PathBuf,
    state: PathBuf,
}

impl IsolatedFiles {
    fn new() -> Option<Self> {
        let nanos =
            harness_value(SystemTime::now().duration_since(SystemTime::UNIX_EPOCH))?.as_nanos();
        let dir = std::env::temp_dir().join(format!("mlf-app-{}-{nanos}", std::process::id()));
        harness_value(std::fs::create_dir(&dir))?;
        let state = dir.join("state");
        let files = Self { dir, state };
        harness_value(std::fs::set_permissions(
            &files.dir,
            std::fs::Permissions::from_mode(0o700),
        ))?;
        harness_value(std::fs::write(&files.state, "1"))?;
        harness_value(std::fs::write(
            files.dir.join("power"),
            r#"#!/bin/sh
state="$1"; value="$2"
if [ -f "$state.fail-$value" ]; then
    echo "fake failure $value" >&2
    exit 1
fi
printf '%s' "$value" > "$state"
"#,
        ))?;
        harness_value(std::fs::set_permissions(
            files.dir.join("power"),
            std::fs::Permissions::from_mode(0o700),
        ))?;
        Some(files)
    }

    fn environment(&self) -> AppEnvironment {
        AppEnvironment::isolated(
            &self.dir,
            self.dir.join("power"),
            vec![self.state.clone().into_os_string()],
        )
    }
}

impl Drop for IsolatedFiles {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.dir) {
            harness_error(&format!("Could not remove {}: {error}", self.dir.display()));
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        // Also runs during assertion unwinding, before the fake program is removed.
        self.delegate.teardown();
    }
}

fn run_harness() -> Option<()> {
    let Some(mtm) = MainThreadMarker::new() else {
        harness_error("Test runner must execute on the main thread!");
        return None;
    };

    let app = NSApplication::sharedApplication(mtm);
    assert!(
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory),
        "test application must use accessory activation policy"
    );

    let files = IsolatedFiles::new()?;
    let unrelated = files.dir.join("unrelated.sock");
    let _unrelated_listener = harness_value(std::os::unix::net::UnixListener::bind(&unrelated))?;
    check_initial_controls(mtm);
    check_stale_recovery(mtm, &app)?;
    check_teardown(mtm, &app)?;
    let harness = initialize_harness(mtm, &app, files.environment())?;
    assert!(
        unrelated.exists(),
        "initialization must preserve unrelated endpoint"
    );
    assert_eq!(
        harness_value(std::fs::read_to_string(&files.state))?,
        "1",
        "all initialization paths must leave preexisting fake sleep suppression untouched"
    );
    harness_value(std::fs::write(&files.state, "0"))?;
    check_delegate_controls(&harness);
    check_control_wiring(&harness)?;
    check_indefinite_lid_selection(&harness)?;
    check_noninteractive_lid(&harness, &files)?;
    check_interactive_authorization(&harness, &files)?;
    check_fake_lid_session(&harness, &files)?;
    check_inline_errors(&harness)?;
    check_finite_session(&harness);
    check_countdown(&harness)?;
    check_indefinite_display(&harness)?;
    check_invalid_duration(&harness);
    check_unit_selection(&harness);
    check_failed_quit(&harness, &files)?;
    check_busy_state(&harness)?;
    check_popover_session(&harness)?;
    check_ipc_invalid_toggle(&harness);
    check_ipc_finite_start(&harness);
    check_ipc_stop_toggle(&harness);
    check_ipc_indefinite_start(&harness);
    check_expiry(&harness)?;
    check_tracking_expiry(&harness)?;
    check_native_popover_close(&harness)?;
    check_popover_focus_log(&harness, &files)?;
    check_thermal_cutoff(&harness, &files)?;
    check_termination(&harness);
    render_if_requested(&harness)?;
    check_quit(&harness);
    Some(())
}

fn check_stale_recovery(mtm: MainThreadMarker, app: &NSApplication) -> Option<()> {
    let files = IsolatedFiles::new()?;
    let harness = initialize_harness(mtm, app, files.environment().with_failed_recovery())?;
    assert_eq!(
        harness.error_label.stringValue().to_string(),
        "Couldn't restore normal sleep: couldn't reset sleep suppression: isolated recovery failure. Run `sudo pmset -a disablesleep 0` in Terminal.",
        "startup recovery failure must show complete actionable restoration message"
    );
    assert!(
        !harness.error_label.isHidden(),
        "startup recovery error must be visible"
    );
    Some(())
}

fn check_control_wiring(harness: &Harness) -> Option<()> {
    let button = harness
        .status_item
        .button(MainThreadMarker::from(&*harness.delegate))?;
    let expected_mask = NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp;
    let previous = button.sendActionOn(expected_mask);
    button.sendActionOn(NSEventMask::from_bits_retain(u64::try_from(previous).ok()?));
    assert_eq!(
        u64::try_from(previous).ok(),
        Some(expected_mask.bits()),
        "status button must dispatch both mouse buttons"
    );
    for (control, action) in [
        (&**button, sel!(statusItemClicked:)),
        (&*harness.no_time_limit_button, sel!(noTimeLimitChanged:)),
        (&*harness.mode_system_button, sel!(awakeModeChanged:)),
        (&*harness.mode_display_button, sel!(awakeModeChanged:)),
        (&*harness.mode_lid_button, sel!(awakeModeChanged:)),
        (&*harness.start_stop_button, sel!(startStopClicked:)),
    ] {
        assert_eq!(
            control.target().as_deref().map(std::ptr::from_ref),
            Some(std::ptr::from_ref(&*harness.delegate).cast()),
            "control must dispatch to owning delegate"
        );
        assert_eq!(
            control.action(),
            Some(action),
            "control must dispatch its expected action"
        );
    }
    Some(())
}

fn check_interactive_authorization(harness: &Harness, files: &IsolatedFiles) -> Option<()> {
    harness
        .no_time_limit_button
        .setState(NSControlStateValueOff);
    harness
        .duration_field
        .setStringValue(&NSString::from_str("1"));
    harness_value(harness.delegate.ivars().state.try_borrow())?
        .as_ref()?
        .controls
        .set_awake_mode(AwakeMode::LidClosed);
    let marker = files.dir.join("state.fail-1");
    harness_value(std::fs::write(&marker, ""))?;
    let result = harness.delegate.handle_start_stop(true);
    harness_value(std::fs::remove_file(marker))?;
    assert_eq!(
        result,
        Err(String::from(
            "Lid-closed authorization is unavailable in this environment."
        )),
        "interactive fake failure must refuse authorization without launching osascript"
    );
    assert_eq!(
        harness.error_label.stringValue().to_string(),
        "Lid-closed authorization is unavailable in this environment.",
        "authorization refusal must be visible"
    );
    Some(())
}

fn check_cool_thermal_state(harness: &Harness, files: &IsolatedFiles) -> Option<()> {
    let delegate = &*harness.delegate;
    harness
        .no_time_limit_button
        .setState(NSControlStateValueOff);
    harness
        .duration_field
        .setStringValue(&NSString::from_str("1"));
    harness_value(delegate.ivars().state.try_borrow())?
        .as_ref()?
        .controls
        .set_awake_mode(AwakeMode::LidClosed);
    assert_eq!(
        delegate.handle_start_stop(true),
        Ok(true),
        "start fake finite lid session"
    );
    delegate
        .ivars()
        .environment
        .set_thermal(ThermalSource::Fixed(false));
    delegate.check_thermal_state_for_test();
    {
        let state = harness_value(delegate.ivars().state.try_borrow())?;
        let state = state.as_ref()?;
        assert!(
            state.lid.is_some(),
            "cool thermal check must preserve lid session"
        );
        assert!(
            state.thermal_timer.is_some(),
            "cool thermal check must preserve guard timer"
        );
        assert_eq!(
            state.controls.awake_mode(),
            AwakeMode::LidClosed,
            "cool check must preserve mode"
        );
        assert!(
            state.power.is_active(),
            "cool check must preserve power assertion"
        );
    }
    assert_eq!(
        harness_value(std::fs::read_to_string(&files.state))?,
        "1",
        "cool check must preserve fake sleep suppression"
    );
    Some(())
}

fn check_thermal_cutoff(harness: &Harness, files: &IsolatedFiles) -> Option<()> {
    check_cool_thermal_state(harness, files)?;
    let delegate = &*harness.delegate;
    delegate
        .ivars()
        .environment
        .set_thermal(ThermalSource::Fixed(true));
    delegate.check_thermal_state_for_test();
    {
        let state = harness_value(delegate.ivars().state.try_borrow())?;
        let state = state.as_ref()?;
        assert!(state.lid.is_none(), "hot check must release lid session");
        assert!(
            state.thermal_timer.is_none(),
            "hot check must remove thermal timer"
        );
        assert!(
            state.power.is_active(),
            "hot check must preserve ordinary power assertion"
        );
        assert_eq!(
            state.controls.awake_mode(),
            AwakeMode::SystemOnly,
            "hot check must reset awake mode"
        );
    }
    assert_eq!(
        harness_value(std::fs::read_to_string(&files.state))?,
        "0",
        "hot check must restore fake sleep setting"
    );
    assert_eq!(
        harness.error_label.stringValue().to_string(),
        "Mac is too hot — lid-closed mode turned off.",
        "thermal cutoff must explain mode change"
    );
    assert!(
        !harness.error_label.isHidden(),
        "thermal notice must be visible"
    );
    delegate
        .ivars()
        .environment
        .set_thermal(ThermalSource::Fixed(false));
    assert!(
        delegate.stop_session().is_ok(),
        "stop remaining ordinary session"
    );
    Some(())
}

fn check_teardown(mtm: MainThreadMarker, app: &NSApplication) -> Option<()> {
    let files = IsolatedFiles::new()?;
    let harness = initialize_harness(mtm, app, files.environment())?;
    let delegate = &*harness.delegate;
    assert!(
        delegate
            .start_session(Some(Duration::from_secs(60)), false)
            .is_ok(),
        "start finite teardown fixture"
    );
    {
        let state = harness_value(delegate.ivars().state.try_borrow())?;
        let state = state.as_ref()?;
        assert!(
            state.power.is_active(),
            "teardown fixture must have active power"
        );
        assert!(
            state.ipc_server.is_some(),
            "teardown fixture must have IPC server"
        );
        assert!(
            state.expiry_timer.is_some(),
            "finite session must have expiry timer"
        );
    }
    let socket = files.dir.join("ipc.sock");
    assert!(socket.exists(), "teardown fixture must publish socket");
    delegate.teardown();
    {
        let state = harness_value(delegate.ivars().state.try_borrow())?;
        let state = state.as_ref()?;
        assert!(
            !state.power.is_active(),
            "teardown must release power assertion"
        );
        assert!(
            state.ipc_server.is_none(),
            "teardown must remove IPC server"
        );
        assert!(
            state.expiry_timer.is_none()
                && state.countdown_timer.is_none()
                && state.thermal_timer.is_none(),
            "teardown must remove all timers"
        );
        assert!(!socket.exists(), "teardown must unlink IPC socket");
    }
    Some(())
}

struct Harness {
    delegate: objc2::rc::Retained<AppDelegate>,
    notif: objc2::rc::Retained<NSNotification>,
    duration_field: objc2::rc::Retained<objc2_app_kit::NSTextField>,
    no_time_limit_button: objc2::rc::Retained<objc2_app_kit::NSButton>,
    mode_system_button: objc2::rc::Retained<objc2_app_kit::NSButton>,
    mode_display_button: objc2::rc::Retained<objc2_app_kit::NSButton>,
    mode_lid_button: objc2::rc::Retained<objc2_app_kit::NSButton>,
    unit_popup: objc2::rc::Retained<objc2_app_kit::NSPopUpButton>,
    start_stop_button: objc2::rc::Retained<objc2_app_kit::NSButton>,
    status_item: objc2::rc::Retained<objc2_app_kit::NSStatusItem>,
    time_label: objc2::rc::Retained<objc2_app_kit::NSTextField>,
    error_label: objc2::rc::Retained<objc2_app_kit::NSTextField>,
}
fn initialize_harness(
    mtm: MainThreadMarker,
    app: &NSApplication,
    environment: AppEnvironment,
) -> Option<Harness> {
    // 2. Test AppDelegate allocation and full lifecycle
    let delegate = AppDelegate::new_with_environment(mtm, environment);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    // The constructor initializes state; repeated initialization must retain isolation.
    delegate.init_app_state(mtm);

    // Test delegate notifications
    #[expect(
        unsafe_code,
        reason = "NSNotification construction accepts an optional foreign object pointer"
    )]
    // SAFETY: Notification name is retained and the optional object is None.
    let notif = unsafe {
        NSNotification::notificationWithName_object(&NSString::from_str("TestNotification"), None)
    };
    #[expect(
        unsafe_code,
        reason = "Exercise the registered applicationDidFinishLaunching: Objective-C callback through its selector ABI"
    )]
    // SAFETY: The retained delegate implements the registered selector with a single object sender argument; receiver and sender remain live on the main thread.
    unsafe {
        let _: () = msg_send![&*delegate, applicationDidFinishLaunching: &*notif];
    }
    #[expect(
        unsafe_code,
        reason = "Exercise the registered applicationDidResignActive: Objective-C callback through its selector ABI"
    )]
    // SAFETY: The retained delegate implements the registered selector with a single object sender argument; receiver and sender remain live on the main thread.
    unsafe {
        let _: () = msg_send![&*delegate, applicationDidResignActive: &*notif];
    }

    // Inspect ivars
    let state_opt = delegate.ivars().state.borrow();
    let state = state_opt.as_ref()?;
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
    Some(Harness {
        delegate,
        notif,
        duration_field,
        no_time_limit_button,
        mode_system_button,
        mode_display_button,
        mode_lid_button,
        unit_popup,
        start_stop_button,
        status_item,
        time_label,
        error_label,
    })
}
fn check_initial_controls(mtm: MainThreadMarker) {
    // 1. Test build_content_view
    let controls = build_content_view(mtm);
    assert_eq!(
        controls.no_time_limit_button.title().to_string(),
        TITLE_NO_TIME_LIMIT,
        "Unexpected value for controls.no_time_limit_button.title().to_string()"
    );
    assert_eq!(
        controls.start_stop_button.title().to_string(),
        TITLE_START,
        "Unexpected value for controls.start_stop_button.title().to_string()"
    );
    assert_eq!(
        controls.mode_system_button.title().to_string(),
        TITLE_MODE_SYSTEM,
        "Unexpected value for controls.mode_system_button.title().to_string()"
    );
    assert_eq!(
        controls.mode_display_button.title().to_string(),
        TITLE_MODE_DISPLAY,
        "Unexpected value for controls.mode_display_button.title().to_string()"
    );
    assert_eq!(
        controls.mode_lid_button.title().to_string(),
        TITLE_MODE_LID,
        "Unexpected value for controls.mode_lid_button.title().to_string()"
    );
    assert_eq!(
        controls.mode_system_button.state(),
        NSControlStateValueOn,
        "Unexpected value for controls.mode_system_button.state()"
    );
    assert_eq!(
        controls.mode_display_button.state(),
        NSControlStateValueOff,
        "Unexpected value for controls.mode_display_button.state()"
    );
    assert_eq!(
        controls.mode_lid_button.state(),
        NSControlStateValueOff,
        "Unexpected value for controls.mode_lid_button.state()"
    );
    assert_eq!(
        controls.awake_mode(),
        AwakeMode::SystemOnly,
        "Unexpected value for controls.awake_mode()"
    );

    check_initial_radio_selection(&controls);
}
fn check_initial_radio_selection(controls: &app::ui::PopoverControls) {
    // Test exclusive radio selection updates
    controls.set_awake_mode(AwakeMode::Display);
    assert_eq!(
        controls.mode_system_button.state(),
        NSControlStateValueOff,
        "Unexpected value for controls.mode_system_button.state()"
    );
    assert_eq!(
        controls.mode_display_button.state(),
        NSControlStateValueOn,
        "Unexpected value for controls.mode_display_button.state()"
    );
    assert_eq!(
        controls.mode_lid_button.state(),
        NSControlStateValueOff,
        "Unexpected value for controls.mode_lid_button.state()"
    );
    assert_eq!(
        controls.awake_mode(),
        AwakeMode::Display,
        "Unexpected value for controls.awake_mode()"
    );

    controls.set_awake_mode(AwakeMode::LidClosed);
    assert_eq!(
        controls.mode_system_button.state(),
        NSControlStateValueOff,
        "Unexpected value for controls.mode_system_button.state()"
    );
    assert_eq!(
        controls.mode_display_button.state(),
        NSControlStateValueOff,
        "Unexpected value for controls.mode_display_button.state()"
    );
    assert_eq!(
        controls.mode_lid_button.state(),
        NSControlStateValueOn,
        "Unexpected value for controls.mode_lid_button.state()"
    );
    assert_eq!(
        controls.awake_mode(),
        AwakeMode::LidClosed,
        "Unexpected value for controls.awake_mode()"
    );

    controls.set_awake_mode(AwakeMode::SystemOnly);
    assert_eq!(
        controls.mode_system_button.state(),
        NSControlStateValueOn,
        "Unexpected value for controls.mode_system_button.state()"
    );
    assert_eq!(
        controls.mode_display_button.state(),
        NSControlStateValueOff,
        "Unexpected value for controls.mode_display_button.state()"
    );
    assert_eq!(
        controls.mode_lid_button.state(),
        NSControlStateValueOff,
        "Unexpected value for controls.mode_lid_button.state()"
    );
    assert_eq!(
        controls.awake_mode(),
        AwakeMode::SystemOnly,
        "Unexpected value for controls.awake_mode()"
    );

    assert_eq!(
        controls.duration_field.stringValue().to_string(),
        DEFAULT_DURATION_TEXT,
        "Unexpected value for controls.duration_field.stringValue().to_string()"
    );
}
fn check_delegate_controls(harness: &Harness) {
    let mode_system_button = &*harness.mode_system_button;
    let mode_display_button = &*harness.mode_display_button;
    let mode_lid_button = &*harness.mode_lid_button;

    assert_eq!(
        mode_system_button.title().to_string(),
        TITLE_MODE_SYSTEM,
        "Unexpected value for mode_system_button.title().to_string()"
    );
    assert_eq!(
        mode_system_button.state(),
        NSControlStateValueOn,
        "Unexpected value for mode_system_button.state()"
    );
    assert!(
        mode_system_button.isEnabled(),
        "Expected condition: mode_system_button.isEnabled()"
    );
    assert_eq!(
        mode_display_button.title().to_string(),
        TITLE_MODE_DISPLAY,
        "Unexpected value for mode_display_button.title().to_string()"
    );
    assert_eq!(
        mode_display_button.state(),
        NSControlStateValueOff,
        "Unexpected value for mode_display_button.state()"
    );
    assert!(
        mode_display_button.isEnabled(),
        "Expected condition: mode_display_button.isEnabled()"
    );
    assert_eq!(
        mode_lid_button.title().to_string(),
        TITLE_MODE_LID,
        "Unexpected value for mode_lid_button.title().to_string()"
    );
    assert_eq!(
        mode_lid_button.state(),
        NSControlStateValueOff,
        "Unexpected value for mode_lid_button.state()"
    );
    assert!(
        mode_lid_button.isEnabled(),
        "Expected condition: mode_lid_button.isEnabled()"
    );
}

fn check_indefinite_lid_selection(harness: &Harness) -> Option<()> {
    let delegate = &*harness.delegate;
    let no_time_limit_button = &*harness.no_time_limit_button;
    let mode_system_button = &*harness.mode_system_button;
    let mode_lid_button = &*harness.mode_lid_button;
    // Ticking No time limit while LidClosed is selected switches to SystemOnly and disables lid radio
    delegate
        .ivars()
        .state
        .borrow()
        .as_ref()?
        .controls
        .set_awake_mode(AwakeMode::LidClosed);
    assert_eq!(
        mode_lid_button.state(),
        NSControlStateValueOn,
        "Unexpected value for mode_lid_button.state()"
    );
    assert_eq!(
        delegate
            .ivars()
            .state
            .borrow()
            .as_ref()?
            .controls
            .awake_mode(),
        AwakeMode::LidClosed,
        "Unexpected value for delegate .ivars().state .borrow() .as_ref() ? .controls .awake_mode()"
    );

    no_time_limit_button.setState(NSControlStateValueOn);
    #[expect(
        unsafe_code,
        reason = "Exercise the registered noTimeLimitChanged: Objective-C callback through its selector ABI"
    )]
    // SAFETY: The retained delegate implements the registered selector with a single object sender argument; receiver and sender remain live on the main thread.
    unsafe {
        let _: () = msg_send![delegate, noTimeLimitChanged: no_time_limit_button];
    }
    assert_eq!(
        mode_system_button.state(),
        NSControlStateValueOn,
        "Unexpected value for mode_system_button.state()"
    );
    assert_eq!(
        mode_lid_button.state(),
        NSControlStateValueOff,
        "Unexpected value for mode_lid_button.state()"
    );
    assert!(
        !mode_lid_button.isEnabled(),
        "Expected condition: !mode_lid_button.isEnabled()"
    );
    assert_eq!(
        delegate
            .ivars()
            .state
            .borrow()
            .as_ref()?
            .controls
            .awake_mode(),
        AwakeMode::SystemOnly,
        "Unexpected value for delegate .ivars().state .borrow() .as_ref() ? .controls .awake_mode()"
    );

    // Untick No time limit
    no_time_limit_button.setState(NSControlStateValueOff);
    #[expect(
        unsafe_code,
        reason = "Exercise the registered noTimeLimitChanged: Objective-C callback through its selector ABI"
    )]
    // SAFETY: The retained delegate implements the registered selector with a single object sender argument; receiver and sender remain live on the main thread.
    unsafe {
        let _: () = msg_send![delegate, noTimeLimitChanged: no_time_limit_button];
    }
    assert!(
        mode_lid_button.isEnabled(),
        "Expected condition: mode_lid_button.isEnabled()"
    );

    Some(())
}

fn check_noninteractive_lid(harness: &Harness, files: &IsolatedFiles) -> Option<()> {
    let delegate = &*harness.delegate;
    // The fake enable failure deterministically exercises the non-interactive setup error.
    // Interactive actions are safe only because isolated authorization is Unavailable.
    let marker = files.dir.join("state.fail-1");
    harness_value(std::fs::write(&marker, ""))?;
    delegate
        .ivars()
        .state
        .borrow()
        .as_ref()?
        .controls
        .set_awake_mode(AwakeMode::LidClosed);
    let toggle_lid_resp = delegate.execute_ipc_command(&IpcCommand::Toggle);
    assert_eq!(
        toggle_lid_resp,
        IpcResponse::Err(String::from(
            "Open Melaffeine and click Start once to set up lid-closed mode."
        )),
        "non-interactive failure must instruct setup without prompting"
    );
    assert_eq!(
        harness_value(std::fs::read_to_string(&files.state))?,
        "0",
        "failed enable must never activate fake power"
    );
    harness_value(std::fs::remove_file(marker))?;
    delegate
        .ivars()
        .state
        .borrow()
        .as_ref()?
        .controls
        .set_awake_mode(AwakeMode::SystemOnly);
    delegate.update_ui();
    Some(())
}

fn check_fake_lid_session(harness: &Harness, files: &IsolatedFiles) -> Option<()> {
    let delegate = &*harness.delegate;
    delegate
        .ivars()
        .state
        .borrow()
        .as_ref()?
        .controls
        .set_awake_mode(AwakeMode::LidClosed);
    for fail_reset in [false, true] {
        assert_eq!(
            delegate.execute_ipc_command(&IpcCommand::Toggle),
            IpcResponse::Ok(String::from("Started sleep prevention")),
            "fake finite lid session must start through non-interactive IPC"
        );
        assert_eq!(
            harness_value(std::fs::read_to_string(&files.state))?,
            "1",
            "guardian must acknowledge enable before returning"
        );
        let marker = files.dir.join("state.fail-0");
        if fail_reset {
            harness_value(std::fs::write(&marker, ""))?;
        }
        let response = delegate.execute_ipc_command(&IpcCommand::Stop);
        if fail_reset {
            assert!(
                matches!(response, IpcResponse::Err(_)),
                "reset failure must be an IPC error: {response:?}"
            );
            let IpcResponse::Err(error) = response else {
                return None;
            };
            assert!(
                error.starts_with("Couldn't restore normal sleep: "),
                "reset error must preserve restore context: {error}"
            );
            assert!(
                error.contains("fake failure 0"),
                "retain fake reset diagnostics: {error}"
            );
            assert_eq!(
                harness.error_label.stringValue().to_string(),
                error,
                "reset error must be visible in the UI"
            );
            assert!(
                !harness.error_label.isHidden(),
                "restore error must not be hidden"
            );
            harness_value(std::fs::remove_file(marker))?;
        } else {
            assert_eq!(
                response,
                IpcResponse::Ok(String::from("Stopped sleep prevention")),
                "successful stop response"
            );
            assert_eq!(
                harness_value(std::fs::read_to_string(&files.state))?,
                "0",
                "stop must acknowledge restoration"
            );
        }
    }
    // Restore the fake through a new acknowledged session after the intentional failure.
    assert_eq!(
        delegate.execute_ipc_command(&IpcCommand::Toggle),
        IpcResponse::Ok(String::from("Started sleep prevention")),
        "restart after failed reset"
    );
    assert_eq!(
        delegate.execute_ipc_command(&IpcCommand::Stop),
        IpcResponse::Ok(String::from("Stopped sleep prevention")),
        "restore fake state"
    );
    check_hot_lid_refusal(harness, files)
}

fn check_hot_lid_refusal(harness: &Harness, files: &IsolatedFiles) -> Option<()> {
    let delegate = &*harness.delegate;
    delegate
        .ivars()
        .environment
        .set_thermal(ThermalSource::Fixed(true));
    assert_eq!(
        delegate.execute_ipc_command(&IpcCommand::Toggle),
        IpcResponse::Err(String::from(
            "Mac is too hot to keep running with the lid closed. Let it cool down and try again."
        )),
        "fixed hot thermal source must refuse lid start"
    );
    assert_eq!(
        harness_value(std::fs::read_to_string(&files.state))?,
        "0",
        "thermal refusal must not enable fake power"
    );
    delegate
        .ivars()
        .environment
        .set_thermal(ThermalSource::Fixed(false));
    delegate
        .ivars()
        .state
        .borrow()
        .as_ref()?
        .controls
        .set_awake_mode(AwakeMode::SystemOnly);
    delegate.clear_error();
    Some(())
}

fn check_inline_errors(harness: &Harness) -> Option<()> {
    let delegate = &*harness.delegate;
    let no_time_limit_button = &*harness.no_time_limit_button;
    let error_label = &*harness.error_label;
    // Validation error display and persistence across update_ui
    let idle_height = delegate
        .ivars()
        .state
        .borrow()
        .as_ref()?
        .controls
        .layout_footer();
    delegate.show_error("Test Error Message");
    assert_eq!(
        error_label.stringValue().to_string(),
        "Test Error Message",
        "Unexpected value for error_label.stringValue().to_string()"
    );
    assert!(
        !error_label.isHidden(),
        "Expected condition: !error_label.isHidden()"
    );
    let error_height = delegate
        .ivars()
        .state
        .borrow()
        .as_ref()?
        .controls
        .layout_footer();
    assert!(
        idle_height < error_height,
        "Expected idle content height ({idle_height}) to be less than height with error shown ({error_height})"
    );
    delegate.update_ui();
    assert_eq!(
        error_label.stringValue().to_string(),
        "Test Error Message",
        "Unexpected value for error_label.stringValue().to_string()"
    );
    assert!(
        !error_label.isHidden(),
        "Expected condition: !error_label.isHidden()"
    );
    // Action handler: noTimeLimitChanged clears error
    #[expect(
        unsafe_code,
        reason = "Exercise the registered noTimeLimitChanged: Objective-C callback through its selector ABI"
    )]
    // SAFETY: The retained delegate implements the registered selector with a single object sender argument; receiver and sender remain live on the main thread.
    unsafe {
        let _: () = msg_send![delegate, noTimeLimitChanged: no_time_limit_button];
    }
    assert_eq!(
        error_label.stringValue().to_string(),
        "",
        "Unexpected value for error_label.stringValue().to_string()"
    );
    assert!(
        error_label.isHidden(),
        "Expected condition: error_label.isHidden()"
    );

    Some(())
}

fn check_finite_session(harness: &Harness) {
    let delegate = &*harness.delegate;
    let duration_field = &*harness.duration_field;
    let mode_lid_button = &*harness.mode_lid_button;
    let unit_popup = &*harness.unit_popup;
    let start_stop_button = &*harness.start_stop_button;
    let time_label = &*harness.time_label;
    let error_label = &*harness.error_label;
    // Finite session start / stop (minutes)
    duration_field.setStringValue(&NSString::from_str("15"));
    unit_popup.selectItemAtIndex(UNIT_MINUTES_INDEX);
    assert_eq!(
        delegate.handle_start_stop(true),
        Ok(true),
        "Unexpected value for delegate.handle_start_stop(true)"
    );
    assert_eq!(
        error_label.stringValue().to_string(),
        "",
        "Unexpected value for error_label.stringValue().to_string()"
    );
    assert!(
        error_label.isHidden(),
        "Expected condition: error_label.isHidden()"
    );
    assert!(
        !time_label.isHidden(),
        "Expected condition: !time_label.isHidden()"
    );
    assert_eq!(
        start_stop_button.title().to_string(),
        TITLE_STOP,
        "Unexpected value for start_stop_button.title().to_string()"
    );
    assert!(
        !mode_lid_button.isEnabled(),
        "Expected condition: !mode_lid_button.isEnabled()"
    );
}

fn check_countdown(harness: &Harness) -> Option<()> {
    let delegate = &*harness.delegate;
    let mode_lid_button = &*harness.mode_lid_button;
    let start_stop_button = &*harness.start_stop_button;
    let future_end = SystemTime::now().checked_add(Duration::from_mins(15))?;
    let countdown_res = delegate.format_countdown(Some(future_end));
    assert!(
        countdown_res.is_some(),
        "Expected condition: countdown_res.is_some()"
    );
    let countdown_str = countdown_res?;
    assert!(
        countdown_str.starts_with("Ends at "),
        "Expected condition: countdown_str.starts_with(\"Ends at \")"
    );
    assert!(
        countdown_str.contains(" · "),
        "Expected condition: countdown_str.contains(\" · \")"
    );
    assert!(
        countdown_str.ends_with(" left"),
        "Expected condition: countdown_str.ends_with(\" left\")"
    );
    let past_end = SystemTime::now().checked_sub(Duration::from_secs(10))?;
    assert!(
        delegate.format_countdown(Some(past_end)).is_none(),
        "Expected condition: delegate.format_countdown(Some(past_end)).is_none()"
    );
    assert!(
        delegate.format_countdown(None).is_none(),
        "Expected condition: delegate.format_countdown(None).is_none()"
    );

    // Stop session
    assert_eq!(
        delegate.handle_start_stop(true),
        Ok(false),
        "Unexpected value for delegate.handle_start_stop(true)"
    );
    assert_eq!(
        start_stop_button.title().to_string(),
        TITLE_START,
        "Unexpected value for start_stop_button.title().to_string()"
    );
    assert!(
        mode_lid_button.isEnabled(),
        "Expected condition: mode_lid_button.isEnabled()"
    );

    Some(())
}

fn check_indefinite_display(harness: &Harness) -> Option<()> {
    let delegate = &*harness.delegate;
    let no_time_limit_button = &*harness.no_time_limit_button;
    let mode_lid_button = &*harness.mode_lid_button;
    let start_stop_button = &*harness.start_stop_button;
    // Indefinite session start / stop with display awake
    no_time_limit_button.setState(NSControlStateValueOn);
    delegate.update_ui();
    assert!(
        !mode_lid_button.isEnabled(),
        "Expected condition: !mode_lid_button.isEnabled()"
    );
    delegate
        .ivars()
        .state
        .borrow()
        .as_ref()?
        .controls
        .set_awake_mode(AwakeMode::Display);
    assert_eq!(
        delegate.handle_start_stop(true),
        Ok(true),
        "Unexpected value for delegate.handle_start_stop(true)"
    );
    assert_eq!(
        start_stop_button.title().to_string(),
        TITLE_STOP,
        "Unexpected value for start_stop_button.title().to_string()"
    );
    assert!(
        !mode_lid_button.isEnabled(),
        "Expected condition: !mode_lid_button.isEnabled()"
    );
    assert_eq!(
        delegate.handle_start_stop(true),
        Ok(false),
        "Unexpected value for delegate.handle_start_stop(true)"
    );
    assert_eq!(
        start_stop_button.title().to_string(),
        TITLE_START,
        "Unexpected value for start_stop_button.title().to_string()"
    );

    Some(())
}

fn check_invalid_duration(harness: &Harness) {
    let delegate = &*harness.delegate;
    let duration_field = &*harness.duration_field;
    let no_time_limit_button = &*harness.no_time_limit_button;
    let mode_lid_button = &*harness.mode_lid_button;
    let error_label = &*harness.error_label;
    // Invalid input error path
    no_time_limit_button.setState(NSControlStateValueOff);
    delegate.update_ui();
    assert!(
        mode_lid_button.isEnabled(),
        "Expected condition: mode_lid_button.isEnabled()"
    );
    duration_field.setStringValue(&NSString::from_str("invalid_number"));
    assert_eq!(
        delegate.handle_start_stop(true),
        Err(String::from(ERROR_DURATION_INVALID)),
        "Unexpected value for delegate.handle_start_stop(true)"
    );
    assert_eq!(
        error_label.stringValue().to_string(),
        ERROR_DURATION_INVALID,
        "Unexpected value for error_label.stringValue().to_string()"
    );
    assert!(
        !error_label.isHidden(),
        "Expected condition: !error_label.isHidden()"
    );
}

fn check_unit_selection(harness: &Harness) {
    let delegate = &*harness.delegate;
    let duration_field = &*harness.duration_field;
    let unit_popup = &*harness.unit_popup;
    let error_label = &*harness.error_label;
    // Hours and Days units
    duration_field.setStringValue(&NSString::from_str("3"));
    unit_popup.selectItemAtIndex(UNIT_HOURS_INDEX);
    assert_eq!(
        delegate.handle_start_stop(true),
        Ok(true),
        "Unexpected value for delegate.handle_start_stop(true)"
    );
    assert_eq!(
        error_label.stringValue().to_string(),
        "",
        "Unexpected value for error_label.stringValue().to_string()"
    );
    assert!(
        error_label.isHidden(),
        "Expected condition: error_label.isHidden()"
    );
    assert_eq!(
        delegate.stop_session().map_err(|error| error.to_string()),
        Ok(()),
        "restore normal sleep"
    );

    duration_field.setStringValue(&NSString::from_str("2"));
    unit_popup.selectItemAtIndex(UNIT_DAYS_INDEX);
    assert_eq!(
        delegate.handle_start_stop(true),
        Ok(true),
        "Unexpected value for delegate.handle_start_stop(true)"
    );
    assert_eq!(
        delegate.stop_session().map_err(|error| error.to_string()),
        Ok(()),
        "restore normal sleep"
    );
}

fn check_popover_session(harness: &Harness) -> Option<()> {
    let delegate = &*harness.delegate;
    let duration_field = &*harness.duration_field;
    let unit_popup = &*harness.unit_popup;
    let start_stop_button = &*harness.start_stop_button;
    let status_item = &*harness.status_item;
    let mtm = MainThreadMarker::from(&*harness.delegate);
    // Popover toggle
    if let Some(button) = status_item.button(mtm) {
        delegate.toggle_popover_relative_to(&button);
        let popover = harness_value(delegate.ivars().state.try_borrow())?
            .as_ref()?
            .popover
            .clone();
        let opening = Instant::now();
        while !popover.isShown() && opening.elapsed() < Duration::from_secs(3) {
            NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.05));
        }
        assert!(popover.isShown(), "first toggle must show popover");

        duration_field.setStringValue(&NSString::from_str("10"));
        unit_popup.selectItemAtIndex(UNIT_MINUTES_INDEX);
        assert_eq!(
            delegate.handle_start_stop(true),
            Ok(true),
            "Unexpected value for delegate.handle_start_stop(true)"
        );
        assert_eq!(
            start_stop_button.title().to_string(),
            TITLE_STOP,
            "Unexpected value for start_stop_button.title().to_string()"
        );

        delegate.toggle_popover_relative_to(&button);
        let closing = Instant::now();
        while popover.isShown() && closing.elapsed() < Duration::from_secs(3) {
            NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.05));
        }
        assert!(!popover.isShown(), "second toggle must hide popover");
        delegate.close_popover();
        assert_eq!(
            delegate.stop_session().map_err(|error| error.to_string()),
            Ok(()),
            "restore normal sleep"
        );
    }

    // Outside click monitors and teardown
    delegate.install_outside_click_monitor();
    delegate.remove_outside_click_monitor();
    delegate.teardown();
    let status_resp = delegate.execute_ipc_command(&IpcCommand::Status);
    assert_eq!(
        status_resp,
        IpcResponse::Status(None),
        "Unexpected value for status_resp"
    );
    Some(())
}

fn capture_logs<T>(path: &std::path::Path, action: impl FnOnce() -> T) -> Option<(T, String)> {
    let log_file = harness_value(std::fs::File::create(path))?;
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(std::sync::Mutex::new(log_file))
        .finish();
    let result = tracing::subscriber::with_default(subscriber, action);
    Some((result, harness_value(std::fs::read_to_string(path))?))
}

fn check_popover_focus_log(harness: &Harness, files: &IsolatedFiles) -> Option<()> {
    let delegate = &*harness.delegate;
    let button = harness
        .status_item
        .button(MainThreadMarker::from(delegate))?;
    let ((), log) = capture_logs(&files.dir.join("focus.log"), || {
        delegate.toggle_popover_relative_to(&button);
    })?;
    let popover = harness_value(delegate.ivars().state.try_borrow())?
        .as_ref()?
        .popover
        .clone();
    let opening = Instant::now();
    while !popover.isShown() && opening.elapsed() < Duration::from_secs(3) {
        NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.05));
    }
    assert!(popover.isShown(), "focus test must open popover");
    let window = popover.contentViewController()?.view().window()?;
    assert_eq!(
        window.firstResponder().as_deref().map(std::ptr::from_ref),
        Some(std::ptr::from_ref(&**window)),
        "opening popover must clear the field responder to the window"
    );
    assert!(
        !log.contains("popover window refused to clear its first responder"),
        "successful focus clearing must not log a refusal: {log}"
    );
    delegate.close_popover();
    let closing = Instant::now();
    while popover.isShown() && closing.elapsed() < Duration::from_secs(3) {
        NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.05));
    }
    assert!(!popover.isShown(), "focus test must close popover");
    Some(())
}

fn check_ipc_invalid_toggle(harness: &Harness) {
    let delegate = &*harness.delegate;
    let duration_field = &*harness.duration_field;
    let no_time_limit_button = &*harness.no_time_limit_button;
    // Toggle when inactive with invalid input must report the validation error.
    no_time_limit_button.setState(NSControlStateValueOff);
    duration_field.setStringValue(&NSString::from_str("invalid_number"));
    let invalid_toggle = delegate.execute_ipc_command(&IpcCommand::Toggle);
    assert_eq!(
        invalid_toggle,
        IpcResponse::Err(String::from(ERROR_DURATION_INVALID)),
        "Unexpected value for invalid_toggle"
    );
    let invalid_status = delegate.execute_ipc_command(&IpcCommand::Status);
    assert_eq!(
        invalid_status,
        IpcResponse::Status(None),
        "Unexpected value for invalid_status"
    );
}

fn check_ipc_finite_start(harness: &Harness) {
    let delegate = &*harness.delegate;
    let no_time_limit_button = &*harness.no_time_limit_button;
    let time_label = &*harness.time_label;
    let error_label = &*harness.error_label;
    // A finite IPC start remains finite even when the editable checkbox is checked.
    no_time_limit_button.setState(NSControlStateValueOn);
    delegate.show_error("stale successful start error");
    let start_resp = delegate.execute_ipc_command(&IpcCommand::Start {
        duration: Some(Duration::from_mins(2)),
        keep_display_awake: true,
    });
    assert!(
        matches!(start_resp, IpcResponse::Ok(_)),
        "Expected condition: matches!(start_resp, IpcResponse::Ok(_))"
    );
    delegate.update_ui();
    assert!(
        !time_label.isHidden(),
        "Expected condition: !time_label.isHidden()"
    );
    assert_ne!(
        time_label.stringValue().to_string(),
        "",
        "Unexpected value for time_label.stringValue().to_string()"
    );
    assert_eq!(
        error_label.stringValue().to_string(),
        "",
        "Unexpected value for error_label.stringValue().to_string()"
    );
    assert!(
        error_label.isHidden(),
        "Expected condition: error_label.isHidden()"
    );

    let status_resp2 = delegate.execute_ipc_command(&IpcCommand::Status);
    assert!(
        matches!(
            status_resp2,
            IpcResponse::Status(Some(SessionStatus {
                keep_display_awake: true,
                remaining_compact: Some(_),
            }))
        ),
        "Expected condition: matches!( status_resp2, IpcResponse::Status(Some(SessionStatus {{ keep_display_awake: true, remaining_compact: Some(_), }})) )"
    );
}

fn check_ipc_stop_toggle(harness: &Harness) {
    let delegate = &*harness.delegate;
    let duration_field = &*harness.duration_field;
    let unit_popup = &*harness.unit_popup;
    // Toggle while active stops sleep prevention.
    let toggle_resp = delegate.execute_ipc_command(&IpcCommand::Toggle);
    assert_eq!(
        toggle_resp,
        IpcResponse::Ok(String::from("Stopped sleep prevention")),
        "Unexpected value for toggle_resp"
    );
    assert_eq!(
        delegate.execute_ipc_command(&IpcCommand::Status),
        IpcResponse::Status(None),
        "Unexpected value for delegate.execute_ipc_command(&IpcCommand::Status)"
    );

    let stop_resp = delegate.execute_ipc_command(&IpcCommand::Stop);
    assert_eq!(
        stop_resp,
        IpcResponse::Ok(String::from("Stopped sleep prevention")),
        "Unexpected value for stop_resp"
    );

    // Toggle when inactive with valid input starts session
    duration_field.setStringValue(&NSString::from_str("2"));
    unit_popup.selectItemAtIndex(UNIT_HOURS_INDEX);
    let toggle_inactive = delegate.execute_ipc_command(&IpcCommand::Toggle);
    assert_eq!(
        toggle_inactive,
        IpcResponse::Ok(String::from("Started sleep prevention")),
        "Unexpected value for toggle_inactive"
    );
    assert_eq!(
        delegate.stop_session().map_err(|error| error.to_string()),
        Ok(()),
        "restore normal sleep"
    );
}

fn check_ipc_indefinite_start(harness: &Harness) {
    let delegate = &*harness.delegate;
    let status_item = &*harness.status_item;
    let mtm = MainThreadMarker::from(&*harness.delegate);
    // Start indefinite via IPC
    let start_indef_resp = delegate.execute_ipc_command(&IpcCommand::Start {
        duration: None,
        keep_display_awake: false,
    });
    assert!(
        matches!(start_indef_resp, IpcResponse::Ok(_)),
        "Expected condition: matches!(start_indef_resp, IpcResponse::Ok(_))"
    );
    assert_eq!(
        delegate.execute_ipc_command(&IpcCommand::Status),
        IpcResponse::Status(Some(SessionStatus {
            keep_display_awake: false,
            remaining_compact: None,
        })),
        "Unexpected value for delegate.execute_ipc_command(&IpcCommand::Status)"
    );
    assert_eq!(
        delegate.stop_session().map_err(|error| error.to_string()),
        Ok(()),
        "restore normal sleep"
    );
    // Popover toggle open and close
    if let Some(btn) = status_item.button(mtm) {
        delegate.toggle_popover_relative_to(&btn);
        delegate.toggle_popover_relative_to(&btn);
    }
    delegate.close_popover();
}

fn check_expiry(harness: &Harness) -> Option<()> {
    let delegate = &*harness.delegate;
    let start_stop_button = &*harness.start_stop_button;
    // Real timer expiry via NSRunLoop
    let start_finite = delegate.execute_ipc_command(&IpcCommand::Start {
        duration: Some(Duration::from_secs(1)),
        keep_display_awake: false,
    });
    assert!(
        matches!(start_finite, IpcResponse::Ok(_)),
        "Expected condition: matches!(start_finite, IpcResponse::Ok(_))"
    );
    assert!(
        delegate.ivars().state.borrow().as_ref()?.power.is_active(),
        "Expected condition: delegate .ivars().state .borrow() .as_ref() ? .power .is_active()"
    );

    let run_loop = NSRunLoop::currentRunLoop();
    let start_time = Instant::now();
    while delegate.ivars().state.borrow().as_ref()?.power.is_active()
        && start_time.elapsed() < Duration::from_secs(3)
    {
        let until = NSDate::dateWithTimeIntervalSinceNow(0.1);
        run_loop.runUntilDate(&until);
    }

    let is_active = delegate.ivars().state.borrow().as_ref()?.power.is_active();
    assert!(
        !is_active,
        "PowerController must be inactive after expiry timer fires"
    );
    assert_eq!(
        delegate.execute_ipc_command(&IpcCommand::Status),
        IpcResponse::Status(None),
        "Unexpected value for delegate.execute_ipc_command(&IpcCommand::Status)"
    );
    assert_eq!(
        start_stop_button.title().to_string(),
        TITLE_START,
        "Unexpected value for start_stop_button.title().to_string()"
    );
    Some(())
}

fn check_tracking_expiry(harness: &Harness) -> Option<()> {
    let delegate = &*harness.delegate;
    assert!(
        matches!(
            delegate.execute_ipc_command(&IpcCommand::Start {
                duration: Some(Duration::from_millis(100)),
                keep_display_awake: false,
            }),
            IpcResponse::Ok(_)
        ),
        "tracking regression must start a short finite session"
    );
    #[expect(
        unsafe_code,
        reason = "AppKit exports its event-tracking mode as a foreign static"
    )]
    // SAFETY: AppKit supplies this immutable, process-lifetime mode name.
    let tracking_mode = unsafe { NSEventTrackingRunLoopMode };
    let run_loop = NSRunLoop::currentRunLoop();
    let started = Instant::now();
    while delegate.ivars().state.borrow().as_ref()?.power.is_active()
        && started.elapsed() < Duration::from_secs(2)
    {
        run_loop.runMode_beforeDate(tracking_mode, &NSDate::dateWithTimeIntervalSinceNow(0.05));
    }
    assert!(
        !delegate.ivars().state.borrow().as_ref()?.power.is_active(),
        "finite session must expire while only event-tracking mode runs"
    );
    assert_eq!(
        harness.start_stop_button.title().to_string(),
        TITLE_START,
        "event-tracking expiry must restore Start in the UI"
    );
    harness_output("Regression: finite session expired during event tracking.");
    Some(())
}

fn check_native_popover_close(harness: &Harness) -> Option<()> {
    let delegate = &*harness.delegate;
    assert!(
        matches!(
            delegate.execute_ipc_command(&IpcCommand::Start {
                duration: Some(Duration::from_secs(60)),
                keep_display_awake: false,
            }),
            IpcResponse::Ok(_)
        ),
        "native close regression must start a finite session"
    );
    let button = harness
        .status_item
        .button(MainThreadMarker::from(delegate))?;
    delegate.toggle_popover_relative_to(&button);
    let popover = {
        let state = delegate.ivars().state.borrow();
        let state = state.as_ref()?;
        assert!(
            state.popover.isShown(),
            "normal show path must open popover"
        );
        assert!(
            state.countdown_timer.is_some(),
            "shown finite session needs countdown timer"
        );
        assert!(
            state.outside_click_monitor.is_some(),
            "normal show path needs outside-click monitor"
        );
        state.popover.clone()
    };
    popover.close();
    // Closing animates; wait for AppKit to finish instead of assuming a fixed duration.
    let run_loop = NSRunLoop::currentRunLoop();
    let started = Instant::now();
    while popover.isShown() && started.elapsed() < Duration::from_secs(3) {
        run_loop.runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.05));
    }
    {
        let state = delegate.ivars().state.borrow();
        let state = state.as_ref()?;
        assert!(!state.popover.isShown(), "native close must hide popover");
        assert!(
            state.countdown_timer.is_none(),
            "native close must remove countdown timer"
        );
        assert!(
            state.outside_click_monitor.is_none(),
            "native close must remove outside-click monitor"
        );
        assert!(
            state.power.is_active(),
            "native close must preserve active power session"
        );
    }
    delegate.close_popover();
    assert_eq!(
        delegate.stop_session().map_err(|error| error.to_string()),
        Ok(()),
        "stop finite session after native close"
    );
    harness_output("Regression: native popover close cleaned up without stopping power.");
    Some(())
}

fn check_busy_state(harness: &Harness) -> Option<()> {
    let delegate = &*harness.delegate;
    let state = harness_value(delegate.ivars().state.try_borrow_mut())?;
    for command in [
        IpcCommand::Status,
        IpcCommand::Stop,
        IpcCommand::Toggle,
        IpcCommand::Quit,
        IpcCommand::Start {
            duration: None,
            keep_display_awake: false,
        },
    ] {
        let response = delegate.execute_ipc_command(&command);
        assert!(
            matches!(response, IpcResponse::Err(ref message) if message.contains("App state is busy")),
            "busy state must return an internal error, never success: {response:?}"
        );
    }
    assert!(
        matches!(
            delegate.stop_session(),
            Err(app::app_delegate::SessionError::StateUnavailable)
        ),
        "typed stop must reject borrowed state"
    );
    drop(state);
    assert!(
        harness_value(delegate.ivars().state.try_borrow())?.is_some(),
        "state must remain available after reentry"
    );
    harness_output(
        "Regression: borrowed state returns errors without panicking or scheduling Quit.",
    );
    Some(())
}

fn check_failed_quit(harness: &Harness, files: &IsolatedFiles) -> Option<()> {
    let delegate = &*harness.delegate;
    harness
        .no_time_limit_button
        .setState(NSControlStateValueOff);
    harness
        .duration_field
        .setStringValue(&NSString::from_str("1"));
    harness.unit_popup.selectItemAtIndex(UNIT_HOURS_INDEX);
    harness.mode_lid_button.setState(NSControlStateValueOn);
    harness_value(delegate.ivars().state.try_borrow())?
        .as_ref()?
        .controls
        .set_awake_mode(AwakeMode::LidClosed);
    assert_eq!(
        delegate.execute_ipc_command(&IpcCommand::Toggle),
        IpcResponse::Ok(String::from("Started sleep prevention")),
        "start lid session before failed Quit"
    );
    let marker = files.dir.join("state.fail-0");
    harness_value(std::fs::write(&marker, ""))?;
    let (response, log) = capture_logs(&files.dir.join("restore.log"), || {
        delegate.execute_ipc_command(&IpcCommand::Quit)
    })?;
    assert!(
        log.contains("session operation failed") && log.contains("fake failure 0"),
        "failed restoration must be logged with its cause: {log}"
    );
    assert!(
        matches!(response, IpcResponse::Err(ref message) if message.starts_with("Couldn't restore normal sleep: ") && message.contains("fake failure 0")),
        "Quit must reject failed restore: {response:?}"
    );
    assert!(
        !harness.error_label.isHidden(),
        "failed Quit must show the restore error"
    );
    NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.1));
    let state = harness_value(delegate.ivars().state.try_borrow())?;
    assert!(
        !state.as_ref()?.power.is_active(),
        "failed Quit must release ordinary assertions"
    );
    assert!(
        state.as_ref()?.ipc_server.is_some(),
        "failed Quit must not tear down the app"
    );
    drop(state);
    harness_value(std::fs::remove_file(marker))?;
    // A subsequent acknowledged fake session restores the deliberately failed setting.
    assert_eq!(
        delegate.execute_ipc_command(&IpcCommand::Toggle),
        IpcResponse::Ok(String::from("Started sleep prevention")),
        "restart after failed Quit"
    );
    assert_eq!(
        delegate.execute_ipc_command(&IpcCommand::Stop),
        IpcResponse::Ok(String::from("Stopped sleep prevention")),
        "restore fake state after failed Quit"
    );
    harness_output("Regression: failed-reset Quit returned ERR and left the app alive.");
    Some(())
}

fn check_termination(harness: &Harness) {
    let delegate = &*harness.delegate;
    let notif = &*harness.notif;
    // Termination lifecycle
    #[expect(
        unsafe_code,
        reason = "Exercise the registered applicationWillTerminate: Objective-C callback through its selector ABI"
    )]
    // SAFETY: The retained delegate implements the registered selector with a single object sender argument; receiver and sender remain live on the main thread.
    unsafe {
        let _: () = msg_send![delegate, applicationWillTerminate: notif];
    }
    harness_output("All main thread integration tests PASSED!");
}

fn check_quit(harness: &Harness) {
    let delegate = &*harness.delegate;
    // Quit IPC command is tested last so the 0.05s termination timer does not
    // fire during NSRunLoop processing in earlier tests.
    let quit_resp = delegate.execute_ipc_command(&IpcCommand::Quit);
    assert_eq!(
        quit_resp,
        IpcResponse::Ok(String::from("Terminating")),
        "Unexpected value for quit_resp"
    );
}

fn render_view(harness: &Harness, window: &objc2_app_kit::NSWindow, path: &str) -> Option<()> {
    let delegate = &*harness.delegate;
    let render_state = delegate.ivars().state.borrow();
    let render_controls = &render_state.as_ref()?.controls;
    let total_h = render_controls.layout_footer();
    let view = &render_controls.view;
    window.setContentSize(objc2_core_foundation::CGSize::new(
        app::ui::MENU_WIDTH,
        total_h,
    ));
    window.setContentView(Some(view));
    view.layoutSubtreeIfNeeded();
    view.displayIfNeeded();

    let bounds = view.bounds();
    let rep = view.bitmapImageRepForCachingDisplayInRect(bounds)?;
    view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
    let dict = objc2_foundation::NSDictionary::new();
    #[expect(
        unsafe_code,
        reason = "Encode a retained bitmap using the AppKit image representation API"
    )]
    // SAFETY: The retained bitmap representation and immutable properties dictionary are valid for PNG encoding.
    let data = unsafe {
        rep.representationUsingType_properties(objc2_app_kit::NSBitmapImageFileType::PNG, &dict)
    }?;
    #[expect(
        unsafe_code,
        reason = "Borrow immutable NSData bytes only while its retained owner remains live"
    )]
    // SAFETY: The retained immutable NSData owns these bytes through the synchronous file write, with no mutation.
    let bytes = unsafe { data.as_bytes_unchecked() };
    if let Err(error) = std::fs::write(path, bytes) {
        harness_error(&format!("failed to write PNG: {error}"));
        return None;
    }
    harness_output(&format!(
        "Rendered popover view to {path} (height={total_h})"
    ));
    Some(())
}

fn render_if_requested(harness: &Harness) -> Option<()> {
    let delegate = &*harness.delegate;
    let mtm = MainThreadMarker::from(&*harness.delegate);
    if let Ok(render_prefix) = std::env::var("MELAFFEINE_RENDER_POPOVER") {
        use objc2_app_kit::{
            NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSBackingStoreType,
            NSWindow, NSWindowStyleMask,
        };
        use objc2_core_foundation::{CGPoint, CGRect, CGSize};

        let initial_h = delegate
            .ivars()
            .state
            .borrow()
            .as_ref()?
            .controls
            .layout_footer();
        let win_frame = CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(app::ui::MENU_WIDTH, initial_h),
        );
        #[expect(
            unsafe_code,
            reason = "NSWindow initialization requires a main-thread receiver and valid geometry"
        )]
        // SAFETY: MainThreadMarker proves the main thread; the frame, backing mode and style are valid.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                win_frame,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        #[expect(
            unsafe_code,
            reason = "Read the immutable NSAppearanceNameAqua AppKit global for deterministic rendering"
        )]
        // SAFETY: NSAppearanceNameAqua is an immutable AppKit global accessed on the main thread.
        if let Some(aqua) = unsafe { NSAppearance::appearanceNamed(NSAppearanceNameAqua) } {
            window.setAppearance(Some(&aqua));
        }

        // Render idle state
        assert_eq!(
            delegate.stop_session().map_err(|error| error.to_string()),
            Ok(()),
            "restore normal sleep"
        );
        let idle_state = delegate.ivars().state.borrow();
        let idle_controls = &idle_state.as_ref()?.controls;
        idle_controls
            .no_time_limit_button
            .setState(NSControlStateValueOff);
        idle_controls.set_awake_mode(AwakeMode::SystemOnly);
        drop(idle_state);
        delegate.update_ui();
        render_view(harness, &window, &format!("{render_prefix}-idle.png"))?;

        // Render active state with countdown visible (started via IPC Start with 2h)
        let render_start_response = delegate.execute_ipc_command(&IpcCommand::Start {
            duration: Some(Duration::from_hours(2)),
            keep_display_awake: false,
        });
        assert!(
            matches!(render_start_response, IpcResponse::Ok(_)),
            "Expected condition: matches!(render_start_response, IpcResponse::Ok(_))"
        );
        delegate.update_ui();
        render_view(harness, &window, &format!("{render_prefix}-active.png"))?;
        assert_eq!(
            delegate.stop_session().map_err(|error| error.to_string()),
            Ok(()),
            "restore normal sleep"
        );

        // Render error state while idle
        delegate.show_error("Open Melaffeine and click Start once to set up lid-closed mode.");
        delegate.update_ui();
        render_view(harness, &window, &format!("{render_prefix}-error.png"))?;
        delegate.clear_error();
    }

    Some(())
}

#[expect(
    clippy::print_stdout,
    reason = "The custom harness emits test discovery, results, and optional rendering output on stdout"
)]
fn harness_output(message: &str) {
    println!("{message}");
}

#[expect(
    clippy::print_stderr,
    reason = "The custom harness reports an unusable main-thread runner on stderr"
)]
fn harness_error(message: &str) {
    eprintln!("{message}");
}

fn harness_value<T>(result: Result<T, impl std::fmt::Display>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            harness_error(&format!("Isolated harness operation failed: {error}"));
            None
        }
    }
}
