//! `AppKit` `NSApplicationDelegate` implementation and lifecycle coordinator.

use std::cell::RefCell;
use std::ptr::NonNull;
use std::time::{Duration, SystemTime};

use crate::iokit::IOKitProvider;
use crate::ipc_server::IpcServer;
use crate::power::PowerController;
use crate::ui::{
    AwakeMode, COUNTDOWN_TIMER_TOLERANCE, COUNTDOWN_UPDATE_INTERVAL, ERROR_DURATION_INVALID,
    MENU_WIDTH, NOTICE_THERMAL_CUTOFF, PopoverControls, THERMAL_CHECK_INTERVAL, TITLE_QUIT,
    UNIT_DAYS_INDEX, UNIT_MINUTES_INDEX, build_content_view, compute_ui_projection,
};
use app_core::duration::{DurationUnit, format_compact_duration, parse_duration};
use app_core::ipc::{IpcCommand, IpcResponse, SessionStatus, socket_path};
use block2::RcBlock;
use objc2::encode::RefEncode;
use objc2::rc::{Retained, Weak};
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationDelegate, NSButton, NSControl, NSControlStateValueOn, NSEvent,
    NSEventMask, NSEventType, NSImage, NSMenu, NSMenuItem, NSPopover, NSPopoverBehavior,
    NSStatusBar, NSStatusBarButton, NSStatusItem, NSVariableStatusItemLength, NSViewController,
};
use objc2_foundation::{
    NSDate, NSDateFormatter, NSDateFormatterStyle, NSNotification, NSObject, NSObjectProtocol,
    NSRectEdge, NSSize, NSString, NSTimer,
};
/// Owned `AppKit` state and controllers for the status item, popover, and timers.
pub struct AppState {
    /// Native status item in the menu bar.
    pub status_item: Retained<NSStatusItem>,
    /// Transient popover containing application controls.
    pub popover: Retained<NSPopover>,
    /// Controls container owning all popover subviews.
    pub controls: PopoverControls,
    /// Active power management controller.
    pub power: PowerController<IOKitProvider>,
    /// Active lid-closed mode session, if enabled for the current finite run.
    pub lid: Option<crate::lid::LidSession>,
    /// Repeating timer checking thermal state while lid-closed mode is active.
    pub thermal_timer: Option<Retained<NSTimer>>,
    /// Retained global event monitor token for clicks outside the popover.
    pub outside_click_monitor: Option<Retained<AnyObject>>,
    /// Repeating timer for updating countdown text while popover is open.
    pub countdown_timer: Option<Retained<NSTimer>>,
    /// Active IPC server handling CLI commands over Unix domain socket.
    pub ipc_server: Option<IpcServer>,
    /// Single-shot timer to terminate power assertion upon duration expiry.
    pub expiry_timer: Option<Retained<NSTimer>>,
    /// Cached external sleep-prevention application conflict notice.
    pub conflict_notice: Option<String>,
    /// Persisted validation or runtime error message.
    pub error_message: Option<String>,
}

define_class!(
    /// Main application delegate coordinating AppKit events, popovers, and power assertions.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = RefCell<Option<AppState>>]
    pub struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn application_did_finish_launching(&self, _notification: &NSNotification) {
            let mtm = MainThreadMarker::from(self);
            self.init_app_state(mtm);
        }

        #[unsafe(method(applicationWillTerminate:))]
        fn application_will_terminate(&self, _notification: &NSNotification) {
            self.teardown();
        }

        #[unsafe(method(applicationDidResignActive:))]
        fn application_did_resign_active(&self, _notification: &NSNotification) {
            self.close_popover();
        }
    }

    impl AppDelegate {
        /// Handles left and right clicks on the status item button.
        #[unsafe(method(statusItemClicked:))]
        fn status_item_clicked(&self, sender: &NSStatusBarButton) {
            // SAFETY: currentEvent is called on the shared application on the main thread.
            let app = NSApplication::sharedApplication(MainThreadMarker::from(self));
            if let Some(ev) = app.currentEvent()
                && ev.r#type() == NSEventType::RightMouseUp
            {
                self.show_context_menu();
                return;
            }
            self.toggle_popover_relative_to(sender);
        }

        /// Toggles popover or updates input enable states on control modification.
        #[unsafe(method(controlChanged:))]
        fn control_changed(&self, _sender: &NSControl) {
            self.clear_error();
            self.with_state(|state| {
                if state.controls.no_time_limit_button.state() == NSControlStateValueOn
                    && state.controls.awake_mode() == AwakeMode::LidClosed
                {
                    state.controls.set_awake_mode(AwakeMode::SystemOnly);
                }
            });
            self.update_ui();
        }

        /// Handles awake mode radio button selection changes.
        #[unsafe(method(awakeModeChanged:))]
        fn awake_mode_changed(&self, _sender: &NSButton) {
            self.clear_error();
            self.update_ui();
        }

        /// Handles changes to the "No time limit" checkbox.
        #[unsafe(method(noTimeLimitChanged:))]
        fn no_time_limit_changed(&self, _sender: &NSButton) {
            self.clear_error();
            self.with_state(|state| {
                if state.controls.no_time_limit_button.state() == NSControlStateValueOn
                    && state.controls.awake_mode() == AwakeMode::LidClosed
                {
                    state.controls.set_awake_mode(AwakeMode::SystemOnly);
                }
            });
            self.update_ui();
        }

        /// Handles clicks on the primary Start / Stop button.
        #[unsafe(method(startStopClicked:))]
        fn start_stop_clicked(&self, _sender: &NSButton) {
            if let Err(error) = self.handle_start_stop(true) {
                self.show_error(&error);
            }
        }
        /// Terminates the application when Quit is clicked in the context menu.
        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            self.terminate_app();
        }
    }
);

impl AppDelegate {
    /// Creates and initializes a new retained instance of `AppDelegate`.
    #[must_use]
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(RefCell::new(None));
        // SAFETY: standard NSObject init on a freshly allocated instance.
        unsafe { msg_send![super(this), init] }
    }
    /// Initializes native `AppKit` UI elements and controllers.
    pub fn init_app_state(&self, mtm: MainThreadMarker) {
        let status_bar = NSStatusBar::systemStatusBar();
        let status_item = status_bar.statusItemWithLength(NSVariableStatusItemLength);

        if let Some(button) = status_item.button(mtm) {
            // SAFETY: setTarget, setAction, and sendActionOn are called on the main thread.
            unsafe {
                button.setTarget(Some(self));
                button.setAction(Some(sel!(statusItemClicked:)));
                button.sendActionOn(NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp);
            }
            button.setToolTip(Some(&NSString::from_str("Melaffeine")));
        } else {
            eprintln!("Melaffeine: Failed to obtain NSStatusBarButton");
        }
        let popover = NSPopover::new(mtm);
        popover.setBehavior(NSPopoverBehavior::Transient);
        let controls = build_content_view(mtm);
        let height = controls.layout_footer();
        popover.setContentSize(NSSize::new(MENU_WIDTH, height));

        // SAFETY: setTarget and setAction are called on the main thread.
        unsafe {
            controls.no_time_limit_button.setTarget(Some(self));
            controls
                .no_time_limit_button
                .setAction(Some(sel!(noTimeLimitChanged:)));

            controls.mode_system_button.setTarget(Some(self));
            controls
                .mode_system_button
                .setAction(Some(sel!(awakeModeChanged:)));

            controls.mode_display_button.setTarget(Some(self));
            controls
                .mode_display_button
                .setAction(Some(sel!(awakeModeChanged:)));

            controls.mode_lid_button.setTarget(Some(self));
            controls
                .mode_lid_button
                .setAction(Some(sel!(awakeModeChanged:)));

            controls.start_stop_button.setTarget(Some(self));
            controls
                .start_stop_button
                .setAction(Some(sel!(startStopClicked:)));
        }
        let view_controller = NSViewController::new(mtm);
        view_controller.setView(&controls.view);
        popover.setContentViewController(Some(&view_controller));

        let app_state = AppState {
            status_item,
            popover,
            controls,
            power: PowerController::new(IOKitProvider),
            lid: None,
            thermal_timer: None,
            outside_click_monitor: None,
            ipc_server: None,
            countdown_timer: None,
            expiry_timer: None,
            conflict_notice: None,
            error_message: None,
        };

        *self.ivars().borrow_mut() = Some(app_state);
        crate::lid::reset_if_stale();
        let weak_self: Weak<Self> = Weak::from_retained(&Retained::from(self));
        let server = IpcServer::start(mtm, &socket_path(), move |cmd| {
            weak_self.load().map_or_else(
                || IpcResponse::Err(String::from("App unavailable")),
                |delegate| delegate.execute_ipc_command(cmd),
            )
        });
        if server.is_none() {
            eprintln!("Melaffeine: IPC server failed to start; CLI control unavailable");
        }
        self.with_state_mut(|state| state.ipc_server = server);
        self.update_ui();
    }

    /// Toggles popover visibility relative to the status bar button.
    pub fn toggle_popover_relative_to(&self, button: &NSStatusBarButton) {
        let is_shown = self.with_state(|s| s.popover.isShown()).unwrap_or(false);

        if is_shown {
            self.close_popover();
            return;
        }

        let conflict = crate::conflicts::detect_external_conflict();
        self.with_state_mut(|state| state.conflict_notice = conflict);
        self.update_ui();
        let mtm = MainThreadMarker::from(self);
        let app = NSApplication::sharedApplication(mtm);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
        self.with_state(|state| {
            let bounds = button.bounds();
            state
                .popover
                .showRelativeToRect_ofView_preferredEdge(bounds, button, NSRectEdge::MinY);
            if let Some(vc) = state.popover.contentViewController()
                && let Some(window) = vc.view().window()
            {
                let _ = window.makeFirstResponder(None);
            }
        });

        self.install_outside_click_monitor();
        self.start_countdown_timer_if_needed();
    }

    /// Closes the popover, stops countdown timers, and removes event monitors.
    pub fn close_popover(&self) {
        self.remove_outside_click_monitor();
        self.stop_countdown_timer();

        self.with_state(|state| {
            if state.popover.isShown() {
                // SAFETY: performClose is called on the main thread.
                unsafe { state.popover.performClose(None) };
            }
        });
    }

    /// Displays the ephemeral right-click context menu containing Quit.
    pub fn show_context_menu(&self) {
        let mtm = MainThreadMarker::from(self);
        let menu = NSMenu::new(mtm);
        // SAFETY: initWithTitle_action_keyEquivalent is called on the main thread.
        let quit_item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                mtm.alloc(),
                &NSString::from_str(TITLE_QUIT),
                Some(sel!(quit:)),
                &NSString::from_str(""),
            )
        };
        menu.addItem(&quit_item);

        let status_item =
            self.with_state(|state| (state.status_item.clone(), state.status_item.button(mtm)));
        if let Some((item, button)) = status_item {
            item.setMenu(Some(&menu));
            if let Some(btn) = button {
                // SAFETY: performClick is called on the main thread.
                unsafe { btn.performClick(None) };
            }
            item.setMenu(None);
        }
    }

    /// Installs a global mouse down event monitor to close transient popovers.
    pub fn install_outside_click_monitor(&self) {
        self.remove_outside_click_monitor();

        let block = self.weak_block::<NSEvent>(Self::close_popover);

        let monitor = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(
            NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown,
            &block,
        );

        self.with_state_mut(|state| state.outside_click_monitor = monitor);
    }

    /// Removes and drops the active global event monitor if present.
    pub fn remove_outside_click_monitor(&self) {
        self.with_state_mut(|state| {
            if let Some(monitor) = state.outside_click_monitor.take() {
                // SAFETY: removeMonitor is safe to call with a retained event monitor.
                unsafe { NSEvent::removeMonitor(&monitor) };
            }
        });
    }

    /// Starts or updates power assertion based on UI inputs.
    ///
    /// Returns `Ok(true)` if sleep prevention was started, `Ok(false)` if stopped.
    pub fn handle_start_stop(&self, interactive: bool) -> Result<bool, String> {
        let (is_active, duration, mode) = self
            .with_state(
                |state| -> Result<(bool, Option<Duration>, AwakeMode), String> {
                    let is_active = state.power.is_active();
                    if is_active {
                        return Ok((true, None, AwakeMode::SystemOnly));
                    }

                    let no_time_limit =
                        state.controls.no_time_limit_button.state() == NSControlStateValueOn;
                    let mode = state.controls.awake_mode();

                    if no_time_limit {
                        let effective_mode = if mode == AwakeMode::LidClosed {
                            AwakeMode::SystemOnly
                        } else {
                            mode
                        };
                        Ok((false, None, effective_mode))
                    } else {
                        let input = state.controls.duration_field.stringValue().to_string();
                        let unit = match state.controls.unit_popup.indexOfSelectedItem() {
                            UNIT_MINUTES_INDEX => DurationUnit::Minutes,
                            UNIT_DAYS_INDEX => DurationUnit::Days,
                            _ => DurationUnit::Hours,
                        };
                        let duration = parse_duration(input.trim(), unit)
                            .ok_or_else(|| String::from(ERROR_DURATION_INVALID))?;
                        Ok((false, Some(duration), mode))
                    }
                },
            )
            .ok_or_else(|| String::from("App state not initialized"))??;

        if is_active {
            self.stop_session();
            Ok(false)
        } else {
            let use_lid = duration.is_some() && mode == AwakeMode::LidClosed;
            let keep_display = mode == AwakeMode::Display;

            let lid_session = if use_lid {
                match crate::lid::LidSession::start() {
                    Ok(session) => Some(session),
                    Err(_start_err) => {
                        if interactive {
                            if let Err(auth_err) = crate::lid::authorize() {
                                self.show_error(&auth_err);
                                return Err(auth_err);
                            }
                            match crate::lid::LidSession::start() {
                                Ok(session) => Some(session),
                                Err(retry_err) => {
                                    self.show_error(&retry_err);
                                    return Err(retry_err);
                                }
                            }
                        } else {
                            let err_msg = String::from(
                                "Open Melaffeine and click Start once to set up lid-closed mode.",
                            );
                            self.show_error(&err_msg);
                            return Err(err_msg);
                        }
                    }
                }
            } else {
                None
            };

            if let Err(power_err) = self.start_session(duration, keep_display) {
                drop(lid_session);
                self.show_error(&power_err);
                return Err(power_err);
            }

            if let Some(session) = lid_session {
                let timer =
                    self.schedule_timer(THERMAL_CHECK_INTERVAL, true, Self::check_thermal_state);
                self.with_state_mut(|state| {
                    state.lid = Some(session);
                    state.thermal_timer = Some(timer);
                });
            }

            Ok(true)
        }
    }

    /// Starts a power assertion session for the specified duration and display setting.
    pub fn start_session(
        &self,
        duration_opt: Option<Duration>,
        keep_display: bool,
    ) -> Result<(), String> {
        let start_result = self
            .with_state_mut(|state| {
                state.error_message = None;
                Self::invalidate_timers(state);
                state
                    .power
                    .start(duration_opt, keep_display, SystemTime::now())
            })
            .ok_or_else(|| String::from("App state not initialized"))?;

        if let Err(error) = start_result {
            self.update_ui();
            return Err(error.to_string());
        }

        if duration_opt.is_some() {
            self.schedule_expiry_timer();
        }
        self.update_ui();
        Ok(())
    }

    /// Executes an incoming IPC command and generates a structured response.
    #[allow(clippy::option_if_let_else)]
    pub fn execute_ipc_command(&self, command: &IpcCommand) -> IpcResponse {
        match command {
            IpcCommand::Status => self
                .with_state(|state| {
                    if !state.power.is_active() {
                        return IpcResponse::Status(None);
                    }
                    let remaining_compact = state.power.ends_at().map(|ends_at| {
                        let remaining = ends_at
                            .duration_since(SystemTime::now())
                            .unwrap_or(Duration::ZERO);
                        format_compact_duration(remaining)
                    });
                    IpcResponse::Status(Some(SessionStatus {
                        keep_display_awake: state.power.keep_display_awake(),
                        remaining_compact,
                    }))
                })
                .unwrap_or_else(|| IpcResponse::Err(String::from("App not initialized"))),
            IpcCommand::Stop => {
                self.stop_session();
                IpcResponse::Ok(String::from("Stopped sleep prevention"))
            }
            // Toggle when inactive intentionally starts from the current UI control
            // state (duration field, unit popup, radios), so remote toggling
            // reflects what the user last configured in the popover.
            IpcCommand::Toggle => match self.handle_start_stop(false) {
                Ok(true) => IpcResponse::Ok(String::from("Started sleep prevention")),
                Ok(false) => IpcResponse::Ok(String::from("Stopped sleep prevention")),
                Err(error) => IpcResponse::Err(error),
            },
            IpcCommand::Start {
                duration,
                keep_display_awake,
            } => {
                let target_mode = if *keep_display_awake {
                    AwakeMode::Display
                } else {
                    AwakeMode::SystemOnly
                };
                self.with_state(|state| {
                    state.controls.set_awake_mode(target_mode);
                });
                match self.start_session(*duration, *keep_display_awake) {
                    Ok(()) => {
                        let desc = duration.map_or_else(
                            || String::from("Started indefinite session"),
                            |d| format!("Started finite session ({})", format_compact_duration(d)),
                        );
                        IpcResponse::Ok(desc)
                    }
                    Err(e) => IpcResponse::Err(e),
                }
            }
            IpcCommand::Quit => {
                self.schedule_timer(0.05, false, Self::terminate_app);
                IpcResponse::Ok(String::from("Terminating"))
            }
        }
    }

    /// Stops the active session (power assertion and timers) and updates UI.
    pub fn stop_session(&self) {
        self.with_state_mut(|state| {
            Self::invalidate_timers(state);
            state.power.stop();
        });
        self.update_ui();
    }

    /// Schedules a one-shot expiry timer that re-checks the wall-clock end time on
    /// each fire, rescheduling for the true remainder if the run loop paused during
    /// system sleep. Stops the session when `ends_at` is absent or already past.
    fn schedule_expiry_timer(&self) {
        let remaining_opt = self.with_state_mut(|state| {
            if let Some(timer) = state.expiry_timer.take() {
                timer.invalidate();
            }
            state
                .power
                .ends_at()
                .and_then(|ends_at| ends_at.duration_since(SystemTime::now()).ok())
                .filter(|remaining| !remaining.is_zero())
        });

        match remaining_opt {
            Some(Some(remaining)) => {
                let timer = self.schedule_timer(
                    remaining.as_secs_f64(),
                    false,
                    Self::schedule_expiry_timer,
                );
                self.with_state_mut(|state| state.expiry_timer = Some(timer));
            }
            Some(None) => {
                self.stop_session();
            }
            None => {}
        }
    }
    /// Synchronizes all native UI elements to match current power and settings projection.
    pub fn update_ui(&self) {
        let updated = self.with_state(|state| {
            let active = state.power.is_active();
            let ends_at = state.power.ends_at();
            let no_time_limit = if active {
                ends_at.is_none()
            } else {
                state.controls.no_time_limit_button.state() == NSControlStateValueOn
            };
            let countdown_text = self.format_countdown(ends_at);
            let projection = compute_ui_projection(active, no_time_limit, countdown_text);

            state
                .controls
                .start_stop_button
                .setTitle(&NSString::from_str(projection.start_stop_title));
            state
                .controls
                .no_time_limit_button
                .setEnabled(projection.inputs_enabled);
            state
                .controls
                .mode_system_button
                .setEnabled(projection.inputs_enabled);
            state
                .controls
                .mode_display_button
                .setEnabled(projection.inputs_enabled);
            state
                .controls
                .duration_field
                .setEnabled(projection.duration_enabled);
            state
                .controls
                .unit_popup
                .setEnabled(projection.duration_enabled);
            state
                .controls
                .mode_lid_button
                .setEnabled(projection.lid_enabled);
            if let Some(countdown) = &projection.countdown_text {
                state
                    .controls
                    .time_label
                    .setStringValue(&NSString::from_str(countdown));
                state.controls.time_label.setHidden(false);
            } else {
                state
                    .controls
                    .time_label
                    .setStringValue(&NSString::from_str(""));
                state.controls.time_label.setHidden(true);
            }

            if let Some(err) = &state.error_message {
                state
                    .controls
                    .error_label
                    .setStringValue(&NSString::from_str(err));
                state.controls.error_label.setHidden(false);
            } else if !active && let Some(conflict_app) = &state.conflict_notice {
                state
                    .controls
                    .error_label
                    .setStringValue(&NSString::from_str(&format!(
                        "Note: {conflict_app} is also running."
                    )));
                state.controls.error_label.setHidden(false);
            } else {
                state
                    .controls
                    .error_label
                    .setStringValue(&NSString::from_str(""));
                state.controls.error_label.setHidden(true);
            }

            let height = state.controls.layout_footer();
            if (state.popover.contentSize().height - height).abs() > f64::EPSILON {
                state
                    .popover
                    .setContentSize(NSSize::new(MENU_WIDTH, height));
            }

            if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str(projection.status_icon),
                Some(&NSString::from_str("Melaffeine")),
            ) {
                image.setTemplate(true);
                if let Some(button) = state.status_item.button(MainThreadMarker::from(self)) {
                    button.setImage(Some(&image));
                    button.setTitle(&NSString::from_str(""));
                }
            }
        });
        if updated.is_some() {
            self.start_countdown_timer_if_needed();
        }
    }

    /// Formats the countdown string if a finite session is active with a future end time.
    pub fn format_countdown(&self, ends_at: Option<SystemTime>) -> Option<String> {
        let ends_at = ends_at?;
        let remaining = ends_at.duration_since(SystemTime::now()).ok()?;
        if remaining.is_zero() {
            return None;
        }

        let compact = format_compact_duration(remaining);

        let _mtm = MainThreadMarker::from(self);
        let formatter = NSDateFormatter::new();
        formatter.setTimeStyle(NSDateFormatterStyle::ShortStyle);
        formatter.setDateStyle(NSDateFormatterStyle::NoStyle);

        let Ok(elapsed_since_epoch) = ends_at.duration_since(SystemTime::UNIX_EPOCH) else {
            return None;
        };
        let ns_date = NSDate::dateWithTimeIntervalSince1970(elapsed_since_epoch.as_secs_f64());
        let time_str = formatter.stringFromDate(&ns_date).to_string();

        Some(format!("Ends at {time_str} · {compact} left"))
    }

    pub fn show_error(&self, message: &str) {
        self.with_state_mut(|state| {
            state.error_message = Some(message.to_string());
        });
        self.update_ui();
    }

    /// Clears any displayed error message and updates UI.
    pub fn clear_error(&self) {
        self.with_state_mut(|state| {
            if state.error_message.as_deref() != Some(NOTICE_THERMAL_CUTOFF) {
                state.error_message = None;
            }
        });
        self.update_ui();
    }

    pub fn start_countdown_timer_if_needed(&self) {
        let should_start = self
            .with_state(|s| {
                s.countdown_timer.is_none()
                    && s.popover.isShown()
                    && s.power.is_active()
                    && s.power.ends_at().is_some()
            })
            .unwrap_or(false);

        if !should_start {
            return;
        }

        let timer = self.schedule_timer(COUNTDOWN_UPDATE_INTERVAL, true, Self::update_ui);
        timer.setTolerance(COUNTDOWN_TIMER_TOLERANCE);

        self.with_state_mut(|state| state.countdown_timer = Some(timer));
    }

    /// Stops and releases the active countdown timer.
    pub fn stop_countdown_timer(&self) {
        self.with_state_mut(|state| {
            if let Some(timer) = state.countdown_timer.take() {
                timer.invalidate();
            }
        });
    }

    /// Tears down all timers, monitors, and active power assertions upon quit.
    pub fn teardown(&self) {
        self.with_state_mut(|state| {
            if let Some(mut server) = state.ipc_server.take() {
                server.stop();
            }
        });
        self.remove_outside_click_monitor();
        self.stop_session();
    }

    /// Runs `f` with a shared borrow of the app state, if initialized.
    fn with_state<R>(&self, f: impl FnOnce(&AppState) -> R) -> Option<R> {
        self.ivars().borrow().as_ref().map(f)
    }

    /// Runs `f` with an exclusive borrow of the app state, if initialized.
    fn with_state_mut<R>(&self, f: impl FnOnce(&mut AppState) -> R) -> Option<R> {
        self.ivars().borrow_mut().as_mut().map(f)
    }

    /// Schedules an `NSTimer` on the main run loop invoking `action` on a weakly-held delegate.
    fn schedule_timer(&self, interval: f64, repeats: bool, action: fn(&Self)) -> Retained<NSTimer> {
        let block = self.weak_block::<NSTimer>(action);
        // SAFETY: scheduledTimerWithTimeInterval_repeats_block is called on the main thread
        // with an interval >= 0, and the weak block safely drops invocations if deallocated.
        unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(interval, repeats, &block) }
    }

    /// Builds a block that invokes `f` on a weakly-held delegate, no-op after dealloc.
    fn weak_block<T: RefEncode>(&self, f: impl Fn(&Self) + 'static) -> RcBlock<dyn Fn(NonNull<T>)> {
        let weak_self: Weak<Self> = Weak::from_retained(&Retained::from(self));
        RcBlock::new(move |_: NonNull<T>| {
            if let Some(this) = weak_self.load() {
                f(&this);
            }
        })
    }

    /// Checks system thermal state and disables lid-closed mode if the Mac is too hot.
    fn check_thermal_state(&self) {
        let has_lid = self.with_state(|s| s.lid.is_some()).unwrap_or(false);
        if !has_lid {
            self.with_state_mut(Self::clear_lid_session);
            return;
        }
        if crate::lid::thermal_too_hot() {
            self.with_state_mut(|state| {
                Self::clear_lid_session(state);
                state.controls.set_awake_mode(AwakeMode::SystemOnly);
                state.error_message = Some(String::from(NOTICE_THERMAL_CUTOFF));
            });
            self.update_ui();
        }
    }

    /// Drops the active lid-closed session and invalidates its thermal guard timer.
    fn clear_lid_session(state: &mut AppState) {
        if let Some(timer) = state.thermal_timer.take() {
            timer.invalidate();
        }
        state.lid = None;
    }

    /// Invalidates and releases the expiry, countdown, and thermal timers, and drops any lid session.
    fn invalidate_timers(state: &mut AppState) {
        Self::clear_lid_session(state);
        if let Some(timer) = state.expiry_timer.take() {
            timer.invalidate();
        }
        if let Some(timer) = state.countdown_timer.take() {
            timer.invalidate();
        }
    }

    /// Tears down app state and terminates the shared application.
    fn terminate_app(&self) {
        self.teardown();
        let mtm = MainThreadMarker::from(self);
        let app = NSApplication::sharedApplication(mtm);
        // SAFETY: terminate is called on the main thread during explicit user quit.
        app.terminate(None);
    }
}
