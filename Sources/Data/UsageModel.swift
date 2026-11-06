import Foundation

/// A rolling window ending now, as in T3 Code's Usage page.
public enum UsagePeriod: Int, CaseIterable, Sendable {
    case day = 1, week = 7, month = 30, quarter = 90

    public static let `default`: UsagePeriod = .week

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

    public func start(before now: Date) -> Date {
        now.addingTimeInterval(-Double(days) * 86_400)
    }
}

/// Token counts as providers report them. `input` already includes cached reads and cache writes,
/// so the parts shown to the user are derived from it rather than stored twice.
public struct TokenBreakdown: Equatable, Sendable {
    public var input = 0
    public var cachedInput = 0
    public var cacheCreation = 0
    public var output = 0

    public init(input: Int = 0, cachedInput: Int = 0, cacheCreation: Int = 0, output: Int = 0) {
        self.input = input
        self.cachedInput = cachedInput
        self.cacheCreation = cacheCreation
        self.output = output
    }

    public var uncachedInput: Int { max(0, input - cachedInput - cacheCreation) }

    /// T3 Code's "Processed tokens".
    public var processed: Int { input + output }

    /// T3 Code's formula: cached reads over all input. Nil when there was no input.
    public var cacheHit: Double? {
        let denominator = uncachedInput + cachedInput + cacheCreation
        return denominator > 0 ? Double(cachedInput) / Double(denominator) : nil
    }

    public static func + (lhs: TokenBreakdown, rhs: TokenBreakdown) -> TokenBreakdown {
        TokenBreakdown(input: lhs.input + rhs.input, cachedInput: lhs.cachedInput + rhs.cachedInput,
                       cacheCreation: lhs.cacheCreation + rhs.cacheCreation, output: lhs.output + rhs.output)
    }
}

public struct ProviderUsage: Equatable, Sendable {
    public let provider: ProviderKind
    public let tokens: TokenBreakdown
    public let turns: Int
}

/// One `GROUP BY driver` row from the database, before drivers are mapped to providers.
struct DriverUsage: Sendable {
    let driver: String?
    let tokens: TokenBreakdown
    let turns: Int
    let turnsWithoutUsage: Int
    let partialTurns: Int
}

public struct UsageSnapshot: Equatable, Sendable {
    public let period: UsagePeriod
    /// Providers with at least one turn that reported usage, largest first.
    public let providers: [ProviderUsage]
    /// Turns that reported no token counts at all.
    public let turnsWithoutUsage: Int
    /// Turns counted in the totals whose provider flagged the counts as incomplete.
    public let partialTurns: Int

    public var total: TokenBreakdown { providers.reduce(TokenBreakdown()) { $0 + $1.tokens } }
    public var turns: Int { providers.reduce(0) { $0 + $1.turns } }
    public var isEmpty: Bool { turns == 0 }

    public init(period: UsagePeriod, providers: [ProviderUsage], turnsWithoutUsage: Int, partialTurns: Int) {
        self.period = period
        self.providers = providers
        self.turnsWithoutUsage = turnsWithoutUsage
        self.partialTurns = partialTurns
    }

    /// Drivers that resolve to the same provider are merged, so every unknown driver becomes one row.
    init(period: UsagePeriod, drivers: [DriverUsage]) {
        var byProvider: [ProviderKind: ProviderUsage] = [:]
        for row in drivers where row.turns > 0 {
            let kind = ProviderKind.resolve(row.driver ?? "")
            let merged = byProvider[kind].map {
                ProviderUsage(provider: kind, tokens: $0.tokens + row.tokens, turns: $0.turns + row.turns)
            }
            byProvider[kind] = merged ?? ProviderUsage(provider: kind, tokens: row.tokens, turns: row.turns)
        }
        let ranked = byProvider.values.sorted {
            $0.tokens.processed != $1.tokens.processed
                ? $0.tokens.processed > $1.tokens.processed
                : $0.provider.rawValue < $1.provider.rawValue
        }
        self.init(period: period, providers: ranked,
                  turnsWithoutUsage: drivers.reduce(0) { $0 + $1.turnsWithoutUsage },
                  partialTurns: drivers.reduce(0) { $0 + $1.partialTurns })
    }
}

/// Token counts the way T3 Code writes them: compact, at most one decimal (`1.1B`, `193M`, `4.2K`).
public enum TokenFormat {
    private static let units: [(scale: Double, symbol: String, word: String)] = [
        (1e9, "B", "billion"), (1e6, "M", "million"), (1e3, "K", "thousand"),
    ]

    public static func compact(_ count: Int, locale: Locale = .current) -> String {
        let (value, unit) = scaled(count)
        return number(value, locale: locale) + (unit?.symbol ?? "")
    }

    /// For VoiceOver, where "193M" would be read letter by letter.
    public static func spoken(_ count: Int, locale: Locale = .current) -> String {
        let (value, unit) = scaled(count)
        return number(value, locale: locale) + (unit.map { " " + $0.word } ?? "")
    }

    public static func turns(_ count: Int, locale: Locale = .current) -> String {
        "\(count.formatted(.number.locale(locale))) \(count == 1 ? "turn" : "turns")"
    }

    public static func percent(_ ratio: Double) -> String {
        "\(Int((ratio * 100).rounded()))%"
    }

    private static func scaled(_ count: Int) -> (value: Double, unit: (scale: Double, symbol: String, word: String)?) {
        let magnitude = Double(max(0, count))
        for (index, unit) in units.enumerated() where magnitude >= unit.scale {
            let value = rounded(magnitude / unit.scale)
            // 999.96M rounds to 1000M, which reads as 1B.
            if value >= 1000, index > 0 {
                return (rounded(magnitude / units[index - 1].scale), units[index - 1])
            }
            return (value, unit)
        }
        return (magnitude, nil)
    }

    private static func rounded(_ value: Double) -> Double { (value * 10).rounded() / 10 }

    private static func number(_ value: Double, locale: Locale) -> String {
        value.formatted(.number.precision(.fractionLength(0...1)).locale(locale))
    }
}
