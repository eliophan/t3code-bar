import Foundation
import Testing
@testable import T3CodeBar

struct UsageChartModelTests {
    private let zone = TimeZone(identifier: "Asia/Ho_Chi_Minh")!

    private func timeline(_ values: [[UsageProviderKind: (Double, Int)]], days: Int? = nil) -> UsageTimeline {
        let count = days ?? values.count
        let first = 20_763 - count + 1
        var columns = (0..<count).map { UsageTimeline.Column(day: first + $0, hourStartMs: nil) }
        for (index, entry) in values.enumerated() {
            for (kind, amount) in entry {
                columns[index].byProvider[kind] = .init(costUsd: amount.0, tokens: amount.1)
            }
        }
        return UsageTimeline(resolution: .day, timeZone: zone, untilMs: 0, columns: columns)
    }

    @Test func ticksStepByOneTwoFiveOrTenTimesAPowerOfTenAndRoundUpPastTheMaximum() {
        func ticks(_ maximum: Double) -> [Double] { UsageChartModel.scale(maximum: maximum).ticks }
        #expect(ticks(100) == [0, 50, 100])
        #expect(ticks(1_234) == [0, 500, 1_000, 1_500])
        #expect(ticks(7) == [0, 2, 4, 6, 8])
        #expect(UsageChartModel.scale(maximum: 7).maximum == 8)
        #expect(ticks(0) == [0])
        #expect(ticks(0.4).first == 0)
    }

    @Test func zeroSitsOnTheBaselineAndTheMaximumNearTheTop() {
        #expect(UsageChartModel.y(0, 50) == 260)
        #expect(UsageChartModel.y(50, 50) == 8)
        #expect(UsageChartModel.y(25, 50) == 134)
        #expect(UsageChartModel.y(0, 0) == 260)
    }

    @Test func theCurveBendsThroughAPeakWithoutOvershooting() {
        let points = [UsageChartModel.Point(x: 0, y: 10), .init(x: 1, y: 20), .init(x: 2, y: 10)]
        #expect(UsageChartModel.slopes(points) == [10, 0, -10])
        let curves = UsageChartModel.curves(points)
        #expect(curves.count == 2)
        #expect(abs(curves[0].control1.x - 1.0 / 3) < 1e-12 && abs(curves[0].control1.y - (10 + 10.0 / 3)) < 1e-12)
        #expect(abs(curves[0].control2.x - 2.0 / 3) < 1e-12 && curves[0].control2.y == 20)
    }

    @Test func aFlatSeriesHasFlatControlPointsAndOneColumnHasNoLine() {
        let flat = UsageChartModel.curves([.init(x: 0, y: 5), .init(x: 1, y: 5), .init(x: 2, y: 5)])
        #expect(flat.allSatisfy { $0.control1.y == 5 && $0.control2.y == 5 })
        #expect(UsageChartModel.curves([.init(x: 0, y: 5)]).isEmpty)
    }

    @Test func aTooSteepSegmentIsClampedToThreeTimesItsSecant() {
        let slopes = UsageChartModel.slopes([.init(x: 0, y: 0), .init(x: 1, y: 1), .init(x: 2, y: 100)])
        let secant = 1.0
        #expect(slopes[0] <= 3 * secant + 1e-9)
        #expect(slopes[1] <= 3 * secant + 1e-9)
    }

    @Test func headingNamesTheResolutionAndMetricLikeT3() {
        let daily = timeline([[:], [:]])
        #expect(UsageChartModel(timeline: daily, providers: [], metric: .cost).heading == "Daily cost")
        #expect(UsageChartModel(timeline: daily, providers: [], metric: .tokens).heading == "Daily processed tokens")
        let hourly = UsageTimeline(window: UsagePeriod.day.window(now: date("2026-11-06T09:08:30Z"), timeZone: zone))
        #expect(UsageChartModel(timeline: hourly, providers: [], metric: .cost).heading == "Hourly cost")
    }

    @Test func seriesTotalsMatchTheTimelineAndTheBiggestProviderIsDrawnFirst() {
        let timeline = timeline([[.codex: (1, 100), .claude: (5, 50)], [.codex: (2, 300), .claude: (6, 70)], [:]])
        let cost = UsageChartModel(timeline: timeline, providers: [.codex, .claude], metric: .cost)
        #expect(cost.series.map(\.provider) == [.claude, .codex])
        #expect(cost.series.map(\.total) == [11, 3])
        let tokens = UsageChartModel(timeline: timeline, providers: [.codex, .claude], metric: .tokens)
        #expect(tokens.series.map(\.provider) == [.codex, .claude])
        #expect(tokens.series.map(\.total) == [400, 120])
    }

    @Test func tooltipListsEveryProviderAndTheTotalInTheMetricsFormat() throws {
        let timeline = timeline([[.codex: (1.5, 2_500_000), .claude: (0.25, 1_000)], [:]])
        let cost = UsageChartModel(timeline: timeline, providers: [.codex, .claude], metric: .cost)
        let column = try #require(cost.columns.first)
        #expect(column.rows.map(\.text) == ["$1.50", "$0.25"])
        #expect(column.totalText == "$1.75")
        let tokens = UsageChartModel(timeline: timeline, providers: [.codex, .claude], metric: .tokens)
        #expect(tokens.columns[0].rows.map(\.text) == ["2.50M", "1K"])
        #expect(tokens.columns[1].rows.map(\.text) == ["0", "0"])
    }

    @Test func tickLabelsUseCurrencyForCostAndCompactNumbersForTokens() {
        let timeline = timeline([[.codex: (80, 4_000_000_000)], [:]])
        let cost = UsageChartModel(timeline: timeline, providers: [.codex], metric: .cost)
        #expect(cost.ticks.map(\.label) == ["0", "$20.00", "$40.00", "$60.00", "$80.00"])
        let tokens = UsageChartModel(timeline: timeline, providers: [.codex], metric: .tokens)
        #expect(tokens.ticks.map(\.label) == ["0", "1B", "2B", "3B", "4B"])
    }

    @Test func theAxisPrintsTheFirstMiddleAndLastColumnInUpperCase() {
        let model = UsageChartModel(timeline: timeline([[:], [:], [:], [:], [:], [:], [:]]), providers: [], metric: .cost)
        #expect(model.axisLabels == ["OCT 31", "NOV 3", "NOV 6"])
    }

    @Test func hourlyColumnsReadTodayAndYesterdayInTheirTooltips() throws {
        let window = UsagePeriod.day.window(now: date("2026-11-06T09:08:30Z"), timeZone: zone)
        let model = UsageChartModel(timeline: UsageTimeline(window: window), providers: [.codex], metric: .cost)
        #expect(model.columns.count == 24)
        #expect(model.columns.first?.title == "4 PM yesterday")
        #expect(model.columns.last?.title == "3 PM today")
        #expect(model.axisLabels == ["4 PM", "4 AM", "3 PM"])
    }

    @Test func thePointerPicksTheNearestColumn() {
        let model = UsageChartModel(timeline: timeline([[:], [:], [:], [:], [:]]), providers: [], metric: .cost)
        #expect(model.columnIndex(atFraction: 0) == 0)
        #expect(model.columnIndex(atFraction: 0.26) == 1)
        #expect(model.columnIndex(atFraction: 0.5) == 2)
        #expect(model.columnIndex(atFraction: 1.4) == 4)
        #expect(model.columnIndex(atFraction: -3) == 0)
        #expect(UsageChartModel(timeline: timeline([]), providers: [], metric: .cost).columnIndex(atFraction: 0.5) == nil)
    }
}
