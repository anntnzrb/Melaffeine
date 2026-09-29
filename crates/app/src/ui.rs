//! UI projection model, layout constants, and programmatic `AppKit` view hierarchy.

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSBezelStyle, NSButton, NSColor, NSPopUpButton, NSTextField, NSView};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSArray, NSString};

/// UI dimensions and geometry constants.
pub const MENU_WIDTH: f64 = 260.0;
pub const MENU_HEIGHT: f64 = 174.0;
pub const MENU_PADDING: f64 = 16.0;
pub const CONTROL_SPACING: f64 = 8.0;
pub const DURATION_FIELD_WIDTH: f64 = 72.0;
pub const CHECKBOX_HEIGHT: f64 = 24.0;
pub const TEXT_FIELD_HEIGHT: f64 = 28.0;
pub const POPUP_HEIGHT: f64 = 32.0;
pub const BUTTON_HEIGHT: f64 = 30.0;
pub const LABEL_HEIGHT: f64 = 16.0;
pub const INDEFINITE_Y: f64 = 136.0;
pub const DURATION_Y: f64 = 102.0;
pub const UNIT_POPUP_Y: f64 = 100.0;
pub const DISPLAY_AWAKE_Y: f64 = 72.0;
pub const COUNTDOWN_Y: f64 = 48.0;
pub const START_BUTTON_Y: f64 = 14.0;
pub const ERROR_Y: f64 = 0.0;
pub const UNIT_POPUP_WIDTH: f64 = 116.0;
pub const START_BUTTON_WIDTH: f64 = 228.0;
pub const COUNTDOWN_UPDATE_INTERVAL: f64 = 60.0;
pub const COUNTDOWN_TIMER_TOLERANCE: f64 = 15.0;

/// String constants for UI titles, SF Symbols, and messages.
pub const ICON_INACTIVE: &str = "cup.and.saucer";
pub const ICON_ACTIVE: &str = "cup.and.saucer.fill";
pub const TITLE_START: &str = "Start";
pub const TITLE_STOP: &str = "Stop";
pub const TITLE_QUIT: &str = "Quit";
pub const TITLE_RUN_INDEFINITELY: &str = "Run indefinitely";
pub const TITLE_KEEP_DISPLAY_AWAKE: &str = "Keep display awake too";
pub const DURATION_PLACEHOLDER: &str = "Duration";
pub const DEFAULT_DURATION_TEXT: &str = "2";
pub const UNIT_MINUTES_TITLE: &str = "Minutes";
pub const UNIT_HOURS_TITLE: &str = "Hours";
pub const UNIT_DAYS_TITLE: &str = "Days";
pub const ERROR_DURATION_INVALID: &str = "Enter a whole number from 1 to 365 days.";
pub const COUNTDOWN_STOPS_IN_PREFIX: &str = "Stops in ";
pub const COUNTDOWN_AT_SEPARATOR: &str = " at ";

/// Selected unit index in the unit popup button.
pub const UNIT_MINUTES_INDEX: isize = 0;
pub const UNIT_HOURS_INDEX: isize = 1;
pub const UNIT_DAYS_INDEX: isize = 2;

/// A projection representing the state of all UI elements in the popover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiProjection {
    /// Title of the start/stop button ("Start" or "Stop").
    pub start_stop_title: &'static str,
    /// SF Symbol icon name for the status item ("cup.and.saucer" or "cup.and.saucer.fill").
    pub status_icon: &'static str,
    /// Whether checkboxes (indefinite, keep display awake) are enabled.
    pub inputs_enabled: bool,
    /// Whether duration input (field and unit popup) is enabled.
    pub duration_enabled: bool,
    /// Optional formatted countdown text if an active finite session is running.
    pub countdown_text: Option<String>,
}
/// Computes the declarative UI projection given the power controller state,
/// indefinite checkbox state, and optional formatted countdown text.
#[must_use]
pub fn compute_ui_projection(
    active: bool,
    indefinite: bool,
    countdown_text: Option<String>,
) -> UiProjection {
    if active {
        UiProjection {
            start_stop_title: TITLE_STOP,
            status_icon: ICON_ACTIVE,
            inputs_enabled: false,
            duration_enabled: false,
            countdown_text,
        }
    } else {
        UiProjection {
            start_stop_title: TITLE_START,
            status_icon: ICON_INACTIVE,
            inputs_enabled: true,
            duration_enabled: !indefinite,
            countdown_text: None,
        }
    }
}

/// Container owning all `AppKit` UI controls created for the popover content view.
pub struct PopoverControls {
    /// Root view containing all controls.
    pub view: Retained<NSView>,
    /// "Run indefinitely" checkbox.
    pub indefinite_button: Retained<NSButton>,
    /// Numeric duration input text field.
    pub duration_field: Retained<NSTextField>,
    /// Duration unit popup button (Minutes / Hours / Days).
    pub unit_popup: Retained<NSPopUpButton>,
    /// "Keep display awake too" checkbox.
    pub keep_display_awake_button: Retained<NSButton>,
    /// Countdown remaining time label.
    pub time_label: Retained<NSTextField>,
    /// Primary Start / Stop action button.
    pub start_stop_button: Retained<NSButton>,
    /// Error message label.
    pub error_label: Retained<NSTextField>,
}

/// Builds the popover content view hierarchy and initializes all controls.
#[must_use]
#[allow(deprecated)]
pub fn build_content_view(mtm: MainThreadMarker) -> PopoverControls {
    let view_frame = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(MENU_WIDTH, MENU_HEIGHT));
    let view = NSView::initWithFrame(mtm.alloc(), view_frame);

    let x = MENU_PADDING;
    let width = MENU_PADDING.mul_add(-2.0, MENU_WIDTH);
    let place = |v: &NSView, y: f64, w: f64, h: f64| {
        v.setFrame(CGRect::new(CGPoint::new(x, y), CGSize::new(w, h)));
        view.addSubview(v);
    };

    // 1. Run Indefinitely checkbox
    // SAFETY: checkboxWithTitle_target_action is called on the main thread.
    let indefinite_button = unsafe {
        NSButton::checkboxWithTitle_target_action(
            &NSString::from_str(TITLE_RUN_INDEFINITELY),
            None,
            None,
            mtm,
        )
    };
    place(&indefinite_button, INDEFINITE_Y, width, CHECKBOX_HEIGHT);

    // 2. Duration text field
    let duration_field =
        NSTextField::textFieldWithString(&NSString::from_str(DEFAULT_DURATION_TEXT), mtm);
    duration_field.setPlaceholderString(Some(&NSString::from_str(DURATION_PLACEHOLDER)));
    place(
        &duration_field,
        DURATION_Y,
        DURATION_FIELD_WIDTH,
        TEXT_FIELD_HEIGHT,
    );

    // 3. Unit popup button
    let unit_popup_frame = CGRect::new(
        CGPoint::new(x + DURATION_FIELD_WIDTH + CONTROL_SPACING, UNIT_POPUP_Y),
        CGSize::new(UNIT_POPUP_WIDTH, POPUP_HEIGHT),
    );
    let unit_popup = NSPopUpButton::initWithFrame_pullsDown(mtm.alloc(), unit_popup_frame, false);
    let unit_titles = NSArray::from_retained_slice(&[
        NSString::from_str(UNIT_MINUTES_TITLE),
        NSString::from_str(UNIT_HOURS_TITLE),
        NSString::from_str(UNIT_DAYS_TITLE),
    ]);
    unit_popup.addItemsWithTitles(&unit_titles);
    unit_popup.selectItemAtIndex(UNIT_HOURS_INDEX);
    view.addSubview(&unit_popup);

    // 4. Keep Display Awake checkbox
    // SAFETY: checkboxWithTitle_target_action is called on the main thread.
    let keep_display_awake_button = unsafe {
        NSButton::checkboxWithTitle_target_action(
            &NSString::from_str(TITLE_KEEP_DISPLAY_AWAKE),
            None,
            None,
            mtm,
        )
    };
    place(
        &keep_display_awake_button,
        DISPLAY_AWAKE_Y,
        width,
        CHECKBOX_HEIGHT,
    );

    // 5. Countdown time label
    let time_label = NSTextField::labelWithString(&NSString::from_str(""), mtm);
    time_label.setTextColor(Some(&NSColor::secondaryLabelColor()));
    time_label.setHidden(true);
    place(&time_label, COUNTDOWN_Y, width, LABEL_HEIGHT);

    // 6. Start / Stop button
    // SAFETY: buttonWithTitle_target_action is called on the main thread.
    let start_stop_button = unsafe {
        NSButton::buttonWithTitle_target_action(&NSString::from_str(TITLE_START), None, None, mtm)
    };
    start_stop_button.setBezelStyle(NSBezelStyle::Rounded);
    place(
        &start_stop_button,
        START_BUTTON_Y,
        START_BUTTON_WIDTH,
        BUTTON_HEIGHT,
    );

    // 7. Error label
    let error_label = NSTextField::labelWithString(&NSString::from_str(""), mtm);
    error_label.setTextColor(Some(&NSColor::systemRedColor()));
    error_label.setHidden(true);
    place(&error_label, ERROR_Y, width, LABEL_HEIGHT);

    PopoverControls {
        view,
        indefinite_button,
        duration_field,
        unit_popup,
        keep_display_awake_button,
        time_label,
        start_stop_button,
        error_label,
    }
}
