use app::ui::{
    ICON_ACTIVE, ICON_INACTIVE, TITLE_START, TITLE_STOP, UiProjection, compute_ui_projection,
};

#[test]
fn inactive_finite_projection() {
    let projection = compute_ui_projection(false, false, None);
    assert_eq!(projection.start_stop_title, TITLE_START);
    assert_eq!(projection.status_icon, ICON_INACTIVE);
    assert!(projection.duration_enabled);
    assert!(projection.unit_enabled);
    assert!(projection.display_enabled);
    assert!(projection.indefinite_enabled);
    assert_eq!(projection.countdown_text, None);
}

#[test]
fn inactive_finite_projection_ignores_countdown_argument() {
    let projection = compute_ui_projection(false, false, Some("Ignored".to_string()));
    assert_eq!(projection.start_stop_title, TITLE_START);
    assert_eq!(projection.status_icon, ICON_INACTIVE);
    assert!(projection.duration_enabled);
    assert!(projection.unit_enabled);
    assert!(projection.display_enabled);
    assert!(projection.indefinite_enabled);
    assert_eq!(projection.countdown_text, None);
}

#[test]
fn inactive_indefinite_projection() {
    let projection = compute_ui_projection(false, true, None);
    assert_eq!(projection.start_stop_title, TITLE_START);
    assert_eq!(projection.status_icon, ICON_INACTIVE);
    assert!(!projection.duration_enabled);
    assert!(!projection.unit_enabled);
    assert!(projection.display_enabled);
    assert!(projection.indefinite_enabled);
    assert_eq!(projection.countdown_text, None);
}

#[test]
fn active_finite_with_countdown() {
    let countdown = Some("Stops in 2h at 3:00 PM".to_string());
    let projection = compute_ui_projection(true, false, countdown.clone());
    assert_eq!(projection.start_stop_title, TITLE_STOP);
    assert_eq!(projection.status_icon, ICON_ACTIVE);
    assert!(!projection.duration_enabled);
    assert!(!projection.unit_enabled);
    assert!(!projection.display_enabled);
    assert!(!projection.indefinite_enabled);
    assert_eq!(projection.countdown_text, countdown);
}

#[test]
fn active_finite_without_countdown() {
    let projection = compute_ui_projection(true, false, None);
    assert_eq!(projection.start_stop_title, TITLE_STOP);
    assert_eq!(projection.status_icon, ICON_ACTIVE);
    assert!(!projection.duration_enabled);
    assert!(!projection.unit_enabled);
    assert!(!projection.display_enabled);
    assert!(!projection.indefinite_enabled);
    assert_eq!(projection.countdown_text, None);
}

#[test]
fn active_indefinite_with_countdown_passed() {
    let countdown = Some("Stops in 2h at 3:00 PM".to_string());
    let projection = compute_ui_projection(true, true, countdown);
    assert_eq!(projection.start_stop_title, TITLE_STOP);
    assert_eq!(projection.status_icon, ICON_ACTIVE);
    assert!(!projection.duration_enabled);
    assert!(!projection.unit_enabled);
    assert!(!projection.display_enabled);
    assert!(!projection.indefinite_enabled);
    assert_eq!(projection.countdown_text, None);
}

#[test]
fn active_indefinite_without_countdown() {
    let projection = compute_ui_projection(true, true, None);
    assert_eq!(projection.start_stop_title, TITLE_STOP);
    assert_eq!(projection.status_icon, ICON_ACTIVE);
    assert!(!projection.duration_enabled);
    assert!(!projection.unit_enabled);
    assert!(!projection.display_enabled);
    assert!(!projection.indefinite_enabled);
    assert_eq!(projection.countdown_text, None);
}

#[test]
fn ui_projection_traits() {
    let proj1 = UiProjection {
        start_stop_title: TITLE_START,
        status_icon: ICON_INACTIVE,
        duration_enabled: true,
        unit_enabled: true,
        display_enabled: true,
        indefinite_enabled: true,
        countdown_text: None,
    };

    // Test Clone
    let proj2 = proj1.clone();

    // Test PartialEq and Eq (reflexivity and symmetry)
    assert_eq!(proj1, proj2);
    assert_eq!(proj2, proj1);

    // Test Debug formatting
    let debug_str = format!("{proj1:?}");
    assert!(debug_str.contains("UiProjection"));
    assert!(debug_str.contains("start_stop_title"));
    assert!(debug_str.contains("Start"));
    assert!(debug_str.contains("status_icon"));
    assert!(debug_str.contains("cup.and.saucer"));

    // Test inequality
    let proj_diff = compute_ui_projection(true, false, None);
    assert_ne!(proj1, proj_diff);

    let proj_with_countdown = compute_ui_projection(true, false, Some("10m".to_string()));
    assert_ne!(proj1, proj_with_countdown);
}
