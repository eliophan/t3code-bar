import Foundation
import SQLite3
import Testing
@testable import T3CodeBar

struct UsageReportTests {
    private let zone = TimeZone(identifier: "Asia/Ho_Chi_Minh")!
    private let now = date("2026-11-06T09:08:00Z")
    private let noon = ms("2026-11-05T05:00:00Z")

    private var prices: PriceBook {
        PriceBook(table: PriceTable(entries: [
            .init(name: "m", values: ["input_cost_per_token": 1e-6, "output_cost_per_token": 2e-6, "cache_read_input_token_cost": 1e-7,
                                      "cache_creation_input_token_cost": 1.5e-6], fastMultiplier: 2),
        ]))
    }

    private func report(_ files: [CacheFixture.FileEntry], sources: [(String, String)] = [("claude", "/c"), ("codex", "/x")],
                        prices: PriceBook? = nil, days: Int = 7) throws -> UsageReport {
        let cache = CacheFixture(sources: sources, models: ["m", "mystery", "alias-me"], sessions: ["", "s1", "s2"], files: files)
        return UsageReportBuilder.build(dataset: try cache.decoded(), window: .calendarDays(days, now: now, timeZone: zone),
                                        prices: prices ?? self.prices)
    }

    private func row(_ ts: Double, model: Int = 0, session: Int = 1, uncached: Int = 0, cached: Int = 0, creation: Int = 0,
                     output: Int = 0, reasoning: Int = 0, cost: Double? = nil, speed: Int = 0) -> String {
        CacheFixture.Row(timestampMs: ts, model: model, session: session, uncached: uncached, cached: cached, creation: creation,
                         output: output, reasoning: reasoning, cost: cost, speed: speed).json
    }

    private func file(_ kind: String, _ rows: [String]) -> CacheFixture.FileEntry {
        .init(path: "/\(kind == "claude" ? "c" : "x")/a.jsonl", kind: kind, rows: rows)
    }

    @Test func totalsSplitTokensByTypeAndProcessedExcludesReasoningTwice() throws {
        let report = try report([file("claude", [row(noon, uncached: 10, cached: 20, creation: 30, output: 40, reasoning: 15)])])
        #expect(report.tokens == .init(uncachedInput: 10, cachedInput: 20, cacheCreation: 30, output: 40, reasoning: 15))
        #expect(report.tokens.processed == 100)
        #expect(report.tokens.cacheHit == 20.0 / 60.0)
    }

    @Test func costByTypeSplitsTheCostAndOtherHoldsWhatHasNoSplit() throws {
        let report = try report([file("claude", [
            row(noon, uncached: 1_000_000, cached: 1_000_000, creation: 1_000_000, output: 1_000_000),
            row(noon + 1, model: 1, uncached: 5, cost: 0.25),
        ])])
        #expect(abs(report.costByType.input - 1.0) < 1e-9)
        #expect(abs(report.costByType.cacheRead - 0.1) < 1e-9)
        #expect(abs(report.costByType.cacheWrite - 1.5) < 1e-9)
        #expect(abs(report.costByType.output - 2.0) < 1e-9)
        #expect(abs(report.costByType.other - 0.25) < 1e-9)
        #expect(abs(report.costUsd - 4.85) < 1e-9)
    }

    @Test func costBySpeedSplitsFastAndStandardAndTotalsThePremium() throws {
        let report = try report([file("codex", [row(noon, uncached: 1_000_000), row(noon + 1, uncached: 1_000_000, speed: 1)])])
        #expect(abs(report.costBySpeed.fast - 2.0) < 1e-9)
        #expect(abs(report.costBySpeed.standard - 1.0) < 1e-9)
        #expect(abs(report.costBySpeed.premium - 1.0) < 1e-9)
    }

    @Test func cacheSavingsAreCachedTokensTimesInputMinusCacheReadPrice() throws {
        let report = try report([file("claude", [row(noon, cached: 1_000_000)])])
        #expect(abs(report.cacheSavingsUsd - 0.9) < 1e-9)
    }

    @Test func categoryCostsRoundPerBucketToAMillionthOfADollarBeforeTheyAreAdded() throws {
        // 0.4 millionths of input cost per record: two in one bucket round to 1, split across two days round to 0.
        let sameDay = try report([file("claude", [row(noon, uncached: 0), row(noon + 1, uncached: 1), row(noon + 2, uncached: 1)])],
                                 prices: PriceBook(table: PriceTable(entries: [
                                    .init(name: "m", values: ["input_cost_per_token": 4e-7, "output_cost_per_token": 0], fastMultiplier: nil)])))
        #expect(abs(sameDay.costByType.input - 1e-6) < 1e-12)
        let twoDays = try report([file("claude", [row(noon, uncached: 1), row(noon - 86_400_000, uncached: 1)])],
                                 prices: PriceBook(table: PriceTable(entries: [
                                    .init(name: "m", values: ["input_cost_per_token": 4e-7, "output_cost_per_token": 0], fastMultiplier: nil)])))
        #expect(twoDays.costByType.input == 0)
        #expect(abs(twoDays.costUsd - 8e-7) < 1e-15)
    }

    @Test func anAliasMovesRecordsToTheTargetModelAndPricesThemThere() throws {
        let aliased = PriceBook(table: prices.table, aliases: ModelAliases(["alias-me": "m"]))
        let report = try report([file("claude", [row(noon, model: 2, uncached: 1_000_000)])], prices: aliased)
        #expect(report.models.map(\.name) == ["m"])
        #expect(abs(report.costUsd - 1.0) < 1e-9)
        #expect(report.unpricedRecords == 0)
    }

    @Test func unpricedRecordsAreCountedAndAModelIsUnpricedOnlyWhenEveryRecordIs() throws {
        let report = try report([file("claude", [
            row(noon, model: 1, uncached: 10), row(noon + 1, model: 1, uncached: 10), row(noon + 2, model: 0, uncached: 10),
        ])])
        #expect(report.unpricedRecords == 2)
        #expect(abs(report.unpricedShare - 2.0 / 3.0) < 1e-12)
        #expect(report.models.first { $0.name == "mystery" }?.isUnpriced == true)
        #expect(report.models.first { $0.name == "m" }?.isUnpriced == false)
        #expect(report.tokens.uncachedInput == 30)
    }

    @Test func providersFollowT3sOrderAndHideThoseWithoutTokensOrCost() throws {
        let report = try report([
            file("codex", [row(noon, uncached: 5)]), file("claude", [row(noon, uncached: 9)]),
            .init(path: "/p/a.jsonl", kind: "pi", rows: [row(noon, uncached: 0)]),
        ], sources: [("pi", "/p"), ("codex", "/x"), ("claude", "/c")])
        #expect(report.providers.map(\.kind) == [.codex, .claude])
    }

    @Test func aProviderWithOnlyReportedCostStillShows() throws {
        let report = try report([file("claude", [row(noon, model: 1, uncached: 0, cost: 0.4)])])
        #expect(report.providers.map(\.kind) == [.claude])
        #expect(report.providers[0].costUsd == 0.4)
    }

    @Test func anUnknownProviderKindCountsInTheTotalsWithItsRawName() throws {
        let report = try report([file("claude", [row(noon, uncached: 5)]), .init(path: "/n/a.jsonl", kind: "newcomer", rows: [row(noon, uncached: 7)])],
                                sources: [("claude", "/c"), ("newcomer", "/n")])
        #expect(report.tokens.uncachedInput == 12)
        #expect(report.providers.map(\.kind) == [.claude, .other("newcomer")])
        #expect(report.providers.last?.kind.style.label == "newcomer")
    }

    @Test func modelsAreSortedByCostThenTokens() throws {
        let report = try report([file("claude", [
            row(noon, model: 0, uncached: 1_000), row(noon + 1, model: 1, uncached: 9_000_000), row(noon + 2, model: 2, uncached: 2_000),
        ])])
        #expect(report.models.map(\.name) == ["m", "mystery", "alias-me"])
    }

    @Test func sessionsAndPartialDataComeFromTheCacheAndSources() throws {
        var entry = file("claude", [row(noon, session: 1), row(noon + 1, session: 2)])
        entry.malformed = 3
        let report = try report([entry])
        #expect(report.sessions == 2)
        #expect(report.malformedRecords == 3)
        #expect(report.isPartial)
        #expect(!report.isEmpty)
    }

    @Test func anEmptyWindowGivesAnEmptyReport() throws {
        let report = try report([file("claude", [row(ms("2026-01-01T00:00:00Z"), uncached: 5)])])
        #expect(report.isEmpty)
        #expect(report.providers.isEmpty)
        #expect(report.costUsd == 0)
    }
}

struct CodexTierTests {
    private var database: OpaquePointer?
    private let url = temporaryDirectory().appendingPathExtension("sqlite")

    init() throws {
        #expect(sqlite3_open(url.path, &database) == SQLITE_OK)
        for statement in [
            "CREATE TABLE orchestration_events (aggregate_kind TEXT, application_event_version INTEGER, stream_id TEXT, event_type TEXT, payload_json TEXT, sequence INTEGER)",
            "CREATE TABLE orchestration_v2_projection_provider_turns (run_attempt_id TEXT, thread_id TEXT, provider_thread_id TEXT, payload_json TEXT)",
            "CREATE TABLE orchestration_v2_projection_run_attempts (attempt_id TEXT, thread_id TEXT, provider_thread_id TEXT, run_id TEXT, payload_json TEXT)",
        ] { try run(statement) }
    }

    private func run(_ sql: String) throws {
        guard sqlite3_exec(database, sql, nil, nil, nil) == SQLITE_OK else { throw TierFixtureError.sql(sql) }
    }

    private enum TierFixtureError: Error { case sql(String) }

    /// One Codex turn with a recorded model selection.
    private func addTurn(_ turn: String, thread: String = "native-1", selection: String?, attempt: String? = nil) throws {
        let attempt = attempt ?? "att-\(turn)"
        try run("INSERT INTO orchestration_v2_projection_run_attempts VALUES ('\(attempt)', 't-\(attempt)', 'pt-\(attempt)', 'run-\(attempt)', '{\"nativeThreadId\":\"\(thread)\"}')")
        try run("INSERT INTO orchestration_v2_projection_provider_turns VALUES ('\(attempt)', 't-\(attempt)', 'pt-\(attempt)', '{\"nativeTurnRef\":{\"driver\":\"codex\",\"nativeId\":\"\(turn)\"}}')")
        if let selection {
            let payload = "{\"id\":\"run-\(attempt)\",\"activeAttemptId\":\"\(attempt)\",\"status\":\"running\",\"modelSelection\":\(selection)}"
            try run("INSERT INTO orchestration_events VALUES ('thread', 2, 't-\(attempt)', 'run.updated', '\(payload)', 1)")
        }
    }

    private func dataset(_ turns: [String], model: String = "gpt-6.1-sol", session: String = "native-1", kind: String = "codex") throws -> UsageDataset {
        let rows = turns.map { CacheFixture.Row(timestampMs: 1, model: 0, session: 0, uncached: 1, turn: $0).json }
        let cache = CacheFixture(sources: [(kind, "/x")], models: [model], sessions: [session],
                                 files: [.init(path: "/x/a.jsonl", kind: kind, rows: rows)])
        return try cache.decoded()
    }

    private func speeds(_ dataset: UsageDataset) -> [UsageSpeed] { dataset.files.flatMap(\.records).map(\.speed) }

    private func selection(_ options: String, model: String = "gpt-6.1-sol") -> String {
        "{\"model\":\"\(model)\",\"options\":[\(options)]}"
    }

    @Test func aServiceTierOptionDecidesTheSpeed() throws {
        try addTurn("t1", selection: selection("{\"id\":\"serviceTier\",\"value\":\"fast\"}"))
        try addTurn("t2", selection: selection("{\"id\":\"serviceTier\",\"value\":\"ultrafast\"}"))
        try addTurn("t3", selection: selection("{\"id\":\"serviceTier\",\"value\":\"default\"}"))
        try addTurn("t4", selection: selection("{\"id\":\"serviceTier\",\"value\":\"priority\"}"))
        let resolved = CodexTierResolver.resolve(try dataset(["t1", "t2", "t3", "t4"]), database: url)
        #expect(speeds(resolved) == [.fast, .ultrafast, .standard, .fast])
        #expect(resolved.files[0].records.allSatisfy { $0.unresolvedTierTurn == nil })
    }

    @Test func fastModeWithoutATierMeansFastAndNoOptionsMeansStandard() throws {
        try addTurn("t1", selection: selection("{\"id\":\"fastMode\",\"value\":true}"))
        try addTurn("t2", selection: selection(""))
        try addTurn("t3", selection: selection("{\"id\":\"fastMode\",\"value\":false}"))
        #expect(speeds(CodexTierResolver.resolve(try dataset(["t1", "t2", "t3"]), database: url)) == [.fast, .standard, .standard])
    }

    @Test func aTurnThatCannotBeMatchedStaysUnresolved() throws {
        try addTurn("odd-tier", selection: selection("{\"id\":\"serviceTier\",\"value\":\"weird\"}"))
        try addTurn("not-text", selection: selection("{\"id\":\"serviceTier\",\"value\":3}"))
        try addTurn("other-thread", thread: "someone-else", selection: selection("{\"id\":\"serviceTier\",\"value\":\"fast\"}"))
        try addTurn("other-model", selection: selection("{\"id\":\"serviceTier\",\"value\":\"fast\"}", model: "gpt-other"))
        try addTurn("no-event", selection: nil)
        let turns = ["odd-tier", "not-text", "other-thread", "other-model", "no-event", "unknown-turn"]
        let resolved = CodexTierResolver.resolve(try dataset(turns), database: url)
        #expect(speeds(resolved) == Array(repeating: .standard, count: 6))
        #expect(resolved.files[0].records.allSatisfy { $0.unresolvedTierTurn != nil })
    }

    @Test func aTurnIdThatAppearsTwiceIsAmbiguousAndLeftAlone() throws {
        try addTurn("dup", selection: selection("{\"id\":\"serviceTier\",\"value\":\"fast\"}"), attempt: "a1")
        try addTurn("dup", selection: selection("{\"id\":\"serviceTier\",\"value\":\"fast\"}"), attempt: "a2")
        #expect(speeds(CodexTierResolver.resolve(try dataset(["dup"]), database: url)) == [.standard])
    }

    @Test func otherProvidersAreNotLookedUp() throws {
        try addTurn("t1", selection: selection("{\"id\":\"serviceTier\",\"value\":\"fast\"}"))
        #expect(speeds(CodexTierResolver.resolve(try dataset(["t1"], kind: "claude"), database: url)) == [.standard])
    }

    @Test func aMissingDatabaseLeavesTheCacheAsItIs() throws {
        let missing = temporaryDirectory().appendingPathComponent("statev2.sqlite")
        let original = try dataset(["t1"])
        let resolved = CodexTierResolver.resolve(original, database: missing)
        #expect(resolved.files[0].records == original.files[0].records)
        #expect(!FileManager.default.fileExists(atPath: missing.path))
    }
}
