//! Lid-closed sleep prevention support for Melaffeine.
//!
//! # Mechanism
//!
//! macOS sleep prevention with the laptop lid closed requires toggling the
//! system-wide power management setting `/usr/bin/pmset -a disablesleep 1`.
//! This setting requires superuser (`root`) privileges.
//!
//! # One-Time `sudoers` Installation
//!
//! To avoid asking for credentials on every toggle or requiring paid Apple
//! developer signing identities, Melaffeine uses a one-time privilege
//! escalation via the macOS administrator authorization dialog (`/usr/bin/osascript`).
//! This installs a scoped `sudoers` drop-in file at `/etc/sudoers.d/melaffeine`.
//! The file grants the current user passwordless `sudo` (`NOPASSWD`) for exactly
//! two commands:
//! - `/usr/bin/pmset -a disablesleep 1`
//! - `/usr/bin/pmset -a disablesleep 0`
//!
//! The installation script creates a temporary file with `/usr/bin/mktemp`,
//! writes the rule with `/usr/bin/printf`, verifies syntax with `/usr/sbin/visudo -cf`,
//! and installs with `/usr/bin/install -m 0440 -o root -g wheel`.
//! If `visudo` validation fails, the script aborts without installing anything.
//!
//! # Crash Watchdog
//!
//! Because `pmset -a disablesleep 1` is a persistent system setting that outlives
//! processes, an active [`LidSession`] spawns a detached `/bin/sh` watchdog in
//! its own process group. The watchdog polls `/bin/kill -0 <pid>` every 2 seconds.
//! If Melaffeine terminates unexpectedly or crashes, the watchdog immediately runs
//! `/usr/bin/sudo -n /usr/bin/pmset -a disablesleep 0` to restore sleep behavior.
//!
//! When [`LidSession`] drops normally, it turns off `disablesleep` and kills/reaps
//! the watchdog process.
//!
//! # Uninstallation
//!
//! To remove the passwordless `sudoers` rule at any time, run:
//! ```sh
//! sudo rm /etc/sudoers.d/melaffeine
//! ```

use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};

/// True when the `sudoers` rule is installed and `sudo -n` can run the `pmset` commands without a password.
#[must_use]
pub fn is_authorized() -> bool {
    let Ok(status) = Command::new("/usr/bin/sudo")
        .args(["-n", "-l", "/usr/bin/pmset", "-a", "disablesleep", "1"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    else {
        return false;
    };
    status.success()
}

/// Shows the macOS administrator password dialog once and installs the sudoers rule. Blocks until the user answers.
/// Err(message) is user-facing and short (e.g. user cancelled, or not an administrator).
pub fn authorize() -> Result<(), String> {
    let username = current_username()?;
    let script = build_osascript_install_script(&username)?;

    let output = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(&script)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("Failed to launch osascript: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("-128") || output.status.code() == Some(-128) {
            return Err(String::from("Password prompt was cancelled."));
        }
        let trimmed = stderr.trim();
        if trimmed.is_empty() {
            return Err(String::from("Authorization failed."));
        }
        return Err(format!("Authorization failed: {trimmed}"));
    }

    if is_authorized() {
        Ok(())
    } else {
        Err(String::from("Authorization was not verified."))
    }
}

/// Active lid-closed mode. While alive, `disablesleep` is 1. `Drop` sets it back to 0 and stops the watchdog.
#[derive(Debug)]
pub struct LidSession {
    watchdog: Child,
}

impl LidSession {
    /// Enables `disablesleep` and spawns a watchdog that resets it if this process dies.
    pub fn start() -> Result<Self, String> {
        set_sleep_disabled(true)?;

        let pid = std::process::id();
        let watchdog_script = format!(
            "while /bin/kill -0 {pid} 2>/dev/null; do /bin/sleep 2; done; exec /usr/bin/sudo -n /usr/bin/pmset -a disablesleep 0"
        );

        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c")
            .arg(&watchdog_script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);

        let watchdog = match cmd.spawn() {
            Ok(child) => child,
            Err(spawn_err) => {
                if let Err(reset_err) = set_sleep_disabled(false) {
                    eprintln!(
                        "Failed to reset disablesleep after watchdog spawn failure: {reset_err}"
                    );
                }
                return Err(format!("Failed to spawn lid watchdog process: {spawn_err}"));
            }
        };

        Ok(Self { watchdog })
    }
}

impl Drop for LidSession {
    fn drop(&mut self) {
        if let Err(err) = set_sleep_disabled(false) {
            eprintln!("Failed to reset disablesleep on drop: {err}");
        }
        let _ = self.watchdog.kill();
        let _ = self.watchdog.wait();
    }
}

/// Called at app launch: if `SleepDisabled` is currently 1 (read via `pmset -g`, no root needed) and we are authorized, set it back to 0. Best-effort, silent.
pub fn reset_if_stale() {
    let Ok(output) = Command::new("/usr/bin/pmset")
        .arg("-g")
        .stdin(Stdio::null())
        .output()
    else {
        return;
    };

    if !output.status.success() {
        return;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if parse_sleep_disabled(&stdout) && is_authorized() {
        let _ = set_sleep_disabled(false);
    }
}

/// True when `NSProcessInfo` `thermalState` is `Serious` or `Critical`.
#[must_use]
pub fn thermal_too_hot() -> bool {
    let state = objc2_foundation::NSProcessInfo::processInfo().thermalState();
    matches!(
        state,
        objc2_foundation::NSProcessInfoThermalState::Serious
            | objc2_foundation::NSProcessInfoThermalState::Critical
    )
}

fn set_sleep_disabled(disabled: bool) -> Result<(), String> {
    let flag = if disabled { "1" } else { "0" };
    let output = Command::new("/usr/bin/sudo")
        .args(["-n", "/usr/bin/pmset", "-a", "disablesleep", flag])
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("Failed to execute sudo pmset: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let trimmed = stderr.trim();
        return Err(format!("sudo pmset failed: {trimmed}"));
    }
    Ok(())
}

fn current_username() -> Result<String, String> {
    let output = Command::new("/usr/bin/id")
        .arg("-un")
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("Failed to run /usr/bin/id: {err}"))?;

    if !output.status.success() {
        return Err(String::from(
            "Failed to determine current user with /usr/bin/id -un.",
        ));
    }

    let username_raw = String::from_utf8(output.stdout)
        .map_err(|err| format!("Username is not valid UTF-8: {err}"))?;
    let username = username_raw.trim();

    validate_username(username)?;
    Ok(username.to_owned())
}

fn validate_username(username: &str) -> Result<(), String> {
    if username.is_empty() {
        return Err(String::from("Username cannot be empty."));
    }
    if username.len() > 64 {
        return Err(String::from(
            "Username exceeds maximum length of 64 characters.",
        ));
    }
    let is_valid = username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-');
    if !is_valid {
        return Err(String::from("Username contains invalid characters."));
    }
    Ok(())
}

#[must_use]
fn sudoers_rule(username: &str) -> String {
    format!(
        "{username} ALL=(root) NOPASSWD: /usr/bin/pmset -a disablesleep 1, /usr/bin/pmset -a disablesleep 0"
    )
}

#[must_use]
fn quote_sh(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

#[must_use]
fn escape_for_applescript(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
    out
}

fn build_osascript_install_script(username: &str) -> Result<String, String> {
    validate_username(username)?;
    let rule = sudoers_rule(username);
    let quoted_rule = quote_sh(&rule);

    let sh_script = format!(
        "tmp=$(/usr/bin/mktemp /tmp/melaffeine.XXXXXX) || exit 1; \
         trap '/bin/rm -f \"$tmp\"' EXIT; \
         /usr/bin/printf '%s\\n' {quoted_rule} > \"$tmp\" || exit 1; \
         /usr/sbin/visudo -cf \"$tmp\" || exit 1; \
         /usr/bin/install -m 0440 -o root -g wheel \"$tmp\" /etc/sudoers.d/melaffeine || exit 1; \
         /bin/rm -f \"$tmp\"; \
         trap - EXIT"
    );

    let applescript_sh = escape_for_applescript(&sh_script);
    Ok(format!(
        "do shell script \"{applescript_sh}\" with administrator privileges with prompt \"Melaffeine needs your password once to keep your Mac awake with the lid closed.\""
    ))
}

#[must_use]
fn parse_sleep_disabled(output: &str) -> bool {
    for line in output.lines() {
        let mut words = line.split_whitespace();
        if words.next() == Some("SleepDisabled") {
            return words.next() == Some("1");
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_username_accepts_valid() {
        assert!(validate_username("annt").is_ok());
        assert!(validate_username("john.doe").is_ok());
        assert!(validate_username("a_b-1").is_ok());
    }

    #[test]
    fn test_validate_username_rejects_invalid() {
        assert!(validate_username("").is_err());
        assert!(validate_username(" ").is_err());
        assert!(validate_username("john doe").is_err());
        assert!(validate_username("\"").is_err());
        assert!(validate_username("'").is_err());
        assert!(validate_username(";").is_err());
        assert!(validate_username("user;evil").is_err());
        assert!(validate_username("$").is_err());
        assert!(validate_username("$USER").is_err());
        assert!(validate_username("\n").is_err());
        assert!(validate_username("user\n").is_err());
        let long_username = "a".repeat(65);
        assert!(validate_username(&long_username).is_err());
    }

    #[test]
    fn test_sudoers_rule_content() {
        let rule = sudoers_rule("annt");
        assert_eq!(
            rule,
            "annt ALL=(root) NOPASSWD: /usr/bin/pmset -a disablesleep 1, /usr/bin/pmset -a disablesleep 0"
        );
    }

    #[test]
    fn test_build_osascript_install_script_valid() {
        let script = build_osascript_install_script("annt").unwrap();
        assert!(script.contains("/usr/sbin/visudo -cf"));
        assert!(script.contains("/usr/bin/install -m 0440"));
        assert!(script.contains("/etc/sudoers.d/melaffeine"));
        assert!(script.contains("with administrator privileges"));
        assert!(script.contains("annt ALL=(root) NOPASSWD:"));
    }

    #[test]
    fn test_build_osascript_install_script_rejects_hostile() {
        assert!(build_osascript_install_script("annt; rm -rf /").is_err());
        assert!(build_osascript_install_script("root\nevil").is_err());
        assert!(build_osascript_install_script("").is_err());
        assert!(build_osascript_install_script("$USER").is_err());
        assert!(build_osascript_install_script("user\"name").is_err());
    }

    #[test]
    fn test_parse_sleep_disabled() {
        assert!(parse_sleep_disabled(" SleepDisabled\t\t1"));
        assert!(!parse_sleep_disabled(" SleepDisabled\t\t0"));
        assert!(parse_sleep_disabled("SleepDisabled 1"));
        assert!(!parse_sleep_disabled("SleepDisabled 0"));
        assert!(!parse_sleep_disabled(" SleepDisabled"));
        assert!(!parse_sleep_disabled(" SleepDisabledOther 1"));
        assert!(parse_sleep_disabled(
            "System-wide power settings:\n SleepDisabled\t\t1\n autopoweroff 1"
        ));
        assert!(!parse_sleep_disabled(
            "System-wide power settings:\n SleepDisabled\t\t0\n autopoweroff 1"
        ));
        assert!(!parse_sleep_disabled(""));
        assert!(!parse_sleep_disabled("other settings 1"));
    }
}
