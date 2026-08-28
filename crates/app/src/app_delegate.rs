//! `AppKit` `NSApplicationDelegate` implementation and lifecycle coordinator.

use std::cell::RefCell;
use std::ptr::NonNull;
use std::time::SystemTime;

use crate::iokit::IOKitProvider;
use crate::login::LoginItemService;
use crate::power::PowerController;
use crate::ui::{
    COUNTDOWN_AT_SEPARATOR, COUNTDOWN_STOPS_IN_PREFIX, COUNTDOWN_TIMER_TOLERANCE,
    COUNTDOWN_UPDATE_INTERVAL, ERROR_DURATION_INVALID, ERROR_START_FAILED, ICON_ACTIVE,
    ICON_INACTIVE, MENU_HEIGHT, MENU_WIDTH, TITLE_QUIT, UNIT_DAYS_INDEX, UNIT_MINUTES_INDEX,
    build_content_view, compute_ui_projection,
};
use app_core::duration::{DurationUnit, format_compact_duration, parse_duration};
use block2::RcBlock;
use objc2::rc::{Retained, Weak};
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationDelegate, NSButton, NSControl, NSControlStateValueOff,
    NSControlStateValueOn, NSEvent, NSEventMask, NSEventType, NSImage, NSMenu, NSMenuItem,
    NSPopUpButton, NSPopover, NSPopoverBehavior, NSStatusBar, NSStatusBarButton, NSStatusItem,
    NSTextField, NSVariableStatusItemLength, NSViewController,
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
    /// "Run indefinitely" checkbox.
    pub indefinite_button: Retained<NSButton>,
    /// Numeric duration input text field.
    pub duration_field: Retained<NSTextField>,
    /// Duration unit popup button.
    pub unit_popup: Retained<NSPopUpButton>,
    /// "Keep display awake too" checkbox.
    pub display_awake_button: Retained<NSButton>,
    /// "Launch at login" checkbox.
    pub launch_at_login_button: Retained<NSButton>,
    /// Primary Start / Stop action button.
    pub start_stop_button: Retained<NSButton>,
    /// Countdown remaining time label.
    pub time_label: Retained<NSTextField>,
    /// Error message label.
    pub error_label: Retained<NSTextField>,
    /// Active power management controller.
    pub power: PowerController<IOKitProvider>,
    /// Retained global event monitor token for clicks outside the popover.
    pub outside_click_monitor: Option<Retained<AnyObject>>,
    /// Repeating timer for updating countdown text while popover is open.
    pub countdown_timer: Option<Retained<NSTimer>>,
    /// Single-shot timer to terminate power assertion upon duration expiry.
    pub expiry_timer: Option<Retained<NSTimer>>,
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
            let mtm = MainThreadMarker::from(self);
            let app = NSApplication::sharedApplication(mtm);
            // SAFETY: currentEvent is called on the shared application on the main thread.
            let event = app.currentEvent();
            if let Some(ev) = event {
                let event_type = ev.r#type();
                if event_type == NSEventType::RightMouseUp {
                    self.show_context_menu();
                    return;
                }
            }
            self.toggle_popover_relative_to(sender);
        }

        /// Toggles popover or updates input enable states on control modification.
        #[unsafe(method(controlChanged:))]
        fn control_changed(&self, _sender: &NSControl) {
            self.update_ui();
        }

        /// Handles clicks on the primary Start / Stop button.
        #[unsafe(method(startStopClicked:))]
        fn start_stop_clicked(&self, _sender: &NSButton) {
            self.handle_start_stop();
        }

        /// Handles changes to the "Launch at login" checkbox.
        #[unsafe(method(launchAtLoginChanged:))]
        fn launch_at_login_changed(&self, _sender: &NSButton) {
            let state_opt = self.ivars().borrow();
            let Some(state) = state_opt.as_ref() else {
                return;
            };
            let is_checked = state.launch_at_login_button.state() == NSControlStateValueOn;
            drop(state_opt);

            if let Err(err_msg) = LoginItemService::set_enabled(is_checked) {
                self.show_error(&err_msg);
            }
            self.update_ui();
        }

        /// Terminates the application when Quit is clicked in the context menu.
        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            self.teardown();
            let mtm = MainThreadMarker::from(self);
            let app = NSApplication::sharedApplication(mtm);
            // SAFETY: terminate is called on the main thread during explicit user quit.
            app.terminate(None);
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
    fn init_app_state(&self, mtm: MainThreadMarker) {
        let status_bar = NSStatusBar::systemStatusBar();
        let status_item = status_bar.statusItemWithLength(NSVariableStatusItemLength);

        let Some(button) = status_item.button(mtm) else {
            eprintln!("Melaffeine: Failed to obtain NSStatusBarButton");
            return;
        };

        // SAFETY: setTarget, setAction, and sendActionOn are called on the main thread.
        unsafe {
            button.setTarget(Some(self));
            button.setAction(Some(sel!(statusItemClicked:)));
            button.sendActionOn(NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp);
        }
        button.setToolTip(Some(&NSString::from_str("Melaffeine")));
        let popover = NSPopover::new(mtm);
        popover.setBehavior(NSPopoverBehavior::Transient);
        popover.setContentSize(NSSize::new(MENU_WIDTH, MENU_HEIGHT));

        let controls = build_content_view(mtm);

        // SAFETY: setTarget and setAction are called on the main thread.
        unsafe {
            controls.indefinite_button.setTarget(Some(self));
            controls
                .indefinite_button
                .setAction(Some(sel!(controlChanged:)));

            controls.duration_field.setTarget(Some(self));
            controls
                .duration_field
                .setAction(Some(sel!(controlChanged:)));

            controls.unit_popup.setTarget(Some(self));
            controls.unit_popup.setAction(Some(sel!(controlChanged:)));

            controls.keep_display_awake_button.setTarget(Some(self));
            controls
                .keep_display_awake_button
                .setAction(Some(sel!(controlChanged:)));

            controls.start_stop_button.setTarget(Some(self));
            controls
                .start_stop_button
                .setAction(Some(sel!(startStopClicked:)));

            controls.launch_at_login_button.setTarget(Some(self));
            controls
                .launch_at_login_button
                .setAction(Some(sel!(launchAtLoginChanged:)));
        }
        let view_controller = NSViewController::new(mtm);
        view_controller.setView(&controls.view);
        popover.setContentViewController(Some(&view_controller));

        let app_state = AppState {
            status_item,
            popover,
            indefinite_button: controls.indefinite_button,
            duration_field: controls.duration_field,
            unit_popup: controls.unit_popup,
            display_awake_button: controls.keep_display_awake_button,
            launch_at_login_button: controls.launch_at_login_button,
            start_stop_button: controls.start_stop_button,
            time_label: controls.time_label,
            error_label: controls.error_label,
            power: PowerController::new(IOKitProvider),
            outside_click_monitor: None,
            countdown_timer: None,
            expiry_timer: None,
        };

        *self.ivars().borrow_mut() = Some(app_state);
        self.update_ui();
    }

    /// Toggles popover visibility relative to the status bar button.
    pub fn toggle_popover_relative_to(&self, button: &NSStatusBarButton) {
        let is_shown = {
            let state_opt = self.ivars().borrow();
            state_opt.as_ref().is_some_and(|s| s.popover.isShown())
        };

        if is_shown {
            self.close_popover();
            return;
        }

        self.update_ui();
        let mtm = MainThreadMarker::from(self);
        let app = NSApplication::sharedApplication(mtm);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
        let state_opt = self.ivars().borrow();
        if let Some(state) = state_opt.as_ref() {
            let bounds = button.bounds();
            state
                .popover
                .showRelativeToRect_ofView_preferredEdge(bounds, button, NSRectEdge::MinY);
            if let Some(vc) = state.popover.contentViewController()
                && let Some(window) = vc.view().window()
            {
                let _ = window.makeFirstResponder(None);
            }
        }
        drop(state_opt);

        self.install_outside_click_monitor();
        self.start_countdown_timer_if_needed();
    }

    /// Closes the popover, stops countdown timers, and removes event monitors.
    pub fn close_popover(&self) {
        self.remove_outside_click_monitor();
        self.stop_countdown_timer();

        let state_opt = self.ivars().borrow();
        if let Some(state) = state_opt.as_ref()
            && state.popover.isShown()
        {
            // SAFETY: performClose is called on the main thread.
            unsafe { state.popover.performClose(None) };
        }
    }

    /// Displays the ephemeral right-click context menu containing Quit.
    fn show_context_menu(&self) {
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

        let state_opt = self.ivars().borrow();
        if let Some(state) = state_opt.as_ref() {
            state.status_item.setMenu(Some(&menu));
            if let Some(button) = state.status_item.button(mtm) {
                // SAFETY: performClick is called on the main thread.
                unsafe { button.performClick(None) };
            }
            state.status_item.setMenu(None);
        }
    }

    /// Installs a global mouse down event monitor to close transient popovers.
    fn install_outside_click_monitor(&self) {
        self.remove_outside_click_monitor();

        let weak_self: Weak<Self> = Weak::from_retained(&Retained::from(self));
        let block = RcBlock::new(move |_event: NonNull<NSEvent>| {
            if let Some(strong_self) = weak_self.load() {
                strong_self.close_popover();
            }
        });

        let monitor = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(
            NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown,
            &block,
        );

        let mut state_opt = self.ivars().borrow_mut();
        if let Some(state) = state_opt.as_mut() {
            state.outside_click_monitor = monitor;
        }
    }

    /// Removes and drops the active global event monitor if present.
    fn remove_outside_click_monitor(&self) {
        let mut state_opt = self.ivars().borrow_mut();
        if let Some(state) = state_opt.as_mut()
            && let Some(monitor) = state.outside_click_monitor.take()
        {
            // SAFETY: removeMonitor is safe to call with a retained event monitor.
            unsafe {
                NSEvent::removeMonitor(&monitor);
            }
        }
    }

    /// Starts or updates power assertion based on UI inputs.
    fn handle_start_stop(&self) {
        let is_active = {
            let state_opt = self.ivars().borrow();
            state_opt.as_ref().is_some_and(|s| s.power.is_active())
        };

        if is_active {
            self.stop_power_and_expiry();
            self.update_ui();
            return;
        }

        let (duration, keep_display) = {
            let state_opt = self.ivars().borrow();
            let Some(state) = state_opt.as_ref() else {
                return;
            };

            let indefinite = state.indefinite_button.state() == NSControlStateValueOn;
            let keep_display = state.display_awake_button.state() == NSControlStateValueOn;

            if indefinite {
                (Ok(None), keep_display)
            } else {
                let input_str = state.duration_field.stringValue().to_string();
                let unit = match state.unit_popup.indexOfSelectedItem() {
                    UNIT_MINUTES_INDEX => DurationUnit::Minutes,
                    UNIT_DAYS_INDEX => DurationUnit::Days,
                    _ => DurationUnit::Hours,
                };
                parse_duration(&input_str, unit)
                    .map_or((Err(ERROR_DURATION_INVALID), keep_display), |dur| {
                        (Ok(Some(dur)), keep_display)
                    })
            }
        };

        let duration_opt = match duration {
            Ok(dur) => dur,
            Err(err_msg) => {
                self.show_error(err_msg);
                return;
            }
        };

        let mut state_opt = self.ivars().borrow_mut();
        let Some(state) = state_opt.as_mut() else {
            return;
        };

        let now = SystemTime::now();
        if let Err(_err) = state.power.start(duration_opt, keep_display, now) {
            drop(state_opt);
            self.show_error(ERROR_START_FAILED);
            return;
        }

        if let Some(dur) = duration_opt {
            let weak_self: Weak<Self> = Weak::from_retained(&Retained::from(self));
            let block = RcBlock::new(move |_timer: NonNull<NSTimer>| {
                if let Some(strong_self) = weak_self.load() {
                    strong_self.stop_power_and_expiry();
                    strong_self.update_ui();
                }
            });
            let timer = unsafe {
                NSTimer::scheduledTimerWithTimeInterval_repeats_block(
                    dur.as_secs_f64(),
                    false,
                    &block,
                )
            };
            state.expiry_timer = Some(timer);
        }

        drop(state_opt);
        self.update_ui();
    }

    /// Stops power assertion and invalidates expiry timer.
    fn stop_power_and_expiry(&self) {
        let mut state_opt = self.ivars().borrow_mut();
        if let Some(state) = state_opt.as_mut() {
            if let Some(timer) = state.expiry_timer.take() {
                timer.invalidate();
            }
            state.power.stop();
        }
    }

    /// Synchronizes all native UI elements to match current power and settings projection.
    pub fn update_ui(&self) {
        let mtm = MainThreadMarker::from(self);
        let (active, indefinite, ends_at) = {
            let state_opt = self.ivars().borrow();
            let Some(state) = state_opt.as_ref() else {
                return;
            };
            (
                state.power.is_active(),
                state.indefinite_button.state() == NSControlStateValueOn,
                state.power.ends_at(),
            )
        };

        let countdown_text = self.format_countdown(ends_at);
        let projection = compute_ui_projection(active, indefinite, countdown_text);

        let state_opt = self.ivars().borrow();
        let Some(state) = state_opt.as_ref() else {
            return;
        };

        state
            .start_stop_button
            .setTitle(&NSString::from_str(projection.start_stop_title));
        state
            .indefinite_button
            .setEnabled(projection.indefinite_enabled);
        state.duration_field.setEnabled(projection.duration_enabled);
        state.unit_popup.setEnabled(projection.unit_enabled);
        state
            .display_awake_button
            .setEnabled(projection.display_enabled);

        let login_enabled = LoginItemService::is_enabled();
        state.launch_at_login_button.setState(if login_enabled {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });

        if let Some(countdown) = &projection.countdown_text {
            state
                .time_label
                .setStringValue(&NSString::from_str(countdown));
            state.time_label.setHidden(false);
        } else {
            state.time_label.setStringValue(&NSString::from_str(""));
            state.time_label.setHidden(true);
        }

        let symbol_name = if active { ICON_ACTIVE } else { ICON_INACTIVE };
        if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str(symbol_name),
            Some(&NSString::from_str("Melaffeine")),
        ) {
            image.setTemplate(true);
            if let Some(button) = state.status_item.button(mtm) {
                button.setImage(Some(&image));
                button.setTitle(&NSString::from_str(""));
            }
        }

        drop(state_opt);
        self.start_countdown_timer_if_needed();
    }

    /// Formats the countdown string if a finite session is active with a future end time.
    fn format_countdown(&self, ends_at: Option<SystemTime>) -> Option<String> {
        let ends_at = ends_at?;
        let now = SystemTime::now();
        let remaining = ends_at.duration_since(now).ok()?;
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

        Some(format!(
            "{COUNTDOWN_STOPS_IN_PREFIX}{compact}{COUNTDOWN_AT_SEPARATOR}{time_str}"
        ))
    }

    /// Displays an error message on the error label and unhides it.
    fn show_error(&self, message: &str) {
        let state_opt = self.ivars().borrow();
        if let Some(state) = state_opt.as_ref() {
            state
                .error_label
                .setStringValue(&NSString::from_str(message));
            state.error_label.setHidden(false);
            state.time_label.setStringValue(&NSString::from_str(""));
            state.time_label.setHidden(true);
        }
    }

    /// Starts repeating countdown timer if popover is open for an active finite session.
    fn start_countdown_timer_if_needed(&self) {
        let should_start = {
            let state_opt = self.ivars().borrow();
            state_opt.as_ref().is_some_and(|s| {
                s.countdown_timer.is_none()
                    && s.popover.isShown()
                    && s.power.is_active()
                    && s.power.ends_at().is_some()
            })
        };

        if !should_start {
            return;
        }

        let weak_self: Weak<Self> = Weak::from_retained(&Retained::from(self));
        let block = RcBlock::new(move |_timer: NonNull<NSTimer>| {
            if let Some(strong_self) = weak_self.load() {
                strong_self.update_ui();
            }
        });

        // SAFETY: scheduledTimerWithTimeInterval_repeats_block is called on the main thread.
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_repeats_block(
                COUNTDOWN_UPDATE_INTERVAL,
                true,
                &block,
            )
        };
        timer.setTolerance(COUNTDOWN_TIMER_TOLERANCE);

        let mut state_opt = self.ivars().borrow_mut();
        if let Some(state) = state_opt.as_mut() {
            state.countdown_timer = Some(timer);
        }
    }

    /// Stops and releases the active countdown timer.
    fn stop_countdown_timer(&self) {
        let mut state_opt = self.ivars().borrow_mut();
        if let Some(state) = state_opt.as_mut()
            && let Some(timer) = state.countdown_timer.take()
        {
            timer.invalidate();
        }
    }

    /// Tears down all timers, monitors, and active power assertions upon quit.
    fn teardown(&self) {
        self.remove_outside_click_monitor();
        self.stop_countdown_timer();
        self.stop_power_and_expiry();
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_test_app_delegate_lifecycle() {
        if let Some(mtm) = MainThreadMarker::new() {
            let delegate = AppDelegate::new(mtm);
            delegate.init_app_state(mtm);
            delegate.update_ui();
            delegate.show_error("Test Error");
            delegate.close_popover();
            let _ = delegate.format_countdown(Some(SystemTime::now()));
            delegate.teardown();
        }
    }
}
