use std::error::Error;
use std::fmt;
use std::time::{Duration, SystemTime};

/// Types of power assertions that can be acquired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssertionKind {
    /// Prevent the system from sleeping while on battery or AC power.
    PreventSystemSleep,
    /// Prevent the display from turning off or sleeping.
    PreventDisplaySleep,
}

/// Errors that can occur during power assertion management.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PowerError {
    /// Failed to acquire the power assertion with the given return code.
    AcquisitionFailed(i32),
    /// The requested duration overflows the system clock.
    DurationOverflow,
}

impl fmt::Display for PowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AcquisitionFailed(code) => {
                write!(f, "Failed to acquire power assertion (code: {code})")
            }
            Self::DurationOverflow => {
                write!(
                    f,
                    "Requested duration exceeds the representable system time"
                )
            }
        }
    }
}

impl Error for PowerError {}

impl PowerError {
    /// Returns the underlying error code if available.
    #[must_use]
    pub const fn code(&self) -> Option<i32> {
        match self {
            Self::AcquisitionFailed(code) => Some(*code),
            Self::DurationOverflow => None,
        }
    }
}

/// Abstract provider capable of acquiring power assertions.
pub trait AssertionProvider {
    /// RAII handle representing an acquired assertion.
    type Handle;

    /// Acquires a power assertion of the specified kind.
    ///
    /// # Errors
    ///
    /// Returns `PowerError` if the underlying platform call fails.
    fn acquire(&self, kind: AssertionKind) -> Result<Self::Handle, PowerError>;
}

struct ActiveSession<H> {
    _handle: H,
    kind: AssertionKind,
    started_at: SystemTime,
    ends_at: Option<SystemTime>,
}

/// Coordinates power assertion lifecycle, state tracking, and session timestamps.
pub struct PowerController<P: AssertionProvider> {
    provider: P,
    session: Option<ActiveSession<P::Handle>>,
}

impl<P: AssertionProvider> fmt::Debug for PowerController<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PowerController")
            .field("is_active", &self.is_active())
            .field("keep_display_awake", &self.keep_display_awake())
            .field("started_at", &self.started_at())
            .field("ends_at", &self.ends_at())
            .finish()
    }
}

impl<P: AssertionProvider> PowerController<P> {
    /// Creates a new inactive `PowerController` using the provided assertion provider.
    #[must_use]
    pub const fn new(provider: P) -> Self {
        Self {
            provider,
            session: None,
        }
    }

    /// Returns whether a power assertion session is currently active.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.session.is_some()
    }

    /// Returns whether the active session prevents display sleep.
    #[must_use]
    pub fn keep_display_awake(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| session.kind == AssertionKind::PreventDisplaySleep)
    }

    /// Returns the timestamp when the current session was started, if active.
    #[must_use]
    pub fn started_at(&self) -> Option<SystemTime> {
        self.session.as_ref().map(|s| s.started_at)
    }

    /// Returns the timestamp when the current session will end, if finite and active.
    #[must_use]
    pub fn ends_at(&self) -> Option<SystemTime> {
        self.session.as_ref().and_then(|s| s.ends_at)
    }

    /// Starts a power assertion session.
    ///
    /// If an existing session is active, it is stopped (and its handle dropped)
    /// before attempting to acquire the new assertion.
    ///
    /// # Errors
    ///
    /// Returns `PowerError` if acquiring the assertion fails or the requested duration
    /// overflows the system clock (`PowerError::DurationOverflow`). On error, the controller
    /// remains inactive with no active timestamps.
    pub fn start(
        &mut self,
        duration: Option<Duration>,
        keep_display_awake: bool,
        now: SystemTime,
    ) -> Result<(), PowerError> {
        self.stop();
        let kind = if keep_display_awake {
            AssertionKind::PreventDisplaySleep
        } else {
            AssertionKind::PreventSystemSleep
        };
        let ends_at = duration
            .map(|d| now.checked_add(d).ok_or(PowerError::DurationOverflow))
            .transpose()?;
        let handle = self.provider.acquire(kind)?;
        self.session = Some(ActiveSession {
            _handle: handle,
            kind,
            started_at: now,
            ends_at,
        });
        Ok(())
    }

    /// Stops the active session and releases the power assertion handle.
    pub fn stop(&mut self) {
        self.session = None;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TestHandle(Arc<AtomicUsize>);
    impl Drop for TestHandle {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[derive(Clone, Default)]
    struct TestProvider {
        drop_count: Arc<AtomicUsize>,
        fail: bool,
    }

    impl AssertionProvider for TestProvider {
        type Handle = TestHandle;
        fn acquire(&self, _kind: AssertionKind) -> Result<Self::Handle, PowerError> {
            if self.fail {
                Err(PowerError::AcquisitionFailed(-1))
            } else {
                Ok(TestHandle(self.drop_count.clone()))
            }
        }
    }

    #[test]
    fn unit_test_power_controller_full_lifecycle() {
        let provider = TestProvider::default();
        let drop_count = provider.drop_count.clone();
        let mut controller = PowerController::new(provider);

        assert!(!controller.is_active());
        assert!(!controller.keep_display_awake());
        assert_eq!(controller.started_at(), None);
        assert_eq!(controller.ends_at(), None);

        let now = SystemTime::now();
        let dur = Duration::from_secs(3600);
        assert!(controller.start(Some(dur), true, now).is_ok());
        assert!(controller.is_active());
        assert!(controller.keep_display_awake());
        assert_eq!(controller.started_at(), Some(now));
        assert_eq!(controller.ends_at(), Some(now + dur));
        assert_eq!(controller.started_at(), Some(now));
        let active_dbg = format!("{controller:?}");
        assert!(active_dbg.contains("PowerController"));
        assert!(active_dbg.contains("is_active: true"));
        assert!(controller.start(None, false, now).is_ok());
        assert_eq!(drop_count.load(Ordering::SeqCst), 1);
        assert!(controller.is_active());
        assert!(!controller.keep_display_awake());
        assert_eq!(controller.ends_at(), None);

        // Stop
        controller.stop();
        assert_eq!(drop_count.load(Ordering::SeqCst), 2);
        assert!(!controller.is_active());

        // Idempotent stop
        controller.stop();
        assert_eq!(drop_count.load(Ordering::SeqCst), 2);

        // Failed start
        let fail_provider = TestProvider {
            drop_count: Arc::new(AtomicUsize::new(0)),
            fail: true,
        };
        let mut fail_controller = PowerController::new(fail_provider);
        assert!(fail_controller.start(None, false, now).is_err());
        assert!(!fail_controller.is_active());

        // Debug formatting
        let debug_repr = format!("{controller:?}");
        assert!(debug_repr.contains("PowerController"));
    }

    #[test]
    fn unit_test_iokit_power_controller() {
        use crate::iokit::IOKitProvider;
        let mut controller = PowerController::new(IOKitProvider);
        let dbg = format!("{controller:?}");
        assert!(dbg.contains("PowerController"));
        assert!(!controller.is_active());
        assert!(!controller.keep_display_awake());
        assert_eq!(controller.started_at(), None);
        assert_eq!(controller.ends_at(), None);
        let now = SystemTime::now();
        if controller
            .start(Some(Duration::from_secs(60)), true, now)
            .is_ok()
        {
            let active_dbg = format!("{controller:?}");
            assert!(active_dbg.contains("PowerController"));
            assert_eq!(controller.started_at(), Some(now));
            controller.stop();
        }
    }
    #[test]
    fn unit_test_power_error_and_kinds() {
        let err = PowerError::AcquisitionFailed(42);
        assert_eq!(err.code(), Some(42));
        assert_eq!(PowerError::DurationOverflow.code(), None);
        assert_eq!(
            format!("{}", PowerError::DurationOverflow),
            "Requested duration exceeds the representable system time"
        );
        assert!(format!("{err}").contains("42"));
        assert!(err.source().is_none());

        let err2 = err.clone();
        assert_eq!(err, err2);

        let kind = AssertionKind::PreventSystemSleep;
        let kind2 = AssertionKind::PreventDisplaySleep;
        assert_ne!(kind, kind2);
        assert_eq!(format!("{kind:?}"), "PreventSystemSleep");
        let kind3 = kind;
        assert_eq!(kind, kind3);
    }

    #[test]
    fn unit_test_power_controller_duration_overflow() {
        let provider = TestProvider::default();
        let mut controller = PowerController::new(provider);
        let now = SystemTime::now();

        assert_eq!(
            controller.start(Some(Duration::MAX), false, now),
            Err(PowerError::DurationOverflow)
        );
        assert!(!controller.is_active());
        assert_eq!(controller.started_at(), None);
        assert_eq!(controller.ends_at(), None);
    }
}
