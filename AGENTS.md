# Repository Guidelines

## Project Overview

Melaffeine is a tiny native macOS menu-bar utility that prevents sleep using native IOKit power assertions. It is intentionally Objective-C/AppKit only: no Swift, no SwiftUI, no Xcode project, no package manager.

User-facing behavior:
- icon-only menu-bar app, no Dock icon
- left-click opens controls
- right-click shows Quit
- Start/Stop sleep prevention
- finite duration in hours/days or true indefinite mode
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
Scripts/   build/package/run scripts
*.app/     generated/committed local app bundle artifact
```

## Development Commands

From repo root:

```sh
Scripts/package_app.sh       # build, package, ad-hoc sign Melaffeine.app
Scripts/compile_and_run.sh   # kill running app, rebuild, launch
Scripts/launch.sh            # open existing Melaffeine.app
open Melaffeine.app          # manual launch
```

There is no test command currently.

## Code Conventions & Common Patterns

- Objective-C with ARC: `clang -Os -fobjc-arc`.
- Programmatic AppKit only. No XIB/storyboard.
- Constants:
  - UI strings: `CTTitleStart`, `CTTitleRunIndefinitely`, etc.
  - layout values: `CTMenuWidth`, `CTMenuPadding`, etc.
  - time values: `CTSecondsPerHour`, `CTSecondsPerDay`.
- State sync:
  - Do not read UI as source of truth except control values at Start time.
  - `PowerController.active` determines Start/Stop and status icon state.
  - Timer expiry must notify UI through `PowerControllerDidChangeNotification`.
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
Sources/Constants.m         strings, layout constants, time constants
project.env                 APP_NAME/BUNDLE_ID/MACOS_MIN_VERSION
Scripts/package_app.sh      clang build + app bundle + Info.plist + codesign
README.md                   high-signal user/build notes
```

## Runtime/Tooling Preferences

- Required platform: macOS 14+ currently configured.
- Required compiler/tooling: Apple Command Line Tools with `clang`, `codesign`, `open`.
- Build links frameworks: `Cocoa`, `IOKit`, `ServiceManagement`.
- Signing is local ad-hoc only.
- No Node/Bun/npm/SwiftPM/Xcode workflow.
- `project.env` is the script config source of truth.

Current script caveats:
- `SIGNING_MODE` is declared but signing is hardcoded ad-hoc.
- Bundle version is hardcoded in `Scripts/package_app.sh` heredoc.
- Adding/removing `.m` files requires updating the hardcoded clang source list.

## Testing & QA

There is no automated test suite yet. Required manual/smoke checks after changes:

```sh
Scripts/package_app.sh
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
- indefinite mode does not persist across relaunch
- Launch at Login checkbox reflects `SMAppService` state

Optional system-level assertion check:

```sh
pmset -g assertions
```

Performance expectation:
- idle CPU should be effectively zero
- app bundle should stay around ~100 KB unless new dependencies are added
