import Foundation

// Port of the Limits logic in T3 Code's client (`usageLimits-*.js` and `UsageLimits-*.js`).
// Names follow that code where it helps a reader compare the two.

public enum LimitWindowKind: Int, Sendable, CaseIterable {
    case session, weekly, monthly, other

    /// Unknown kinds sort and render as `other`, as in T3's own order table.
    public init(raw: String?) {
        switch raw {
        case "session": self = .session
        case "weekly": self = .weekly
        case "monthly": self = .monthly
        default: self = .other
        }
    }
}

public struct LimitWindow: Equatable, Sendable {
    public let id: String
    public let kind: LimitWindowKind
    public let label: String
    /// As T3 reports it. T3 clamps only where it shows "left", so the layout math uses this raw value.
    public let usedPercent: Double
    public let resetsAt: Date?
    public let durationMins: Double?

    public init(id: String, kind: LimitWindowKind, label: String, usedPercent: Double,
                resetsAt: Date? = nil, durationMins: Double? = nil) {
        self.id = id
        self.kind = kind
        self.label = label
        self.usedPercent = usedPercent
        self.resetsAt = resetsAt
        self.durationMins = durationMins
    }

    public var clampedUsedPercent: Double { min(100, max(0, usedPercent)) }
}

public struct ResetCredits: Equatable, Sendable {
    public let availableCount: Int
    public let nextExpiresAt: Date?

    public init(availableCount: Int, nextExpiresAt: Date? = nil) {
        self.availableCount = availableCount
        self.nextExpiresAt = nextExpiresAt
    }
}

public struct ExternalUsage: Equatable, Sendable {
    public let label: String
    public let url: String

    public init(label: String, url: String) {
        self.label = label
        self.url = url
    }
}

/// One signed-in account after T3's merge. It carries no email: the email only builds the merge key.
public struct LimitsAccount: Equatable, Sendable, Identifiable {
    public let id: String
    public let driver: String
    public let displayName: String?
    public let plan: String?
    public let checkedAt: Date
    public let windows: [LimitWindow]
    public let resetCredits: ResetCredits?
    /// Position of the account in T3's name-then-key order. It breaks ties between equal reset times.
    public let nameOrder: Int

    public init(id: String, driver: String, displayName: String? = nil, plan: String? = nil, checkedAt: Date,
                windows: [LimitWindow], resetCredits: ResetCredits? = nil, nameOrder: Int = 0) {
        self.id = id
        self.driver = driver
        self.displayName = displayName
        self.plan = plan
        self.checkedAt = checkedAt
        self.windows = windows
        self.resetCredits = resetCredits
        self.nameOrder = nameOrder
    }

    public var name: String { displayName ?? LimitsDrivers.label(driver) }

    /// How long ago T3 last probed this account. The file keeps old bars after a T3 restart, so show this.
    public func age(now: Date) -> TimeInterval { max(0, now.timeIntervalSince(checkedAt)) }
}

enum LimitsDate {
    /// ISO 8601 with or without fractional seconds. Anything else is no date, as `Date.parse` NaN is in T3.
    static func parse(_ text: String) -> Date? {
        (try? Date(text, strategy: Date.ISO8601FormatStyle(includingFractionalSeconds: true)))
            ?? (try? Date(text, strategy: .iso8601))
    }
}

public enum LimitsDrivers {
    private static let labels = [
        "codex": "Codex", "claudeAgent": "Claude", "antigravity": "Antigravity", "cursor": "Cursor",
        "grok": "Grok", "fx": "fx", "kiro": "Kiro", "muse": "Muse Code", "acpRegistry": "ACP Registry",
        "pi": "Pi", "opencode": "OpenCode",
    ]

    /// A driver the app does not know keeps its raw name.
    public static func label(_ driver: String) -> String { labels[driver] ?? driver }
}

public struct ExternalUsageLink: Equatable, Sendable {
    public let label: String
    public let url: String
    public let message: String?
    public let accounts: [String]
}

public enum LimitsState: Equatable, Sendable {
    case noCacheDirectory
    case noLimitsReported
    case ok
}

public struct LimitsSnapshot: Equatable, Sendable {
    public let accounts: [LimitsAccount]
    /// Accounts without bars that T3 reports in a warning, such as "Could not read limits."
    public let notices: [String]
    public let links: [ExternalUsageLink]
    public let state: LimitsState
    /// Cache files that could not be decoded and had no earlier good copy.
    public let skippedFiles: Int

    public init(accounts: [LimitsAccount] = [], notices: [String] = [], links: [ExternalUsageLink] = [],
                state: LimitsState, skippedFiles: Int = 0) {
        self.accounts = accounts
        self.notices = notices
        self.links = links
        self.state = state
        self.skippedFiles = skippedFiles
    }

    public func groups(now: Date) -> [LimitsDriverGroup] { LimitsLayout.groups(accounts, now: now) }

    public var newestCheck: Date? { accounts.map(\.checkedAt).max() }
}

public enum LimitPace: Sendable, Equatable {
    case ahead, on, under

    /// T3's tooltip text for the pace icon.
    public var label: String {
        switch self {
        case .ahead: return "Ahead of pace: spending faster than the window elapses"
        case .on: return "On pace with the window"
        case .under: return "Under pace: headroom left for the rest of the window"
        }
    }
}

/// An account's window after T3's "session shows 100% while weekly is exhausted" rule.
public struct LimitMember: Equatable, Sendable {
    public let account: LimitsAccount
    public let window: LimitWindow
    public let refillPercent: Double
}

public struct LimitColumn: Equatable, Sendable {
    public let account: LimitsAccount
    public let member: LimitMember?
}

public struct LimitReset: Equatable, Sendable {
    public let accountID: String
    public let at: Date
    public let restoresPercent: Int
}

public struct LimitRow: Equatable, Sendable, Identifiable {
    public let id: String
    public let kind: LimitWindowKind
    public let label: String
    public let members: [LimitMember]
    public let columns: [LimitColumn]
    public let usedPercent: Int
    public let remainingPercent: Int
    public let pace: LimitPace?
    public let resets: [LimitReset]

    public func reset(for accountID: String) -> LimitReset? { resets.first { $0.accountID == accountID } }
}

public struct LimitsDriverGroup: Equatable, Sendable, Identifiable {
    public var id: String { driver }
    public let driver: String
    public let accounts: [LimitsAccount]
    public let rows: [LimitRow]

    public var label: String { LimitsDrivers.label(driver) }
}

public enum LimitsLayout {
    static func ms(_ date: Date) -> Double { (date.timeIntervalSince1970 * 1000).rounded() }

    static func jsRound(_ value: Double) -> Int { Int((value + 0.5).rounded(.down)) }

    static func stableSorted<T>(_ items: [T], by areInIncreasingOrder: (T, T) -> Bool) -> [T] {
        items.enumerated().sorted { a, b in
            if areInIncreasingOrder(a.element, b.element) { return true }
            if areInIncreasingOrder(b.element, a.element) { return false }
            return a.offset < b.offset
        }.map(\.element)
    }

    /// T3's `Left`: whole percent, rounded half up, with used clamped to 0...100.
    public static func leftPercent(_ window: LimitWindow) -> Int { jsRound(100 - window.clampedUsedPercent) }

    /// Fraction of the window that has passed, or nil without a reset time and duration.
    public static func elapsedFraction(_ window: LimitWindow, now: Date) -> Double? {
        guard let reset = window.resetsAt, let mins = window.durationMins else { return nil }
        let duration = mins * 60_000
        guard duration > 0 else { return nil }
        return max(0, min(1, (duration - (ms(reset) - ms(now))) / duration))
    }

    public static func evenPaceLeft(_ window: LimitWindow, now: Date) -> Int? {
        elapsedFraction(window, now: now).map { jsRound((1 - $0) * 100) }
    }

    public static func pace(used: Double, elapsed: Double) -> LimitPace {
        let delta = used - elapsed * 100
        return delta > 5 ? .ahead : delta < -5 ? .under : .on
    }

    public static func pace(_ window: LimitWindow, now: Date) -> LimitPace? {
        elapsedFraction(window, now: now).map { pace(used: window.usedPercent, elapsed: $0) }
    }

    /// T3's `Y`. A Codex or Claude session shows 100% used while the weekly window is exhausted.
    static func resolve(_ account: LimitsAccount, _ window: LimitWindow, now: Date) -> LimitMember {
        let exhausted: LimitWindow? = window.kind == .session
            ? account.windows.first {
                $0.usedPercent >= 100 && (account.driver == "claudeAgent"
                    ? $0.id == "seven_day" : account.driver == "codex" && $0.kind == .weekly)
            } : nil
        guard let exhausted else {
            return LimitMember(account: account, window: window, refillPercent: window.usedPercent)
        }
        let floor = max(ms(now), ms(account.checkedAt))
        var resetsAt: Date?
        var refill = 0.0
        if let weeklyReset = exhausted.resetsAt, let sessionReset = window.resetsAt,
           ms(weeklyReset) > floor, ms(sessionReset) > floor {
            if ms(sessionReset) <= ms(weeklyReset) {
                resetsAt = weeklyReset
                refill = 100
            } else if window.usedPercent >= 100 {
                resetsAt = sessionReset
                refill = 100
            } else {
                resetsAt = weeklyReset
                refill = 100 - window.clampedUsedPercent
            }
        }
        let shown = LimitWindow(id: window.id, kind: window.kind, label: window.label, usedPercent: 100,
                                resetsAt: resetsAt, durationMins: nil)
        return LimitMember(account: account, window: shown, refillPercent: refill)
    }

    /// T3's `J`: Codex pools windows by kind, every other driver by kind and id.
    private static func poolKey(_ driver: String, _ window: LimitWindow) -> String {
        driver == "codex" ? "\(window.kind)" : "\(window.kind):\(window.id)"
    }

    /// T3's `Dt`: one group per driver in first-seen order, accounts by soonest reset.
    public static func groups(_ accounts: [LimitsAccount], now: Date) -> [LimitsDriverGroup] {
        var order: [String] = []
        var byDriver: [String: [LimitsAccount]] = [:]
        for account in accounts {
            if byDriver[account.driver] == nil { order.append(account.driver) }
            byDriver[account.driver, default: []].append(account)
        }
        return order.map { driver in
            let members = byDriver[driver] ?? []
            let first = stableSorted(members.flatMap(\.windows)) { $0.kind.rawValue < $1.kind.rawValue }.first
            func soonest(_ account: LimitsAccount) -> Double {
                guard let first, let window = account.windows.first(where: { poolKey(driver, $0) == poolKey(driver, first) })
                else { return .infinity }
                return resolve(account, window, now: now).window.resetsAt.map(ms) ?? .infinity
            }
            let sorted = stableSorted(members) { a, b in
                let (x, y) = (soonest(a), soonest(b))
                return x != y ? x < y : a.nameOrder < b.nameOrder
            }
            return LimitsDriverGroup(driver: driver, accounts: sorted,
                                     rows: displayOrder(driver, rows(sorted, now: now)))
        }
    }

    /// T3's `Ot`: windows of the same pool across accounts become one row.
    static func rows(_ accounts: [LimitsAccount], now: Date) -> [LimitRow] {
        var keys: [String] = []
        var pools: [String: [LimitMember]] = [:]
        for account in accounts {
            for window in account.windows {
                let member = resolve(account, window, now: now)
                let shared = poolKey(account.driver, member.window)
                let key = pools[shared]?.contains { $0.account.id == account.id } == true
                    ? "\(member.window.kind):\(member.window.id)" : shared
                if pools[key] == nil { keys.append(key) }
                pools[key, default: []].append(member)
            }
        }
        let rows = keys.map { key -> LimitRow in
            let members = pools[key] ?? []
            let head = members[0].window
            let used = members.reduce(0) { $0 + $1.window.usedPercent } / Double(members.count)
            let paced = members.compactMap { member in
                elapsedFraction(member.window, now: now).map { (used: member.window.usedPercent, elapsed: $0) }
            }
            let pace = paced.isEmpty ? nil : LimitsLayout.pace(
                used: paced.reduce(0) { $0 + $1.used } / Double(paced.count),
                elapsed: paced.reduce(0) { $0 + $1.elapsed } / Double(paced.count))
            let resets = stableSorted(members.compactMap { member in
                member.window.resetsAt.map { (member: member, at: $0) }
            }) { ms($0.at) < ms($1.at) }.map {
                LimitReset(accountID: $0.member.account.id, at: $0.at,
                           restoresPercent: jsRound($0.member.refillPercent / Double(members.count)))
            }
            return LimitRow(
                id: members[0].account.driver == "codex" ? key : head.id,
                kind: head.kind, label: head.label, members: members,
                columns: accounts.map { account in
                    LimitColumn(account: account, member: members.first { $0.account.id == account.id })
                },
                usedPercent: jsRound(used), remainingPercent: jsRound(100 - used), pace: pace, resets: resets)
        }
        return stableSorted(rows) { $0.kind.rawValue < $1.kind.rawValue }
    }

    private static let cursorOrder = ["totalPercentUsed", "autoPercentUsed", "apiPercentUsed"]

    /// T3's `Et`: Antigravity and Cursor reorder their rows.
    static func displayOrder(_ driver: String, _ rows: [LimitRow]) -> [LimitRow] {
        switch driver {
        case "antigravity":
            func stem(_ id: String) -> String {
                for suffix in ["-5h", "-weekly", "-quota"] where id.hasSuffix(suffix) {
                    return String(id.dropLast(suffix.count))
                }
                return id
            }
            return rows.sorted { a, b in
                let (ga, gb) = (a.id.hasPrefix("gemini-") ? 0 : 1, b.id.hasPrefix("gemini-") ? 0 : 1)
                if ga != gb { return ga < gb }
                let order = stem(a.id).compare(stem(b.id), locale: Locale(identifier: "en_US"))
                if order != .orderedSame { return order == .orderedAscending }
                return a.kind.rawValue < b.kind.rawValue
            }
        case "cursor":
            let hasBoth = rows.contains { $0.id == "autoPercentUsed" } && rows.contains { $0.id == "apiPercentUsed" }
            let kept = rows.filter { !hasBoth || $0.id != "totalPercentUsed" }
            return stableSorted(kept) {
                (cursorOrder.firstIndex(of: $0.id) ?? cursorOrder.count) < (cursorOrder.firstIndex(of: $1.id) ?? cursorOrder.count)
            }
        default:
            return rows
        }
    }
}
