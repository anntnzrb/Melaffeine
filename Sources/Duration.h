#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

FOUNDATION_EXPORT NSString * const CTCountdownLessThanMinute;
FOUNDATION_EXPORT NSString * const CTDurationDaysHoursFormat;
FOUNDATION_EXPORT NSString * const CTDurationHoursMinutesFormat;
FOUNDATION_EXPORT NSString * const CTDurationMinutesFormat;

FOUNDATION_EXPORT const NSInteger CTUnitMinutesIndex;
FOUNDATION_EXPORT const NSInteger CTUnitHoursIndex;
FOUNDATION_EXPORT const NSInteger CTUnitDaysIndex;
FOUNDATION_EXPORT const NSInteger CTMinimumDurationValue;
FOUNDATION_EXPORT const NSInteger CTMaximumFiniteDurationDays;

FOUNDATION_EXPORT const NSTimeInterval CTSecondsPerMinute;
FOUNDATION_EXPORT const NSTimeInterval CTSecondsPerHour;
FOUNDATION_EXPORT const NSTimeInterval CTSecondsPerDay;
FOUNDATION_EXPORT const NSTimeInterval CTMaximumFiniteDurationSeconds;

NSTimeInterval CTDurationMultiplierForUnitIndex(NSInteger unitIndex);
NSNumber *_Nullable CTPositiveIntegerFromString(NSString *string, NSInteger maxValue);
NSNumber *_Nullable CTDurationSecondsFromInputString(NSString *string, NSInteger unitIndex);
NSString *CTCompactDurationStringForInterval(NSTimeInterval interval);

NS_ASSUME_NONNULL_END