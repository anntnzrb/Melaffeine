//! Platform effects selected once at application initialization.

use std::cell::Cell;
use std::path::PathBuf;

use crate::lid::{self, LidError, LidSession, PowerCommand};

/// Whether startup may repair the system sleep setting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StaleRecovery {
    /// Recover stale system sleep suppression.
    System,
    /// Leave system settings untouched.
    Skip,
}

/// Whether administrator authorization is available.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Authorization {
    /// Use the macOS administrator prompt.
    System,
    /// Refuse authorization without launching any program.
    Unavailable,
}

/// Source of thermal safety decisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThermalSource {
    /// Read the system thermal state.
    System,
    /// Use a fixed too-hot decision.
    Fixed(bool),
}

/// Application paths and platform effects, retained across lifecycle initialization.
#[derive(Clone, Debug)]
pub struct AppEnvironment {
    ipc_socket: Option<PathBuf>,
    stale_recovery: StaleRecovery,
    power: PowerCommand,
    authorization: Authorization,
    thermal: Cell<ThermalSource>,
}

impl AppEnvironment {
    /// Selects the normal macOS paths, power commands, and safety checks.
    #[must_use]
    pub fn system() -> Self {
        let ipc_socket = match app_core::ipc::socket_path() {
            Ok(path) => Some(path),
            Err(error) => {
                tracing::warn!(%error, "failed to locate IPC socket");
                None
            }
        };
        Self {
            ipc_socket,
            stale_recovery: StaleRecovery::System,
            power: PowerCommand::production(),
            authorization: Authorization::System,
            thermal: Cell::new(ThermalSource::System),
        }
    }

    /// Selects a private socket, fake power command, no recovery or authorization,
    /// and a cool thermal state. The caller owns the directory and fake program.
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn isolated(
        dir: &std::path::Path,
        power_program: PathBuf,
        power_args: Vec<std::ffi::OsString>,
    ) -> Self {
        Self {
            ipc_socket: Some(dir.join("ipc.sock")),
            stale_recovery: StaleRecovery::Skip,
            power: PowerCommand::isolated(power_program, power_args),
            authorization: Authorization::Unavailable,
            thermal: Cell::new(ThermalSource::Fixed(false)),
        }
    }

    /// Sets the injected thermal source for subsequent safety checks.
    #[cfg(feature = "test-util")]
    pub fn set_thermal(&self, thermal: ThermalSource) {
        self.thermal.set(thermal);
    }

    pub(crate) fn ipc_socket(&self) -> Option<&std::path::Path> {
        self.ipc_socket.as_deref()
    }

    pub(crate) fn recover_stale(&self) -> Result<(), LidError> {
        match self.stale_recovery {
            StaleRecovery::System => lid::reset_if_stale(),
            StaleRecovery::Skip => Ok(()),
        }
    }

    pub(crate) fn start_lid(&self) -> Result<LidSession, LidError> {
        LidSession::start_with(&self.power)
    }

    pub(crate) fn authorize(&self) -> Result<(), String> {
        match self.authorization {
            Authorization::System => lid::authorize(),
            Authorization::Unavailable => Err(String::from(
                "Lid-closed authorization is unavailable in this environment.",
            )),
        }
    }

    pub(crate) fn thermal_too_hot(&self) -> bool {
        match self.thermal.get() {
            ThermalSource::System => lid::thermal_too_hot(),
            ThermalSource::Fixed(too_hot) => too_hot,
        }
    }
}
