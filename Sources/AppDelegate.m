#import "AppDelegate.h"
#import "Constants.h"
#import "Duration.h"
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
@property (nonatomic) NSTextField *timeLabel;
@property (nonatomic, nullable) NSTimer *countdownTimer;
@property (nonatomic) NSDateFormatter *timeFormatter;
@property (nonatomic) NSNumberFormatter *durationFormatter;
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
    [self stopCountdownTimer];
}

- (void)applicationDidResignActive:(NSNotification *)notification {
    [self closePopover];
}
- (void)dealloc {
    [NSNotificationCenter.defaultCenter removeObserver:self];
    [self removeOutsideClickMonitor];
    [self stopCountdownTimer];
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
    self.popover.contentSize = NSMakeSize(CTMenuWidth, CTMenuHeight);


    NSViewController *controller = [NSViewController new];
    controller.view = [self buildContentView];
    self.popover.contentViewController = controller;
}

- (NSView *)buildContentView {
    NSView *view = [[NSView alloc] initWithFrame:NSMakeRect(0, 0, CTMenuWidth, CTMenuHeight)];
    CGFloat x = CTMenuPadding;
    CGFloat width = CTMenuWidth - (CTMenuPadding * 2.0);


    self.indefiniteButton = [NSButton checkboxWithTitle:CTTitleRunIndefinitely target:self action:@selector(controlChanged:)];
    self.indefiniteButton.frame = NSMakeRect(x, CTIndefiniteY, width, CTCheckboxHeight);

    [view addSubview:self.indefiniteButton];

    self.durationField = [NSTextField textFieldWithString:CTDefaultDurationText];
    self.durationField.placeholderString = CTDurationPlaceholder;
    self.durationField.target = self;
    self.durationField.action = @selector(controlChanged:);
    self.durationFormatter = [NSNumberFormatter new];
    self.durationFormatter.numberStyle = NSNumberFormatterNoStyle;
    self.durationFormatter.allowsFloats = NO;
    self.durationFormatter.minimum = @(CTMinimumDurationValue);
    self.durationFormatter.maximum = @(CTMaximumFiniteDurationSeconds / CTSecondsPerMinute);
    self.durationField.formatter = self.durationFormatter;
    self.durationField.frame = NSMakeRect(x, CTDurationY, CTDurationFieldWidth, CTTextFieldHeight);

    [view addSubview:self.durationField];

    self.unitPopup = [NSPopUpButton new];
    self.unitPopup.controlSize = NSControlSizeRegular;
    [self.unitPopup addItemsWithTitles:@[CTUnitMinutesTitle, CTUnitHoursTitle, CTUnitDaysTitle]];
    [self.unitPopup selectItemAtIndex:CTUnitHoursIndex];
    self.unitPopup.frame = NSMakeRect(x + CTDurationFieldWidth + CTControlSpacing, CTUnitPopupY, CTUnitPopupWidth, CTPopupHeight);

    [view addSubview:self.unitPopup];

    self.keepDisplayAwakeButton = [NSButton checkboxWithTitle:CTTitleKeepDisplayAwake target:nil action:nil];
    self.keepDisplayAwakeButton.frame = NSMakeRect(x, CTDisplayAwakeY, width, CTCheckboxHeight);

    [view addSubview:self.keepDisplayAwakeButton];

    self.timeLabel = [NSTextField labelWithString:@""];
    self.timeLabel.textColor = NSColor.secondaryLabelColor;
    self.timeLabel.hidden = YES;
    self.timeLabel.frame = NSMakeRect(x, CTCountdownY, width, CTLabelHeight);

    [view addSubview:self.timeLabel];

    self.startStopButton = [NSButton buttonWithTitle:CTTitleStart target:self action:@selector(startStopClicked:)];
    self.startStopButton.bezelStyle = NSBezelStyleRounded;
    self.startStopButton.frame = NSMakeRect(x, CTStartButtonY, CTStartButtonWidth, CTButtonHeight);

    [view addSubview:self.startStopButton];

    self.launchAtLoginButton = [NSButton checkboxWithTitle:CTTitleLaunchAtLogin target:self action:@selector(launchAtLoginChanged:)];
    self.launchAtLoginButton.frame = NSMakeRect(x + CTLaunchAtLoginXOffset, CTLaunchAtLoginY, CTLaunchAtLoginWidth, CTCheckboxHeight);

    [view addSubview:self.launchAtLoginButton];

    self.errorLabel = [NSTextField labelWithString:@""];
    self.errorLabel.textColor = NSColor.systemRedColor;
    self.errorLabel.hidden = YES;
    self.errorLabel.frame = NSMakeRect(x, CTErrorY, width, CTLabelHeight);

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
    [self startCountdownTimerIfNeeded];
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
    [self stopCountdownTimer];
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
        [self showError:CTErrorDurationInvalid];
        return;
    }

    NSError *error = nil;
    BOOL ok = [self.power startWithDuration:duration
                           keepDisplayAwake:self.keepDisplayAwakeButton.state == NSControlStateValueOn
                                      error:&error];
    if (!ok) {
        [self showError:error.localizedDescription ?: CTErrorStartFailed];
        return;
    }

    [self updateStateUI];
}

- (NSNumber *)selectedDuration {
    if (self.indefiniteButton.state == NSControlStateValueOn) { return nil; }
    return CTDurationSecondsFromInputString(self.durationField.stringValue,
                                            self.unitPopup.indexOfSelectedItem);
}

- (void)showError:(NSString *)message {
    self.errorLabel.stringValue = message;
    self.errorLabel.hidden = NO;
}

- (void)clearError {
    self.errorLabel.stringValue = @"";
    self.timeLabel.stringValue = @"";
    self.timeLabel.hidden = YES;
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

    [self updateCountdownUI];
    [self startCountdownTimerIfNeeded];

    [self updateStatusIconActive:active];
}

- (void)updateStatusIconActive:(BOOL)active {
    NSImage *image = [NSImage imageWithSystemSymbolName:(active ? CTIconActive : CTIconInactive) accessibilityDescription:CTAppName];
    image.template = YES;
    self.statusItem.button.image = image;
    self.statusItem.button.title = @"";
}

- (void)updateCountdownUI {
    NSDate *endsAt = self.power.endsAt;
    if (!self.power.active || !endsAt) {
        [self stopCountdownTimer];
        self.timeLabel.stringValue = @"";
        self.timeLabel.hidden = YES;
        return;
    }

    NSTimeInterval remaining = [endsAt timeIntervalSinceNow];
    if (remaining <= 0.0) {
        self.timeLabel.stringValue = @"";
        self.timeLabel.hidden = YES;
        return;
    }

    if (!self.timeFormatter) {
        self.timeFormatter = [NSDateFormatter new];
        self.timeFormatter.timeStyle = NSDateFormatterShortStyle;
        self.timeFormatter.dateStyle = NSDateFormatterNoStyle;
    }

    NSString *duration = CTCompactDurationStringForInterval(remaining);
    NSString *time = [self.timeFormatter stringFromDate:endsAt];
    self.timeLabel.stringValue = [NSString stringWithFormat:CTCountdownFormat, CTCountdownStopsInPrefix, duration, CTCountdownAtSeparator, time];
    self.timeLabel.hidden = NO;
}

- (void)startCountdownTimerIfNeeded {
    if (self.countdownTimer || !self.popover.shown || !self.power.active || !self.power.endsAt) { return; }

    __weak typeof(self) weakSelf = self;
    self.countdownTimer = [NSTimer scheduledTimerWithTimeInterval:CTCountdownUpdateInterval repeats:YES block:^(NSTimer *timer) {
        [weakSelf updateCountdownUI];
    }];
    self.countdownTimer.tolerance = CTCountdownTimerTolerance;
}

- (void)stopCountdownTimer {
    [self.countdownTimer invalidate];
    self.countdownTimer = nil;
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
