#import <Foundation/Foundation.h>
#import "../Sources/Duration.h"

static int failures = 0;

static void
fail(NSString *name, NSString *message)
{
    failures++;
    fprintf(stderr, "FAIL %s: %s\n", name.UTF8String, message.UTF8String);
}

static void
expectNil(NSString *name, id value)
{
    if (value) {
        fail(name, [NSString stringWithFormat:@"expected nil, got %@", value]);
    }
}

static void
expectInteger(NSString *name, NSNumber *value, NSInteger expected)
{
    if (!value || value.integerValue != expected) {
        fail(name, [NSString stringWithFormat:@"expected %ld, got %@",
                    (long)expected, value]);
    }
}

static void
expectString(NSString *name, NSString *value, NSString *expected)
{
    if (![value isEqualToString:expected]) {
        fail(name, [NSString stringWithFormat:@"expected %@, got %@",
                    expected, value]);
    }
}

static void
testPositiveIntegerParsing(void)
{
    expectNil(@"empty rejected", CTPositiveIntegerFromString(@"", 10));
    expectNil(@"zero rejected", CTPositiveIntegerFromString(@"0", 10));
    expectNil(@"negative rejected", CTPositiveIntegerFromString(@"-1", 10));
    expectNil(@"decimal rejected", CTPositiveIntegerFromString(@"1.5", 10));
    expectNil(@"alpha rejected", CTPositiveIntegerFromString(@"abc", 10));
    expectNil(@"comma rejected", CTPositiveIntegerFromString(@"99,000", 100000));
    expectNil(@"space rejected", CTPositiveIntegerFromString(@" 1", 10));
    expectNil(@"above max rejected", CTPositiveIntegerFromString(@"11", 10));
    expectInteger(@"one accepted", CTPositiveIntegerFromString(@"1", 10), 1);
    expectInteger(@"leading zero accepted", CTPositiveIntegerFromString(@"007", 10), 7);
    expectInteger(@"max accepted", CTPositiveIntegerFromString(@"10", 10), 10);
}

static void
testDurationSeconds(void)
{
    expectInteger(@"one minute", CTDurationSecondsFromInputString(@"1", CTUnitMinutesIndex), 60);
    expectInteger(@"two hours", CTDurationSecondsFromInputString(@"2", CTUnitHoursIndex), 7200);
    expectInteger(@"one day", CTDurationSecondsFromInputString(@"1", CTUnitDaysIndex), 86400);
    expectInteger(@"max days", CTDurationSecondsFromInputString(@"365", CTUnitDaysIndex), 31536000);
    expectInteger(@"max minutes", CTDurationSecondsFromInputString(@"525600", CTUnitMinutesIndex), 31536000);
    expectNil(@"zero duration rejected", CTDurationSecondsFromInputString(@"0", CTUnitMinutesIndex));
    expectNil(@"negative duration rejected", CTDurationSecondsFromInputString(@"-1", CTUnitHoursIndex));
    expectNil(@"decimal duration rejected", CTDurationSecondsFromInputString(@"1.5", CTUnitHoursIndex));
    expectNil(@"huge minutes rejected", CTDurationSecondsFromInputString(@"99000000", CTUnitMinutesIndex));
    expectNil(@"too many days rejected", CTDurationSecondsFromInputString(@"366", CTUnitDaysIndex));
}

static void
testDurationMultiplier(void)
{
    if (CTDurationMultiplierForUnitIndex(CTUnitMinutesIndex) != CTSecondsPerMinute) {
        fail(@"minute multiplier", @"wrong seconds");
    }
    if (CTDurationMultiplierForUnitIndex(CTUnitHoursIndex) != CTSecondsPerHour) {
        fail(@"hour multiplier", @"wrong seconds");
    }
    if (CTDurationMultiplierForUnitIndex(CTUnitDaysIndex) != CTSecondsPerDay) {
        fail(@"day multiplier", @"wrong seconds");
    }
    if (CTDurationMultiplierForUnitIndex(999) != CTSecondsPerDay) {
        fail(@"fallback multiplier", @"wrong seconds");
    }
}

static void
testCompactDurationFormatting(void)
{
    expectString(@"zero seconds", CTCompactDurationStringForInterval(0), @"<1m");
    expectString(@"negative seconds", CTCompactDurationStringForInterval(-1), @"<1m");
    expectString(@"fifty nine seconds", CTCompactDurationStringForInterval(59), @"1m");
    expectString(@"sixty seconds", CTCompactDurationStringForInterval(60), @"1m");
    expectString(@"sixty one seconds", CTCompactDurationStringForInterval(61), @"2m");
    expectString(@"one hour", CTCompactDurationStringForInterval(3600), @"1h 0m");
    expectString(@"two hours", CTCompactDurationStringForInterval(7200), @"2h 0m");
    expectString(@"one day", CTCompactDurationStringForInterval(86400), @"1d 0h");
    expectString(@"one day one hour", CTCompactDurationStringForInterval(90000), @"1d 1h");
}

int
main(void)
{
    @autoreleasepool {
        testPositiveIntegerParsing();
        testDurationSeconds();
        testDurationMultiplier();
        testCompactDurationFormatting();
    }

    if (failures) {
        fprintf(stderr, "%d duration test(s) failed\n", failures);
        return 1;
    }

    puts("Duration tests passed");
    return 0;
}