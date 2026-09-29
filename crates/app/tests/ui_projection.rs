use app::ui::{
    ICON_ACTIVE, ICON_INACTIVE, TITLE_START, TITLE_STOP, UiProjection, compute_ui_projection,
};

#[test]
fn compute_ui_projection_table() {
    let inactive = |finite: bool| UiProjection {
        start_stop_title: TITLE_START,
        status_icon: ICON_INACTIVE,
        inputs_enabled: true,
        duration_enabled: finite,
        lid_enabled: finite,
        countdown_text: None,
    };
    let active = |countdown_text: Option<String>| UiProjection {
        start_stop_title: TITLE_STOP,
        status_icon: ICON_ACTIVE,
        inputs_enabled: false,
        duration_enabled: false,
        lid_enabled: false,
        countdown_text,
    };

    // (active, no_time_limit, countdown input) -> expected projection
    let cases = [
        (false, false, None, inactive(true)),
        (false, false, Some("Ignored"), inactive(true)),
        (false, true, None, inactive(false)),
        (false, true, Some("Ignored"), inactive(false)),
        (true, false, Some("Ends at 18:30 · 1h 59m left"), {
            let mut p = active(None);
            p.countdown_text = Some(String::from("Ends at 18:30 · 1h 59m left"));
            p
        }),
        (true, false, None, active(None)),
        (true, true, Some("Ends at 18:30 · 1h 59m left"), {
            let mut p = active(None);
            p.countdown_text = Some(String::from("Ends at 18:30 · 1h 59m left"));
            p
        }),
        (true, true, None, active(None)),
    ];

    for (is_active, no_time_limit, countdown, expected) in cases {
        assert_eq!(
            compute_ui_projection(is_active, no_time_limit, countdown.map(str::to_string)),
            expected
        );
    }
}
