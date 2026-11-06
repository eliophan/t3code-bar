import Foundation
import Testing
@testable import T3CodeBar

struct UsageWindowTests {
    private let saigon = TimeZone(identifier: "Asia/Ho_Chi_Minh")!
    private let newYork = TimeZone(identifier: "America/New_York")!

    private func inside(_ window: UsageWindow, _ iso: String) -> Bool { window.slot(ofMs: ms(iso)) != nil }

    @Test func pastDayEndsAtTheCurrentMinuteAndStartsTwentyFourHoursEarlier() {
        let window = UsageWindow.rolling24h(now: date("2026-11-06T09:08:30Z"), timeZone: saigon)
        #expect(window.untilMs == ms("2026-11-06T09:08:00Z"))
        #expect(window.sinceMs == ms("2026-11-05T09:08:00Z"))
        #expect(window.resolution == .hour)
    }

    @Test func aRecordOneMillisecondInsideTheRollingEdgesCountsAndOneOutsideDoesNot() {
        let window = UsageWindow.rolling24h(now: date("2026-11-06T09:08:30Z"), timeZone: saigon)
        #expect(inside(window, "2026-11-05T09:08:00.000Z"))
        #expect(!inside(window, "2026-11-05T09:07:59.999Z"))
        #expect(inside(window, "2026-11-06T09:07:59.999Z"))
        #expect(!inside(window, "2026-11-06T09:08:00.000Z"))
    }

    @Test func rollingHoursAreSlotsCountedFromTheStartNotWallClockHours() {
        let window = UsageWindow.rolling24h(now: date("2026-11-06T09:08:30Z"), timeZone: saigon)
        #expect(window.slot(ofMs: ms("2026-11-05T09:08:00Z"))?.hour == 0)
        #expect(window.slot(ofMs: ms("2026-11-05T10:07:59.999Z"))?.hour == 0)
        #expect(window.slot(ofMs: ms("2026-11-05T10:08:00Z"))?.hour == 1)
        #expect(window.slot(ofMs: ms("2026-11-06T09:07:59.999Z"))?.hour == 23)
    }

    @Test func aRollingRecordKnowsItsLocalDay() {
        let window = UsageWindow.rolling24h(now: date("2026-11-06T09:08:00Z"), timeZone: saigon)
        #expect(window.slot(ofMs: ms("2026-11-05T16:59:59.999Z"))?.day == dayNumber("2026-11-05"))
        #expect(window.slot(ofMs: ms("2026-11-05T17:00:00Z"))?.day == dayNumber("2026-11-06"))
    }

    @Test func sevenDaysAreCalendarDaysEndingTodayInTheLocalZone() {
        let window = UsageWindow.calendarDays(7, now: date("2026-11-06T09:08:00Z"), timeZone: saigon)
        #expect(window.sinceDay == dayNumber("2026-10-31"))
        #expect(window.untilDay == dayNumber("2026-11-06"))
        #expect(window.resolution == .day)
    }

    @Test func calendarDaysCountTheWholeFirstDayAndTheWholeOfToday() {
        let window = UsageWindow.calendarDays(7, now: date("2026-11-06T09:08:00Z"), timeZone: saigon)
        #expect(!inside(window, "2026-10-30T16:59:59.999Z"))
        #expect(inside(window, "2026-10-30T17:00:00.000Z"))
        #expect(inside(window, "2026-11-06T16:59:59.999Z"))
        #expect(!inside(window, "2026-11-06T17:00:00.000Z"))
    }

    @Test func oneCalendarDayIsJustToday() {
        let window = UsageWindow.calendarDays(1, now: date("2026-11-06T09:08:00Z"), timeZone: saigon)
        #expect(window.sinceDay == window.untilDay)
        #expect(!inside(window, "2026-11-05T16:59:59.999Z"))
        #expect(inside(window, "2026-11-05T17:00:00.000Z"))
    }

    @Test func aDayThatGainsAnHourStillStartsAtLocalMidnight() {
        // New York left daylight time on 2026-11-01 at 06:00Z, so Nov 1 lasted 25 hours.
        let twoDays = UsageWindow.calendarDays(2, now: date("2026-11-02T12:00:00Z"), timeZone: newYork)
        #expect(!inside(twoDays, "2026-11-01T03:59:59.999Z"))
        #expect(inside(twoDays, "2026-11-01T04:00:00.000Z"))
        let today = UsageWindow.calendarDays(1, now: date("2026-11-02T12:00:00Z"), timeZone: newYork)
        #expect(!inside(today, "2026-11-02T04:59:59.999Z"))
        #expect(inside(today, "2026-11-02T05:00:00.000Z"))
    }

    @Test func aRecordBeforeAndAfterTheTransitionLandsOnTheRightLocalDay() {
        let window = UsageWindow.calendarDays(3, now: date("2026-11-02T12:00:00Z"), timeZone: newYork)
        #expect(window.slot(ofMs: ms("2026-11-01T05:30:00Z"))?.day == dayNumber("2026-11-01"))
        #expect(window.slot(ofMs: ms("2026-11-01T06:30:00Z"))?.day == dayNumber("2026-11-01"))
        #expect(window.slot(ofMs: ms("2026-11-02T04:30:00Z"))?.day == dayNumber("2026-11-01"))
        #expect(window.slot(ofMs: ms("2026-11-02T05:30:00Z"))?.day == dayNumber("2026-11-02"))
    }

    @Test func theEarliestZoneOnEarthStillGetsItsWholeDay() throws {
        let kiritimati = try #require(TimeZone(identifier: "Pacific/Kiritimati")) // UTC+14
        let window = UsageWindow.calendarDays(1, now: date("2026-11-06T09:08:00Z"), timeZone: kiritimati)
        #expect(!inside(window, "2026-11-05T09:59:59.999Z"))
        #expect(inside(window, "2026-11-05T10:00:00.000Z"))
        #expect(inside(window, "2026-11-06T09:59:59.999Z"))
        #expect(!inside(window, "2026-11-06T10:00:00.000Z"))
    }

    @Test func aHalfHourZoneUsesItsOwnMidnight() throws {
        let kolkata = try #require(TimeZone(identifier: "Asia/Kolkata")) // UTC+5:30
        let window = UsageWindow.calendarDays(1, now: date("2026-11-06T09:08:00Z"), timeZone: kolkata)
        #expect(!inside(window, "2026-11-05T18:29:59.999Z"))
        #expect(inside(window, "2026-11-05T18:30:00.000Z"))
    }

    @Test func filesAreReadFromThirtySixHoursBeforeTheWindow() {
        let rolling = UsageWindow.rolling24h(now: date("2026-11-06T09:08:30Z"), timeZone: saigon)
        #expect(rolling.fileCutoffMs == ms("2026-11-05T09:08:00Z") - 129_600_000)
        let calendar = UsageWindow.calendarDays(7, now: date("2026-11-06T09:08:00Z"), timeZone: saigon)
        #expect(calendar.fileCutoffMs == ms("2026-10-31T00:00:00Z") - 129_600_000)
    }

    @Test func fileRetentionNeverReachesFurtherBackThanNinetyTwoDays() {
        let window = UsageWindow.calendarDays(90, now: date("2026-11-06T09:08:00Z"), timeZone: saigon)
        #expect(window.fileCutoffMs >= window.nowMs - 7_948_800_000)
    }
}
