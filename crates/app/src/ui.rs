//! UI projection model, layout constants, and programmatic `AppKit` view hierarchy.

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBezelStyle, NSButton, NSColor, NSControlStateValueOff,
    NSControlStateValueOn, NSPopUpButton, NSTextField, NSView,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSArray, NSSize, NSString};
/// UI dimensions and geometry constants.
pub const MENU_WIDTH: f64 = 260.0;
pub const PADDING_OUTER: f64 = 16.0;
pub const CONTENT_WIDTH: f64 = MENU_WIDTH - 2.0 * PADDING_OUTER;
pub const SPACING_SECTION: f64 = 14.0;
pub const SPACING_HEADER_TO_ROW: f64 = 4.0;
pub const SPACING_INSIDE_SECTION: f64 = 6.0;

pub const LABEL_HEIGHT: f64 = 16.0;
pub const ROW_DURATION_HEIGHT: f64 = 32.0;
pub const DURATION_FIELD_HEIGHT: f64 = 28.0;
pub const POPUP_HEIGHT: f64 = 32.0;
pub const DURATION_FIELD_WIDTH: f64 = 72.0;
pub const SPACING_DURATION_POPUP: f64 = 8.0;

pub const CHECKBOX_HEIGHT: f64 = 20.0;
pub const BUTTON_HEIGHT: f64 = 30.0;

/// Fixed height from the top of the popover through the bottom of the Start/Stop button.
pub const TOP_SECTION_HEIGHT: f64 = PADDING_OUTER
    + LABEL_HEIGHT
    + SPACING_HEADER_TO_ROW
    + ROW_DURATION_HEIGHT
    + SPACING_INSIDE_SECTION
    + CHECKBOX_HEIGHT
    + SPACING_SECTION
    + LABEL_HEIGHT
    + SPACING_HEADER_TO_ROW
    + CHECKBOX_HEIGHT
    + SPACING_INSIDE_SECTION
    + CHECKBOX_HEIGHT
    + SPACING_INSIDE_SECTION
    + CHECKBOX_HEIGHT
    + SPACING_SECTION
    + BUTTON_HEIGHT;
pub const COUNTDOWN_UPDATE_INTERVAL: f64 = 60.0;
pub const COUNTDOWN_TIMER_TOLERANCE: f64 = 15.0;
pub const THERMAL_CHECK_INTERVAL: f64 = 30.0;

/// String constants for UI titles, SF Symbols, and messages.
pub const ICON_INACTIVE: &str = "cup.and.saucer";
pub const ICON_ACTIVE: &str = "cup.and.saucer.fill";
pub const TITLE_START: &str = "Start";
pub const TITLE_STOP: &str = "Stop";
pub const TITLE_QUIT: &str = "Quit";
pub const TITLE_SECTION_DURATION: &str = "Keep awake for";
pub const TITLE_NO_TIME_LIMIT: &str = "No time limit";
pub const TITLE_SECTION_MODE: &str = "While active";
pub const TITLE_MODE_SYSTEM: &str = "Let the screen turn off";
pub const TITLE_MODE_DISPLAY: &str = "Keep the screen on";
pub const TITLE_MODE_LID: &str = "Keep running with lid closed";
pub const DURATION_PLACEHOLDER: &str = "Duration";
pub const DEFAULT_DURATION_TEXT: &str = "2";
pub const UNIT_MINUTES_TITLE: &str = "Minutes";
pub const UNIT_HOURS_TITLE: &str = "Hours";
pub const UNIT_DAYS_TITLE: &str = "Days";
pub const ERROR_DURATION_INVALID: &str = "Enter a whole number from 1 to 365 days.";
pub const NOTICE_THERMAL_CUTOFF: &str = "Mac is too hot — lid-closed mode turned off.";

/// Selected unit index in the unit popup button.
pub const UNIT_MINUTES_INDEX: isize = 0;
pub const UNIT_HOURS_INDEX: isize = 1;
pub const UNIT_DAYS_INDEX: isize = 2;

/// Awake mode selection among exclusive options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AwakeMode {
    /// Allow the screen to turn off while keeping the system awake.
    #[default]
    SystemOnly,
    /// Keep the screen turned on.
    Display,
    /// Keep the system awake even when the laptop lid is closed.
    LidClosed,
}

/// A projection representing the state of all UI elements in the popover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiProjection {
    /// Title of the start/stop button ("Start" or "Stop").
    pub start_stop_title: &'static str,
    /// SF Symbol icon name for the status item ("cup.and.saucer" or "cup.and.saucer.fill").
    pub status_icon: &'static str,
    /// Whether general inputs (no time limit checkbox, system/display radios) are enabled.
    pub inputs_enabled: bool,
    /// Whether finite-session duration inputs (duration field, unit popup) are enabled.
    pub duration_enabled: bool,
    /// Whether the lid-closed radio button is enabled.
    pub lid_enabled: bool,
    /// Optional formatted countdown text if an active finite session is running.
    pub countdown_text: Option<String>,
}

/// Computes the declarative UI projection given the power controller state,
/// no-time-limit checkbox state, and optional formatted countdown text.
#[must_use]
pub fn compute_ui_projection(
    active: bool,
    no_time_limit: bool,
    countdown_text: Option<String>,
) -> UiProjection {
    if active {
        UiProjection {
            start_stop_title: TITLE_STOP,
            status_icon: ICON_ACTIVE,
            inputs_enabled: false,
            duration_enabled: false,
            lid_enabled: false,
            countdown_text,
        }
    } else {
        let finite = !no_time_limit;
        UiProjection {
            start_stop_title: TITLE_START,
            status_icon: ICON_INACTIVE,
            inputs_enabled: true,
            duration_enabled: finite,
            lid_enabled: finite,
            countdown_text: None,
        }
    }
}

/// Container owning all `AppKit` UI controls created for the popover content view.
pub struct PopoverControls {
    /// Root view containing all controls.
    pub view: Retained<NSView>,
    /// "No time limit" checkbox.
    pub no_time_limit_button: Retained<NSButton>,
    /// Numeric duration input text field.
    pub duration_field: Retained<NSTextField>,
    /// Duration unit popup button (Minutes / Hours / Days).
    pub unit_popup: Retained<NSPopUpButton>,
    /// "Let the screen turn off" radio button.
    pub mode_system_button: Retained<NSButton>,
    /// "Keep the screen on" radio button.
    pub mode_display_button: Retained<NSButton>,
    /// "Keep running with lid closed" radio button.
    pub mode_lid_button: Retained<NSButton>,
    /// Countdown remaining time label.
    pub time_label: Retained<NSTextField>,
    /// Primary Start / Stop action button.
    pub start_stop_button: Retained<NSButton>,
    /// Error message label.
    pub error_label: Retained<NSTextField>,
}

impl PopoverControls {
    /// Returns the currently selected awake mode among the three radio buttons.
    #[must_use]
    pub fn awake_mode(&self) -> AwakeMode {
        if self.mode_lid_button.state() == NSControlStateValueOn {
            AwakeMode::LidClosed
        } else if self.mode_display_button.state() == NSControlStateValueOn {
            AwakeMode::Display
        } else {
            AwakeMode::SystemOnly
        }
    }

    /// Explicitly updates the states of all three radio buttons to match `mode`.
    pub fn set_awake_mode(&self, mode: AwakeMode) {
        self.mode_system_button
            .setState(if mode == AwakeMode::SystemOnly {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
        self.mode_display_button
            .setState(if mode == AwakeMode::Display {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
        self.mode_lid_button
            .setState(if mode == AwakeMode::LidClosed {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
    }

    /// Positions visible footer labels under the Start/Stop button and resizes the content view.
    ///
    /// Returns the total content height.
    #[must_use]
    pub fn layout_footer(&self) -> f64 {
        let labels = [&self.time_label, &self.error_label];
        let mut placements: [Option<(&NSTextField, f64, f64)>; 2] = [None, None];
        let mut cursor = TOP_SECTION_HEIGHT;

        for (slot, label) in placements.iter_mut().zip(labels) {
            if !label.isHidden() {
                cursor += SPACING_INSIDE_SECTION;
                let fitted = label.sizeThatFits(NSSize::new(CONTENT_WIDTH, f64::MAX));
                let h = fitted.height.max(LABEL_HEIGHT);
                *slot = Some((label, cursor, h));
                cursor += h;
            }
        }

        let total_height = cursor + PADDING_OUTER;
        self.view
            .setFrameSize(NSSize::new(MENU_WIDTH, total_height));

        for (label, top, h) in placements.into_iter().flatten() {
            let y = total_height - top - h;
            label.setFrame(CGRect::new(
                CGPoint::new(PADDING_OUTER, y),
                CGSize::new(CONTENT_WIDTH, h),
            ));
        }

        total_height
    }
}
fn build_duration_row(
    mtm: MainThreadMarker,
    x: f64,
    width: f64,
    top: f64,
    total_height: f64,
) -> (Retained<NSTextField>, Retained<NSPopUpButton>) {
    let duration_field =
        NSTextField::textFieldWithString(&NSString::from_str(DEFAULT_DURATION_TEXT), mtm);
    duration_field.setPlaceholderString(Some(&NSString::from_str(DURATION_PLACEHOLDER)));
    let field_h = DURATION_FIELD_HEIGHT;
    let field_top = top + (ROW_DURATION_HEIGHT - DURATION_FIELD_HEIGHT) / 2.0;
    let field_y = total_height - field_top - field_h;
    duration_field.setFrame(CGRect::new(
        CGPoint::new(x, field_y),
        CGSize::new(DURATION_FIELD_WIDTH, field_h),
    ));
    duration_field.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinYMargin);

    let popup_x = x + DURATION_FIELD_WIDTH + SPACING_DURATION_POPUP;
    let popup_w = width - DURATION_FIELD_WIDTH - SPACING_DURATION_POPUP;
    let popup_h = POPUP_HEIGHT;
    let popup_top = top;
    let popup_y = total_height - popup_top - popup_h;
    let unit_popup_frame = CGRect::new(
        CGPoint::new(popup_x, popup_y),
        CGSize::new(popup_w, popup_h),
    );
    let unit_popup = NSPopUpButton::initWithFrame_pullsDown(mtm.alloc(), unit_popup_frame, false);
    let unit_titles = NSArray::from_retained_slice(&[
        NSString::from_str(UNIT_MINUTES_TITLE),
        NSString::from_str(UNIT_HOURS_TITLE),
        NSString::from_str(UNIT_DAYS_TITLE),
    ]);
    unit_popup.addItemsWithTitles(&unit_titles);
    unit_popup.selectItemAtIndex(UNIT_HOURS_INDEX);
    unit_popup.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinYMargin);

    (duration_field, unit_popup)
}

fn build_mode_radios(
    mtm: MainThreadMarker,
) -> (Retained<NSButton>, Retained<NSButton>, Retained<NSButton>) {
    // SAFETY: radioButtonWithTitle_target_action is called on the main thread.
    let mode_system_button = unsafe {
        NSButton::radioButtonWithTitle_target_action(
            &NSString::from_str(TITLE_MODE_SYSTEM),
            None,
            None,
            mtm,
        )
    };
    mode_system_button.setState(NSControlStateValueOn);

    // SAFETY: radioButtonWithTitle_target_action is called on the main thread.
    let mode_display_button = unsafe {
        NSButton::radioButtonWithTitle_target_action(
            &NSString::from_str(TITLE_MODE_DISPLAY),
            None,
            None,
            mtm,
        )
    };
    mode_display_button.setState(NSControlStateValueOff);

    // SAFETY: radioButtonWithTitle_target_action is called on the main thread.
    let mode_lid_button = unsafe {
        NSButton::radioButtonWithTitle_target_action(
            &NSString::from_str(TITLE_MODE_LID),
            None,
            None,
            mtm,
        )
    };
    mode_lid_button.setState(NSControlStateValueOff);

    (mode_system_button, mode_display_button, mode_lid_button)
}

/// Builds the popover content view hierarchy and initializes all controls.
#[must_use]
#[allow(deprecated)]
pub fn build_content_view(mtm: MainThreadMarker) -> PopoverControls {
    let view_frame = CGRect::new(
        CGPoint::new(0.0, 0.0),
        CGSize::new(MENU_WIDTH, TOP_SECTION_HEIGHT),
    );
    let view = NSView::initWithFrame(mtm.alloc(), view_frame);

    let x = PADDING_OUTER;
    let width = CONTENT_WIDTH;
    let mut cursor = PADDING_OUTER;

    // 1. "Keep awake for" section header
    let duration_header =
        NSTextField::labelWithString(&NSString::from_str(TITLE_SECTION_DURATION), mtm);
    duration_header.setTextColor(Some(&NSColor::secondaryLabelColor()));
    let duration_header_top = cursor;
    cursor += LABEL_HEIGHT + SPACING_HEADER_TO_ROW;

    // 2. Duration row: duration field + unit popup
    let duration_row_top = cursor;
    cursor += ROW_DURATION_HEIGHT + SPACING_INSIDE_SECTION;
    let (duration_field, unit_popup) =
        build_duration_row(mtm, x, width, duration_row_top, TOP_SECTION_HEIGHT);
    view.addSubview(&duration_field);
    view.addSubview(&unit_popup);

    // 3. "No time limit" checkbox
    // SAFETY: checkboxWithTitle_target_action is called on the main thread.
    let no_time_limit_button = unsafe {
        NSButton::checkboxWithTitle_target_action(
            &NSString::from_str(TITLE_NO_TIME_LIMIT),
            None,
            None,
            mtm,
        )
    };
    no_time_limit_button.setState(NSControlStateValueOff);
    let no_time_limit_top = cursor;
    cursor += CHECKBOX_HEIGHT + SPACING_SECTION;

    // 4. "While active" section header
    let mode_header = NSTextField::labelWithString(&NSString::from_str(TITLE_SECTION_MODE), mtm);
    mode_header.setTextColor(Some(&NSColor::secondaryLabelColor()));
    let mode_header_top = cursor;
    cursor += LABEL_HEIGHT + SPACING_HEADER_TO_ROW;

    // 5-7. Awake mode radios
    let (mode_system_button, mode_display_button, mode_lid_button) = build_mode_radios(mtm);
    let mode_system_top = cursor;
    cursor += CHECKBOX_HEIGHT + SPACING_INSIDE_SECTION;
    let mode_display_top = cursor;
    cursor += CHECKBOX_HEIGHT + SPACING_INSIDE_SECTION;
    let mode_lid_top = cursor;
    cursor += CHECKBOX_HEIGHT + SPACING_SECTION;

    // 8. Start / Stop button
    // SAFETY: buttonWithTitle_target_action is called on the main thread.
    let start_stop_button = unsafe {
        NSButton::buttonWithTitle_target_action(&NSString::from_str(TITLE_START), None, None, mtm)
    };
    start_stop_button.setBezelStyle(NSBezelStyle::Rounded);
    let start_stop_top = cursor;
    // 9. Countdown / Status label
    let time_label = NSTextField::wrappingLabelWithString(&NSString::from_str(""), mtm);
    time_label.setTextColor(Some(&NSColor::secondaryLabelColor()));
    time_label.setPreferredMaxLayoutWidth(width);
    time_label.setSelectable(false);
    time_label.setHidden(true);
    time_label.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinYMargin);
    view.addSubview(&time_label);

    // 10. Error label
    let error_label = NSTextField::wrappingLabelWithString(&NSString::from_str(""), mtm);
    error_label.setTextColor(Some(&NSColor::systemRedColor()));
    error_label.setPreferredMaxLayoutWidth(width);
    error_label.setSelectable(false);
    error_label.setHidden(true);
    error_label.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinYMargin);
    view.addSubview(&error_label);

    let place = |v: &NSView, top: f64, h: f64| {
        let y = TOP_SECTION_HEIGHT - top - h;
        v.setFrame(CGRect::new(CGPoint::new(x, y), CGSize::new(width, h)));
        v.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinYMargin);
        view.addSubview(v);
    };

    place(&duration_header, duration_header_top, LABEL_HEIGHT);
    place(&no_time_limit_button, no_time_limit_top, CHECKBOX_HEIGHT);
    place(&mode_header, mode_header_top, LABEL_HEIGHT);
    place(&mode_system_button, mode_system_top, CHECKBOX_HEIGHT);
    place(&mode_display_button, mode_display_top, CHECKBOX_HEIGHT);
    place(&mode_lid_button, mode_lid_top, CHECKBOX_HEIGHT);
    place(&start_stop_button, start_stop_top, BUTTON_HEIGHT);

    let controls = PopoverControls {
        view,
        no_time_limit_button,
        duration_field,
        unit_popup,
        mode_system_button,
        mode_display_button,
        mode_lid_button,
        time_label,
        start_stop_button,
        error_label,
    };
    let _ = controls.layout_footer();
    controls
}
