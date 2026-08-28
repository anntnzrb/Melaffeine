//! macOS 13+ `SMAppService` wrapper for managing launch-at-login.

use objc2_service_management::{SMAppService, SMAppServiceStatus};

/// Service wrapper for managing application launch-at-login status via `SMAppService`.
pub struct LoginItemService;

impl LoginItemService {
    /// Checks whether launch-at-login is currently enabled.
    #[must_use]
    pub fn is_enabled() -> bool {
        // SAFETY: `mainAppService` and `status` are standard AppKit/ServiceManagement APIs safe to call on macOS 13+.
        let service = unsafe { SMAppService::mainAppService() };
        let status = unsafe { service.status() };
        status == SMAppServiceStatus::Enabled
    }

    /// Enables or disables launch-at-login for the current application bundle.
    ///
    /// # Errors
    ///
    /// Returns a localized error string if registration or unregistration fails.
    pub fn set_enabled(enabled: bool) -> Result<(), String> {
        // SAFETY: `mainAppService`, `registerAndReturnError`, and `unregisterAndReturnError` are standard
        // ServiceManagement APIs safe to call on macOS 13+ with proper error handling.
        let service = unsafe { SMAppService::mainAppService() };
        let res = if enabled {
            unsafe { service.registerAndReturnError() }
        } else {
            unsafe { service.unregisterAndReturnError() }
        };
        res.map_err(|err| err.localizedDescription().to_string())
    }
}
