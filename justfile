set shell := ["sh", "-cu"]

app_name := "Melaffeine"
app := app_name + ".app"
bin := app + "/Contents/MacOS/" + app_name
plist := app + "/Contents/Info.plist"

xattr := env("XATTR", "xattr -cr")
codesign := env("CODESIGN", "codesign --force --sign -")

[default]
build:
    cargo build --release --package app --bin {{ app_name }}
    rm -rf "{{ app }}"
    mkdir -p "{{ app }}/Contents/MacOS"
    cp "target/release/{{ app_name }}" "{{ bin }}"
    cp "Resources/Info.plist" "{{ plist }}"
    {{ xattr }} "{{ app }}"
    {{ codesign }} "{{ app }}"
    printf 'Created %s\n' "{{ app }}"

test:
    cargo nextest run --workspace

run: build
    open "{{ app }}"

open:
    open "{{ app }}"

clean:
    cargo clean
    rm -rf "{{ app }}"

format:
    nix fmt

check:
    cargo check --workspace --all-targets
    cargo nextest run --workspace
    cargo clippy --workspace --all-targets -- --deny warnings
