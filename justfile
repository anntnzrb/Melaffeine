#!/usr/bin/env -S just --justfile

set shell := ["sh", "-cu"]

app_name := "Melaffeine"
app := app_name + ".app"
bin := app + "/Contents/MacOS/" + app_name
plist := app + "/Contents/Info.plist"

xattr := env("XATTR", "xattr -cr")
codesign := env("CODESIGN", "codesign --force --sign -")

alias b := build
alias c := check
alias t := test
alias r := run
alias fmt := format
alias cov := coverage

[default]
[private]
default:
    @just --list

# assemble app bundle from built binary and ad-hoc sign
[private]
bundle profile:
    rm -rf "{{ app }}"
    mkdir -p "{{ app }}/Contents/MacOS"
    cp "target/{{ profile }}/{{ app_name }}" "{{ bin }}"
    cp "Resources/Info.plist" "{{ plist }}"
    {{ xattr }} "{{ app }}"
    {{ codesign }} "{{ app }}"

# build release app bundle and ad-hoc sign
build:
    cargo build --release --package app --bin {{ app_name }}
    @just bundle release
    printf 'Created %s\n' "{{ app }}"

# run workspace tests via cargo-nextest
test *args:
    cargo nextest run --workspace --all-features {{ args }}

# run workspace code coverage via cargo-llvm-cov
coverage *args:
    cargo llvm-cov --workspace {{ args }}

# build debug app bundle and launch Melaffeine in the foreground
run:
    cargo build --package app --bin {{ app_name }}
    @just bundle debug
    "{{ bin }}"

# run Melaffeine CLI controller
cli *args:
    cargo run --package melaffeine-cli --bin melaffeine -- {{ args }}

# remove target, result, and generated bundle artifacts
clean:
    cargo clean
    rm -rf "{{ app }}" result

# format project sources via nix fmt
format:
    nix fmt

# run formatting check, cargo check, nextest, and clippy with denied warnings
check:
    nix fmt -- --fail-on-change
    cargo check --workspace --all-targets
    cargo nextest run --workspace --all-features
    cargo clippy --workspace --all-targets --all-features --locked -- --deny warnings

# run workspace checks and trusted governance; mutation testing is the separate mutants recipe
# Policy step 8 (mutants) is `just mutants`; step 10 (Miri) is unavailable in this shell and is reported as unrun.
gate base="origin/main":
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    cargo nextest run --workspace --all-features --locked
    uv run --script .gate/governance-check.py --doctests
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace --all-features --locked
    cargo deny --locked check
    cargo machete
    @set -eu; \
        tmp=$(mktemp); \
        trap 'rm -f "$tmp"' EXIT HUP INT TERM; \
        base_ref={{ quote(base) }}; base_ref="${base_ref#base=}"; \
        git show "$base_ref:.gate/governance-check.py" > "$tmp"; \
        uv run --script "$tmp" "$base_ref" --inventory

# test changed code with capped synthetic lints; the sandboxed AppKit harness flashes the menu bar
# Untracked filenames containing newlines are unsupported by the diff collection.
mutants base="origin/main":
    @set -eu; \
        tmp=$(mktemp -d); \
        trap 'rm -rf "$tmp"' EXIT HUP INT TERM; \
        printf '%s\n' '#!/bin/sh' 'exec "$@" --cap-lints=warn' > "$tmp/cap-lints-rustc"; \
        chmod 700 "$tmp/cap-lints-rustc"; \
        base_ref={{ quote(base) }}; base_ref="${base_ref#base=}"; \
        git diff "$base_ref" > "$tmp/diff"; \
        git ls-files --others --exclude-standard > "$tmp/untracked"; \
        while IFS= read -r f; do \
            git diff --no-index -- /dev/null "$f" >> "$tmp/diff" || { \
                status=$?; [ "$status" -eq 1 ] || exit "$status"; \
            }; \
        done < "$tmp/untracked"; \
        RUSTC_WORKSPACE_WRAPPER="$tmp/cap-lints-rustc" CARGO_TARGET_DIR="$tmp/mutants-target" \
            cargo mutants --in-diff "$tmp/diff" --test-tool nextest --output "$tmp"
