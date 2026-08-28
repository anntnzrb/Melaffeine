use app::ui::{UiProjection, compute_ui_projection};

#[test]
fn inactive_finite_projection() {
    let projection: UiProjection = compute_ui_projection(false, false, None);
    assert_eq!(projection.start_stop_title, "Start");
    assert_eq!(projection.status_icon, "cup.and.saucer");
    assert!(projection.duration_enabled);
    assert!(projection.unit_enabled);
    assert!(projection.display_enabled);
    assert!(projection.indefinite_enabled);
    assert_eq!(projection.countdown_text, None);
}

#[test]
fn inactive_indefinite_projection() {
    let projection: UiProjection = compute_ui_projection(false, true, None);
    assert_eq!(projection.start_stop_title, "Start");
    assert_eq!(projection.status_icon, "cup.and.saucer");
    assert!(!projection.duration_enabled);
    assert!(!projection.unit_enabled);
    assert!(projection.display_enabled);
    assert!(projection.indefinite_enabled);
    assert_eq!(projection.countdown_text, None);
}

#[test]
fn active_finite_projection() {
    let countdown = Some("Stops in 2h at 3:00 PM".to_string());
    let projection: UiProjection = compute_ui_projection(true, false, countdown.clone());
    assert_eq!(projection.start_stop_title, "Stop");
    assert_eq!(projection.status_icon, "cup.and.saucer.fill");
    assert!(!projection.duration_enabled);
    assert!(!projection.unit_enabled);
    assert!(!projection.display_enabled);
    assert!(!projection.indefinite_enabled);
    assert_eq!(projection.countdown_text, countdown);
}

#[test]
fn active_indefinite_projection() {
    let projection: UiProjection = compute_ui_projection(true, true, None);
    assert_eq!(projection.start_stop_title, "Stop");
    assert_eq!(projection.status_icon, "cup.and.saucer.fill");
    assert!(!projection.duration_enabled);
    assert!(!projection.unit_enabled);
    assert!(!projection.display_enabled);
    assert!(!projection.indefinite_enabled);
    assert_eq!(projection.countdown_text, None);
}
