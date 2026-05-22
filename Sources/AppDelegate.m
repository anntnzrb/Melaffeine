#import "AppDelegate.h"
#import "Constants.h"
#import "PowerController.h"
#import <ServiceManagement/ServiceManagement.h>

@interface AppDelegate ()
@property (nonatomic) PowerController *power;
@property (nonatomic) NSStatusItem *statusItem;
@property (nonatomic) NSPopover *popover;
@property (nonatomic) NSButton *startStopButton;
@property (nonatomic) NSButton *indefiniteButton;
@property (nonatomic) NSTextField *durationField;
@property (nonatomic) NSPopUpButton *unitPopup;
@property (nonatomic) NSButton *keepDisplayAwakeButton;
@property (nonatomic) NSButton *launchAtLoginButton;
@property (nonatomic) NSTextField *errorLabel;
@property (nonatomic, nullable) id outsideClickMonitor;
@end

@implementation AppDelegate

- (void)applicationDidFinishLaunching:(NSNotification *)notification {
    self.power = [PowerController new];
    self.statusItem = [NSStatusBar.systemStatusBar statusItemWithLength:NSVariableStatusItemLength];
    [self configureStatusItem];
    [self configurePopover];
    [self updateStateUI];
    [NSNotificationCenter.defaultCenter addObserver:self selector:@selector(powerDidChange:) name:PowerControllerDidChangeNotification object:self.power];

}

- (void)applicationWillTerminate:(NSNotification *)notification {
    [self.power stop];
}

- (void)applicationDidResignActive:(NSNotification *)notification {
    [self closePopover];
}
- (void)dealloc {
    [NSNotificationCenter.defaultCenter removeObserver:self];
    [self removeOutsideClickMonitor];
}


- (void)configureStatusItem {
    NSStatusBarButton *button = self.statusItem.button;
    button.target = self;
    button.action = @selector(statusItemClicked:);
    [button sendActionOn:NSEventMaskLeftMouseUp | NSEventMaskRightMouseUp];
    button.toolTip = CTAppName;
    button.imagePosition = NSImageOnly;
}

- (void)configurePopover {
    self.popover = [NSPopover new];
    self.popover.behavior = NSPopoverBehaviorTransient;
    self.popover.contentSize = NSMakeSize(CTMenuWidth, 150.0);


    NSViewController *controller = [NSViewController new];
    controller.view = [self buildContentView];
    self.popover.contentViewController = controller;
}

- (NSView *)buildContentView {
    NSView *view = [[NSView alloc] initWithFrame:NSMakeRect(0, 0, CTMenuWidth, 150.0)];
    CGFloat x = CTMenuPadding;
    CGFloat width = CTMenuWidth - (CTMenuPadding * 2.0);


    self.indefiniteButton = [NSButton checkboxWithTitle:CTTitleRunIndefinitely target:self action:@selector(controlChanged:)];
    self.indefiniteButton.frame = NSMakeRect(x, 112.0, width, 24.0);

    [view addSubview:self.indefiniteButton];

    self.durationField = [NSTextField textFieldWithString:@"2"];
    self.durationField.placeholderString = CTDurationPlaceholder;
    self.durationField.target = self;
    self.durationField.action = @selector(controlChanged:);
    self.durationField.frame = NSMakeRect(x, 78.0, CTDurationFieldWidth, 28.0);

    [view addSubview:self.durationField];

    self.unitPopup = [NSPopUpButton new];
    self.unitPopup.controlSize = NSControlSizeRegular;
    [self.unitPopup addItemsWithTitles:@[@"Hours", @"Days"]];
    self.unitPopup.frame = NSMakeRect(x + CTDurationFieldWidth + CTControlSpacing, 76.0, 116.0, 32.0);

    [view addSubview:self.unitPopup];

    self.keepDisplayAwakeButton = [NSButton checkboxWithTitle:CTTitleKeepDisplayAwake target:nil action:nil];
    self.keepDisplayAwakeButton.frame = NSMakeRect(x, 48.0, width, 24.0);

    [view addSubview:self.keepDisplayAwakeButton];

    self.startStopButton = [NSButton buttonWithTitle:CTTitleStart target:self action:@selector(startStopClicked:)];
    self.startStopButton.bezelStyle = NSBezelStyleRounded;
    self.startStopButton.frame = NSMakeRect(x, 14.0, 86.0, 30.0);

    [view addSubview:self.startStopButton];

    self.launchAtLoginButton = [NSButton checkboxWithTitle:CTTitleLaunchAtLogin target:self action:@selector(launchAtLoginChanged:)];
    self.launchAtLoginButton.frame = NSMakeRect(x + 98.0, 18.0, 160.0, 24.0);

    [view addSubview:self.launchAtLoginButton];

    self.errorLabel = [NSTextField labelWithString:@""];
    self.errorLabel.textColor = NSColor.systemRedColor;
    self.errorLabel.hidden = YES;
    self.errorLabel.frame = NSMakeRect(x, 0.0, width, 16.0);

    [view addSubview:self.errorLabel];

    return view;
}

- (void)statusItemClicked:(NSStatusBarButton *)sender {
    NSEventType eventType = NSApp.currentEvent.type;
    if (eventType == NSEventTypeRightMouseUp) {
        [self showContextMenu];
        return;
    }
    [self togglePopoverRelativeTo:sender];
}

- (void)togglePopoverRelativeTo:(NSStatusBarButton *)button {
    if (self.popover.shown) {
        [self closePopover];
        return;
    }

    [self updateStateUI];
    [NSApp activateIgnoringOtherApps:YES];
    [self.popover showRelativeToRect:button.bounds ofView:button preferredEdge:NSRectEdgeMinY];
    [self.popover.contentViewController.view.window makeFirstResponder:nil];
    [self installOutsideClickMonitor];
}

- (void)showContextMenu {
    NSMenu *menu = [NSMenu new];
    [menu addItem:[[NSMenuItem alloc] initWithTitle:CTTitleQuit action:@selector(quit:) keyEquivalent:@""]];
    self.statusItem.menu = menu;
    [self.statusItem.button performClick:nil];
    self.statusItem.menu = nil;
}

- (void)installOutsideClickMonitor {
    [self removeOutsideClickMonitor];
    __weak typeof(self) weakSelf = self;
    self.outsideClickMonitor = [NSEvent addGlobalMonitorForEventsMatchingMask:NSEventMaskLeftMouseDown | NSEventMaskRightMouseDown handler:^(NSEvent *event) {
        [weakSelf closePopover];
    }];
}

- (void)removeOutsideClickMonitor {
    if (self.outsideClickMonitor) {
        [NSEvent removeMonitor:self.outsideClickMonitor];
        self.outsideClickMonitor = nil;
    }
}

- (void)closePopover {
    [self removeOutsideClickMonitor];
    if (!self.popover.shown) { return; }
    [self.popover performClose:nil];
}

- (void)powerDidChange:(NSNotification *)notification {
    [self updateStateUI];
}

- (void)controlChanged:(id)sender {
    BOOL indefinite = self.indefiniteButton.state == NSControlStateValueOn;
    self.durationField.enabled = !indefinite && !self.power.active;
    self.unitPopup.enabled = !indefinite && !self.power.active;
}

- (void)startStopClicked:(id)sender {
    if (self.power.active) {
        [self.power stop];
        [self updateStateUI];
        return;
    }

    NSNumber *duration = [self selectedDuration];
    if (!duration && self.indefiniteButton.state != NSControlStateValueOn) {
        [self showError:@"Duration must be greater than zero."];
        return;
    }

    NSError *error = nil;
    BOOL ok = [self.power startWithDuration:duration
                           keepDisplayAwake:self.keepDisplayAwakeButton.state == NSControlStateValueOn
                                      error:&error];
    if (!ok) {
        [self showError:error.localizedDescription ?: @"Failed to start."];
        return;
    }

    [self updateStateUI];
}

- (NSNumber *)selectedDuration {
    if (self.indefiniteButton.state == NSControlStateValueOn) { return nil; }
    double value = self.durationField.doubleValue;
    if (!isfinite(value) || value <= 0.0) { return nil; }
    NSTimeInterval multiplier = self.unitPopup.indexOfSelectedItem == 0 ? CTSecondsPerHour : CTSecondsPerDay;
    return @(value * multiplier);
}

- (void)showError:(NSString *)message {
    self.errorLabel.stringValue = message;
    self.errorLabel.hidden = NO;
}

- (void)clearError {
    self.errorLabel.stringValue = @"";
    self.errorLabel.hidden = YES;
}

- (void)updateStateUI {
    [self clearError];
    BOOL active = self.power.active;
    self.startStopButton.title = active ? CTTitleStop : CTTitleStart;
    self.indefiniteButton.enabled = !active;
    self.durationField.enabled = !active && self.indefiniteButton.state != NSControlStateValueOn;
    self.unitPopup.enabled = !active && self.indefiniteButton.state != NSControlStateValueOn;
    self.keepDisplayAwakeButton.enabled = !active;
    [self syncLaunchAtLoginState];

    [self updateStatusIconActive:active];
}

- (void)updateStatusIconActive:(BOOL)active {
    NSImage *image = [NSImage imageWithSystemSymbolName:(active ? CTIconActive : CTIconInactive) accessibilityDescription:CTAppName];
    image.template = YES;
    self.statusItem.button.image = image;
    self.statusItem.button.title = @"";
}

- (void)launchAtLoginChanged:(id)sender {
    if (@available(macOS 13.0, *)) {
        NSError *error = nil;
        if (self.launchAtLoginButton.state == NSControlStateValueOn) {
            [[SMAppService mainAppService] registerAndReturnError:&error];
        } else {
            [[SMAppService mainAppService] unregisterAndReturnError:&error];
        }
        if (error) { [self showError:error.localizedDescription]; }
    }
}
- (void)syncLaunchAtLoginState {
    if (@available(macOS 13.0, *)) {
        self.launchAtLoginButton.state = [SMAppService mainAppService].status == SMAppServiceStatusEnabled
            ? NSControlStateValueOn
            : NSControlStateValueOff;
    }
}

- (void)quit:(id)sender {
    [NSApp terminate:nil];
}

@end
