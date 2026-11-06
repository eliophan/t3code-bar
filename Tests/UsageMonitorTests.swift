import Foundation
import Testing
@testable import T3CodeBar

@MainActor
struct UsageMonitorTests {
    private func defaults() -> UserDefaults {
        UserDefaults(suiteName: "usage-tests-\(UUID().uuidString)")!
    }

    private func snapshot(_ period: UsagePeriod = .week, input: Int = 100) -> UsageSnapshot {
        UsageSnapshot(period: period, drivers: [
            DriverUsage(driver: "codex", tokens: TokenBreakdown(input: input), turns: 1, turnsWithoutUsage: 0, partialTurns: 0),
        ])
    }

    private func waitUntil(_ condition: () -> Bool) async {
        for _ in 0..<200 where !condition() { try? await Task.sleep(for: .milliseconds(10)) }
    }

    @Test func opensOnSevenDaysAndRemembersTheChoice() {
        let store = defaults()
        let first = UsageMonitor(defaults: store)
        #expect(first.period == .week)
        first.period = .quarter
        #expect(UsageMonitor(defaults: store).period == .quarter)
    }

    @Test func anUnknownStoredPeriodFallsBackToTheDefault() {
        let store = defaults()
        store.set(14, forKey: UsageMonitor.periodKey)
        #expect(UsageMonitor(defaults: store).period == .week)
    }

    @Test func aFailedReadKeepsTheLastGoodSnapshotAndRecordsTheError() {
        let monitor = UsageMonitor(defaults: defaults())
        monitor.apply(.success(snapshot(input: 100)))
        monitor.apply(.failure(.busy))
        #expect(monitor.snapshot?.total.input == 100)
        #expect(monitor.readError == .busy)
        monitor.apply(.success(snapshot(input: 200)))
        #expect(monitor.readError == nil)
        #expect(monitor.snapshot?.total.input == 200)
    }

    @Test func aSnapshotForAnotherPeriodIsMarkedStale() {
        let monitor = UsageMonitor(defaults: defaults())
        monitor.apply(.success(snapshot(.week)))
        #expect(monitor.isStale == false)
        monitor.period = .day
        #expect(monitor.isStale)
    }

    @Test func readsOnStartAndAgainWhenThePeriodChanges() async throws {
        let fixture = try DatabaseFixture()
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let usage: [String: Any] = ["inputTokens": 40, "cachedInputTokens": 0, "cacheCreationTokens": 0, "outputTokens": 2]
        try fixture.turn(driver: "codex", startedAt: formatter.string(from: Date().addingTimeInterval(-3 * 86_400)), usage: usage)

        let monitor = UsageMonitor(database: DatabaseManager(config: fixture.config), defaults: defaults())
        monitor.start()
        defer { monitor.stop() }
        await waitUntil { monitor.snapshot?.period == .week }
        #expect(monitor.snapshot?.total.processed == 42)

        monitor.period = .day
        await waitUntil { monitor.snapshot?.period == .day }
        #expect(monitor.snapshot?.isEmpty == true)
        #expect(monitor.isStale == false)
    }

    @Test func nothingIsReadBeforeStartOrAfterStop() async throws {
        let fixture = try DatabaseFixture()
        let monitor = UsageMonitor(database: DatabaseManager(config: fixture.config), defaults: defaults())
        monitor.period = .month
        try await Task.sleep(for: .milliseconds(100))
        #expect(monitor.snapshot == nil)

        monitor.start()
        await waitUntil { monitor.snapshot != nil }
        monitor.stop()
        let before = monitor.snapshot
        monitor.period = .day
        try await Task.sleep(for: .milliseconds(100))
        #expect(monitor.snapshot == before)
    }

    @Test func anIncompatibleDatabaseSurfacesAsAReadError() async throws {
        let fixture = try DatabaseFixture(schema: false)
        let monitor = UsageMonitor(database: DatabaseManager(config: fixture.config), defaults: defaults())
        monitor.start()
        defer { monitor.stop() }
        await waitUntil { monitor.readError != nil }
        #expect(monitor.readError == .incompatibleSchema)
        #expect(monitor.snapshot == nil)
    }
}

struct UsageScreenTextTests {
    private let en = Locale(identifier: "en_US")

    @Test func theSummaryLineNamesTurnsAndCacheHit() {
        #expect(UsageView.summary(turns: 1_091, cacheHit: 0.962, locale: en) == "1,091 turns · 96% cache hit")
        #expect(UsageView.summary(turns: 1, cacheHit: nil, locale: en) == "1 turn")
    }

    @Test func theFootnoteAppearsOnlyWhenSomeTurnsAreIncomplete() throws {
        #expect(UsageView.footnote(partial: 0, withoutUsage: 0) == nil)
        #expect(try #require(UsageView.footnote(partial: 12, withoutUsage: 0)).text == "12 turns reported partial usage.")
        #expect(try #require(UsageView.footnote(partial: 0, withoutUsage: 1)).text == "1 turn reported no usage.")
        let both = try #require(UsageView.footnote(partial: 2, withoutUsage: 3))
        #expect(both.text == "2 turns reported partial usage. 3 turns reported no usage.")
        #expect(both.help.contains("left out of the totals"))
    }

    @Test func aMissingTableExplainsItselfInsteadOfShowingARawError() {
        #expect(UsageView.noticeText(for: .incompatibleSchema) == "This T3 Code version does not record token usage.")
        #expect(UsageView.noticeText(for: .busy) == DatabaseReadError.busy.errorDescription)
    }

    @Test func turnsAreSingularForOneAndGroupedForMany() {
        #expect(TokenFormat.turns(1, locale: en) == "1 turn")
        #expect(TokenFormat.turns(0, locale: en) == "0 turns")
        #expect(TokenFormat.turns(5_701, locale: en) == "5,701 turns")
    }
}
