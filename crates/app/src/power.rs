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
            ends_at,
        });
        Ok(())
    }

    /// Stops the active session and releases the power assertion handle.
    pub fn stop(&mut self) {
        self.session = None;
    }
}
