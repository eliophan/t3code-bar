import Foundation

/// The labels of T3 Code's chart and date range. Like the rest of the screen they ignore the Mac's locale:
/// T3 Code writes en-US, with a 12-hour clock unless its own setting says otherwise.
public enum UsageChartFormat {
    private static let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]

    /// `Nov 6` for a local day counted from 1970-01-01.
    public static func dayLabel(_ day: Int) -> String {
        let date = UsageTimeline.civil(day)
        return "\(months[date.month - 1]) \(date.day)"
    }

    /// `4 PM`, `12 AM`.
    public static func hourLabel(_ ms: Double, timeZone: TimeZone) -> String {
        clock(hour: localParts(ms, timeZone).hour)
    }

    /// `Nov 5, 4 PM`.
    public static func dateAndHour(_ ms: Double, timeZone: TimeZone) -> String {
        let parts = localParts(ms, timeZone)
        return "\(dayLabel(parts.day)), \(clock(hour: parts.hour))"
    }

    /// A chart tooltip's hour: `4 PM today`, `4 PM yesterday`, or a date and hour further back.
    public static func tooltipHour(_ ms: Double, reference: Double, timeZone: TimeZone) -> String {
        let parts = localParts(ms, timeZone)
        switch localParts(reference, timeZone).day - parts.day {
        case 0: return "\(clock(hour: parts.hour)) today"
        case 1: return "\(clock(hour: parts.hour)) yesterday"
        default: return dateAndHour(ms, timeZone: timeZone)
        }
    }

    private static func clock(hour: Int) -> String {
        "\(hour % 12 == 0 ? 12 : hour % 12) \(hour < 12 ? "AM" : "PM")"
    }

    private static func localParts(_ ms: Double, _ timeZone: TimeZone) -> (day: Int, hour: Int) {
        let offset = Double(timeZone.secondsFromGMT(for: Date(timeIntervalSince1970: ms / 1000))) * 1000
        let local = ms + offset
        let day = (local / UsageWindow.dayMs).rounded(.down)
        return (Int(day), Int(((local - day * UsageWindow.dayMs) / UsageWindow.hourMs).rounded(.down)))
    }
}
