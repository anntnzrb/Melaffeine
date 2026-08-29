use app::ui::{
    BUTTON_HEIGHT, CHECKBOX_HEIGHT, CONTROL_SPACING, COUNTDOWN_AT_SEPARATOR,
    COUNTDOWN_STOPS_IN_PREFIX, COUNTDOWN_TIMER_TOLERANCE, COUNTDOWN_UPDATE_INTERVAL, COUNTDOWN_Y,
    DEFAULT_DURATION_TEXT, DISPLAY_AWAKE_Y, DURATION_FIELD_WIDTH, DURATION_PLACEHOLDER, DURATION_Y,
    ERROR_DURATION_INVALID, ERROR_Y, ICON_ACTIVE, ICON_INACTIVE, INDEFINITE_Y, LABEL_HEIGHT,
    MENU_HEIGHT, MENU_PADDING, MENU_WIDTH, POPUP_HEIGHT, START_BUTTON_WIDTH, START_BUTTON_Y,
    TEXT_FIELD_HEIGHT, TITLE_KEEP_DISPLAY_AWAKE, TITLE_QUIT, TITLE_RUN_INDEFINITELY, TITLE_START,
    TITLE_STOP, UNIT_DAYS_INDEX, UNIT_DAYS_TITLE, UNIT_HOURS_INDEX, UNIT_HOURS_TITLE,
    UNIT_MINUTES_INDEX, UNIT_MINUTES_TITLE, UNIT_POPUP_WIDTH, UNIT_POPUP_Y,
};

#[test]
fn layout_dimensions() {
    assert_eq!(MENU_WIDTH, 260.0);
    assert_eq!(MENU_HEIGHT, 174.0);
    assert_eq!(MENU_PADDING, 16.0);
    assert_eq!(CONTROL_SPACING, 8.0);
    assert_eq!(DURATION_FIELD_WIDTH, 72.0);
    assert_eq!(CHECKBOX_HEIGHT, 24.0);
    assert_eq!(TEXT_FIELD_HEIGHT, 28.0);
    assert_eq!(POPUP_HEIGHT, 32.0);
    assert_eq!(BUTTON_HEIGHT, 30.0);
    assert_eq!(LABEL_HEIGHT, 16.0);
    assert_eq!(INDEFINITE_Y, 136.0);
    assert_eq!(DURATION_Y, 102.0);
    assert_eq!(UNIT_POPUP_Y, 100.0);
    assert_eq!(DISPLAY_AWAKE_Y, 72.0);
    assert_eq!(COUNTDOWN_Y, 48.0);
    assert_eq!(START_BUTTON_Y, 14.0);
    assert_eq!(ERROR_Y, 0.0);
    assert_eq!(UNIT_POPUP_WIDTH, 116.0);
    assert_eq!(START_BUTTON_WIDTH, 228.0);
    assert_eq!(COUNTDOWN_UPDATE_INTERVAL, 60.0);
    assert_eq!(COUNTDOWN_TIMER_TOLERANCE, 15.0);
}

#[test]
fn string_constants() {
    assert_eq!(ICON_INACTIVE, "cup.and.saucer");
    assert_eq!(ICON_ACTIVE, "cup.and.saucer.fill");
    assert_eq!(TITLE_START, "Start");
    assert_eq!(TITLE_STOP, "Stop");
    assert_eq!(TITLE_QUIT, "Quit");
    assert_eq!(TITLE_RUN_INDEFINITELY, "Run indefinitely");
    assert_eq!(TITLE_KEEP_DISPLAY_AWAKE, "Keep display awake too");
    assert_eq!(DURATION_PLACEHOLDER, "Duration");
    assert_eq!(DEFAULT_DURATION_TEXT, "2");
    assert_eq!(UNIT_MINUTES_TITLE, "Minutes");
    assert_eq!(UNIT_HOURS_TITLE, "Hours");
    assert_eq!(UNIT_DAYS_TITLE, "Days");
    assert_eq!(
        ERROR_DURATION_INVALID,
        "Enter a whole number from 1 to 365 days."
    );
    assert_eq!(COUNTDOWN_STOPS_IN_PREFIX, "Stops in ");
    assert_eq!(COUNTDOWN_AT_SEPARATOR, " at ");
}

#[test]
fn unit_indices() {
    assert_eq!(UNIT_MINUTES_INDEX, 0);
    assert_eq!(UNIT_HOURS_INDEX, 1);
    assert_eq!(UNIT_DAYS_INDEX, 2);
}
