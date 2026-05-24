#import "Duration.h"

NSString *const CTCountdownLessThanMinute = @"<1m";
NSString *const CTDurationDaysHoursFormat = @"%ldd %ldh";
NSString *const CTDurationHoursMinutesFormat = @"%ldh %ldm";
NSString *const CTDurationMinutesFormat = @"%ldm";

const NSInteger CTUnitMinutesIndex = 0;
const NSInteger CTUnitHoursIndex = 1;
const NSInteger CTUnitDaysIndex = 2;
const NSInteger CTMinimumDurationValue = 1;
const NSInteger CTMaximumFiniteDurationDays = 365;

const NSTimeInterval CTSecondsPerMinute = 60.0;
const NSTimeInterval CTSecondsPerHour = 3600.0;
const NSTimeInterval CTSecondsPerDay = 86400.0;
const NSTimeInterval CTMaximumFiniteDurationSeconds =
    CTSecondsPerDay * CTMaximumFiniteDurationDays;

NSTimeInterval CTDurationMultiplierForUnitIndex(NSInteger unitIndex) {
  if (unitIndex == CTUnitMinutesIndex) {
    return CTSecondsPerMinute;
  }
  if (unitIndex == CTUnitHoursIndex) {
    return CTSecondsPerHour;
  }
  return CTSecondsPerDay;
}

NSNumber *CTPositiveIntegerFromString(NSString *string, NSInteger maxValue) {
  if (string.length == 0) {
    return nil;
  }

  NSInteger value = 0;
  for (NSUInteger index = 0; index < string.length; index++) {
    unichar character = [string characterAtIndex:index];
    if (character < '0' || character > '9') {
      return nil;
    }

    value = (value * 10) + (character - '0');
    if (value > maxValue) {
      return nil;
    }
  }

  if (value < CTMinimumDurationValue) {
    return nil;
  }
  return @(value);
}

NSNumber *CTDurationSecondsFromInputString(NSString *string,
                                           NSInteger unitIndex) {
  NSTimeInterval multiplier = CTDurationMultiplierForUnitIndex(unitIndex);
  NSInteger maxValue =
      (NSInteger)floor(CTMaximumFiniteDurationSeconds / multiplier);
  NSNumber *value = CTPositiveIntegerFromString(string, maxValue);
  if (!value) {
    return nil;
  }
  return @(value.integerValue * multiplier);
}

NSString *CTCompactDurationStringForInterval(NSTimeInterval interval) {
  NSInteger minutes = (NSInteger)ceil(interval / CTSecondsPerMinute);
  if (minutes <= 0) {
    return CTCountdownLessThanMinute;
  }

  NSInteger days = minutes / (NSInteger)(CTSecondsPerDay / CTSecondsPerMinute);
  NSInteger hours =
      (minutes / (NSInteger)(CTSecondsPerHour / CTSecondsPerMinute)) %
      (NSInteger)(CTSecondsPerDay / CTSecondsPerHour);
  NSInteger remainingMinutes =
      minutes % (NSInteger)(CTSecondsPerHour / CTSecondsPerMinute);

  if (days > 0) {
    return [NSString
        stringWithFormat:CTDurationDaysHoursFormat, (long)days, (long)hours];
  }
  if (hours > 0) {
    return [NSString stringWithFormat:CTDurationHoursMinutesFormat, (long)hours,
                                      (long)remainingMinutes];
  }
  return [NSString
      stringWithFormat:CTDurationMinutesFormat, (long)remainingMinutes];
}