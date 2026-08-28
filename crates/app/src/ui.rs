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
pub const ERROR_START_FAILED: &str = "Failed to start.";
pub const COUNTDOWN_STOPS_IN_PREFIX: &str = "Stops in ";
pub const COUNTDOWN_AT_SEPARATOR: &str = " at ";

/// Selected unit index in the unit popup button.
pub const UNIT_MINUTES_INDEX: isize = 0;
pub const UNIT_HOURS_INDEX: isize = 1;
pub const UNIT_DAYS_INDEX: isize = 2;

/// A projection representing the state of all UI elements in the popover.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct UiProjection {
    /// Title of the start/stop button ("Start" or "Stop").
    pub start_stop_title: &'static str,
    /// SF Symbol icon name for the status item ("cup.and.saucer" or "cup.and.saucer.fill").
    pub status_icon: &'static str,
    /// Whether the duration input field is enabled.
    pub duration_enabled: bool,
    /// Whether the duration unit popup button is enabled.
    pub unit_enabled: bool,
    /// Whether the "Keep display awake too" checkbox is enabled.
    pub display_enabled: bool,
    /// Whether the "Run indefinitely" checkbox is enabled.
    pub indefinite_enabled: bool,
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
            duration_enabled: false,
            unit_enabled: false,
            display_enabled: false,
            indefinite_enabled: false,
            countdown_text: if indefinite { None } else { countdown_text },
        }
    } else {
        UiProjection {
            start_stop_title: TITLE_START,
            status_icon: ICON_INACTIVE,
            duration_enabled: !indefinite,
            unit_enabled: !indefinite,
            display_enabled: true,
            indefinite_enabled: true,
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
#[allow(clippy::too_many_lines, deprecated)]
pub fn build_content_view(mtm: MainThreadMarker) -> PopoverControls {
    let view_frame = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(MENU_WIDTH, MENU_HEIGHT));
    let view = NSView::initWithFrame(mtm.alloc(), view_frame);

    let x = MENU_PADDING;
    let width = MENU_PADDING.mul_add(-2.0, MENU_WIDTH);

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
    indefinite_button.setFrame(CGRect::new(
        CGPoint::new(x, INDEFINITE_Y),
        CGSize::new(width, CHECKBOX_HEIGHT),
    ));
    view.addSubview(&indefinite_button);

    // 2. Duration text field
    let duration_field =
        NSTextField::textFieldWithString(&NSString::from_str(DEFAULT_DURATION_TEXT), mtm);
    duration_field.setPlaceholderString(Some(&NSString::from_str(DURATION_PLACEHOLDER)));
    duration_field.setFrame(CGRect::new(
        CGPoint::new(x, DURATION_Y),
        CGSize::new(DURATION_FIELD_WIDTH, TEXT_FIELD_HEIGHT),
    ));
    view.addSubview(&duration_field);

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
    keep_display_awake_button.setFrame(CGRect::new(
        CGPoint::new(x, DISPLAY_AWAKE_Y),
        CGSize::new(width, CHECKBOX_HEIGHT),
    ));
    view.addSubview(&keep_display_awake_button);

    // 5. Countdown time label
    let time_label = NSTextField::labelWithString(&NSString::from_str(""), mtm);
    time_label.setTextColor(Some(&NSColor::secondaryLabelColor()));
    time_label.setHidden(true);
    time_label.setFrame(CGRect::new(
        CGPoint::new(x, COUNTDOWN_Y),
        CGSize::new(width, LABEL_HEIGHT),
    ));
    view.addSubview(&time_label);

    // 6. Start / Stop button
    // SAFETY: buttonWithTitle_target_action is called on the main thread.
    let start_stop_button = unsafe {
        NSButton::buttonWithTitle_target_action(&NSString::from_str(TITLE_START), None, None, mtm)
    };
    start_stop_button.setBezelStyle(NSBezelStyle::Rounded);
    start_stop_button.setFrame(CGRect::new(
        CGPoint::new(x, START_BUTTON_Y),
        CGSize::new(START_BUTTON_WIDTH, BUTTON_HEIGHT),
    ));
    view.addSubview(&start_stop_button);
    // 7. Error label
    let error_label = NSTextField::labelWithString(&NSString::from_str(""), mtm);
    error_label.setTextColor(Some(&NSColor::systemRedColor()));
    error_label.setHidden(true);
    error_label.setFrame(CGRect::new(
        CGPoint::new(x, ERROR_Y),
        CGSize::new(width, LABEL_HEIGHT),
    ));
    view.addSubview(&error_label);

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
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_test_compute_ui_projection() {
        let p_inactive_finite = compute_ui_projection(false, false, None);
        assert_eq!(p_inactive_finite.start_stop_title, TITLE_START);
        assert_eq!(p_inactive_finite.status_icon, ICON_INACTIVE);
        assert!(p_inactive_finite.duration_enabled);
        assert!(p_inactive_finite.unit_enabled);
        assert!(p_inactive_finite.display_enabled);
        assert!(p_inactive_finite.indefinite_enabled);
        assert_eq!(p_inactive_finite.countdown_text, None);

        let p_inactive_indefinite = compute_ui_projection(false, true, None);
        assert!(!p_inactive_indefinite.duration_enabled);
        assert!(!p_inactive_indefinite.unit_enabled);
        assert!(p_inactive_indefinite.display_enabled);
        assert!(p_inactive_indefinite.indefinite_enabled);

        let p_active_finite = compute_ui_projection(true, false, Some("1h".to_string()));
        assert_eq!(p_active_finite.start_stop_title, TITLE_STOP);
        assert_eq!(p_active_finite.status_icon, ICON_ACTIVE);
        assert!(!p_active_finite.duration_enabled);
        assert!(!p_active_finite.unit_enabled);
        assert!(!p_active_finite.display_enabled);
        assert!(!p_active_finite.indefinite_enabled);
        assert_eq!(p_active_finite.countdown_text, Some("1h".to_string()));

        let p_active_indefinite = compute_ui_projection(true, true, Some("1h".to_string()));
        assert_eq!(p_active_indefinite.countdown_text, None);
    }

    #[test]
    fn unit_test_build_content_view() {
        let mtm = unsafe { MainThreadMarker::new_unchecked() };
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
    }
}
