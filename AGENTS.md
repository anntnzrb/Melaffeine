# Repository Guidelines

## Project Overview

Melaffeine is a tiny native macOS menu-bar utility that prevents sleep using native IOKit power assertions. It is implemented in 100% Rust (Rust 2024 edition) using modern `objc2` bindings for AppKit, IOKit, and ServiceManagement. It contains no Objective-C, Swift, SwiftUI, or Xcode project files.

User-facing behavior:
- icon-only menu-bar app, no Dock icon (`LSUIElement = true` / `NSApplicationActivationPolicyAccessory`)
- left-click opens controls
- right-click shows Quit with no shortcut hint
- Start/Stop sleep prevention
- finite duration in minutes/hours/days or true indefinite mode
- optional display-awake mode
- launch at login support via `SMAppService`
- no persisted active state after quit/reboot

## Architecture & Data Flow

Melaffeine is structured as a two-crate Cargo workspace:
- `crates/app-core`: Pure domain logic with `#![forbid(unsafe_code)]`. Contains duration parsing (`DurationUnit`, `parse_duration`) and compact minute formatting (`format_compact_duration`).
- `crates/app`: Native macOS AppKit application binary (`Melaffeine`). Contains the `NSApplicationDelegate` lifecycle, UI view construction, `UiProjection` state modeling, `PowerController` assertion management, and Apple framework adapters (`IOKitProvider` and `SMAppService`).

High-level flow:

```text
main.rs
  -> NSApplication + AppDelegate (objc2 define_class!)
  -> AppDelegate owns NSStatusItem, NSPopover, controls, PowerController<IOKitProvider>
  -> PowerController acquires/releases IOPMAssertion via IOKitProvider (objc2-io-kit)
  -> UiProjection derives UI state deterministically from PowerController + control inputs
  -> AppDelegate updates status icon, button labels, countdown text, and input enabled states
```

Key patterns:
- `AppDelegate` is the UI and lifecycle coordinator, strictly constrained to the main thread with `MainThreadMarker` / `MainThreadOnly`.
- `PowerController` is generic over `AssertionProvider` (RAII handle drop semantics) and manages active session timestamps and display mode.
- `UiProjection` computes UI presentation state purely and deterministically from model state.
- Unsafe code is strictly forbidden in `app-core` and isolated to narrow, documented Apple framework adapters in `app`.
- No `Arc<Mutex<_>>`, no async runtime, no thread pools. Main run loop timers (`NSTimer`) handle finite expiry and countdown ticks.
- No persisted runtime state. Active sessions die with the process.

## Key Directories

```text
crates/app-core/         Pure Rust duration domain logic and unit tests
crates/app/              Native macOS AppKit application, IOKit adapters, UI, and integration tests
Resources/               Checked-in Info.plist and app bundle metadata
nix/parts/               Modular flake parts (packages, checks, dev shell, toolchain, formatting)
```

## Development Commands

Do not invoke `cargo`, `rustc`, or `nix` directly; always use `just` recipes within the dev environment (`nix develop`).

From repo root:

```sh
just                         # show interactive menu of available recipes
just build                   # build release app bundle and ad-hoc sign
just check                   # run clippy, tests, and formatting checks
just test                    # run workspace tests via cargo-nextest
just coverage                # run workspace code coverage via cargo-llvm-cov
just format                  # format project sources via nix fmt
just run                     # build debug app bundle and launch Melaffeine.app
just clean                   # remove target, result, and generated bundle artifacts
```

## Code Conventions & Common Patterns

- Rust 2024 edition, workspace resolver 2.
- Strict lints: workspace-level `rust.lints` and `clippy.lints` with warnings denied in CI/checks; test exemptions configured in `clippy.toml`.
- Objective-C Runtime bindings:
  - Modern `objc2` ecosystem (`objc2`, `objc2-foundation`, `objc2-app-kit`, `objc2-io-kit`, `objc2-service-management`, `objc2-core-foundation`).
  - Do not introduce obsolete crates (`cocoa`, `objc`, `objc-foundation`, `objc-id`, `io-kit-sys`).
  - Use `MainThreadMarker` / `MainThreadOnly` for all AppKit UI interactions.
  - Retain cycles prevented via `Weak` delegate references in event monitor / timer block callbacks.
- State sync:
  - Do not read UI as source of truth except user inputs at Start time.
  - `PowerController.is_active()` determines Start/Stop title and status icon state (`cup.and.saucer` vs `cup.and.saucer.fill`).
  - Timer expiry stops `PowerController` and triggers UI projection update.
  - Finite countdown text is derived from `PowerController.ends_at()` and Foundation localized time; UI timer only runs while popover is open for an active finite session.
  - Duration input must parse strictly through `app_core::parse_duration` (checked positive integer, max 365 days / 525,600 minutes).
- Error handling:
  - `PowerController` returns `Result<(), PowerError>`.
  - UI displays errors inline through the red error label.
- Resource cleanup:
  - `IOKitAssertion` releases `IOPMAssertionID` on `Drop`.
  - Popover close removes global event monitors and invalidates countdown timer.
  - App termination cleans up assertions and invalidates timers.

## Important Files

```text
Cargo.toml                               Workspace manifest & lint configuration
clippy.toml                              Clippy configuration
crates/app-core/src/lib.rs               Core domain entry point
crates/app-core/src/duration.rs          Duration parsing and compact formatting
crates/app/src/main.rs                   App entry point & NSApplication bootstrap
crates/app/src/app_delegate.rs           NSApplicationDelegate & AppKit lifecycle
crates/app/src/ui.rs                     Popover UI layout & UiProjection
crates/app/src/power.rs                  PowerController & assertion session model
crates/app/src/iokit.rs                  IOKit IOPMAssertion adapter
crates/app/src/login.rs                  SMAppService login item adapter
Resources/Info.plist                     Bundle Info.plist definition
justfile                                 Primary command runner (POSIX /bin/sh)
flake.nix                                Pinned Nix flake using flake-parts, crane, fenix
nix/parts/                               Modular Nix flake definitions
```

## Runtime/Tooling Preferences

- Target platform: macOS 14+ (`MACOSX_DEPLOYMENT_TARGET = "14.0"`).
- Packaging via Crane and Fenix in Nix flake.
- Local bundle packaging via `just build` uses `Resources/Info.plist` and ad-hoc codesigning.
- Testing via `cargo nextest`.

## Testing & QA

Run automated workspace tests and checks:

```sh
just check
just test
```

Manual smoke checks after changes:

```sh
just build
just run
pgrep -x Melaffeine
pkill -x Melaffeine
```

Functional QA checklist:
- no Dock icon appears
- outline cup (`cup.and.saucer`) when off, filled cup (`cup.and.saucer.fill`) when on
- left-click opens aligned 260x174 popover
- click away closes popover
- right-click shows Quit with no shortcut hint
- Start creates assertion and button becomes Stop
- Stop releases assertion and button becomes Start
- finite duration auto-stops and UI/icon sync back to off
- finite active session shows remaining time and stop clock time in the popover
- finite duration accepts minutes/hours/days, rejects zero, negative, non-numeric, decimal, and excessive values (>365 days)
- indefinite mode does not persist across relaunch
- Launch at Login checkbox reflects `SMAppService` state
- `pmset -g assertions` confirms `PreventUserIdleSystemSleep` / `PreventUserIdleDisplaySleep`
