import Foundation

/// The numbers behind T3 Code's chart over time: one column per hour for Past 24h and one per calendar day
/// otherwise, each holding cost and processed tokens per provider.
///
/// Every slot of the window has a column, even an empty one, because the chart draws them all.
public struct UsageTimeline: Equatable, Sendable {
    public struct Amount: Equatable, Sendable {
        public var costUsd = 0.0
        public var tokens = 0

        public func value(_ metric: UsageMetric) -> Double {
            metric == .cost ? costUsd : Double(tokens)
        }
    }

    public struct Column: Equatable, Sendable {
        /// The local calendar day, as days since 1970-01-01.
        public let day: Int
        /// The start of the hour slot, in epoch milliseconds. Nil for daily columns.
        public let hourStartMs: Double?
        public var byProvider: [UsageProviderKind: Amount] = [:]

        public func value(_ provider: UsageProviderKind, _ metric: UsageMetric) -> Double {
            byProvider[provider]?.value(metric) ?? 0
        }

        public func total(_ metric: UsageMetric) -> Double {
            byProvider.values.reduce(0) { $0 + $1.value(metric) }
        }
    }

    public enum Resolution: Sendable { case hour, day }

    public static let empty = UsageTimeline(resolution: .day, timeZone: .gmt, untilMs: 0, columns: [])

    public let resolution: Resolution
    public let timeZone: TimeZone
    /// The end of a rolling window, which "today" and "yesterday" in a tooltip are counted from.
    public let untilMs: Double
    public var columns: [Column]

    init(window: UsageWindow) {
        let columns: [Column]
        switch window.resolution {
        case .hour:
            columns = (0..<24).map { index in
                let start = window.sinceMs + Double(index) * UsageWindow.hourMs
                return Column(day: window.localDay(ofMs: start), hourStartMs: start)
            }
        case .day:
            columns = (window.sinceDay...max(window.sinceDay, window.untilDay)).map { Column(day: $0, hourStartMs: nil) }
        }
        self.init(resolution: window.resolution == .hour ? .hour : .day, timeZone: window.timeZone,
                  untilMs: window.untilMs, columns: columns)
    }

    init(resolution: Resolution, timeZone: TimeZone, untilMs: Double, columns: [Column]) {
        self.resolution = resolution
        self.timeZone = timeZone
        self.untilMs = untilMs
        self.columns = columns
    }

    mutating func add(_ amount: Amount, provider: UsageProviderKind, slot: UsageWindow.Slot, firstDay: Int) {
        let index = resolution == .hour ? slot.hour : slot.day - firstDay
        guard columns.indices.contains(index) else { return }
        columns[index].byProvider[provider, default: Amount()].costUsd += amount.costUsd
        columns[index].byProvider[provider, default: Amount()].tokens += amount.tokens
    }
}

/// The two numbers T3 Code charts and ranks by.
public enum UsageMetric: Sendable {
    case cost, tokens
}

extension UsageTimeline {
    /// `2026-11-06` for a day counted from 1970-01-01.
    public static func isoDay(_ day: Int) -> String {
        let date = civil(day)
        return String(format: "%04d-%02d-%02d", date.year, date.month, date.day)
    }

    /// Howard Hinnant's days-to-civil conversion, which needs no calendar or time zone.
    static func civil(_ day: Int) -> (year: Int, month: Int, day: Int) {
        let shifted = day + 719_468
        let era = (shifted >= 0 ? shifted : shifted - 146_096) / 146_097
        let dayOfEra = shifted - era * 146_097
        let yearOfEra = (dayOfEra - dayOfEra / 1_460 + dayOfEra / 36_524 - dayOfEra / 146_096) / 365
        let dayOfYear = dayOfEra - (365 * yearOfEra + yearOfEra / 4 - yearOfEra / 100)
        let monthIndex = (5 * dayOfYear + 2) / 153
        let month = monthIndex < 10 ? monthIndex + 3 : monthIndex - 9
        let year = yearOfEra + era * 400 + (month <= 2 ? 1 : 0)
        return (year, month, dayOfYear - (153 * monthIndex + 2) / 5 + 1)
    }
}
