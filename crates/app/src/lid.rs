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
//! The file grants the user's numeric UID passwordless `sudo` (`NOPASSWD`) for exactly
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
//! its own process group that blocks reading from an anonymous pipe held by Melaffeine.
//! When the Melaffeine process terminates for any reason (clean exit, crash, SIGKILL,
//! SIGTERM, or force quit), the kernel closes the write end of the pipe. The watchdog's
//! `read` immediately encounters EOF and runs
//! `/usr/bin/sudo -n /usr/bin/pmset -a disablesleep 0` to restore normal sleep behavior.
//! This avoids periodic timer wakeups on battery and eliminates PID-reuse races.
//!
//! When [`LidSession`] drops normally, it turns off `disablesleep` via `pmset` and
//! closes the pipe write end, allowing the watchdog process to complete and exit cleanly.
//!
//! # Uninstallation
//!
//! To remove the passwordless `sudoers` rule at any time, run:
//! ```sh
//! sudo rm /etc/sudoers.d/melaffeine
//! ```
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStdin, Command, Stdio};

/// Err(message) is user-facing and short (e.g. user cancelled, or not an administrator).
pub fn authorize() -> Result<(), String> {
    let uid = current_uid()?;
    let script = build_osascript_install_script(uid);

    let output = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(&script)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("Failed to launch osascript: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("-128") {
            return Err(String::from("Password prompt was cancelled."));
        }
        let trimmed = stderr.trim();
        if trimmed.is_empty() {
            return Err(String::from("Authorization failed."));
        }
        return Err(format!("Authorization failed: {trimmed}"));
    }

    set_sleep_disabled(false).map_err(|_| String::from("Authorization was not verified."))
}

/// Active lid-closed mode. While alive, `disablesleep` is 1. `Drop` sets it back to 0 and releases the watchdog.
#[derive(Debug)]
pub struct LidSession {
    _stdin: ChildStdin,
    watchdog: Child,
}

impl LidSession {
    /// Enables `disablesleep` and spawns a watchdog that resets it if this process dies.
    pub fn start() -> Result<Self, String> {
        set_sleep_disabled(true)?;

        // Rust standard library creates pipes with O_CLOEXEC, so subsequent child
        // processes will not inherit this pipe's write end.
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c")
            .arg("read _ ; exec /usr/bin/sudo -n /usr/bin/pmset -a disablesleep 0")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);

        let mut child = match cmd.spawn() {
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

        let Some(stdin) = child.stdin.take() else {
            if let Err(reset_err) = set_sleep_disabled(false) {
                eprintln!("Failed to reset disablesleep after watchdog stdin missing: {reset_err}");
            }
            return Err(String::from("Failed to capture watchdog stdin pipe"));
        };

        Ok(Self {
            _stdin: stdin,
            watchdog: child,
        })
    }
}

impl Drop for LidSession {
    fn drop(&mut self) {
        if let Err(err) = set_sleep_disabled(false) {
            eprintln!("Failed to reset disablesleep on drop: {err}");
        }
        // Dropping `self._stdin` (which happens next automatically when this struct drops)
        // closes the pipe write end, causing the watchdog's `read _` to see EOF and run
        // its reset as a second attempt. We do not kill the watchdog.
        let _ = self.watchdog.try_wait();
    }
}

/// Called at app launch: if `SleepDisabled` is currently 1 (read via `pmset -g`, no root needed), set it back to 0. Best-effort, silent.
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
    if parse_sleep_disabled(&stdout) {
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

fn current_uid() -> Result<u32, String> {
    let output = Command::new("/usr/bin/id")
        .arg("-u")
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("Failed to run /usr/bin/id: {err}"))?;

    if !output.status.success() {
        return Err(String::from(
            "Failed to determine current user ID with /usr/bin/id -u.",
        ));
    }

    let uid_raw = String::from_utf8(output.stdout)
        .map_err(|err| format!("User ID is not valid UTF-8: {err}"))?;
    let uid_str = uid_raw.trim();

    uid_str
        .parse::<u32>()
        .map_err(|err| format!("Failed to parse user ID '{uid_str}': {err}"))
}

#[must_use]
fn sudoers_rule(uid: u32) -> String {
    format!(
        "#{uid} ALL=(root) NOPASSWD: /usr/bin/pmset -a disablesleep 1, /usr/bin/pmset -a disablesleep 0"
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

#[must_use]
fn build_osascript_install_script(uid: u32) -> String {
    let rule = sudoers_rule(uid);
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
    format!(
        "do shell script \"{applescript_sh}\" with administrator privileges with prompt \"Melaffeine needs your password once to keep your Mac awake with the lid closed.\""
    )
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
    fn test_sudoers_rule_content() {
        let rule = sudoers_rule(501);
        assert_eq!(
            rule,
            "#501 ALL=(root) NOPASSWD: /usr/bin/pmset -a disablesleep 1, /usr/bin/pmset -a disablesleep 0"
        );
    }

    #[test]
    fn test_build_osascript_install_script() {
        let script = build_osascript_install_script(501);
        assert!(script.contains("/usr/sbin/visudo -cf"));
        assert!(script.contains("/usr/bin/install -m 0440"));
        assert!(script.contains("/etc/sudoers.d/melaffeine"));
        assert!(script.contains("with administrator privileges"));
        assert!(script.contains("#501 ALL=(root) NOPASSWD:"));
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
