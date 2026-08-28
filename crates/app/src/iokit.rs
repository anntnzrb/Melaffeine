//! `IOKit` power assertion implementation for macOS.

use crate::power::{AssertionKind, AssertionProvider, PowerError};
use objc2_core_foundation::CFString;

/// RAII handle wrapping an active `IOPMAssertionID`.
#[derive(Debug)]
pub struct IOKitAssertion {
    id: objc2_io_kit::IOPMAssertionID,
}

impl Drop for IOKitAssertion {
    fn drop(&mut self) {
        if self.id != 0 {
            // SAFETY: `self.id` is a valid, active assertion ID obtained from
            // `IOPMAssertionCreateWithName`. Releasing it once here tears down the assertion.
            let _ = objc2_io_kit::IOPMAssertionRelease(self.id);
            self.id = 0;
        }
    }
}

/// Production assertion provider backed by macOS `IOKit` power management APIs.
#[derive(Debug, Clone, Copy, Default)]
pub struct IOKitProvider;

impl AssertionProvider for IOKitProvider {
    type Handle = IOKitAssertion;

    fn acquire(&self, kind: AssertionKind) -> Result<Self::Handle, PowerError> {
        let type_name = match kind {
            AssertionKind::PreventSystemSleep => "PreventUserIdleSystemSleep",
            AssertionKind::PreventDisplaySleep => "PreventUserIdleDisplaySleep",
        };

        let type_cf = CFString::from_str(type_name);
        let name_cf = CFString::from_str("Melaffeine");
        let mut assertion_id: objc2_io_kit::IOPMAssertionID = 0;

        // SAFETY: `IOPMAssertionCreateWithName` is called with valid CFString pointers,
        // valid assertion level constant, and a valid mutable pointer to receive the assertion ID.
        let ret = unsafe {
            objc2_io_kit::IOPMAssertionCreateWithName(
                Some(&type_cf),
                objc2_io_kit::kIOPMAssertionLevelOn,
                Some(&name_cf),
                &raw mut assertion_id,
            )
        };

        if ret == objc2_io_kit::kIOReturnSuccess && assertion_id != 0 {
            Ok(IOKitAssertion { id: assertion_id })
        } else {
            Err(PowerError::AcquisitionFailed(ret))
        }
    }
}
