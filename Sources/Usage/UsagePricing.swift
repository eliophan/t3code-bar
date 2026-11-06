import Foundation

/// Dollars per token.
public struct TokenRates: Equatable, Sendable {
    public var input: Double
    public var output: Double
    public var cacheRead: Double
    public var cacheWrite: Double

    public init(input: Double, output: Double, cacheRead: Double? = nil, cacheWrite: Double? = nil) {
        self.input = input
        self.output = output
        self.cacheRead = cacheRead ?? input
        self.cacheWrite = cacheWrite ?? input
    }

    fileprivate func scaled(by factor: Double) -> TokenRates {
        TokenRates(input: input * factor, output: output * factor, cacheRead: cacheRead * factor, cacheWrite: cacheWrite * factor)
    }
}

/// A model's price at standard speed, and at the faster tiers when the price list has them.
public struct ModelRate: Equatable, Sendable {
    public var standard: TokenRates
    public var fast: TokenRates?
    public var ultrafast: TokenRates?

    public init(standard: TokenRates, fast: TokenRates? = nil, ultrafast: TokenRates? = nil) {
        self.standard = standard
        self.fast = fast
        self.ultrafast = ultrafast
    }

    /// A tier the model has no price for is billed at the standard price.
    public func rates(for speed: UsageSpeed) -> TokenRates {
        switch speed {
        case .standard: return standard
        case .fast: return fast ?? standard
        case .ultrafast: return ultrafast ?? standard
        }
    }
}

/// The price list T3 Code fetched, as `usage-model-rates.json` holds it (LiteLLM's format).
public struct PriceTable: Sendable {
    /// What one entry of the price list contributes. Everything else in the entry is ignored.
    struct Entry {
        var name: String
        var values: [String: Double]
        var fastMultiplier: Double?
    }

    private static let firstParty: Set<String> = ["anthropic", "deepseek", "gemini", "meta", "mistral", "openai", "xai"]
    private static let unpriceable: Set<String> = ["<synthetic>", "synthetic", "opus", "sonnet", "haiku", "fable"]

    public private(set) var rates: [String: ModelRate] = [:]

    public init() {}

    init(entries: [Entry]) {
        var order: [String] = []
        for entry in entries {
            guard let standard = Self.tokenRates(entry.values, suffix: "", standard: nil) else { continue }
            let key = entry.name.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
            guard !key.isEmpty else { continue }
            let multiplier = entry.fastMultiplier.flatMap { $0 > 0 ? $0 : nil }
            let fast = multiplier.map { standard.scaled(by: $0) } ?? Self.tokenRates(entry.values, suffix: "_priority", standard: standard)
            if rates[key] == nil { order.append(key) }
            rates[key] = ModelRate(standard: standard, fast: fast, ultrafast: Self.tokenRates(entry.values, suffix: "_ultrafast", standard: standard))
        }
        registerBareNames(order)
    }

    /// `provider/name` also answers to `name`, unless `name` has a price of its own. First-party providers win,
    /// and two providers of the same rank with different prices cancel the alias.
    private mutating func registerBareNames(_ order: [String]) {
        var candidates: [String: (rate: ModelRate?, firstParty: Bool)] = [:]
        var aliasOrder: [String] = []
        for key in order {
            guard let rate = rates[key], let slash = key.lastIndex(of: "/") else { continue }
            let alias = String(key[key.index(after: slash)...])
            guard !alias.isEmpty, rates[alias] == nil else { continue }
            let isFirstParty = Self.firstParty.contains(String(key[..<key.firstIndex(of: "/")!]))
            if let held = candidates[alias] {
                if isFirstParty && !held.firstParty {
                    candidates[alias] = (rate, true)
                } else if let heldRate = held.rate, isFirstParty == held.firstParty {
                    candidates[alias] = (Self.merged(heldRate, rate), isFirstParty)
                }
            } else {
                candidates[alias] = (rate, isFirstParty)
                aliasOrder.append(alias)
            }
        }
        for alias in aliasOrder {
            if let rate = candidates[alias]?.rate { rates[alias] = rate }
        }
    }

    private static func merged(_ lhs: ModelRate, _ rhs: ModelRate) -> ModelRate? {
        let fast = lhs.fast ?? rhs.fast
        let ultrafast = lhs.ultrafast ?? rhs.ultrafast
        guard lhs.standard == rhs.standard, fast == (rhs.fast ?? fast), ultrafast == (rhs.ultrafast ?? ultrafast) else { return nil }
        return ModelRate(standard: lhs.standard, fast: fast, ultrafast: ultrafast)
    }

    private static func tokenRates(_ values: [String: Double], suffix: String, standard: TokenRates?) -> TokenRates? {
        guard let input = values["input_cost_per_token" + suffix], let output = values["output_cost_per_token" + suffix] else { return nil }
        func cache(_ name: String, standardRate: Double?) -> Double {
            if let value = values[name + suffix] { return value }
            if let standard, standard.input > 0, let standardRate { return standardRate / standard.input * input }
            return input
        }
        return TokenRates(input: input, output: output,
                          cacheRead: cache("cache_read_input_token_cost", standardRate: standard?.cacheRead),
                          cacheWrite: cache("cache_creation_input_token_cost", standardRate: standard?.cacheWrite))
    }

    /// There is no fuzzy match. `opus`, `sonnet`, `haiku`, `fable` and empty names are never priced, and
    /// anything after a `[` (such as `[1m]`) is cut off first.
    public func rate(forModel model: String) -> ModelRate? {
        var key = model.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if let bracket = key.firstIndex(of: "[") { key = String(key[..<bracket]) }
        let name = key.lastIndex(of: "/").map { String(key[key.index(after: $0)...]) } ?? key
        guard !name.isEmpty, !Self.unpriceable.contains(name) else { return nil }
        return rates[key]
    }
}

/// `usageModelAliases` from T3 Code's settings: records of one model are priced and shown as another.
public struct ModelAliases: Equatable, Sendable {
    private var targets: [String: String] = [:]

    public init(_ raw: [String: String] = [:]) {
        for model in raw.keys {
            var seen: Set<String> = [model]
            var target = raw[model]!
            while let next = raw[target], !seen.contains(target) {
                seen.insert(target)
                target = next
            }
            // A target that is itself an alias here means the chain looped, so the model keeps its name.
            if raw[target] == nil { targets[model] = target }
        }
    }

    public func target(for model: String) -> String? { targets[model] }
}

/// `usagePriceOverrides` from T3 Code's settings, in dollars per million tokens there and per token here.
public struct PriceOverrides: Equatable, Sendable {
    public struct PerMillion: Equatable, Sendable {
        public var input: Double
        public var output: Double
        public var cacheRead: Double?
        public var cacheWrite: Double?

        public init(input: Double, output: Double, cacheRead: Double? = nil, cacheWrite: Double? = nil) {
            self.input = input
            self.output = output
            self.cacheRead = cacheRead
            self.cacheWrite = cacheWrite
        }
    }

    public private(set) var rates: [String: ModelRate] = [:]

    public init(_ perMillion: [String: PerMillion] = [:]) {
        for (model, price) in perMillion {
            rates[model.trimmingCharacters(in: .whitespacesAndNewlines)] = ModelRate(standard: TokenRates(
                input: price.input / 1e6, output: price.output / 1e6,
                cacheRead: (price.cacheRead ?? price.input) / 1e6, cacheWrite: (price.cacheWrite ?? price.input) / 1e6))
        }
    }
}

public enum CostSource: Equatable, Sendable {
    case modelPriced, providerReported, unpriced
}

public struct PricedRecord: Equatable, Sendable {
    public struct Categories: Equatable, Sendable {
        public var input = 0.0, cacheRead = 0.0, cacheWrite = 0.0, output = 0.0
        public var total: Double { input + cacheRead + cacheWrite + output }

        func scaled(by factor: Double) -> Categories {
            Categories(input: input * factor, cacheRead: cacheRead * factor, cacheWrite: cacheWrite * factor, output: output * factor)
        }
    }

    public var costUsd: Double
    public var source: CostSource
    /// Nil when the record has no price list entry, or the provider's reported cost has nothing to split.
    public var categories: Categories?
    /// What the faster tier added over the standard price.
    public var speedPremiumUsd: Double
    public var cacheSavingsUsd: Double
}

/// Everything needed to put a dollar figure on a record.
public struct PriceBook: Sendable {
    public var table: PriceTable
    public var overrides: PriceOverrides
    public var aliases: ModelAliases

    public init(table: PriceTable = PriceTable(), overrides: PriceOverrides = PriceOverrides(), aliases: ModelAliases = ModelAliases()) {
        self.table = table
        self.overrides = overrides
        self.aliases = aliases
    }

    /// An override wins over the price list, and switches off the provider's reported cost for that model.
    func resolvedRate(for record: UsageRecord, lookup: (String) -> ModelRate?) -> (rate: ModelRate?, isOverride: Bool) {
        if let override = overrides.rates[record.model.trimmingCharacters(in: .whitespacesAndNewlines)] { return (override, true) }
        let name = record.rateModel.flatMap { $0.isEmpty ? nil : $0 } ?? record.model
        return (lookup(name), false)
    }

    public func price(_ record: UsageRecord) -> PricedRecord {
        price(record, lookup: table.rate(forModel:))
    }

    func price(_ record: UsageRecord, lookup: (String) -> ModelRate?) -> PricedRecord {
        let (rate, isOverride) = resolvedRate(for: record, lookup: lookup)
        let reported = isOverride ? nil : record.reportedCostUsd.flatMap { $0.isFinite ? $0 : nil }
        guard let rate else {
            return PricedRecord(costUsd: reported ?? 0, source: reported == nil ? .unpriced : .providerReported,
                                categories: nil, speedPremiumUsd: 0, cacheSavingsUsd: 0)
        }
        let active = rate.rates(for: record.speed)
        let list = Self.categories(of: record, at: active)
        let listSum = list.total
        let savings = Double(record.cachedInput) * (active.input - active.cacheRead)
        if let reported, listSum <= 0 {
            return PricedRecord(costUsd: reported, source: .providerReported, categories: nil, speedPremiumUsd: 0, cacheSavingsUsd: savings)
        }
        let premium = record.speed == .standard ? 0 : listSum - Self.categories(of: record, at: rate.standard).total
        let scale = reported.map { $0 / listSum } ?? 1
        return PricedRecord(costUsd: reported ?? listSum, source: reported == nil ? .modelPriced : .providerReported,
                            categories: list.scaled(by: scale), speedPremiumUsd: premium * scale, cacheSavingsUsd: savings)
    }

    private static func categories(of record: UsageRecord, at rates: TokenRates) -> PricedRecord.Categories {
        .init(input: Double(record.uncachedInput) * rates.input, cacheRead: Double(record.cachedInput) * rates.cacheRead,
              cacheWrite: Double(record.cacheCreation) * rates.cacheWrite, output: Double(record.output) * rates.output)
    }
}
