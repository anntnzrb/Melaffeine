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

## Dev shell

```sh
nix develop
```

Provides `just`, `clang`, and `clang-tools`.
Nix flake internals live in `nix/flake/`; `flake.nix` is intentionally only the thin output orchestrator.
Run `just` commands inside that environment; `justfile` intentionally does not invoke `nix`.

## Build

```sh
just build
```


## Nix package

```sh
nix build .#melaffeine
```

The default package is also `melaffeine`:

```sh
nix build
```

## Run

```sh
just run
```

Open an existing bundle:

```sh
just open
```

## Clean

```sh
just clean
```

## Test

```sh
just test
```

## Check

```sh
just check
```

## Stack

- Objective-C
- AppKit `NSStatusItem` / `NSPopover`
- IOKit `IOPMAssertion`
- CLI build via `clang`
- No Xcode project
- No Swift / SwiftUI
