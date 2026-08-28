# Melaffeine

Tiny native macOS menu-bar utility for keeping the Mac awake.

## What it does

- Left-click menu-bar icon: open controls.
- Right-click menu-bar icon: Quit.
- Start/Stop native IOKit sleep assertions.
- Duration: minutes, hours, days, or indefinite.
- Finite sessions show remaining time and stop clock time in the popover.
- Optional: keep display awake too.
- No persisted active state after reboot/relaunch.
- No Dock icon.
- Full CLI control via `melaffeine` command-line tool (Unix domain socket IPC).

## Dev Shell

```sh
nix develop
```

## Build & Test

```sh
just build                       # build release binary and package local Melaffeine.app
just test                        # run test suite with cargo-nextest
cargo nextest run --workspace    # run workspace tests directly
nix flake check                  # run Nix flake checks (clippy, tests, formatting)
nix build                        # build macOS app bundle via Nix & Crane
just cli status                    # query running status via CLI
just cli start 2h                  # start a 2-hour sleep prevention session via CLI
just cli stop                      # stop active session via CLI
```

## Stack

- Rust 2024
- AppKit `NSStatusItem` / `NSPopover` via `objc2`
- ServiceManagement: none (pure ephemeral runtime)
- Nix flake with Crane & Fenix
- `clap` CLI controller for scriptable sleep assertions
