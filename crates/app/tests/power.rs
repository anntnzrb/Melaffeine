#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::duration_suboptimal_units,
    clippy::significant_drop_tightening,
    clippy::clone_on_copy,
    dead_code
)]
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use app::power::{AssertionKind, AssertionProvider, PowerController, PowerError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MockEvent {
    Acquired(usize, AssertionKind),
    Released(usize),
}

#[derive(Default)]
struct MockState {
    next_id: usize,
    active_count: usize,
    events: Vec<MockEvent>,
    should_fail: bool,
    fail_code: i32,
}

#[derive(Clone, Default)]
struct MockAssertionProvider {
    state: Rc<RefCell<MockState>>,
}

impl MockAssertionProvider {
    fn events(&self) -> Vec<MockEvent> {
        self.state.borrow().events.clone()
    }

    fn active_count(&self) -> usize {
        self.state.borrow().active_count
    }

    fn set_should_fail(&self, fail: bool) {
        self.state.borrow_mut().should_fail = fail;
    }

    fn set_fail_code(&self, code: i32) {
        let mut state = self.state.borrow_mut();
        state.should_fail = true;
        state.fail_code = code;
    }
}

struct MockHandle {
    id: usize,
    state: Rc<RefCell<MockState>>,
}

impl Drop for MockHandle {
    fn drop(&mut self) {
        let mut state = self.state.borrow_mut();
        state.active_count = state.active_count.saturating_sub(1);
        state.events.push(MockEvent::Released(self.id));
    }
}

impl AssertionProvider for MockAssertionProvider {
    type Handle = MockHandle;

    fn acquire(&self, kind: AssertionKind) -> Result<Self::Handle, PowerError> {
        let mut state = self.state.borrow_mut();
        if state.should_fail {
            return Err(PowerError::AcquisitionFailed(state.fail_code));
        }
        state.next_id += 1;
        let id = state.next_id;
        state.active_count += 1;
        state.events.push(MockEvent::Acquired(id, kind));
        Ok(MockHandle {
            id,
            state: Rc::clone(&self.state),
        })
    }
}

// ---------------------------------------------------------------------------
// PowerError Tests
// ---------------------------------------------------------------------------
#[test]
fn power_error_display_contains_code() {
    let err_neg1 = PowerError::AcquisitionFailed(-1);
    assert!(format!("{err_neg1}").contains("-1"));

    let err_0 = PowerError::AcquisitionFailed(0);
    assert!(format!("{err_0}").contains('0'));

    let err_100 = PowerError::AcquisitionFailed(100);
    assert!(format!("{err_100}").contains("100"));
}

// ---------------------------------------------------------------------------
// PowerController Tests
// ---------------------------------------------------------------------------

#[test]
fn power_controller_new_initializes_inactive_controller() {
    let mock = MockAssertionProvider::default();
    let controller = PowerController::new(mock.clone());

    assert!(!controller.is_active());
    assert!(!controller.keep_display_awake());
    assert_eq!(controller.ends_at(), None);
    assert_eq!(mock.active_count(), 0);
    assert!(mock.events().is_empty());
}

#[test]
fn start_configures_session_parameters() {
    let cases = [
        (
            Some(Duration::from_secs(3600)),
            false,
            Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_003_600)),
            AssertionKind::PreventSystemSleep,
        ),
        (
            Some(Duration::from_secs(1800)),
            true,
            Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_001_800)),
            AssertionKind::PreventDisplaySleep,
        ),
        (None, false, None, AssertionKind::PreventSystemSleep),
    ];

    for (duration, keep_display, expected_ends_at, expected_kind) in cases {
        let mock = MockAssertionProvider::default();
        let mut controller = PowerController::new(mock.clone());
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);

        let result = controller.start(duration, keep_display, now);
        assert!(result.is_ok());
        assert!(controller.is_active());
        assert_eq!(controller.keep_display_awake(), keep_display);
        assert_eq!(controller.ends_at(), expected_ends_at);

        let events = mock.events();
        assert_eq!(events, vec![MockEvent::Acquired(1, expected_kind)]);
    }
}

#[test]
fn start_with_zero_duration_is_active_with_ends_at_equal_to_start() {
    let mock = MockAssertionProvider::default();
    let mut controller = PowerController::new(mock.clone());
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);

    let result = controller.start(Some(Duration::ZERO), false, now);
    assert!(result.is_ok());
    assert!(controller.is_active());
    assert_eq!(controller.ends_at(), Some(now));
    assert_eq!(
        mock.events(),
        vec![MockEvent::Acquired(1, AssertionKind::PreventSystemSleep)]
    );
}

#[test]
fn start_replaces_existing_session_before_acquiring_new() {
    let mock = MockAssertionProvider::default();
    let mut controller = PowerController::new(mock.clone());
    let now1 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let now2 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_500);

    assert!(
        controller
            .start(Some(Duration::from_secs(3600)), false, now1)
            .is_ok()
    );
    assert_eq!(mock.active_count(), 1);

    assert!(
        controller
            .start(Some(Duration::from_secs(7200)), true, now2)
            .is_ok()
    );
    assert_eq!(mock.active_count(), 1);
    assert_eq!(controller.ends_at(), Some(now2 + Duration::from_secs(7200)));
    assert!(controller.keep_display_awake());

    // Proves old handle (1) is released before new (2) is acquired
    let events = mock.events();
    assert_eq!(
        events,
        vec![
            MockEvent::Acquired(1, AssertionKind::PreventSystemSleep),
            MockEvent::Released(1),
            MockEvent::Acquired(2, AssertionKind::PreventDisplaySleep),
        ]
    );
}

#[test]
fn stop_releases_assertion_and_clears_timestamps() {
    let mock = MockAssertionProvider::default();
    let mut controller = PowerController::new(mock.clone());
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);

    assert!(
        controller
            .start(Some(Duration::from_secs(3600)), false, now)
            .is_ok()
    );
    assert!(controller.is_active());
    assert_eq!(mock.active_count(), 1);

    controller.stop();
    assert!(!controller.is_active());
    assert!(!controller.keep_display_awake());
    assert_eq!(controller.ends_at(), None);
    assert_eq!(mock.active_count(), 0);

    let events = mock.events();
    assert_eq!(
        events,
        vec![
            MockEvent::Acquired(1, AssertionKind::PreventSystemSleep),
            MockEvent::Released(1),
        ]
    );
}

#[test]
fn stop_on_inactive_session_is_safe_noop() {
    let mock = MockAssertionProvider::default();
    let mut controller = PowerController::new(mock.clone());

    assert!(!controller.is_active());
    controller.stop();
    assert!(!controller.is_active());
    assert_eq!(controller.ends_at(), None);
    assert_eq!(mock.active_count(), 0);
    assert!(mock.events().is_empty());
}

#[test]
fn controller_drop_releases_active_assertion() {
    let mock = MockAssertionProvider::default();
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);

    {
        let mut controller = PowerController::new(mock.clone());
        assert!(
            controller
                .start(Some(Duration::from_secs(3600)), false, now)
                .is_ok()
        );
        assert!(controller.is_active());
        assert_eq!(mock.active_count(), 1);
    }

    assert_eq!(mock.active_count(), 0);
    let events = mock.events();
    assert_eq!(
        events,
        vec![
            MockEvent::Acquired(1, AssertionKind::PreventSystemSleep),
            MockEvent::Released(1),
        ]
    );
}

#[test]
fn controller_drop_on_inactive_is_safe_noop() {
    let mock = MockAssertionProvider::default();

    {
        let _controller = PowerController::new(mock.clone());
    }

    assert_eq!(mock.active_count(), 0);
    assert!(mock.events().is_empty());
}

#[test]
fn failed_acquisition_leaves_controller_inactive_and_returns_error() {
    let mock = MockAssertionProvider::default();
    mock.set_fail_code(-1);
    let mut controller = PowerController::new(mock.clone());
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);

    let result = controller.start(Some(Duration::from_secs(3600)), false, now);
    assert_eq!(result, Err(PowerError::AcquisitionFailed(-1)));
    assert!(!controller.is_active());
    assert!(!controller.keep_display_awake());
    assert_eq!(controller.ends_at(), None);
    assert_eq!(mock.active_count(), 0);

    let events = mock.events();
    assert!(events.is_empty());
}

#[test]
fn failed_acquisition_after_active_session_stops_previous_and_leaves_inactive() {
    let mock = MockAssertionProvider::default();
    let mut controller = PowerController::new(mock.clone());
    let now1 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let now2 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_500);

    assert!(
        controller
            .start(Some(Duration::from_secs(3600)), false, now1)
            .is_ok()
    );
    assert!(controller.is_active());

    // Make next acquisition fail with code 100
    mock.set_fail_code(100);

    let result = controller.start(Some(Duration::from_secs(1800)), true, now2);
    assert_eq!(result, Err(PowerError::AcquisitionFailed(100)));
    assert!(!controller.is_active());
    assert!(!controller.keep_display_awake());
    assert_eq!(controller.ends_at(), None);
    assert_eq!(mock.active_count(), 0);

    let events = mock.events();
    assert_eq!(
        events,
        vec![
            MockEvent::Acquired(1, AssertionKind::PreventSystemSleep),
            MockEvent::Released(1),
        ]
    );
}

#[test]
fn duration_overflow_leaves_controller_inactive() {
    let mock = MockAssertionProvider::default();
    let mut controller = PowerController::new(mock.clone());
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);

    let result = controller.start(Some(Duration::MAX), false, now);
    assert_eq!(result, Err(PowerError::DurationOverflow));
    assert!(!controller.is_active());
    assert_eq!(controller.ends_at(), None);
    assert_eq!(mock.active_count(), 0);
    assert!(mock.events().is_empty());
}
