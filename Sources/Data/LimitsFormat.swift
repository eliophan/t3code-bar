import Foundation

public enum ClockStyle: Sendable {
    case twelveHour, twentyFourHour
}

/// Strings of T3's Limits tab. Nothing here reads the process locale: T3 follows the system locale,
/// this app always writes the en-US form so a window reads the same on every Mac.
public enum LimitsFormat {
    private static let minute = 60_000.0
    private static let hour = 60 * minute
    private static let day = 24 * hour

    /// T3's `Mt`: "3d 4h", "5h 12m" or "9m", never negative.
    public static func duration(milliseconds: Double) -> String {
        let value = max(0, milliseconds)
        let days = Int((value / day).rounded(.down))
        let hours = Int((value.truncatingRemainder(dividingBy: day) / hour).rounded(.down))
        let minutes = Int((value.truncatingRemainder(dividingBy: hour) / minute).rounded(.down))
        return days > 0 ? "\(days)d \(hours)h" : hours > 0 ? "\(hours)h \(minutes)m" : "\(minutes)m"
    }

    /// T3's `Nt`: "resets in 2d 3h", "Resets momentarily" or "resets now".
    public static func resetsIn(_ window: LimitWindow, now: Date) -> String? {
        guard let reset = window.resetsAt else { return nil }
        let remaining = LimitsLayout.ms(reset) - LimitsLayout.ms(now)
        if remaining <= 0 { return "resets now" }
        if remaining < minute { return "Resets momentarily" }
        return "resets in \(duration(milliseconds: remaining))"
    }

    /// The value next to "Resets" in a window card, for example "4:20 AM · in 2d 3h".
    public static func resets(_ window: LimitWindow, now: Date, calendar: Calendar = .current,
                              clock: ClockStyle = .twelveHour) -> String? {
        guard let reset = window.resetsAt else { return nil }
        var text = futureTime(reset, now: now, calendar: calendar, clock: clock)
        if let countdown = resetsIn(window, now: now) {
            text += " · " + (countdown.hasPrefix("resets in ") ? "in " + countdown.dropFirst("resets in ".count) : countdown)
        }
        return text
    }

    /// T3's reset-credit line. `compact` is the form inside a window card.
    public static func credits(_ credits: ResetCredits, now: Date, compact: Bool = false) -> String {
        let expires = credits.nextExpiresAt.map {
            duration(milliseconds: LimitsLayout.ms($0) - LimitsLayout.ms(now))
        }
        if credits.availableCount == 0 { return "No reset credits banked" }
        if compact { return "\(credits.availableCount) banked" + (expires.map { " · expires in \($0)" } ?? "") }
        let noun = credits.availableCount == 1 ? "reset credit" : "reset credits"
        return "\(credits.availableCount) \(noun) banked" + (expires.map { " · next expires in \($0)" } ?? "")
    }

    /// T3's relative time for a past instant: "just now", "5m ago", "3h ago", "2d ago".
    /// The age is floored to its unit first, so it ticks once a minute, hour or day.
    public static func ago(_ date: Date, now: Date) -> String {
        let age = max(0, LimitsLayout.ms(now) - LimitsLayout.ms(date))
        let unit = age < hour ? minute : age < day ? hour : day
        let seconds = Int(((age / unit).rounded(.down) * unit / 1000).rounded(.down))
        if seconds < 60 { return "just now" }
        let minutes = seconds / 60
        if minutes < 60 { return "\(minutes)m ago" }
        let hours = minutes / 60
        return hours < 24 ? "\(hours)h ago" : "\(hours / 24)d ago"
    }

    /// T3's `S` for a reset time: a time today, "tomorrow at <time>", or a date and time.
    /// A past instant reads "yesterday at <time>" or a date and time.
    public static func futureTime(_ date: Date, now: Date, calendar: Calendar, clock: ClockStyle) -> String {
        var gregorian = Calendar(identifier: .gregorian)
        gregorian.timeZone = calendar.timeZone
        let target = gregorian.dateComponents([.year, .month, .day, .hour, .minute], from: date)
        let today = gregorian.dateComponents([.year, .month, .day], from: now)
        let time = clockText(hour: target.hour ?? 0, minute: target.minute ?? 0, clock: clock)
        let offset = dayIndex(target) - dayIndex(today)
        func dated() -> String {
            let monthDay = "\(target.month ?? 0)/\(target.day ?? 0)"
            return (target.year == today.year ? monthDay : "\(monthDay)/\(target.year ?? 0)") + " " + time
        }
        switch offset {
        case ..<(-1): return dated()
        case -1: return "yesterday at \(time)"
        case 0: return time
        case 1: return "tomorrow at \(time)"
        default: return dated()
        }
    }

    private static func clockText(hour: Int, minute: Int, clock: ClockStyle) -> String {
        switch clock {
        case .twentyFourHour:
            return String(format: "%02d:%02d", hour, minute)
        case .twelveHour:
            return String(format: "%d:%02d %@", hour % 12 == 0 ? 12 : hour % 12, minute, hour < 12 ? "AM" : "PM")
        }
    }

    /// Days since 1970-01-01 of a civil date, so two dates differ by whole calendar days.
    private static func dayIndex(_ parts: DateComponents) -> Int {
        let year = (parts.year ?? 1970) - ((parts.month ?? 1) <= 2 ? 1 : 0)
        let era = (year >= 0 ? year : year - 399) / 400
        let yearOfEra = year - era * 400
        let month = parts.month ?? 1
        let dayOfYear = (153 * (month > 2 ? month - 3 : month + 9) + 2) / 5 + (parts.day ?? 1) - 1
        let dayOfEra = yearOfEra * 365 + yearOfEra / 4 - yearOfEra / 100 + dayOfYear
        return era * 146_097 + dayOfEra - 719_468
    }
}

/// What one window card shows, as T3 words it.
public struct LimitWindowDetail: Equatable, Sendable {
    public let left: String
    public let resets: String?
    public let evenPace: String?
    public let restores: String?
    public let credits: String?

    public init(member: LimitMember, reset: LimitReset?, now: Date, calendar: Calendar = .current,
                clock: ClockStyle = .twelveHour) {
        let window = member.window
        left = "\(LimitsLayout.leftPercent(window))%"
        resets = LimitsFormat.resets(window, now: now, calendar: calendar, clock: clock)
        evenPace = LimitsLayout.evenPaceLeft(window, now: now).map { "\($0)% left by now" }
        restores = reset.flatMap { $0.restoresPercent > 0 ? "+\($0.restoresPercent)% of pool" : nil }
        credits = member.account.resetCredits.flatMap {
            $0.availableCount > 0 ? LimitsFormat.credits($0, now: now, compact: true) : nil
        }
    }
}
