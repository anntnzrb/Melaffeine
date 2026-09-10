//! Detection of external running sleep-prevention applications.

use objc2_app_kit::NSRunningApplication;
use objc2_foundation::NSString;

/// Known third-party sleep prevention application bundle identifiers and names.
const COMPETING_APPS: &[(&str, &str)] = &[
    ("com.lightheadsw.caffeine", "Caffeine"),
    ("com.if.Amphetamine", "Amphetamine"),
    ("info.marcel-dierkes.KeepingYouAwake", "KeepingYouAwake"),
    ("com.lowtechguys.theine", "Theine"),
    ("com.maclife.lungo", "Lungo"),
];

/// Checks if any known third-party sleep prevention utility is currently running.
#[must_use]
pub fn detect_external_conflict() -> Option<String> {
    for (bundle_id, name) in COMPETING_APPS {
        let bundle_ns = NSString::from_str(bundle_id);
        let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&bundle_ns);
        if apps.count() > 0 {
            return Some((*name).to_string());
        }
    }
    None
}
