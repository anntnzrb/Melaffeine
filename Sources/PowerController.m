#import "PowerController.h"
#import "Constants.h"
#import <IOKit/pwr_mgt/IOPMLib.h>

static NSString * const CTPowerErrorDomain = @"dev.annt.melaffeine.power";
NSNotificationName const PowerControllerDidChangeNotification = @"PowerControllerDidChangeNotification";


@interface PowerController ()
@property (nonatomic, readwrite, getter=isActive) BOOL active;
@property (nonatomic, readwrite) BOOL keepDisplayAwake;
@property (nonatomic, readwrite, nullable) NSDate *startedAt;
@property (nonatomic, readwrite, nullable) NSDate *endsAt;
@property (nonatomic) IOPMAssertionID assertionID;
@property (nonatomic, nullable) NSTimer *timer;
@end

@implementation PowerController

- (instancetype)init {
    self = [super init];
    if (self) {
        _assertionID = kIOPMNullAssertionID;
    }
    return self;
}

- (void)dealloc {
    [self.timer invalidate];
    if (self.assertionID != kIOPMNullAssertionID) {
        IOPMAssertionRelease(self.assertionID);
    }
}

- (BOOL)startWithDuration:(NSNumber *)durationSeconds keepDisplayAwake:(BOOL)keepDisplayAwake error:(NSError **)error {
    [self stop];

    CFStringRef assertionType = keepDisplayAwake
        ? kIOPMAssertionTypePreventUserIdleDisplaySleep
        : kIOPMAssertionTypePreventUserIdleSystemSleep;

    IOPMAssertionID newAssertionID = kIOPMNullAssertionID;
    IOReturn result = IOPMAssertionCreateWithName(
        assertionType,
        kIOPMAssertionLevelOn,
        (__bridge CFStringRef)CTAppName,
        &newAssertionID);

    if (result != kIOReturnSuccess) {
        if (error) {
            *error = [NSError errorWithDomain:CTPowerErrorDomain code:result userInfo:nil];
        }
        return NO;
    }

    NSDate *now = NSDate.date;
    self.assertionID = newAssertionID;
    self.startedAt = now;
    self.endsAt = durationSeconds ? [now dateByAddingTimeInterval:durationSeconds.doubleValue] : nil;
    self.keepDisplayAwake = keepDisplayAwake;
    self.active = YES;
    [NSNotificationCenter.defaultCenter postNotificationName:PowerControllerDidChangeNotification object:self];


    if (durationSeconds) {
        self.timer = [NSTimer scheduledTimerWithTimeInterval:durationSeconds.doubleValue
                                                      target:self
                                                    selector:@selector(timerDidFire:)
                                                    userInfo:nil
                                                     repeats:NO];
    }

    return YES;
}

- (void)stop {
    [self.timer invalidate];
    self.timer = nil;

    if (self.assertionID != kIOPMNullAssertionID) {
        IOPMAssertionRelease(self.assertionID);
        self.assertionID = kIOPMNullAssertionID;
    }

    self.active = NO;
    self.startedAt = nil;
    self.endsAt = nil;
    [NSNotificationCenter.defaultCenter postNotificationName:PowerControllerDidChangeNotification object:self];
}

- (void)timerDidFire:(NSTimer *)timer {
    [self stop];
}

@end
