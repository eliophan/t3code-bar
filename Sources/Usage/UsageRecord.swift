import Foundation

public enum UsageSpeed: Int, CaseIterable, Sendable {
    case standard = 0, fast, ultrafast
}

/// One row of T3 Code's scan cache, with the model and session tables already resolved.
///
/// The provider lives on the file, not here. `outputTokens` already includes `reasoningTokens`.
public struct UsageRecord: Equatable, Sendable {
    public var timestampMs: Double
    public var model: String
    /// Price this model instead of `model` when set.
    public var rateModel: String?
    public var session: String
    public var uncachedInput: Int
    public var cachedInput: Int
    public var cacheCreation: Int
    public var output: Int
    public var reasoning: Int
    public var reportedCostUsd: Double?
    public var speed: UsageSpeed
    public var dedupeKey: String?
    /// A Codex turn whose service tier T3 Code had not resolved when it wrote the cache.
    public var unresolvedTierTurn: String?

    public init(timestampMs: Double, model: String, rateModel: String? = nil, session: String = "",
                uncachedInput: Int = 0, cachedInput: Int = 0, cacheCreation: Int = 0, output: Int = 0,
                reasoning: Int = 0, reportedCostUsd: Double? = nil, speed: UsageSpeed = .standard,
                dedupeKey: String? = nil, unresolvedTierTurn: String? = nil) {
        self.timestampMs = timestampMs
        self.model = model
        self.rateModel = rateModel
        self.session = session
        self.uncachedInput = uncachedInput
        self.cachedInput = cachedInput
        self.cacheCreation = cacheCreation
        self.output = output
        self.reasoning = reasoning
        self.reportedCostUsd = reportedCostUsd
        self.speed = speed
        self.dedupeKey = dedupeKey
        self.unresolvedTierTurn = unresolvedTierTurn
    }

    /// T3 Code's "Processed tokens". Reasoning is part of output and is not added again.
    public var processed: Int { uncachedInput + cachedInput + cacheCreation + output }
}
