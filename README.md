# Melaffeine

Tiny native macOS menu-bar utility for keeping the Mac awake.

## What it does

- Left-click menu-bar icon: open controls.
- Right-click menu-bar icon: Quit.
- Start/Stop native IOKit sleep assertions.
- Duration: hours, days, or indefinite.
- Optional: keep display awake too.
- No persisted active state after reboot/relaunch.
- No Dock icon.

## Build

```sh
Scripts/package_app.sh
```

## Run

```sh
open Melaffeine.app
```

Dev loop:

```sh
Scripts/compile_and_run.sh
```

## Stack

- Objective-C
- AppKit `NSStatusItem` / `NSPopover`
- IOKit `IOPMAssertion`
- CLI build via `clang`
- No Xcode project
- No Swift / SwiftUI
