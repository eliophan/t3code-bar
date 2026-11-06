import Foundation

/// One row of T3 Code's model breakdown, with every string already written.
public struct UsageModelRow: Equatable, Identifiable, Sendable {
    public var id: String { "\(kind.raw):\(name)" }
    public let rank: Int
    public let kind: UsageProviderKind
    public let name: String
    /// `$12.34`, or `Unpriced` when every record of the model lacks a price.
    public let costText: String
    public let isUnpriced: Bool
    /// The model's share of the metric the tab ranks by. Empty for an unpriced model on Cost.
    public let shareText: String
    public let tokensText: String
    /// Fraction of the largest row, for the thin bar. Nil when the row has no value to draw.
    public let barShare: Double?
    public let spoken: String
}

/// A slice of the cost bars, before it gets a color.
public struct UsageCostSlice: Equatable, Sendable {
    public enum Kind: String, Sendable {
        case input, cacheRead, cacheWrite, output, other
        case standard, fast, ultrafast
    }

    public let kind: Kind
    public let label: String
    public let amount: Double
}

/// The strings and ordering of the Cost and Tokens tabs, kept apart from the views so they can be tested.
public enum UsagePresentation {
    /// Rows shown before "Show all N models".
    public static let collapsedModelCount = 5

    /// T3 Code's `Other` slice hides below half a cent.
    static let otherThreshold = 0.005

    public static func costHeadline(_ report: UsageReport) -> String { TokenFormat.currency(report.costUsd) }

    public static func costSubtitle(_ report: UsageReport) -> String {
        "\(TokenFormat.sessions(report.sessions)) · API estimate"
    }

    /// The tooltip of the Cost headline, present only when some records have no price.
    public static func unpricedNote(_ report: UsageReport) -> String? {
        report.unpricedShare > 0
            ? "API estimate excludes \(TokenFormat.percent(report.unpricedShare)) unpriced records." : nil
    }

    /// `29.5% of cost · 1.2B tokens`, as T3 Code writes a provider row on the Cost tab.
    public static func costProviderDetail(_ provider: UsageReport.Provider, in report: UsageReport) -> String {
        let share = report.costUsd == 0 ? 0 : provider.costUsd / report.costUsd
        return "\(TokenFormat.percent(share)) of cost · \(TokenFormat.compact(provider.tokens.processed)) tokens"
    }

    public static func costBySlices(_ report: UsageReport) -> [UsageCostSlice] {
        let cost = report.costByType
        return [
            .init(kind: .input, label: "Input", amount: cost.input),
            .init(kind: .cacheRead, label: "Cache read", amount: cost.cacheRead),
            .init(kind: .cacheWrite, label: "Cache write", amount: cost.cacheWrite),
            .init(kind: .output, label: "Output", amount: cost.output),
            .init(kind: .other, label: "Other", amount: cost.other >= otherThreshold ? cost.other : 0),
        ]
    }

    /// Present only when something ran faster than standard.
    public static func speedSlices(_ report: UsageReport) -> [UsageCostSlice] {
        let speed = report.costBySpeed
        guard speed.fast + speed.ultrafast > 0 else { return [] }
        return [
            .init(kind: .standard, label: "Standard", amount: speed.standard),
            .init(kind: .fast, label: "Fast", amount: speed.fast),
            .init(kind: .ultrafast, label: "Ultrafast", amount: speed.ultrafast),
        ]
    }

    public static func premiumText(_ report: UsageReport) -> String { "Premium \(TokenFormat.currency(report.costBySpeed.premium))" }

    /// T3 Code ranks models by cost then tokens on Cost, and by tokens then cost on Tokens.
    public static func modelRows(_ report: UsageReport, metric: UsageMetric) -> [UsageModelRow] {
        let ordered: [UsageReport.Model]
        switch metric {
        case .cost:
            ordered = report.models
        case .tokens:
            ordered = report.models.enumerated().sorted { a, b in
                if a.element.tokens.processed != b.element.tokens.processed { return a.element.tokens.processed > b.element.tokens.processed }
                if a.element.costUsd != b.element.costUsd { return a.element.costUsd > b.element.costUsd }
                return a.offset < b.offset
            }.map(\.element)
        }
        func value(_ model: UsageReport.Model) -> Double {
            metric == .cost ? model.costUsd : Double(model.tokens.processed)
        }
        let largest = ordered.map(value).max() ?? 0
        return ordered.enumerated().map { index, model in
            let amount = value(model)
            let share: Double?
            switch metric {
            case .cost:
                share = model.isUnpriced ? nil : (report.costUsd == 0 ? 0 : model.costUsd / report.costUsd)
            case .tokens:
                share = report.processedTokens == 0 ? 0 : Double(model.tokens.processed) / Double(report.processedTokens)
            }
            let costText = model.isUnpriced ? "Unpriced" : TokenFormat.currency(model.costUsd)
            let shareText = share.map(TokenFormat.percent) ?? ""
            let tokens = model.tokens.processed
            let costSpoken = model.isUnpriced ? "Unpriced" : TokenFormat.spokenCurrency(model.costUsd)
            let spoken = [
                "\(index + 1)", model.name, model.kind.style.label, costSpoken,
                share.map { "\(TokenFormat.percent($0)) of \(metric == .cost ? "cost" : "tokens")" },
                "\(TokenFormat.spoken(tokens)) tokens",
            ].compactMap { $0 }.joined(separator: ", ")
            return UsageModelRow(rank: index + 1, kind: model.kind, name: model.name, costText: costText,
                                 isUnpriced: model.isUnpriced, shareText: shareText,
                                 tokensText: TokenFormat.compact(tokens),
                                 barShare: amount > 0 && largest > 0 ? amount / largest : nil, spoken: spoken)
        }
    }

    /// `Show all 12 models`, or `Show fewer` once they are all out.
    public static func modelToggleTitle(count: Int, expanded: Bool) -> String {
        expanded ? "Show fewer" : "Show all \(count) models"
    }

    public static func visibleModels(_ rows: [UsageModelRow], expanded: Bool) -> [UsageModelRow] {
        expanded ? rows : Array(rows.prefix(collapsedModelCount))
    }

    /// `Oct 31 to Nov 6`, or `Nov 5, 4 PM to Nov 6, 4 PM` for Past 24h: T3 Code's range under its header.
    public static func rangeText(_ timeline: UsageTimeline) -> String? {
        guard let first = timeline.columns.first, let last = timeline.columns.last else { return nil }
        switch timeline.resolution {
        case .day:
            return "\(UsageChartFormat.dayLabel(first.day)) to \(UsageChartFormat.dayLabel(last.day))"
        case .hour:
            guard let start = first.hourStartMs else { return nil }
            return "\(UsageChartFormat.dateAndHour(start, timeZone: timeline.timeZone)) to "
                + UsageChartFormat.dateAndHour(timeline.untilMs, timeZone: timeline.timeZone)
        }
    }
}
