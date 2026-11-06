import Foundation
import Testing
@testable import T3CodeBar

struct UsageDatabaseTests {
    private let now = Date(timeIntervalSince1970: 1_793_966_400) // 2026-11-06T12:00:00Z

    private func database(_ fixture: DatabaseFixture) -> DatabaseManager { DatabaseManager(config: fixture.config) }

    private func timestamp(secondsFromNow offset: Double) -> String {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter.string(from: now.addingTimeInterval(offset))
    }

    private func usage(input: Int, cached: Int = 0, creation: Int = 0, output: Int = 0,
                       status: String = "complete") -> [String: Any] {
        ["inputTokens": input, "cachedInputTokens": cached, "cacheCreationTokens": creation,
         "outputTokens": output, "reasoningTokens": output / 2, "usageStatus": status]
    }

    @Test(arguments: UsagePeriod.allCases)
    func aTurnJustInsideThePeriodCountsAndOneJustOutsideDoesNot(period: UsagePeriod) async throws {
        let fixture = try DatabaseFixture()
        let edge = -Double(period.days) * 86_400
        try fixture.turn(driver: "codex", startedAt: timestamp(secondsFromNow: edge + 1), usage: usage(input: 100))
        try fixture.turn(driver: "codex", startedAt: timestamp(secondsFromNow: edge - 1), usage: usage(input: 1_000))
        let snapshot = try await database(fixture).usage(period: period, now: now)
        #expect(snapshot.total.input == 100)
        #expect(snapshot.turns == 1)
    }

    @Test func aTurnExactlyAtTheCutoffCounts() async throws {
        let fixture = try DatabaseFixture()
        try fixture.turn(driver: "codex", startedAt: timestamp(secondsFromNow: -86_400), usage: usage(input: 5))
        #expect(try await database(fixture).usage(period: .day, now: now).turns == 1)
    }

    @Test func widerPeriodsIncludeEverythingTheNarrowerOnesDo() async throws {
        let fixture = try DatabaseFixture()
        for (days, input) in [(0.5, 1), (3.0, 10), (20.0, 100), (60.0, 1_000)] {
            try fixture.turn(driver: "codex", startedAt: timestamp(secondsFromNow: -days * 86_400), usage: usage(input: input))
        }
        let totals = try await UsagePeriod.allCases.asyncMap { try await database(fixture).usage(period: $0, now: now).total.input }
        #expect(totals == [1, 11, 111, 1_111])
    }

    @Test func driversMapToProvidersAndAnUnknownOneIsOther() async throws {
        let fixture = try DatabaseFixture()
        let start = timestamp(secondsFromNow: -3_600)
        try fixture.turn(driver: "codex", startedAt: start, usage: usage(input: 900, cached: 800, output: 50))
        try fixture.turn(driver: "claudeAgent", startedAt: start, usage: usage(input: 400, cached: 300, creation: 100, output: 20))
        try fixture.turn(driver: "somethingNew", startedAt: start, usage: usage(input: 10))
        let snapshot = try await database(fixture).usage(period: .week, now: now)
        #expect(snapshot.providers.map(\.provider) == [.openAI, .claude, .unknown])
        #expect(snapshot.providers[1].tokens == TokenBreakdown(input: 400, cachedInput: 300, cacheCreation: 100, output: 20))
    }

    @Test func turnsOfOneDriverAreSummed() async throws {
        let fixture = try DatabaseFixture()
        for input in [100, 200, 300] {
            try fixture.turn(driver: "codex", startedAt: timestamp(secondsFromNow: -60), usage: usage(input: input, output: 1))
        }
        let codex = try await database(fixture).usage(period: .day, now: now).providers.first
        #expect(codex?.turns == 3)
        #expect(codex?.tokens.processed == 603)
    }

    @Test func countsPastInt32AreKept() async throws {
        let fixture = try DatabaseFixture()
        for _ in 0..<3 {
            try fixture.turn(driver: "codex", startedAt: timestamp(secondsFromNow: -60), usage: usage(input: 2_000_000_000))
        }
        #expect(try await database(fixture).usage(period: .day, now: now).total.input == 6_000_000_000)
    }

    @Test func turnsWithoutUsageAreCountedSeparatelyAndLeftOutOfTheSums() async throws {
        let fixture = try DatabaseFixture()
        let start = timestamp(secondsFromNow: -60)
        try fixture.turn(driver: "codex", startedAt: start, usage: usage(input: 100))
        try fixture.turn(driver: "codex", startedAt: start)
        try fixture.turn(driver: nil, startedAt: start)
        try fixture.turn(driver: "antigravity", startedAt: start, usage: ["usageStatus": "unavailable", "usageScope": "main_agent"])
        let snapshot = try await database(fixture).usage(period: .day, now: now)
        #expect(snapshot.turns == 1)
        #expect(snapshot.turnsWithoutUsage == 3)
        #expect(snapshot.total.input == 100)
    }

    @Test func partialTurnsAreCountedAndKeepTheirTokens() async throws {
        let fixture = try DatabaseFixture()
        let start = timestamp(secondsFromNow: -60)
        try fixture.turn(driver: "codex", startedAt: start, usage: usage(input: 100))
        try fixture.turn(driver: "codex", startedAt: start, usage: usage(input: 50, status: "partial"))
        let snapshot = try await database(fixture).usage(period: .day, now: now)
        #expect(snapshot.partialTurns == 1)
        #expect(snapshot.turns == 2)
        #expect(snapshot.total.input == 150)
    }

    @Test func turnsWithoutAStartTimeBelongToNoPeriod() async throws {
        let fixture = try DatabaseFixture()
        try fixture.execute("INSERT INTO orchestration_v2_projection_provider_turns VALUES ('x', 't', NULL, '{}')")
        let snapshot = try await database(fixture).usage(period: .quarter, now: now)
        #expect(snapshot.isEmpty)
        #expect(snapshot.turnsWithoutUsage == 0)
    }

    @Test func aPeriodWithNoTurnsIsEmptyRatherThanAnError() async throws {
        let fixture = try DatabaseFixture()
        let snapshot = try await database(fixture).usage(period: .week, now: now)
        #expect(snapshot.isEmpty)
        #expect(snapshot.providers.isEmpty)
        #expect(snapshot.total.cacheHit == nil)
    }

    @Test func aDatabaseWithoutTheTurnsTableIsReportedAsIncompatible() async throws {
        let fixture = try DatabaseFixture(schema: false)
        await #expect(throws: DatabaseReadError.incompatibleSchema) {
            try await database(fixture).usage(period: .week, now: now)
        }
    }

    @Test func aMissingDatabaseIsReportedAsMissing() async throws {
        let fixture = try DatabaseFixture()
        try FileManager.default.removeItem(at: fixture.config.stateDbUrl)
        await #expect(throws: DatabaseReadError.missing) {
            try await database(fixture).usage(period: .week, now: now)
        }
    }

    @Test func payloadsWithoutTheUsageFieldsDegradeToTurnsWithoutUsage() async throws {
        let fixture = try DatabaseFixture()
        try fixture.turn(driver: "codex", startedAt: timestamp(secondsFromNow: -60), usage: ["somethingElse": 1])
        let snapshot = try await database(fixture).usage(period: .day, now: now)
        #expect(snapshot.isEmpty)
        #expect(snapshot.turnsWithoutUsage == 1)
    }
}

private extension Sequence {
    func asyncMap<T>(_ transform: (Element) async throws -> T) async rethrows -> [T] {
        var result: [T] = []
        for element in self { result.append(try await transform(element)) }
        return result
    }
}
