import Foundation

/// A period of the Usage page, with T3 Code's exact edges.
///
/// "Past 24h" slides with the clock: it ends at the current minute, starts 24 hours earlier, and
/// counts a record when `since <= time < until`. The longer periods are whole calendar days in the
/// Mac's time zone, today included in full, even for hours that have not happened yet.
public struct UsageWindow: Sendable {
    public enum Resolution: Sendable { case hour, day }

    public struct Slot: Equatable, Sendable {
        /// Local calendar day as days since 1970-01-01.
        public let day: Int
        /// The hour slot counted from the start of a rolling window, or -1 for calendar days.
        public let hour: Int
    }

    public static let hourMs = 3_600_000.0
    public static let dayMs = 86_400_000.0
    /// T3 Code reads files modified up to 36 hours before the window, to catch late writes.
    static let fileSlackMs = 129_600_000.0
    static let retentionMs = 7_948_800_000.0

    public let resolution: Resolution
    public let timeZone: TimeZone
    public let nowMs: Double
    /// Rolling windows only.
    public let sinceMs: Double
    public let untilMs: Double
    /// First and last local day, as days since 1970-01-01.
    public let sinceDay: Int
    public let untilDay: Int
    private let offsets: ZoneOffsets

    public static func rolling24h(now: Date, timeZone: TimeZone = .current) -> UsageWindow {
        let nowMs = Self.milliseconds(now)
        let until = (nowMs / 60_000).rounded(.down) * 60_000
        let since = until - 24 * hourMs
        let offsets = ZoneOffsets(timeZone: timeZone, around: nowMs)
        return UsageWindow(resolution: .hour, timeZone: timeZone, nowMs: nowMs, sinceMs: since, untilMs: until,
                           sinceDay: offsets.day(ofMs: since), untilDay: offsets.day(ofMs: until), offsets: offsets)
    }

    public static func calendarDays(_ count: Int, now: Date, timeZone: TimeZone = .current) -> UsageWindow {
        let nowMs = Self.milliseconds(now)
        let offsets = ZoneOffsets(timeZone: timeZone, around: nowMs)
        let today = offsets.day(ofMs: nowMs)
        return UsageWindow(resolution: .day, timeZone: timeZone, nowMs: nowMs, sinceMs: 0, untilMs: 0,
                           sinceDay: today - (max(count, 1) - 1), untilDay: today, offsets: offsets)
    }

    /// The local calendar day of an instant, as days since 1970-01-01.
    public func localDay(ofMs timestampMs: Double) -> Int { offsets.day(ofMs: timestampMs) }

    static func milliseconds(_ date: Date) -> Double { (date.timeIntervalSince1970 * 1000).rounded(.down) }

    /// The slot a record at `timestampMs` falls into, or nil when it is outside the window.
    public func slot(ofMs timestampMs: Double) -> Slot? {
        switch resolution {
        case .hour:
            guard timestampMs >= sinceMs, timestampMs < untilMs else { return nil }
            return Slot(day: offsets.day(ofMs: timestampMs), hour: Int(((timestampMs - sinceMs) / Self.hourMs).rounded(.down)))
        case .day:
            // No zone is further than 14 hours from UTC, so most records are rejected before any zone lookup.
            let margin = 14 * Self.hourMs
            guard timestampMs >= Double(sinceDay) * Self.dayMs - margin,
                  timestampMs < Double(untilDay + 1) * Self.dayMs + margin else { return nil }
            let day = offsets.day(ofMs: timestampMs)
            return day >= sinceDay && day <= untilDay ? Slot(day: day, hour: -1) : nil
        }
    }

    /// Files modified before this are not read.
    var fileCutoffMs: Double {
        let start = (resolution == .hour ? sinceMs : Double(sinceDay) * Self.dayMs) - Self.fileSlackMs
        return max(start, nowMs - Self.retentionMs)
    }
}

/// Local-time offsets for one zone around "now", so a record's day costs a comparison instead of a zone lookup.
struct ZoneOffsets: Sendable {
    private let timeZone: TimeZone
    private let segments: [(from: Double, offsetMs: Double)]
    private let coveredFrom: Double
    private let coveredTo: Double

    init(timeZone: TimeZone, around nowMs: Double) {
        self.timeZone = timeZone
        coveredFrom = nowMs - 130 * UsageWindow.dayMs
        coveredTo = nowMs + 3 * UsageWindow.dayMs
        func offset(at ms: Double) -> Double { Double(timeZone.secondsFromGMT(for: Date(timeIntervalSince1970: ms / 1000))) * 1000 }
        var list = [(from: coveredFrom, offsetMs: offset(at: coveredFrom))]
        var cursor = Date(timeIntervalSince1970: coveredFrom / 1000)
        while let next = timeZone.nextDaylightSavingTimeTransition(after: cursor), next.timeIntervalSince1970 * 1000 < coveredTo {
            let ms = next.timeIntervalSince1970 * 1000
            list.append((ms, offset(at: ms)))
            cursor = next
        }
        segments = list
    }

    func day(ofMs ms: Double) -> Int {
        var offset: Double
        if ms >= coveredFrom, ms < coveredTo {
            offset = segments[0].offsetMs
            for segment in segments where ms >= segment.from { offset = segment.offsetMs }
        } else {
            offset = Double(timeZone.secondsFromGMT(for: Date(timeIntervalSince1970: ms / 1000))) * 1000
        }
        return Int(((ms + offset) / UsageWindow.dayMs).rounded(.down))
    }
}
