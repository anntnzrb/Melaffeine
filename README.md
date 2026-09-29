# Melaffeine

Tiny native macOS menu-bar utility for keeping the Mac awake.

## What it does

- Left-click menu-bar icon: open controls.
- Right-click menu-bar icon: Quit.
- Start/Stop native IOKit sleep assertions.
- Duration: minutes, hours, days, or No time limit.
- Finite sessions show remaining time and stop clock time in the popover ("Ends at <time> · <compact> left").
- Three exclusive modes while active: Let the screen turn off (default), Keep the screen on, or Keep running with lid closed (finite timers only).
- No persisted active state after reboot/relaunch.
- No Dock icon.
- Full CLI control via `melaffeine` command-line tool (Unix domain socket IPC).

## Keep running with lid closed

For finite timers, select the **Keep running with lid closed** option under **While active** before clicking **Start** so you can close the lid and put your MacBook in a bag while a job finishes. This option is disabled when **No time limit** is checked so the Mac cannot stay awake forever in a closed bag.

- **One-time setup prompt:** The first time you start a lid-closed session by clicking **Start** in the popover, macOS asks for an administrator password once to install `/etc/sudoers.d/melaffeine`, which grants passwordless access strictly to `/usr/bin/pmset -a disablesleep 1` and `/usr/bin/pmset -a disablesleep 0`. Subsequent runs do not prompt. The CLI never triggers the password prompt (if not authorized yet, it prompts you to click Start in the app once).
- **Crash watchdog:** While lid-closed mode is active, a detached watchdog process monitors Melaffeine via a pipe; on process termination or crash, the watchdog instantly restores normal sleep via pipe EOF (`read _`).
- **Heat cutoff:** While lid-closed mode is active, Melaffeine checks system thermal state every 30 seconds. If the Mac reaches a serious or critical thermal state, lid-closed mode is turned off automatically and reset to screen-off mode (the normal awake session continues, and the Mac will sleep if the lid is closed).
- **Network caveat:** Moving between locations or switching Wi-Fi networks while your Mac is in a bag will still drop non-resumable connections (such as plain SSH sessions).
- **Uninstall helper rule:** To remove the passwordless `pmset` rule at any time, run:
  ```sh
  sudo rm /etc/sudoers.d/melaffeine
  ```

## Installation

Install via Homebrew:

```sh
brew install --cask anntnzrb/tap/melaffeine
```

This installs `Melaffeine.app` into `/Applications` and symlinks the `melaffeine` CLI tool into your PATH.

## Dev Shell

```sh
nix develop
```

## Build & Test

```sh
just build           # build release binary and package local Melaffeine.app
just run             # build debug app bundle and launch Melaffeine
just test            # run test suite with cargo-nextest
just check           # run formatting check, cargo check, nextest, and clippy
just cli status      # query running status via CLI
just cli start 2h    # start a 2-hour sleep prevention session via CLI
just cli stop        # stop active session via CLI
```

## Stack

- Rust 2024
- AppKit `NSStatusItem` / `NSPopover` via `objc2`
- ServiceManagement: none (pure ephemeral runtime)
- Nix flake with Crane & Fenix (stable toolchain)
- `clap` CLI controller for scriptable sleep assertions
