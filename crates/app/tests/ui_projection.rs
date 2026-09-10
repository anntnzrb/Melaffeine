use app::ui::{
    ICON_ACTIVE, ICON_INACTIVE, TITLE_START, TITLE_STOP, UiProjection, compute_ui_projection,
};

#[test]
fn compute_ui_projection_table() {
    let inactive = |duration_enabled| UiProjection {
        start_stop_title: TITLE_START,
        status_icon: ICON_INACTIVE,
        duration_enabled,
        unit_enabled: duration_enabled,
        display_enabled: true,
        indefinite_enabled: true,
        countdown_text: None,
    };
    let active = |countdown_text: Option<String>| UiProjection {
        start_stop_title: TITLE_STOP,
        status_icon: ICON_ACTIVE,
        duration_enabled: false,
        unit_enabled: false,
        display_enabled: false,
        indefinite_enabled: false,
        countdown_text,
    };

    // (active, indefinite, countdown input) -> expected projection
    let cases = [
        (false, false, None, inactive(true)),
        (false, false, Some("Ignored"), inactive(true)),
        (false, true, None, inactive(false)),
        (true, false, Some("Stops in 2h at 3:00 PM"), {
            let mut p = active(None);
            p.countdown_text = Some(String::from("Stops in 2h at 3:00 PM"));
            p
        }),
        (true, false, None, active(None)),
        (true, true, Some("Stops in 2h at 3:00 PM"), active(None)),
        (true, true, None, active(None)),
    ];

    for (is_active, indefinite, countdown, expected) in cases {
        assert_eq!(
            compute_ui_projection(is_active, indefinite, countdown.map(str::to_string)),
            expected
        );
    }
}
