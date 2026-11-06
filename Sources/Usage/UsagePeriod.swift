import Foundation

/// The four periods of T3 Code's Usage page, in its order.
public enum UsagePeriod: Int, CaseIterable, Sendable {
    case day = 1, week = 7, month = 30, quarter = 90

    /// T3 Code opens on 30 days.
    public static let `default`: UsagePeriod = .month

    public var days: Int { rawValue }

    /// T3 Code's label, used for the tooltip and VoiceOver.
    public var label: String {
        switch self {
        case .day: return "Past 24h"
        case .week: return "7 days"
        case .month: return "30 days"
        case .quarter: return "90 days"
        }
    }

    /// The label that fits the 320 pt popover.
    public var shortLabel: String {
        switch self {
        case .day: return "24h"
        case .week: return "7d"
        case .month: return "30d"
        case .quarter: return "90d"
        }
    }

    public func window(now: Date, timeZone: TimeZone) -> UsageWindow {
        self == .day ? .rolling24h(now: now, timeZone: timeZone) : .calendarDays(days, now: now, timeZone: timeZone)
    }
}
