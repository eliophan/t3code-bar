import Foundation
import Testing
@testable import T3CodeBar

struct UsagePresentationTests {
    private func tokens(_ processed: Int) -> UsageReport.Tokens { .init(uncachedInput: processed) }

    private func model(_ name: String, _ kind: UsageProviderKind = .codex, tokens processed: Int, cost: Double,
                       records: Int = 4, unpriced: Int = 0) -> UsageReport.Model {
        .init(kind: kind, name: name, tokens: tokens(processed), costUsd: cost, records: records, unpricedRecords: unpriced)
    }

    private func report(models: [UsageReport.Model], cost: Double? = nil) -> UsageReport {
        var report = UsageReport()
        report.models = models.sorted { ($0.costUsd, $0.tokens.processed) > ($1.costUsd, $1.tokens.processed) }
        report.costUsd = cost ?? models.reduce(0) { $0 + $1.costUsd }
        report.tokens = tokens(models.reduce(0) { $0 + $1.tokens.processed })
        report.records = models.reduce(0) { $0 + $1.records }
        report.unpricedRecords = models.reduce(0) { $0 + $1.unpricedRecords }
        return report
    }

    @Test func theHeadlineIsDollarsAndCentsWithSessionsAndTheEstimateLabel() {
        var report = UsageReport()
        report.costUsd = 2927.925
        report.sessions = 594
        #expect(UsagePresentation.costHeadline(report) == "$2,927.93")
        #expect(UsagePresentation.costSubtitle(report) == "594 sessions · API estimate")
        report.sessions = 1
        #expect(UsagePresentation.costSubtitle(report) == "1 session · API estimate")
    }

    @Test func theUnpricedNoteQuotesTheShareOfRecordsOnlyWhenThereIsOne() {
        var report = UsageReport()
        report.records = 1_000
        #expect(UsagePresentation.unpricedNote(report) == nil)
        report.unpricedRecords = 193
        #expect(UsagePresentation.unpricedNote(report) == "API estimate excludes 19.3% unpriced records.")
        report.unpricedRecords = 1
        #expect(UsagePresentation.unpricedNote(report) == "API estimate excludes 0.1% unpriced records.")
        report.records = 10_000
        #expect(UsagePresentation.unpricedNote(report) == "API estimate excludes <0.1% unpriced records.")
    }

    @Test func aProviderRowReadsPercentOfCostThenTokens() {
        var report = UsageReport()
        report.costUsd = 1_000
        var provider = UsageReport.Provider(kind: .claude)
        provider.costUsd = 295
        provider.tokens = tokens(2_100_000_000)
        #expect(UsagePresentation.costProviderDetail(provider, in: report) == "29.5% of cost · 2.10B tokens")
        report.costUsd = 0
        #expect(UsagePresentation.costProviderDetail(provider, in: report) == "0.0% of cost · 2.10B tokens")
    }

    @Test func spokenAmountsAreWordsNotSymbols() {
        #expect(TokenFormat.spokenCurrency(2927.93) == "2,927 dollars and 93 cents")
        #expect(TokenFormat.spokenCurrency(1) == "1 dollar")
        #expect(TokenFormat.spokenCurrency(0.01) == "0 dollars and 1 cent")
        #expect(TokenFormat.spokenCurrency(12.5) == "12 dollars and 50 cents")
        #expect(TokenFormat.spokenCurrency(-3) == "minus 3 dollars")
    }

    @Test func otherCostHidesBelowHalfACentAndShowsAtIt() {
        var report = UsageReport()
        report.costByType = .init(input: 4, cacheRead: 3, cacheWrite: 2, output: 1, other: 0.004999)
        #expect(UsagePresentation.costBySlices(report).map(\.label) == ["Input", "Cache read", "Cache write", "Output", "Other"])
        #expect(UsagePresentation.costBySlices(report).last?.amount == 0)
        report.costByType.other = 0.005
        #expect(UsagePresentation.costBySlices(report).last?.amount == 0.005)
    }

    @Test func costBySpeedAppearsOnlyWhenSomethingRanFaster() {
        var report = UsageReport()
        report.costBySpeed = .init(standard: 10, fast: 0, ultrafast: 0, premium: 0)
        #expect(UsagePresentation.speedSlices(report).isEmpty)
        report.costBySpeed = .init(standard: 10, fast: 2, ultrafast: 0, premium: 1)
        #expect(UsagePresentation.speedSlices(report).map(\.label) == ["Standard", "Fast", "Ultrafast"])
        #expect(UsagePresentation.premiumText(report) == "Premium $1.00")
        report.costBySpeed = .init(standard: 10, fast: 0, ultrafast: 3, premium: 1.5)
        #expect(!UsagePresentation.speedSlices(report).isEmpty)
    }

    @Test func costRanksModelsByCostThenTokensAndTokensRanksThemTheOtherWayAround() {
        let report = report(models: [
            model("cheap-big", tokens: 9_000, cost: 1),
            model("dear-small", tokens: 10, cost: 5),
            model("tied-big", tokens: 800, cost: 1),
        ])
        #expect(UsagePresentation.modelRows(report, metric: .cost).map(\.name) == ["dear-small", "cheap-big", "tied-big"])
        #expect(UsagePresentation.modelRows(report, metric: .tokens).map(\.name) == ["cheap-big", "tied-big", "dear-small"])
        #expect(UsagePresentation.modelRows(report, metric: .cost).map(\.rank) == [1, 2, 3])
    }

    @Test func aModelWithNoPriceReadsUnpricedAndHasNoShareOnCost() throws {
        let report = report(models: [
            model("gpt-6.1-sol", tokens: 1_200, cost: 6),
            model("free-model", .opencode, tokens: 3_000, cost: 0, records: 5, unpriced: 5),
            model("half-priced", tokens: 100, cost: 2, records: 4, unpriced: 2),
        ])
        let rows = UsagePresentation.modelRows(report, metric: .cost)
        let free = try #require(rows.first { $0.name == "free-model" })
        #expect(free.costText == "Unpriced")
        #expect(free.shareText == "")
        #expect(free.isUnpriced)
        let half = try #require(rows.first { $0.name == "half-priced" })
        #expect(half.costText == "$2.00")
        #expect(!half.isUnpriced)
        // On Tokens the same model has a share, because tokens are known even when the price is not.
        let tokenRows = UsagePresentation.modelRows(report, metric: .tokens)
        #expect(tokenRows.first { $0.name == "free-model" }?.shareText == "69.8%")
    }

    @Test func aRowsShareAndBarFollowTheMetricItIsRankedBy() throws {
        let report = report(models: [model("a", tokens: 750, cost: 3), model("b", tokens: 250, cost: 1)])
        let rows = UsagePresentation.modelRows(report, metric: .cost)
        #expect(rows.map(\.shareText) == ["75.0%", "25.0%"])
        #expect(rows.map(\.barShare) == [1, 1.0 / 3.0])
        #expect(rows.map(\.tokensText) == ["750", "250"])
        #expect(rows[0].spoken == "1, a, Codex, 3 dollars, 75.0% of cost, 750 tokens")
    }

    @Test func aModelWithoutValueGetsNoBar() throws {
        let report = report(models: [model("a", tokens: 5, cost: 2), model("b", tokens: 5, cost: 0, records: 1, unpriced: 1)])
        #expect(UsagePresentation.modelRows(report, metric: .cost).map(\.barShare) == [1, nil])
    }

    @Test func onlyTheFirstFewModelsShowUntilTheyAreExpanded() {
        let many = report(models: (1...12).map { model("m\($0)", tokens: $0 * 100, cost: Double($0)) })
        let rows = UsagePresentation.modelRows(many, metric: .cost)
        #expect(UsagePresentation.visibleModels(rows, expanded: false).count == UsagePresentation.collapsedModelCount)
        #expect(UsagePresentation.visibleModels(rows, expanded: true).count == 12)
        #expect(UsagePresentation.modelToggleTitle(count: rows.count, expanded: false) == "Show all 12 models")
        #expect(UsagePresentation.modelToggleTitle(count: rows.count, expanded: true) == "Show fewer")
    }

    @Test func theRangeUnderTheHeaderNamesFirstAndLastDay() throws {
        let zone = TimeZone(identifier: "Asia/Ho_Chi_Minh")!
        let now = date("2026-11-06T09:08:30Z")
        let week = UsageTimeline(window: UsagePeriod.week.window(now: now, timeZone: zone))
        #expect(UsagePresentation.rangeText(week) == "Oct 31 to Nov 6")
        let day = UsageTimeline(window: UsagePeriod.day.window(now: now, timeZone: zone))
        #expect(UsagePresentation.rangeText(day) == "Nov 5, 4 PM to Nov 6, 4 PM")
    }
}

struct UsageChartFormatTests {
    private let zone = TimeZone(identifier: "Asia/Ho_Chi_Minh")!

    @Test func hoursUseATwelveHourClockInTheMacsZone() {
        #expect(UsageChartFormat.hourLabel(ms("2026-11-06T09:08:00Z"), timeZone: zone) == "4 PM")
        #expect(UsageChartFormat.hourLabel(ms("2026-11-05T17:00:00Z"), timeZone: zone) == "12 AM")
        #expect(UsageChartFormat.hourLabel(ms("2026-11-06T05:00:00Z"), timeZone: zone) == "12 PM")
        #expect(UsageChartFormat.hourLabel(ms("2026-11-06T05:00:00Z"), timeZone: TimeZone(identifier: "UTC")!) == "5 AM")
    }

    @Test func aTooltipHourSaysTodayYesterdayOrTheDate() {
        let reference = ms("2026-11-06T09:08:00Z")
        #expect(UsageChartFormat.tooltipHour(ms("2026-11-06T07:00:00Z"), reference: reference, timeZone: zone) == "2 PM today")
        #expect(UsageChartFormat.tooltipHour(ms("2026-11-05T12:00:00Z"), reference: reference, timeZone: zone) == "7 PM yesterday")
        #expect(UsageChartFormat.tooltipHour(ms("2026-11-04T12:00:00Z"), reference: reference, timeZone: zone) == "Nov 4, 7 PM")
    }

    @Test func daysAreAbbreviatedMonthAndDay() {
        #expect(UsageChartFormat.dayLabel(20_763) == "Nov 6")
        #expect(UsageChartFormat.dayLabel(19_782) == "Feb 29")
    }
}
