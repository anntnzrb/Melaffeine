#import <Foundation/Foundation.h>

FOUNDATION_EXPORT NSNotificationName _Nonnull const PowerControllerDidChangeNotification;
NS_ASSUME_NONNULL_BEGIN

@interface PowerController : NSObject
@property (nonatomic, readonly, getter=isActive) BOOL active;
@property (nonatomic, readonly) BOOL keepDisplayAwake;
@property (nonatomic, readonly, nullable) NSDate *startedAt;
@property (nonatomic, readonly, nullable) NSDate *endsAt;

- (BOOL)startWithDuration:(nullable NSNumber *)durationSeconds keepDisplayAwake:(BOOL)keepDisplayAwake error:(NSError **)error;
- (void)stop;
@end

NS_ASSUME_NONNULL_END
