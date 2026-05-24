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

## Dev Shell

```sh
nix develop
```

## Build

```sh
just build
```
## Stack

- Objective-C
- AppKit `NSStatusItem` / `NSPopover`
- IOKit `IOPMAssertion`
- CLI build via `clang`
- No Xcode project
- No Swift / SwiftUI
