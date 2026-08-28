#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::duration_suboptimal_units,
    clippy::significant_drop_tightening,
    clippy::assert_is_empty
)]
use std::sync::{Arc, Mutex};
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
}

#[derive(Clone, Default)]
struct MockAssertionProvider {
    state: Arc<Mutex<MockState>>,
}

impl MockAssertionProvider {
    fn events(&self) -> Vec<MockEvent> {
        self.state.lock().unwrap().events.clone()
    }

    fn active_count(&self) -> usize {
        self.state.lock().unwrap().active_count
    }

    fn set_should_fail(&self, fail: bool) {
        self.state.lock().unwrap().should_fail = fail;
    }
}

struct MockHandle {
    id: usize,
    state: Arc<Mutex<MockState>>,
}

impl Drop for MockHandle {
    fn drop(&mut self) {
        let mut state = self.state.lock().unwrap();
        state.active_count = state.active_count.saturating_sub(1);
        state.events.push(MockEvent::Released(self.id));
    }
}

impl AssertionProvider for MockAssertionProvider {
    type Handle = MockHandle;

    fn acquire(&self, kind: AssertionKind) -> Result<Self::Handle, PowerError> {
        let mut state = self.state.lock().unwrap();
        if state.should_fail {
            return Err(PowerError::AcquisitionFailed(-1));
        }
        state.next_id += 1;
        let id = state.next_id;
        state.active_count += 1;
        state.events.push(MockEvent::Acquired(id, kind));
        Ok(MockHandle {
            id,
            state: Arc::clone(&self.state),
        })
    }
}

#[test]
fn start_prevent_system_sleep_sets_active_and_timestamps() {
    let mock = MockAssertionProvider::default();
    let mut controller = PowerController::new(mock.clone());
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let duration = Duration::from_secs(3600);

    let result = controller.start(Some(duration), false, now);
    assert!(result.is_ok());
    assert!(controller.is_active());
    assert!(!controller.keep_display_awake());
    assert_eq!(controller.started_at(), Some(now));
    assert_eq!(controller.ends_at(), Some(now + duration));

    let events = mock.events();
    assert_eq!(
        events,
        vec![MockEvent::Acquired(1, AssertionKind::PreventSystemSleep)]
    );
}

#[test]
fn start_prevent_display_sleep_maps_correct_kind() {
    let mock = MockAssertionProvider::default();
    let mut controller = PowerController::new(mock.clone());
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let duration = Duration::from_secs(1800);

    let result = controller.start(Some(duration), true, now);
    assert!(result.is_ok());
    assert!(controller.is_active());
    assert!(controller.keep_display_awake());
    assert_eq!(controller.started_at(), Some(now));
    assert_eq!(controller.ends_at(), Some(now + duration));

    let events = mock.events();
    assert_eq!(
        events,
        vec![MockEvent::Acquired(1, AssertionKind::PreventDisplaySleep)]
    );
}

#[test]
fn start_indefinite_has_no_ends_at() {
    let mock = MockAssertionProvider::default();
    let mut controller = PowerController::new(mock.clone());
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);

    let result = controller.start(None, false, now);
    assert!(result.is_ok());
    assert!(controller.is_active());
    assert!(!controller.keep_display_awake());
    assert_eq!(controller.started_at(), Some(now));
    assert_eq!(controller.ends_at(), None);

    let events = mock.events();
    assert_eq!(
        events,
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
    assert_eq!(controller.started_at(), Some(now2));
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
    assert_eq!(controller.started_at(), None);
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
fn failed_acquisition_leaves_controller_inactive() {
    let mock = MockAssertionProvider::default();
    mock.set_should_fail(true);
    let mut controller = PowerController::new(mock.clone());
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);

    let result = controller.start(Some(Duration::from_secs(3600)), false, now);
    assert!(result.is_err());
    assert!(!controller.is_active());
    assert!(!controller.keep_display_awake());
    assert_eq!(controller.started_at(), None);
    assert_eq!(controller.ends_at(), None);
    assert_eq!(mock.active_count(), 0);

    let events = mock.events();
    assert!(events.is_empty());
}
