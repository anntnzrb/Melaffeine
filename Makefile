.POSIX:

SHELL = /bin/sh

include project.env

VERSION = 0.1.0
BUILD_NUMBER = 1

CC = clang
RM = rm -f
RMDIR = rm -rf
MKDIR_P = mkdir -p
XATTR = xattr
CODESIGN = codesign
OPEN = open

APP = $(APP_NAME).app
BIN = $(APP)/Contents/MacOS/$(APP_NAME)
PLIST = $(APP)/Contents/Info.plist
TEST_BIN = Tests/DurationTests

APP_SRCS = \
	Sources/main.m \
	Sources/AppDelegate.m \
	Sources/PowerController.m \
	Sources/Constants.m \
	Sources/Duration.m

APP_HEADERS = \
	Sources/AppDelegate.h \
	Sources/PowerController.h \
	Sources/Constants.h \
	Sources/Duration.h

TEST_SRCS = \
	Sources/Duration.m \
	Tests/DurationTests.m

CFLAGS = -Os -fobjc-arc -mmacosx-version-min=$(MACOS_MIN_VERSION)
TEST_CFLAGS = $(CFLAGS) -Wall -Wextra -Werror
APP_LDLIBS = -framework Cocoa -framework IOKit -framework ServiceManagement
TEST_LDLIBS = -framework Foundation

all: app

app: $(APP_SRCS) $(APP_HEADERS) project.env FORCE
	$(RMDIR) "$(APP)"
	$(MKDIR_P) "$(APP)/Contents/MacOS"
	$(CC) $(CFLAGS) $(APP_SRCS) $(APP_LDLIBS) -o "$(BIN)"
	printf '%s\n' \
	'<?xml version="1.0" encoding="UTF-8"?>' \
	'<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">' \
	'<plist version="1.0">' \
	'<dict>' \
	'    <key>CFBundleName</key><string>$(APP_NAME)</string>' \
	'    <key>CFBundleDisplayName</key><string>$(APP_NAME)</string>' \
	'    <key>CFBundleIdentifier</key><string>$(BUNDLE_ID)</string>' \
	'    <key>CFBundleExecutable</key><string>$(APP_NAME)</string>' \
	'    <key>CFBundlePackageType</key><string>APPL</string>' \
	'    <key>CFBundleShortVersionString</key><string>$(VERSION)</string>' \
	'    <key>CFBundleVersion</key><string>$(BUILD_NUMBER)</string>' \
	'    <key>LSMinimumSystemVersion</key><string>$(MACOS_MIN_VERSION)</string>' \
	'    <key>LSUIElement</key><true/>' \
	'</dict>' \
	'</plist>' > "$(PLIST)"
	$(XATTR) -cr "$(APP)"
	$(CODESIGN) --force --sign - "$(APP)"
	printf 'Created %s\n' "$(APP)"

run: app FORCE
	$(OPEN) "$(APP)"

open: FORCE
	$(OPEN) "$(APP)"

test: $(TEST_BIN) FORCE
	./$(TEST_BIN)

$(TEST_BIN): $(TEST_SRCS) Sources/Duration.h
	$(CC) $(TEST_CFLAGS) $(TEST_SRCS) $(TEST_LDLIBS) -o "$(TEST_BIN)"

clean: FORCE
	$(RMDIR) "$(APP)" "$(TEST_BIN)"

FORCE: