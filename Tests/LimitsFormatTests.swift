import Foundation
import Testing
@testable import T3CodeBar

struct LimitsFormatTests {
    private func calendar(_ zone: String = "Asia/Ho_Chi_Minh", locale: String = "en_US") -> Calendar {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: zone)!
        calendar.locale = Locale(identifier: locale)
        return calendar
    }

    private func resets(_ reset: String, now: String = "2026-11-06T03:00:00Z", clock: ClockStyle = .twelveHour,
                        calendar: Calendar? = nil) -> String? {
        LimitsFormat.resets(limitWindow("w", .weekly, used: 0, reset: reset), now: limitDate(now),
                            calendar: calendar ?? self.calendar(), clock: clock)
    }

    @Test(arguments: [
        (-5_000.0, "0m"), (59_999, "0m"), (60_000, "1m"), (3_599_000, "59m"), (3_600_000, "1h 0m"),
        (12_300_000, "3h 25m"), (86_400_000, "1d 0h"), (183_600_000, "2d 3h"), (1_000_000_000, "11d 13h"),
    ] as [(Double, String)])
    func durationUsesTheTwoLargestUnits(ms: Double, text: String) {
        #expect(LimitsFormat.duration(milliseconds: ms) == text)
    }

    @Test func resetsInNamesTheThreeStates() {
        let now = limitDate("2026-11-06T10:00:00Z")
        func text(_ offset: TimeInterval) -> String? {
            LimitsFormat.resetsIn(limitWindow("w", .weekly, used: 0, reset: nil).with(reset: now.addingTimeInterval(offset)), now: now)
        }
        #expect(text(-1) == "resets now")
        #expect(text(0) == "resets now")
        #expect(text(30) == "Resets momentarily")
        #expect(text(60) == "resets in 1m")
        #expect(text(5400) == "resets in 1h 30m")
        #expect(LimitsFormat.resetsIn(limitWindow("w", .weekly, used: 0), now: now) == nil)
    }

    @Test func resetsShowsTheTimeThenTheCountdown() {
        #expect(resets("2026-11-06T09:20:55Z") == "4:20 PM · in 6h 20m")
        #expect(resets("2026-11-07T01:00:00Z") == "tomorrow at 8:00 AM · in 22h 0m")
        #expect(resets("2026-11-13T04:20:55Z") == "11/13 11:20 AM · in 7d 1h")
        #expect(resets("2027-01-02T00:00:00Z") == "1/2/2027 7:00 AM · in 56d 21h")
        #expect(resets("2026-11-05T09:00:00Z") == "yesterday at 4:00 PM · resets now")
        #expect(resets("2026-11-03T09:00:00Z") == "11/3 4:00 PM · resets now")
        #expect(resets("2026-11-06T03:00:30Z") == "10:00 AM · Resets momentarily")
    }

    @Test func twelveHourClockHandlesMidnightAndNoon() {
        #expect(resets("2026-11-06T17:05:00Z") == "tomorrow at 12:05 AM · in 14h 5m")
        #expect(resets("2026-11-06T05:00:00Z") == "12:00 PM · in 2h 0m")
    }

    @Test func twentyFourHourClockPadsTheHour() {
        #expect(resets("2026-11-06T09:20:55Z", clock: .twentyFourHour) == "16:20 · in 6h 20m")
        #expect(resets("2026-11-06T17:05:00Z", clock: .twentyFourHour) == "tomorrow at 00:05 · in 14h 5m")
    }

    @Test func theTimeZoneDecidesTheDay() {
        let utc = calendar("UTC")
        #expect(resets("2026-11-06T09:20:55Z", calendar: utc) == "9:20 AM · in 6h 20m")
        #expect(resets("2026-11-07T00:30:00Z", now: "2026-11-06T22:00:00Z", calendar: utc) == "tomorrow at 12:30 AM · in 2h 30m")
    }

    @Test func theCalendarLocaleDoesNotChangeTheText() {
        let reference = resets("2026-11-13T04:20:55Z", calendar: calendar(locale: "en_US"))
        for locale in ["fr_FR", "ar_SA", "ja_JP", "de_DE"] {
            #expect(resets("2026-11-13T04:20:55Z", calendar: calendar(locale: locale)) == reference)
            #expect(resets("2026-11-06T09:20:55Z", calendar: calendar(locale: locale)) == "4:20 PM · in 6h 20m")
        }
    }

    @Test func creditsReadLikeT3() {
        let now = limitDate("2026-11-06T10:00:00Z")
        let soon = limitDate("2026-11-08T13:00:00Z")
        #expect(LimitsFormat.credits(ResetCredits(availableCount: 1, nextExpiresAt: soon), now: now) == "1 reset credit banked · next expires in 2d 3h")
        #expect(LimitsFormat.credits(ResetCredits(availableCount: 1, nextExpiresAt: soon), now: now, compact: true) == "1 banked · expires in 2d 3h")
        #expect(LimitsFormat.credits(ResetCredits(availableCount: 2), now: now) == "2 reset credits banked")
        #expect(LimitsFormat.credits(ResetCredits(availableCount: 0, nextExpiresAt: soon), now: now) == "No reset credits banked")
    }

    @Test(arguments: [
        (0.0, "just now"), (59, "just now"), (60, "1m ago"), (3599, "59m ago"), (3600, "1h ago"),
        (10_800, "3h ago"), (86_399, "23h ago"), (90_000, "1d ago"), (-30, "just now"),
    ] as [(Double, String)])
    func agoFloorsToItsUnit(seconds: Double, text: String) {
        let now = limitDate("2026-11-06T10:00:00Z")
        #expect(LimitsFormat.ago(now.addingTimeInterval(-seconds), now: now) == text)
    }

    @Test func aWindowCardCarriesEveryLineT3Shows() throws {
        let now = limitDate("2026-11-06T03:00:00Z")
        let window = limitWindow("p", .session, used: 16, reset: "2026-11-06T09:20:55Z", minutes: 600)
        let account = limitAccount([window], credits: ResetCredits(availableCount: 1, nextExpiresAt: limitDate("2026-11-08T06:00:00Z")))
        let group = try #require(LimitsLayout.groups([account], now: now).first)
        let row = try #require(group.rows.first)
        let member = try #require(row.members.first)
        let detail = LimitWindowDetail(member: member, reset: row.reset(for: "a"), now: now, calendar: calendar())
        #expect(detail.left == "84%")
        #expect(detail.resets == "4:20 PM · in 6h 20m")
        #expect(detail.evenPace == "63% left by now")
        #expect(detail.restores == "+16% of pool")
        #expect(detail.credits == "1 banked · expires in 2d 3h")
    }
}

extension LimitWindow {
    func with(reset: Date) -> LimitWindow {
        LimitWindow(id: id, kind: kind, label: label, usedPercent: usedPercent, resetsAt: reset, durationMins: durationMins)
    }
}
