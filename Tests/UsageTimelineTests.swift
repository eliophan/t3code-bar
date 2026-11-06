import Foundation
import Testing
@testable import T3CodeBar

struct UsageTimelineTests {
    private let zone = TimeZone(identifier: "Asia/Ho_Chi_Minh")!
    private let now = date("2026-11-06T09:08:30Z")

    private let prices = PriceBook(table: PriceTable(entries: [
        .init(name: "m", values: ["input_cost_per_token": 1e-6, "output_cost_per_token": 2e-6]),
    ]))

    private func timeline(_ rows: [Double], days: Int) throws -> UsageTimeline {
        let files = [CacheFixture.FileEntry(
            path: "/c/a.jsonl", kind: "claude",
            rows: rows.map { CacheFixture.Row(timestampMs: $0, model: 0, session: 1, uncached: 10, cached: 0, creation: 0,
                                              output: 5, reasoning: 0, cost: nil, speed: 0).json })]
        let cache = CacheFixture(sources: [("claude", "/c")], models: ["m"], sessions: ["", "s1"], files: files)
        let period = UsagePeriod(rawValue: days)!
        let window = period.window(now: now, timeZone: zone)
        return UsageReportBuilder.build(dataset: try cache.decoded(), window: window, prices: prices).timeline
    }

    @Test func daysCountedFromTheEpochBecomeCalendarDates() {
        #expect(UsageTimeline.isoDay(0) == "1970-01-01")
        #expect(UsageTimeline.isoDay(19_782) == "2024-02-29")
        #expect(UsageTimeline.isoDay(20_763) == "2026-11-06")
        #expect(UsageTimeline.isoDay(-1) == "1969-12-31")
    }

    @Test func aDailyWindowHasOneColumnPerLocalDayEvenWhenEmpty() throws {
        let timeline = try timeline([], days: 7)
        #expect(timeline.resolution == .day)
        #expect(timeline.columns.map { UsageTimeline.isoDay($0.day) }
            == ["2026-10-31", "2026-11-01", "2026-11-02", "2026-11-03", "2026-11-04", "2026-11-05", "2026-11-06"])
    }

    @Test func aRecordLandsOnTheDayOfTheMacsZoneNotOfUTC() throws {
        // 18:30 UTC on Nov 5 is 01:30 on Nov 6 in Ho Chi Minh City.
        let timeline = try timeline([ms("2026-11-05T18:30:00Z")], days: 7)
        let last = try #require(timeline.columns.last)
        #expect(UsageTimeline.isoDay(last.day) == "2026-11-06")
        #expect(last.byProvider[.claude]?.tokens == 15)
        #expect(timeline.columns.dropLast().allSatisfy { $0.byProvider.isEmpty })
    }

    @Test func pastTwentyFourHoursHasTwentyFourSlotsCountedFromTheRollingStart() throws {
        let since = ms("2026-11-05T09:08:00Z")
        let timeline = try timeline([since, since + 3_599_999, since + 3_600_000, ms("2026-11-06T09:07:59Z")], days: 1)
        #expect(timeline.resolution == .hour)
        #expect(timeline.columns.count == 24)
        #expect(timeline.columns.first?.hourStartMs == since)
        #expect(timeline.columns[0].byProvider[.claude]?.tokens == 30)
        #expect(timeline.columns[1].byProvider[.claude]?.tokens == 15)
        #expect(timeline.columns[23].byProvider[.claude]?.tokens == 15)
    }

    @Test func columnsAddUpToTheReportTotals() throws {
        let rows = [ms("2026-11-01T03:00:00Z"), ms("2026-11-03T20:00:00Z"), ms("2026-11-06T01:00:00Z")]
        let files = [CacheFixture.FileEntry(path: "/c/a.jsonl", kind: "claude", rows: rows.map {
            CacheFixture.Row(timestampMs: $0, model: 0, session: 1, uncached: 100, cached: 0, creation: 0, output: 50,
                             reasoning: 0, cost: nil, speed: 0).json
        })]
        let cache = CacheFixture(sources: [("claude", "/c")], models: ["m"], sessions: ["", "s1"], files: files)
        let report = UsageReportBuilder.build(dataset: try cache.decoded(), window: .calendarDays(30, now: now, timeZone: zone),
                                              prices: prices)
        #expect(Int(report.timeline.columns.reduce(0) { $0 + $1.total(.tokens) }) == report.processedTokens)
        #expect(abs(report.timeline.columns.reduce(0) { $0 + $1.total(.cost) } - report.costUsd) < 1e-12)
    }
}
