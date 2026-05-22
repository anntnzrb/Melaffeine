#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
source "$ROOT/project.env"
CONF=${1:-release}
APP="$ROOT/${APP_NAME}.app"
BIN="$APP/Contents/MacOS/$APP_NAME"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
clang -Os -fobjc-arc \
  Sources/main.m \
  Sources/AppDelegate.m \
  Sources/PowerController.m \
  Sources/Constants.m \
  -framework Cocoa \
  -framework IOKit \
  -framework ServiceManagement \
  -o "$BIN"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>${APP_NAME}</string>
    <key>CFBundleDisplayName</key><string>${APP_NAME}</string>
    <key>CFBundleIdentifier</key><string>${BUNDLE_ID}</string>
    <key>CFBundleExecutable</key><string>${APP_NAME}</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>LSMinimumSystemVersion</key><string>${MACOS_MIN_VERSION}</string>
    <key>LSUIElement</key><true/>
</dict>
</plist>
PLIST
xattr -cr "$APP"
codesign --force --sign - "$APP"
echo "Created $APP"
