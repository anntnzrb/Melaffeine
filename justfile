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

# build release app bundle and ad-hoc sign
build:
    cargo build --release --package app --bin {{ app_name }}
    rm -rf "{{ app }}"
    mkdir -p "{{ app }}/Contents/MacOS"
    cp "target/release/{{ app_name }}" "{{ bin }}"
    cp "Resources/Info.plist" "{{ plist }}"
    {{ xattr }} "{{ app }}"
    {{ codesign }} "{{ app }}"
    printf 'Created %s\n' "{{ app }}"

# run workspace tests via cargo-nextest
test *args:
    cargo nextest run --workspace {{ args }}

[private]
refresh-lock:
    cargo metadata --format-version 1 >/dev/null

# run workspace code coverage via cargo-llvm-cov
coverage *args:
    cargo llvm-cov --workspace {{ args }}

# build debug app bundle and launch Melaffeine in the foreground
run:
    cargo build --package app --bin {{ app_name }}
    rm -rf "{{ app }}"
    mkdir -p "{{ app }}/Contents/MacOS"
    cp "target/debug/{{ app_name }}" "{{ bin }}"
    cp "Resources/Info.plist" "{{ plist }}"
    {{ xattr }} "{{ app }}"
    {{ codesign }} "{{ app }}"
    "{{ bin }}"

# run Melaffeine CLI controller
cli *args:
    cargo run --package melaffeine-cli --bin melaffeine -- {{ args }}

# configure git to use checked-in .githooks
install-hooks:
    git config core.hooksPath .githooks
    chmod +x .githooks/*
    printf 'Git hooks installed from .githooks\n'
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
    cargo nextest run --workspace
    cargo clippy --workspace --all-targets -- --deny warnings
