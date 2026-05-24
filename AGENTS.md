# Repository Guidelines

## Project Overview

Melaffeine is a tiny native macOS menu-bar utility that prevents sleep using native IOKit power assertions. It is intentionally Objective-C/AppKit only: no Swift, no SwiftUI, no Xcode project, no package manager.

User-facing behavior:
- icon-only menu-bar app, no Dock icon
- left-click opens controls
- right-click shows Quit
- Start/Stop sleep prevention
- finite duration in minutes/hours/days or true indefinite mode
- optional display-awake mode
- no persisted active state after quit/reboot

## Architecture & Data Flow

High-level flow:

```text
main.m
  -> NSApplication + AppDelegate
  -> AppDelegate owns status item, popover UI, PowerController
  -> PowerController owns IOPMAssertionID + optional NSTimer
  -> PowerController posts PowerControllerDidChangeNotification
  -> AppDelegate updateStateUI syncs button labels, enabled controls, status icon
```

Key patterns:
- `AppDelegate` is the UI/lifecycle coordinator.
- `PowerController` is the IOKit boundary and owns assertion cleanup.
- Constants live in `Constants.h/.m` with `CT` prefix.
- No persisted settings/state. Runtime state dies with the process.
- `LSUIElement=true` is generated into the app bundle; `main.m` also uses accessory activation policy.

## Key Directories

```text
Sources/   Objective-C source files
Tests/     tiny Objective-C test runners
*.app/     generated local app bundle artifact, ignored by git
```

## Development Commands

From repo root:

```sh
just build                   # build/package app bundle
just test                    # compile and run tiny duration logic tests
just run                     # build and launch
just open                    # open existing Melaffeine.app
just clean                   # remove generated app/test artifacts
nix develop                  # enter pinned dev shell with just/clang tools
nix build                    # build default Nix package: melaffeine
```

## Code Conventions & Common Patterns

- Objective-C with ARC: `clang -Os -fobjc-arc`.
- Programmatic AppKit only. No XIB/storyboard.
- Constants:
  - UI strings: `CTTitleStart`, `CTTitleRunIndefinitely`, etc.
  - layout values: `CTMenuWidth`, `CTMenuPadding`, etc.
  - time values and duration-domain constants: `Duration.h/.m`.
  - `justfile` recipes use POSIX `/bin/sh` and `printf`; do not add Bash-only syntax.
  - `justfile` intentionally must not invoke `nix`; enter `nix develop` first when reproducible tooling is needed.
  - Source autodiscovery convention: app `.m` files live in `Sources/`, tests live in `Tests/`; do not put scratch `.m` files in `Sources/`.
- State sync:
  - Do not read UI as source of truth except control values at Start time.
  - `PowerController.active` determines Start/Stop and status icon state.
  - Timer expiry must notify UI through `PowerControllerDidChangeNotification`.
  - Finite countdown UI is derived from `PowerController.endsAt`; do not run its UI timer while the popover is closed, inactive, or indefinite.
  - Duration input must parse as strict positive integer text and must not exceed `CTMaximumFiniteDurationSeconds`; do not rely on `NSTextField.doubleValue`.
  - Duration parsing, unit conversion, and compact countdown formatting belong in `Duration.m`, not `AppDelegate.m`.
- Error handling:
  - `PowerController` returns `BOOL` + `NSError **` for assertion creation failure.
  - UI displays errors through `errorLabel`.
- Resource cleanup:
  - Always release IOKit assertions in `PowerController -stop` and `-dealloc`.
  - Always remove global event monitors when popover closes.

## Important Files

```text
Sources/main.m              app entry point
Sources/AppDelegate.m       status item, popover UI, launch-at-login, state sync
Sources/PowerController.m   IOKit assertion lifecycle and timer expiry
Sources/Duration.m          strict duration parsing/conversion/formatting
Sources/Constants.m         strings, layout constants, time constants
Tests/DurationTests.m       tiny no-Xcode duration behavior test runner
justfile                    primary command runner
flake.nix                   pinned Nix dev shell
nix/flake/                  modular package/devShell/formatter definitions
flake.lock                  pinned Nix input lock
project.env                 APP_NAME/BUNDLE_ID/MACOS_MIN_VERSION
README.md                   high-signal user/build notes
```

## Runtime/Tooling Preferences

- Required platform: macOS 14+ currently configured.
- Required compiler/tooling: Apple Command Line Tools with `codesign` and `open`; `nix develop` provides reproducible `just`, `clang`, and `clang-tools`, and `justfile` auto-loads `project.env`.
- Build links frameworks: `Cocoa`, `IOKit`, `ServiceManagement`.
- Signing is local ad-hoc only.
- No Node/Bun/npm/SwiftPM/Xcode workflow.
- `project.env` is the script config source of truth.
- Default Nix package is `packages.<system>.melaffeine`, installed under `$out/Applications/Melaffeine.app`; Nix builds skip host `xattr`/`codesign` by overriding `just` variables.

## Testing & QA

Run the tiny automated tests before smoke checks:

```sh
just test
```

Required manual/smoke checks after changes:

```sh
just build
open Melaffeine.app
pgrep -x Melaffeine
pkill -x Melaffeine
```

Functional QA checklist:
- no Dock icon appears
- outline cup when off, filled cup when on
- left-click opens aligned popover
- click away closes popover
- right-click shows Quit with no shortcut hint
- Start creates assertion and button becomes Stop
- Stop releases assertion and button becomes Start
- finite duration auto-stops and UI/icon sync back to off
- finite active session shows remaining time and stop clock time in the popover
- finite duration accepts minutes/hours/days, rejects zero, negative, non-numeric, decimal, and excessive values
- indefinite mode does not persist across relaunch
- Launch at Login checkbox reflects `SMAppService` state

Optional system-level assertion check:

```sh
pmset -g assertions
```

Performance expectation:
- idle CPU should be effectively zero
- app bundle should stay around ~100 KB unless new dependencies are added
