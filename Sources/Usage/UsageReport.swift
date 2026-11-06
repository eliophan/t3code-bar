import Foundation

/// What T3 Code's Usage page shows for one window: the numbers behind its Cost and Tokens tabs.
public struct UsageReport: Equatable, Sendable {
    public struct Tokens: Equatable, Sendable {
        public var uncachedInput = 0
        public var cachedInput = 0
        public var cacheCreation = 0
        /// Includes `reasoning`.
        public var output = 0
        public var reasoning = 0

        /// T3 Code's "Processed tokens".
        public var processed: Int { uncachedInput + cachedInput + cacheCreation + output }

        /// Cached reads over all input. Nil when there was no input.
        public var cacheHit: Double? {
            let input = uncachedInput + cachedInput + cacheCreation
            return input > 0 ? Double(cachedInput) / Double(input) : nil
        }
    }

    public struct CostByType: Equatable, Sendable {
        public var input = 0.0, cacheRead = 0.0, cacheWrite = 0.0, output = 0.0
        /// Cost that has no split, such as a provider-reported total. Never negative.
        public var other = 0.0
    }

    public struct CostBySpeed: Equatable, Sendable {
        public var standard = 0.0, fast = 0.0, ultrafast = 0.0
        /// What the faster tiers added over standard prices.
        public var premium = 0.0
    }

    public struct Provider: Equatable, Sendable {
        public let kind: UsageProviderKind
        public var tokens = Tokens()
        public var costUsd = 0.0
        public var sessions = 0
        public var records = 0
    }

    public struct Model: Equatable, Sendable {
        public let kind: UsageProviderKind
        public let name: String
        public var tokens = Tokens()
        public var costUsd = 0.0
        public var records = 0
        public var unpricedRecords = 0

        /// T3 Code shows "Unpriced" when every record of the model is.
        public var isUnpriced: Bool { records > 0 && unpricedRecords >= records }
    }

    public var tokens = Tokens()
    public var costUsd = 0.0
    public var costByType = CostByType()
    public var costBySpeed = CostBySpeed()
    public var cacheSavingsUsd = 0.0
    /// Registry order, only providers with tokens or cost in the window.
    public var providers: [Provider] = []
    /// Largest cost first, then tokens.
    public var models: [Model] = []
    /// Cost and tokens per provider for each hour or day of the window, for the chart.
    public var timeline = UsageTimeline.empty
    public var sessions = 0
    public var records = 0
    public var unpricedRecords = 0
    public var duplicatesDropped = 0
    /// Cache file entries dropped for an undecodable row, so the numbers may be low.
    public var rejectedFiles = 0
    /// Transcript lines T3 Code could not read.
    public var malformedRecords = 0
    /// When T3 Code last wrote the cache.
    public var cacheModified = Date(timeIntervalSince1970: 0)

    public var localSources: [LocalUsageStatus] = []
    public var scanCacheMissing = false

    public var processedTokens: Int { tokens.processed }

    /// Share of records without a price. The Cost headline's tooltip quotes it.
    public var unpricedShare: Double { records > 0 ? Double(unpricedRecords) / Double(records) : 0 }

    public var isEmpty: Bool { records == 0 }

    /// Cost headline's partial-data warning.
    public var isPartial: Bool { rejectedFiles > 0 || malformedRecords > 0 || scanCacheMissing || localSources.contains { $0.state == .partial || $0.state == .unreadable } }
}

enum UsageReportBuilder {
    private struct BucketKey: Hashable {
        let day: Int
        let hour: Int
        let source: Int
        let model: String
    }

    /// T3 Code rounds some sums per bucket (day, hour slot, provider, model, source) before adding buckets up,
    /// so the rounding has to happen at that grain to land on the same cents.
    private struct Bucket {
        let kind: UsageProviderKind
        let model: String
        var tokens = UsageReport.Tokens()
        var cost = 0.0, savings = 0.0
        var categories: PricedRecord.Categories?
        var fast = 0.0, ultrafast = 0.0, premium = 0.0
        var records = 0, unpriced = 0
    }

    static func build(dataset: UsageDataset, window: UsageWindow, prices: PriceBook) -> UsageReport {
        var timeline = UsageTimeline(window: window)
        var buckets: [Bucket] = []
        var indexes: [BucketKey: Int] = [:]
        var rates: [String: ModelRate?] = [:]
        func rate(_ name: String) -> ModelRate? {
            if let known = rates[name] { return known }
            let found = prices.table.rate(forModel: name)
            rates[name] = .some(found)
            return found
        }

        let selection = dataset.select(in: window) { counted in
            var record = counted.record
            if let target = prices.aliases.target(for: record.model) {
                record.model = target
                record.rateModel = nil
            }
            let key = BucketKey(day: counted.slot.day, hour: counted.slot.hour, source: counted.sourceIndex, model: record.model)
            let index: Int
            if let known = indexes[key] {
                index = known
            } else {
                index = buckets.count
                indexes[key] = index
                buckets.append(Bucket(kind: counted.kind, model: record.model))
            }
            let priced = prices.price(record, lookup: rate)
            timeline.add(.init(costUsd: priced.costUsd, tokens: record.processed), provider: counted.kind,
                         slot: counted.slot, firstDay: window.sinceDay)
            buckets[index].tokens.uncachedInput += record.uncachedInput
            buckets[index].tokens.cachedInput += record.cachedInput
            buckets[index].tokens.cacheCreation += record.cacheCreation
            buckets[index].tokens.output += record.output
            buckets[index].tokens.reasoning += record.reasoning
            buckets[index].cost += priced.costUsd
            buckets[index].savings += priced.cacheSavingsUsd
            if let categories = priced.categories {
                var sum = buckets[index].categories ?? PricedRecord.Categories()
                sum.input += categories.input
                sum.cacheRead += categories.cacheRead
                sum.cacheWrite += categories.cacheWrite
                sum.output += categories.output
                buckets[index].categories = sum
            }
            if record.speed == .fast { buckets[index].fast += priced.costUsd }
            if record.speed == .ultrafast { buckets[index].ultrafast += priced.costUsd }
            buckets[index].premium += priced.speedPremiumUsd
            buckets[index].records += 1
            if priced.source == .unpriced { buckets[index].unpriced += 1 }
        }

        var report = UsageReport()
        var providers: [UsageProviderKind: UsageReport.Provider] = [:]
        var models: [UsageProviderKind: [String: UsageReport.Model]] = [:]
        for tally in selection.sources where tally.distinctSessions > 0 {
            providers[tally.kind, default: .init(kind: tally.kind)].sessions += tally.distinctSessions
        }
        for bucket in buckets {
            report.tokens.add(bucket.tokens)
            report.costUsd += bucket.cost
            report.cacheSavingsUsd += bucket.savings
            if let categories = bucket.categories {
                report.costByType.input += rounded(categories.input)
                report.costByType.cacheRead += rounded(categories.cacheRead)
                report.costByType.cacheWrite += rounded(categories.cacheWrite)
                report.costByType.output += rounded(categories.output)
            }
            report.costBySpeed.fast += rounded(bucket.fast)
            report.costBySpeed.ultrafast += rounded(bucket.ultrafast)
            report.costBySpeed.premium += rounded(bucket.premium)
            report.records += bucket.records
            report.unpricedRecords += bucket.unpriced

            providers[bucket.kind, default: .init(kind: bucket.kind)].tokens.add(bucket.tokens)
            providers[bucket.kind]?.costUsd += bucket.cost
            providers[bucket.kind]?.records += bucket.records
            var model = models[bucket.kind]?[bucket.model] ?? .init(kind: bucket.kind, name: bucket.model)
            model.tokens.add(bucket.tokens)
            model.costUsd += bucket.cost
            model.records += bucket.records
            model.unpricedRecords += bucket.unpriced
            models[bucket.kind, default: [:]][bucket.model] = model
        }
        let categoryTotal = report.costByType.input + report.costByType.cacheRead + report.costByType.cacheWrite + report.costByType.output
        report.costByType.other = max(0, report.costUsd - categoryTotal)
        report.costBySpeed.standard = max(0, report.costUsd - report.costBySpeed.fast - report.costBySpeed.ultrafast)
        report.providers = providers.values
            .filter { $0.tokens.processed > 0 || $0.costUsd > 0 }
            .sorted { UsageProviderKind.displayOrder($0.kind, $1.kind) }
        report.models = models.values.flatMap(\.values).sorted {
            if $0.costUsd != $1.costUsd { return $0.costUsd > $1.costUsd }
            if $0.tokens.processed != $1.tokens.processed { return $0.tokens.processed > $1.tokens.processed }
            return ($0.kind.raw, $0.name) < ($1.kind.raw, $1.name)
        }
        report.timeline = timeline
        report.sessions = selection.sessions
        report.duplicatesDropped = selection.duplicatesDropped
        report.rejectedFiles = dataset.rejectedFiles
        report.malformedRecords = selection.sources.reduce(0) { $0 + $1.malformed }
        report.cacheModified = dataset.modified
        return report
    }

    /// JavaScript's `Math.round(value * 1e6) / 1e6`.
    private static func rounded(_ value: Double) -> Double { (value * 1e6 + 0.5).rounded(.down) / 1e6 }
}

extension UsageReport.Tokens {
    mutating func add(_ other: UsageReport.Tokens) {
        uncachedInput += other.uncachedInput
        cachedInput += other.cachedInput
        cacheCreation += other.cacheCreation
        output += other.output
        reasoning += other.reasoning
    }
}
