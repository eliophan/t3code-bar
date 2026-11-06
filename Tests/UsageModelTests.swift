import Foundation
import Testing
@testable import T3CodeBar

struct TokenBreakdownTests {
    @Test func processedIsInputPlusOutputBecauseInputAlreadyCountsCaches() {
        let tokens = TokenBreakdown(input: 1_000, cachedInput: 600, cacheCreation: 100, output: 50)
        #expect(tokens.processed == 1_050)
        #expect(tokens.uncachedInput == 300)
    }

    @Test func uncachedInputNeverGoesNegative() {
        // Claude reports cache writes that can exceed the input by a rounding sliver.
        let tokens = TokenBreakdown(input: 100, cachedInput: 90, cacheCreation: 20, output: 0)
        #expect(tokens.uncachedInput == 0)
    }

    @Test func cacheHitIsCachedReadsOverAllInput() throws {
        let tokens = TokenBreakdown(input: 1_000, cachedInput: 600, cacheCreation: 100, output: 50)
        #expect(try #require(tokens.cacheHit) == 0.6)
    }

    @Test func cacheHitIsNilWithoutInput() {
        #expect(TokenBreakdown().cacheHit == nil)
        #expect(TokenBreakdown(output: 500).cacheHit == nil)
    }

    @Test func breakdownsAddFieldByField() {
        let sum = TokenBreakdown(input: 10, cachedInput: 4, cacheCreation: 1, output: 2)
            + TokenBreakdown(input: 5, cachedInput: 3, cacheCreation: 0, output: 7)
        #expect(sum == TokenBreakdown(input: 15, cachedInput: 7, cacheCreation: 1, output: 9))
    }
}

struct UsagePeriodTests {
    @Test func periodsAreRollingDayCountsInT3CodesOrder() {
        #expect(UsagePeriod.allCases.map(\.days) == [1, 7, 30, 90])
        #expect(UsagePeriod.default == .week)
    }

    @Test func startIsExactlyThatManyDaysBeforeNow() {
        let now = Date(timeIntervalSince1970: 10_000_000)
        #expect(UsagePeriod.week.start(before: now) == now.addingTimeInterval(-7 * 86_400))
    }

    @Test func everyPeriodHasAShortLabelAndAFullOne() {
        #expect(UsagePeriod.allCases.map(\.shortLabel) == ["24h", "7d", "30d", "90d"])
        #expect(UsagePeriod.allCases.map(\.label) == ["Past 24h", "7 days", "30 days", "90 days"])
    }
}

struct UsageSnapshotTests {
    private func driver(_ name: String?, input: Int, output: Int = 0, turns: Int = 1,
                        without: Int = 0, partial: Int = 0) -> DriverUsage {
        DriverUsage(driver: name, tokens: TokenBreakdown(input: input, output: output),
                    turns: turns, turnsWithoutUsage: without, partialTurns: partial)
    }

    @Test func providersAreRankedByProcessedTokens() {
        let snapshot = UsageSnapshot(period: .week, drivers: [
            driver("opencode", input: 10, output: 5),
            driver("codex", input: 900, output: 100),
            driver("claudeAgent", input: 500),
        ])
        #expect(snapshot.providers.map(\.provider) == [.openAI, .claude, .openCode])
    }

    @Test func unknownDriversShareOneRowAndAMissingDriverIsUnknown() {
        let snapshot = UsageSnapshot(period: .week, drivers: [
            driver("brandNew", input: 100), driver(nil, input: 50, turns: 2), driver("codex", input: 1_000),
        ])
        let unknown = snapshot.providers.first { $0.provider == .unknown }
        #expect(unknown?.tokens.input == 150)
        #expect(unknown?.turns == 3)
        #expect(snapshot.providers.count == 2)
    }

    @Test func driversWithoutAnyUsageProduceNoRowButStillCountTheirTurns() {
        let snapshot = UsageSnapshot(period: .week, drivers: [
            driver("codex", input: 0, turns: 0, without: 4), driver("muse", input: 20, without: 1, partial: 1),
        ])
        #expect(snapshot.providers.map(\.provider) == [.muse])
        #expect(snapshot.turnsWithoutUsage == 5)
        #expect(snapshot.partialTurns == 1)
    }

    @Test func totalsAndTurnsSumAcrossProviders() {
        let snapshot = UsageSnapshot(period: .month, drivers: [
            driver("codex", input: 100, output: 10, turns: 3), driver("claudeAgent", input: 40, output: 4, turns: 2),
        ])
        #expect(snapshot.total.processed == 154)
        #expect(snapshot.turns == 5)
        #expect(snapshot.isEmpty == false)
        #expect(UsageSnapshot(period: .day, drivers: []).isEmpty)
    }
}
