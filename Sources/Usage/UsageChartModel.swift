import Foundation

/// The geometry and text of T3 Code's "Daily cost" and "Hourly processed tokens" chart.
///
/// T3 Code draws one smoothed line and a faint area per provider on a 960 by 260 canvas, scaled to 0 at the
/// bottom and a rounded-up maximum near the top. Providers are not stacked. This is a port of its `Ln`, `Nn`,
/// `kn` and `An`, so the same numbers give the same curve.
public struct UsageChartModel: Equatable, Sendable {
    public static let width = 960.0
    public static let height = 260.0
    /// The plot's usable height. The top 8 units stay free, as in T3 Code.
    static let plotHeight = 252.0
    static let tickCount = 4.0

    public struct Point: Equatable, Sendable {
        public var x: Double
        public var y: Double
    }

    public struct Curve: Equatable, Sendable {
        public let from: Point, control1: Point, control2: Point, to: Point
    }

    public struct Series: Equatable, Sendable {
        public let provider: UsageProviderKind
        public let total: Double
        public let curves: [Curve]
    }

    public struct Tick: Equatable, Sendable {
        public let value: Double
        public let y: Double
        public let label: String
    }

    public struct Row: Equatable, Sendable {
        public let provider: UsageProviderKind
        public let text: String
    }

    public struct Column: Equatable, Sendable {
        public let title: String
        public let rows: [Row]
        public let totalText: String
    }

    public let metric: UsageMetric
    public let heading: String
    public let providers: [UsageProviderKind]
    public let series: [Series]
    public let ticks: [Tick]
    public let columns: [Column]
    /// First, middle and last column, as T3 Code prints under the chart.
    public let axisLabels: [String]
    public let spoken: String

    public init(timeline: UsageTimeline, providers: [UsageProviderKind], metric: UsageMetric) {
        self.metric = metric
        self.providers = providers
        let hourly = timeline.resolution == .hour
        heading = "\(hourly ? "Hourly" : "Daily") \(metric == .tokens ? "processed tokens" : "cost")"
        let format: (Double) -> String = metric == .tokens ? { TokenFormat.compact($0) } : { TokenFormat.currency($0) }

        let count = timeline.columns.count
        let maximum = timeline.columns.flatMap { column in providers.map { column.value($0, metric) } }.max() ?? 0
        let scale = Self.scale(maximum: maximum)
        let step = count < 2 ? 0 : Self.width / Double(count - 1)
        ticks = scale.ticks.map { Tick(value: $0, y: Self.y($0, scale.maximum), label: $0 == 0 ? "0" : format($0)) }
        series = providers.map { provider in
            let values = timeline.columns.map { $0.value(provider, metric) }
            let points = values.enumerated().map { Point(x: Double($0.offset) * step, y: Self.y($0.element, scale.maximum)) }
            return Series(provider: provider, total: values.reduce(0, +), curves: Self.curves(points))
        }.enumerated().sorted { a, b in
            a.element.total != b.element.total ? a.element.total > b.element.total : a.offset < b.offset
        }.map(\.element)

        columns = timeline.columns.map { column in
            let title: String
            if let start = column.hourStartMs {
                title = UsageChartFormat.tooltipHour(start, reference: timeline.untilMs, timeZone: timeline.timeZone)
            } else {
                title = UsageChartFormat.dayLabel(column.day)
            }
            return Column(title: title, rows: providers.map { Row(provider: $0, text: format(column.value($0, metric))) },
                          totalText: format(column.total(metric)))
        }
        func label(_ column: UsageTimeline.Column) -> String {
            column.hourStartMs.map { UsageChartFormat.hourLabel($0, timeZone: timeline.timeZone) } ?? UsageChartFormat.dayLabel(column.day)
        }
        axisLabels = timeline.columns.isEmpty ? [] : [
            label(timeline.columns[0]), label(timeline.columns[count / 2]), label(timeline.columns[count - 1]),
        ].map { $0.uppercased() }
        let total = timeline.columns.reduce(0) { $0 + $1.total(metric) }
        spoken = "\(heading) by provider, \(format(total)) in total"
    }

    /// T3 Code's `In`.
    static func y(_ value: Double, _ maximum: Double) -> Double {
        maximum == 0 ? height : height - value / maximum * plotHeight
    }

    /// T3 Code's `Nn`: about four ticks on a step of 1, 2, 5 or 10 times a power of ten, rounded up past the maximum.
    static func scale(maximum: Double) -> (maximum: Double, ticks: [Double]) {
        guard maximum > 0 else { return (0, [0]) }
        let raw = maximum / tickCount
        let power = pow(10, (log10(raw)).rounded(.down))
        let ratio = raw / power
        let step = (ratio > 5 ? 10 : ratio > 2 ? 5 : ratio > 1 ? 2 : 1) * power
        let top = (maximum / step).rounded(.up) * step
        var ticks: [Double] = []
        var value = 0.0
        while value <= top + step * 1e-6 {
            ticks.append(value)
            value += step
        }
        return (top, ticks)
    }

    /// T3 Code's `kn`: slopes of a monotone cubic through the points, clamped so the curve never overshoots.
    static func slopes(_ points: [Point]) -> [Double] {
        let count = points.count
        guard count >= 2 else { return [0] }
        var secants: [Double] = []
        for index in 0..<(count - 1) {
            let run = points[index + 1].x - points[index].x
            secants.append(run == 0 ? 0 : (points[index + 1].y - points[index].y) / run)
        }
        var result = [Double](repeating: 0, count: count)
        result[0] = secants[0]
        result[count - 1] = secants[count - 2]
        if count > 2 {
            for index in 1..<(count - 1) {
                let before = secants[index - 1], after = secants[index]
                result[index] = before * after <= 0 ? 0 : (before + after) / 2
            }
        }
        for index in 0..<(count - 1) {
            let secant = secants[index]
            if secant == 0 {
                result[index] = 0
                result[index + 1] = 0
                continue
            }
            let a = result[index] / secant, b = result[index + 1] / secant
            let sum = a * a + b * b
            if sum > 9 {
                let factor = 3 / sum.squareRoot()
                result[index] = factor * a * secant
                result[index + 1] = factor * b * secant
            }
        }
        return result
    }

    /// T3 Code's `An`.
    static func curves(_ points: [Point]) -> [Curve] {
        guard points.count >= 2 else { return [] }
        let slopes = slopes(points)
        return (0..<(points.count - 1)).map { index in
            let from = points[index], to = points[index + 1]
            let run = to.x - from.x
            return Curve(from: from,
                         control1: Point(x: from.x + run / 3, y: from.y + slopes[index] * run / 3),
                         control2: Point(x: to.x - run / 3, y: to.y - slopes[index + 1] * run / 3),
                         to: to)
        }
    }

    /// The column nearest a pointer at `fraction` of the plot's width, as T3 Code picks it.
    public func columnIndex(atFraction fraction: Double) -> Int? {
        guard !columns.isEmpty else { return nil }
        let index = Int((min(max(fraction, 0), 1) * Double(columns.count - 1)).rounded(.toNearestOrAwayFromZero))
        return min(columns.count - 1, max(0, index))
    }
}
