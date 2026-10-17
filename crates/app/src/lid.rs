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
//! The guardian enables sleep suppression before acknowledging readiness. Explicit
//! stop and normal drop close its input, wait for restoration, and reap it.
//!
//! # Uninstallation
//!
//! To remove the passwordless `sudoers` rule at any time, run:
//! ```sh
//! sudo rm /etc/sudoers.d/melaffeine
//! ```
use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, PipeReader, PipeWriter, Read};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, ChildStderr, ChildStdin, Command, ExitStatus, Stdio};
use std::thread::{self, JoinHandle};

use rustix::event::{PollFd, PollFlags, poll};
use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
use rustix::io::retry_on_intr;

/// Installs and verifies the scoped passwordless power-management authorization.
///
/// # Errors
/// Returns a user-facing message if UID lookup, administrator authorization, or verification fails.
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

    set_sleep_disabled(false).map_err(|e| format!("Authorization was not verified: {e}"))
}

/// Failure to establish or restore lid-closed sleep prevention.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LidError {
    /// The guardian could not be launched.
    #[error("couldn't spawn sleep guardian: {source}")]
    Spawn {
        /// Underlying process launch failure.
        #[source]
        source: io::Error,
    },
    /// Communication with or reaping of the guardian failed.
    #[error("{operation}: {source}")]
    Io {
        /// Operation that failed.
        operation: &'static str,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// The guardian did not acknowledge successful enablement.
    #[error("couldn't enable lid-closed mode: {detail}")]
    Enable {
        /// Guardian diagnostics.
        detail: String,
    },
    /// Normal sleep could not be restored.
    #[error("couldn't reset sleep suppression: {detail}")]
    Reset {
        /// Power command diagnostics.
        detail: String,
    },
    /// The current system sleep setting could not be queried.
    #[error("couldn't query sleep suppression: {detail}")]
    StaleQuery {
        /// Query diagnostics.
        detail: String,
        /// Process launch failure, when applicable.
        #[source]
        source: Option<io::Error>,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct PowerCommand {
    program: PathBuf,
    args: Vec<OsString>,
}

impl PowerCommand {
    #[cfg(feature = "test-util")]
    pub(crate) const fn isolated(program: PathBuf, args: Vec<OsString>) -> Self {
        Self { program, args }
    }

    pub(crate) fn production() -> Self {
        Self {
            program: PathBuf::from("/usr/bin/sudo"),
            args: ["-n", "/usr/bin/pmset", "-a", "disablesleep"]
                .map(OsString::from)
                .into(),
        }
    }
}

const GUARDIAN_SCRIPT: &str = r#"trap '' PIPE
"$@" 1 >/dev/null || exit 1
printf 'ready\n' || :
read -r _ || :
exec "$@" 0 >/dev/null
"#;

fn spawn_guardian(power: &PowerCommand) -> Result<Child, LidError> {
    Command::new("/bin/sh")
        .args(["-c", GUARDIAN_SCRIPT, "melaffeine-lid-guardian"])
        .arg(&power.program)
        .args(&power.args)
        .process_group(0)
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| LidError::Spawn { source })
}

fn command_detail(status: ExitStatus, stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        status.to_string()
    } else {
        trimmed.to_owned()
    }
}

#[derive(Debug)]
struct DiagnosticDrain {
    cancel: PipeWriter,
    worker: JoinHandle<io::Result<Vec<u8>>>,
}

impl DiagnosticDrain {
    fn join(self) -> thread::Result<io::Result<Vec<u8>>> {
        // EOF wakes poll independently of descendants retaining guardian stderr.
        drop(self.cancel);
        self.worker.join()
    }
}

fn diagnostic_read_result(result: io::Result<usize>, cancelled: bool) -> io::Result<Option<usize>> {
    match result {
        Ok(0) => Ok(None),
        Ok(count) => Ok(Some(count)),
        Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(Some(0)),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            Ok(if cancelled { None } else { Some(0) })
        }
        Err(error) => Err(error),
    }
}

fn drain_diagnostics(mut pipe: ChildStderr, cancel: &PipeReader) -> io::Result<Vec<u8>> {
    let flags = retry_on_intr(|| fcntl_getfl(&pipe))?;
    retry_on_intr(|| fcntl_setfl(&pipe, flags | OFlags::NONBLOCK))?;
    let mut retained = Vec::new();
    let mut buffer = [0; 4096];
    let mut cancelled = false;
    loop {
        if !cancelled {
            let mut descriptors = [
                PollFd::new(&pipe, PollFlags::IN | PollFlags::HUP),
                PollFd::new(cancel, PollFlags::IN),
            ];
            retry_on_intr(|| poll(&mut descriptors, None))?;
            let [_, cancellation] = descriptors;
            cancelled = cancellation
                .revents()
                .intersects(PollFlags::IN | PollFlags::HUP);
        }
        let count = match diagnostic_read_result(pipe.read(&mut buffer), cancelled)? {
            None => return Ok(retained),
            Some(0) => continue,
            Some(count) => count,
        };
        // Drain all bytes, retaining only a bounded diagnostic prefix. After
        // cancellation, nonblocking reads consume only immediately available data.
        retained.extend(
            buffer
                .iter()
                .take(count)
                .take(4096_usize.saturating_sub(retained.len())),
        );
    }
}

/// Active lid-closed mode, owned by a guardian that enables and restores sleep.
///
/// Call [`Self::stop`] to observe restoration failures before starting another session.
/// Dropping without stopping closes the pipe and waits for best-effort restoration,
/// logging failures. The guardian must never be killed: its final act restores sleep.
#[derive(Debug)]
pub struct LidSession {
    stdin: Option<ChildStdin>,
    guardian: Option<Child>,
    stderr: Option<DiagnosticDrain>,
}

impl LidSession {
    /// Starts a guardian and waits for its successful enablement acknowledgment.
    ///
    /// # Errors
    /// Returns a typed error if spawning, enabling, or communicating with the guardian fails.
    pub fn start() -> Result<Self, LidError> {
        Self::start_with(&PowerCommand::production())
    }

    pub(crate) fn start_with(power: &PowerCommand) -> Result<Self, LidError> {
        let (cancel_reader, cancel_writer) = io::pipe().map_err(|source| LidError::Io {
            operation: "couldn't create sleep guardian diagnostics cancellation pipe",
            source,
        })?;
        let mut child = spawn_guardian(power)?;
        let stdout = child.stdout.take();
        let stderr_pipe = child.stderr.take();
        let mut session = Self {
            stdin: child.stdin.take(),
            stderr: None,
            guardian: Some(child),
        };
        session.stderr = Some(DiagnosticDrain {
            cancel: cancel_writer,
            worker: thread::Builder::new()
                .name(String::from("sleep-guardian-stderr"))
                .spawn(move || {
                    stderr_pipe.map_or_else(
                        || Ok(Vec::new()),
                        |pipe| drain_diagnostics(pipe, &cancel_reader),
                    )
                })
                .map_err(|source| LidError::Io {
                    operation: "couldn't start sleep guardian diagnostics drain",
                    source,
                })?,
        });
        let mut line = String::new();

        let acknowledgment = stdout.map_or_else(
            || Err(io::Error::other("missing guardian stdout")),
            |stdout| BufReader::new(stdout).take(64).read_line(&mut line),
        );
        if acknowledgment.is_ok() && line == "ready\n" {
            return Ok(session);
        }
        let (status, stderr) = session.finish()?;
        let detail = match acknowledgment {
            Ok(_) => command_detail(status, &stderr),
            Err(error) => format!("{error}; {}", command_detail(status, &stderr)),
        };
        Err(LidError::Enable { detail })
    }

    /// Restores normal sleep and reaps the guardian before returning.
    ///
    /// # Errors
    /// Returns an error if restoration or guardian communication fails.
    pub fn stop(mut self) -> Result<(), LidError> {
        self.restore()
    }

    fn finish(&mut self) -> Result<(ExitStatus, Vec<u8>), LidError> {
        drop(self.stdin.take());
        let Some(mut guardian) = self.guardian.take() else {
            return Err(LidError::Io {
                operation: "missing sleep guardian",
                source: io::Error::other("guardian already reaped"),
            });
        };
        let status = guardian.wait().map_err(|source| LidError::Io {
            operation: "couldn't reap sleep guardian",
            source,
        });
        // Join even when wait fails: the diagnostic worker must not be detached.
        let stderr = self.stderr.take().map_or_else(
            || Ok(Vec::new()),
            |drain| {
                drain
                    .join()
                    .map_err(|_panic| LidError::Io {
                        operation: "couldn't join sleep guardian diagnostics drain",
                        source: io::Error::other("diagnostics drain panicked"),
                    })?
                    .map_err(|source| LidError::Io {
                        operation: "couldn't read sleep guardian diagnostics",
                        source,
                    })
            },
        );
        let status = status?;
        let stderr = stderr?;
        Ok((status, stderr))
    }

    fn restore(&mut self) -> Result<(), LidError> {
        let (status, stderr) = self.finish()?;
        if status.success() {
            Ok(())
        } else {
            Err(LidError::Reset {
                detail: command_detail(status, &stderr),
            })
        }
    }

    #[cfg(test)]
    fn guardian_pid(&self) -> Option<u32> {
        self.guardian.as_ref().map(Child::id)
    }

    #[cfg(test)]
    fn into_raw_parts(mut self) -> (Option<ChildStdin>, Option<DiagnosticDrain>, Child) {
        let stdin = self.stdin.take();
        let stderr = self.stderr.take();
        let child = self.guardian.take().expect("active guardian");
        (stdin, stderr, child)
    }
}

impl Drop for LidSession {
    fn drop(&mut self) {
        if self.guardian.is_some()
            && let Err(error) = self.restore()
        {
            tracing::error!(%error, "failed to restore normal sleep on drop");
        }
    }
}

/// Restores stale sleep suppression left by an earlier process.
///
/// # Errors
/// Returns an error if querying the setting or restoring normal sleep fails.
pub fn reset_if_stale() -> Result<(), LidError> {
    let output = Command::new("/usr/bin/pmset")
        .arg("-g")
        .stdin(Stdio::null())
        .output()
        .map_err(|source| LidError::StaleQuery {
            detail: source.to_string(),
            source: Some(source),
        })?;
    if !output.status.success() {
        return Err(LidError::StaleQuery {
            detail: command_detail(output.status, &output.stderr),
            source: None,
        });
    }
    if parse_sleep_disabled(&String::from_utf8_lossy(&output.stdout)) {
        set_sleep_disabled(false).map_err(|detail| LidError::Reset { detail })?;
    }
    Ok(())
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
    let power = PowerCommand::production();
    let output = Command::new(&power.program)
        .args(&power.args)
        .arg(flag)
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
    fn diagnostic_reads_retry_only_transient_errors() {
        for cancelled in [false, true] {
            assert_eq!(
                diagnostic_read_result(Err(io::Error::from(io::ErrorKind::Interrupted)), cancelled)
                    .expect("interrupted read must retry"),
                Some(0),
                "interruption must never lose diagnostics"
            );
            assert_eq!(
                diagnostic_read_result(Err(io::Error::from(io::ErrorKind::WouldBlock)), cancelled)
                    .expect("nonblocking read is not fatal"),
                if cancelled { None } else { Some(0) },
                "only cancellation allows an empty nonblocking pipe to finish"
            );
            for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::BrokenPipe] {
                assert_eq!(
                    diagnostic_read_result(Err(io::Error::from(kind)), cancelled)
                        .expect_err("fatal diagnostics error must propagate")
                        .kind(),
                    kind,
                    "fatal errors must not be mistaken for WouldBlock"
                );
            }
            assert_eq!(
                diagnostic_read_result(Ok(0), cancelled).expect("EOF completes drain"),
                None,
                "EOF must finish regardless of cancellation"
            );
            assert_eq!(
                diagnostic_read_result(Ok(17), cancelled).expect("read retains bytes"),
                Some(17),
                "available bytes must still be retained after cancellation"
            );
        }
    }

    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    struct FakePower {
        dir: PathBuf,
        state: PathBuf,
        command: PowerCommand,
    }

    impl FakePower {
        fn new() -> Self {
            // The counter only allocates unique names; it publishes no shared data.
            let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("melaffeine-lid-{}-{id}", std::process::id()));
            fs::create_dir(&dir).expect("create isolated fake power directory");
            let program = dir.join("power");
            let state = dir.join("state");
            fs::write(
                &program,
                r#"#!/bin/sh
state="$1"; value="$2"
if [ "$value" = 1 ]; then printf '%s\n' "$PPID" >> "$state.parent"; fi
if [ -f "$state.noisy-$value" ]; then head -c 1048576 /dev/zero >&2; fi
if [ -f "$state.fail-$value" ]; then
    echo "fake failure $value" >&2
    exit 1
fi
if [ "$value" = 0 ] && [ -f "$state.hold-stderr" ]; then
    sleep 30 >/dev/null &
    printf '%s' "$!" > "$state.holder"
fi
printf '%s' "$value" > "$state"
"#,
            )
            .expect("write fake power command");
            fs::set_permissions(&program, fs::Permissions::from_mode(0o700))
                .expect("make fake power command executable");
            let command = PowerCommand {
                program,
                args: vec![state.clone().into_os_string()],
            };
            Self {
                dir,
                state,
                command,
            }
        }

        fn assert_state(&self, value: &str) {
            assert_eq!(
                fs::read_to_string(&self.state).expect("read fake power state"),
                value,
                "unexpected fake power state"
            );
        }

        fn fail(&self, value: &str) -> PathBuf {
            let marker = self.dir.join(format!("state.fail-{value}"));
            fs::write(&marker, "").expect("create fake failure marker");
            marker
        }
    }

    impl Drop for FakePower {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.dir).expect("remove isolated fake power directory");
        }
    }

    fn noisy_operation(value: &'static str, fail: bool) {
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let fake = FakePower::new();
            fs::write(fake.dir.join(format!("state.noisy-{value}")), "")
                .expect("mark noisy command");
            if fail {
                fake.fail(value);
            }
            let result = LidSession::start_with(&fake.command).and_then(LidSession::stop);
            if result.is_ok() {
                fake.assert_state("0");
            }
            tx.send(result).expect("report noisy operation result");
        });
        let result = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("noisy guardian operation must complete within ten seconds");
        worker.join().expect("join noisy operation");
        if fail {
            let error = result.expect_err("noisy command must fail");
            match error {
                LidError::Enable { detail } => {
                    assert_eq!(value, "1", "only enable may return Enable");
                    assert!(detail.len() <= 4096, "enable diagnostics must be bounded");
                }
                LidError::Reset { detail } => {
                    assert_eq!(value, "0", "only reset may return Reset");
                    assert!(detail.len() <= 4096, "reset diagnostics must be bounded");
                }
                other @ (LidError::Spawn { .. }
                | LidError::Io { .. }
                | LidError::StaleQuery { .. }) => panic!("unexpected noisy command error: {other}"),
            }
        } else {
            result.expect("noisy successful command must complete");
        }
    }

    #[test]
    fn guardian_noisy_enable_completes() {
        noisy_operation("1", false);
    }

    #[test]
    fn guardian_noisy_enable_failure_is_bounded() {
        noisy_operation("1", true);
    }

    #[test]
    fn guardian_noisy_reset_failure_is_bounded() {
        noisy_operation("0", true);
    }

    struct StderrHolder {
        pid_path: PathBuf,
    }

    impl Drop for StderrHolder {
        fn drop(&mut self) {
            let pid = match fs::read_to_string(&self.pid_path) {
                Ok(pid) => pid,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return,
                Err(error) => panic!("read stderr holder PID: {error}"),
            };
            assert!(
                Command::new("/bin/kill")
                    .args(["-TERM", &pid])
                    .status()
                    .expect("terminate stderr holder")
                    .success(),
                "stderr holder must be terminated"
            );
        }
    }

    #[test]
    fn guardian_stop_does_not_wait_for_inherited_stderr() {
        let fake = FakePower::new();
        fs::write(fake.dir.join("state.hold-stderr"), "").expect("mark inherited stderr");
        let (tx, rx) = std::sync::mpsc::channel();
        thread::scope(|scope| {
            let holder = StderrHolder {
                pid_path: fake.dir.join("state.holder"),
            };
            let worker = scope.spawn(|| {
                let result = LidSession::start_with(&fake.command).and_then(LidSession::stop);
                tx.send(result)
                    .expect("report inherited stderr stop result");
            });
            let result = rx.recv_timeout(std::time::Duration::from_secs(10));
            // Release the inherited descriptor before joining even on watchdog failure.
            drop(holder);
            worker.join().expect("join inherited stderr operation");
            result
                .expect("stop must complete within ten seconds despite inherited stderr")
                .expect("reset must succeed despite inherited stderr");
        });
        fake.assert_state("0");
    }

    #[test]
    fn guardian_executes_enable_in_child() {
        let fake = FakePower::new();
        let session = LidSession::start_with(&fake.command).expect("start guardian");
        let parents =
            fs::read_to_string(fake.dir.join("state.parent")).expect("read enable parents");
        let parents: Vec<u32> = parents
            .lines()
            .map(|parent| parent.parse().expect("parse enable parent PID"))
            .collect();
        let guardian_pid = session.guardian_pid().expect("active guardian");
        assert_eq!(parents.len(), 1, "exactly one enable per session start");
        for parent in parents {
            assert_eq!(parent, guardian_pid, "guardian must execute every enable");
            assert_ne!(
                parent,
                std::process::id(),
                "parent must never enable before spawning guardian"
            );
        }
        session.stop().expect("restore and reap guardian");
    }

    #[test]
    fn guardian_start_stop_restores_sleep() {
        let fake = FakePower::new();
        let session = LidSession::start_with(&fake.command).expect("start guardian");
        fake.assert_state("1");
        session.stop().expect("stop and reap guardian");
        fake.assert_state("0");
    }

    #[test]
    fn guardian_enable_failure_is_reported_and_reaped() {
        let fake = FakePower::new();
        fake.fail("1");
        let error = LidSession::start_with(&fake.command).expect_err("enable must fail");
        assert!(
            matches!(error, LidError::Enable { ref detail } if detail.contains("fake failure 1")),
            "expected enable failure diagnostics: {error}"
        );
        assert!(
            !fake.state.exists(),
            "failed enable must never write active state"
        );
    }

    #[test]
    fn guardian_reset_failure_cannot_overtake_next_session() {
        let fake = FakePower::new();
        let session = LidSession::start_with(&fake.command).expect("start guardian");
        fake.assert_state("1");
        let marker = fake.fail("0");
        let error = session.stop().expect_err("reset must fail");
        assert!(
            matches!(error, LidError::Reset { ref detail } if detail.contains("fake failure 0")),
            "expected reset failure diagnostics: {error}"
        );
        fake.assert_state("1");
        fs::remove_file(marker).expect("remove reset failure marker");
        let next =
            LidSession::start_with(&fake.command).expect("start next guardian after reaping");
        fake.assert_state("1");
        next.stop().expect("stop next guardian");
        fake.assert_state("0");
    }

    #[test]
    fn guardian_parent_death_after_ack_restores_sleep() {
        let fake = FakePower::new();
        let session = LidSession::start_with(&fake.command).expect("start guardian");
        fake.assert_state("1");
        let (stdin, stderr, mut child) = session.into_raw_parts();
        drop(stdin);
        assert!(
            child.wait().expect("reap orphaned guardian").success(),
            "guardian reset failed"
        );
        stderr
            .expect("drain worker")
            .join()
            .expect("join drain")
            .expect("drain stderr");
        fake.assert_state("0");
    }

    #[test]
    fn guardian_parent_death_before_ack_restores_sleep() {
        let fake = FakePower::new();
        let mut child = spawn_guardian(&fake.command).expect("spawn guardian");
        drop(child.stdin.take());
        drop(child.stdout.take());
        drop(child.stderr.take());
        assert!(
            child
                .wait()
                .expect("reap unacknowledged guardian")
                .success(),
            "guardian must survive broken acknowledgment pipe and reset"
        );
        fake.assert_state("0");
    }

    #[test]
    fn guardian_drop_restores_sleep_before_returning() {
        let fake = FakePower::new();
        let session = LidSession::start_with(&fake.command).expect("start guardian");
        fake.assert_state("1");
        drop(session);
        fake.assert_state("0");
    }

    #[test]
    fn test_sudoers_rule_content() {
        let rule = sudoers_rule(501);
        assert_eq!(
            rule,
            "#501 ALL=(root) NOPASSWD: /usr/bin/pmset -a disablesleep 1, /usr/bin/pmset -a disablesleep 0",
            "Unexpected value for rule"
        );
    }

    #[test]
    fn test_build_osascript_install_script() {
        let script = build_osascript_install_script(501);
        assert!(
            script.contains("/usr/sbin/visudo -cf"),
            "Expected condition: script.contains(\"/usr/sbin/visudo -cf\")"
        );
        assert!(
            script.contains("/usr/bin/install -m 0440"),
            "Expected condition: script.contains(\"/usr/bin/install -m 0440\")"
        );
        assert!(
            script.contains("/etc/sudoers.d/melaffeine"),
            "Expected condition: script.contains(\"/etc/sudoers.d/melaffeine\")"
        );
        assert!(
            script.contains("with administrator privileges"),
            "Expected condition: script.contains(\"with administrator privileges\")"
        );
        assert!(
            script.contains("#501 ALL=(root) NOPASSWD:"),
            "Expected condition: script.contains(\"#501 ALL=(root) NOPASSWD:\")"
        );
    }

    #[test]
    fn test_parse_sleep_disabled() {
        assert!(
            parse_sleep_disabled(" SleepDisabled\t\t1"),
            "Expected condition: parse_sleep_disabled(\" SleepDisabled\\t\\t1\")"
        );
        assert!(
            !parse_sleep_disabled(" SleepDisabled\t\t0"),
            "Expected condition: !parse_sleep_disabled(\" SleepDisabled\\t\\t0\")"
        );
        assert!(
            parse_sleep_disabled("SleepDisabled 1"),
            "Expected condition: parse_sleep_disabled(\"SleepDisabled 1\")"
        );
        assert!(
            !parse_sleep_disabled("SleepDisabled 0"),
            "Expected condition: !parse_sleep_disabled(\"SleepDisabled 0\")"
        );
        assert!(
            !parse_sleep_disabled(" SleepDisabled"),
            "Expected condition: !parse_sleep_disabled(\" SleepDisabled\")"
        );
        assert!(
            !parse_sleep_disabled(" SleepDisabledOther 1"),
            "Expected condition: !parse_sleep_disabled(\" SleepDisabledOther 1\")"
        );
        assert!(
            parse_sleep_disabled(
                "System-wide power settings:\n SleepDisabled\t\t1\n autopoweroff 1"
            ),
            "Expected condition: parse_sleep_disabled( \"System-wide power settings:\\n SleepDisabled\\t\\t1\\n autopoweroff 1\" )"
        );
        assert!(
            !parse_sleep_disabled(
                "System-wide power settings:\n SleepDisabled\t\t0\n autopoweroff 1"
            ),
            "Expected condition: !parse_sleep_disabled( \"System-wide power settings:\\n SleepDisabled\\t\\t0\\n autopoweroff 1\" )"
        );
        assert!(
            !parse_sleep_disabled(""),
            "Expected condition: !parse_sleep_disabled(\"\")"
        );
        assert!(
            !parse_sleep_disabled("other settings 1"),
            "Expected condition: !parse_sleep_disabled(\"other settings 1\")"
        );
    }
}
