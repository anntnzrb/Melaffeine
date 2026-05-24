set shell := ["sh", "-cu"]

app := "Melaffeine.app"
app_name := "Melaffeine"
bin := app + "/Contents/MacOS/" + app_name
plist := app + "/Contents/Info.plist"
test_bin := "Tests/DurationTests"

version := "0.1.0"
build_number := "1"
macos_min_version := "14.0"

objcflags := "-Os -fobjc-arc -mmacosx-version-min=" + macos_min_version
warnflags := "-Wall -Wextra -Werror"
app_sources := `printf '%s ' Sources/*.m`
test_sources := `printf '%s ' Sources/Duration.m Tests/*.m`
format_sources := `printf '%s ' Sources/*.h Sources/*.m Tests/*.m`
app_frameworks := "-framework Cocoa -framework IOKit -framework ServiceManagement"
test_frameworks := "-framework Foundation"

xattr := env("XATTR", "xattr -cr")
codesign := env("CODESIGN", "codesign --force --sign -")

[default]
build:
    rm -rf "{{ app }}"
    mkdir -p "{{ app }}/Contents/MacOS"
    clang {{ objcflags }} {{ app_sources }} {{ app_frameworks }} -o "{{ bin }}"
    printf '%s\n' \
      '<?xml version="1.0" encoding="UTF-8"?>' \
      '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">' \
      '<plist version="1.0">' \
      '<dict>' \
      '    <key>CFBundleName</key><string>{{ app_name }}</string>' \
      '    <key>CFBundleDisplayName</key><string>{{ app_name }}</string>' \
      '    <key>CFBundleIdentifier</key><string>dev.annt.melaffeine</string>' \
      '    <key>CFBundleExecutable</key><string>{{ app_name }}</string>' \
      '    <key>CFBundlePackageType</key><string>APPL</string>' \
      '    <key>CFBundleShortVersionString</key><string>{{ version }}</string>' \
      '    <key>CFBundleVersion</key><string>{{ build_number }}</string>' \
      '    <key>LSMinimumSystemVersion</key><string>{{ macos_min_version }}</string>' \
      '    <key>LSUIElement</key><true/>' \
      '</dict>' \
      '</plist>' > "{{ plist }}"
    {{ xattr }} "{{ app }}"
    {{ codesign }} "{{ app }}"
    printf 'Created %s\n' "{{ app }}"

test:
    clang {{ objcflags }} {{ warnflags }} {{ test_sources }} {{ test_frameworks }} -o "{{ test_bin }}"
    ./{{ test_bin }}

run: build
    open "{{ app }}"

open:
    open "{{ app }}"

clean:
    rm -rf "{{ app }}" "{{ test_bin }}"

format:
    clang-format -i {{ format_sources }}

format-check:
    clang-format --dry-run --Werror {{ format_sources }}

analyze:
    clang --analyze -Xclang -analyzer-output=text -Wno-unused-command-line-argument -fobjc-arc -mmacosx-version-min={{ macos_min_version }} {{ app_sources }}

check: format-check test analyze
