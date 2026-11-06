import Foundation
import Testing
@testable import T3CodeBar

private final class TestClock: @unchecked Sendable {
    private let lock = NSLock()
    private var current: Date

    init(_ iso: String) { current = date(iso) }

    var now: Date {
        get { lock.withLock { current } }
        set { lock.withLock { current = newValue } }
    }
}

/// A temporary T3 Code state directory with one Claude file of three records.
private struct StateFixture {
    static let now = "2026-11-06T09:08:30Z"

    let directory = temporaryDirectory()

    init(version: Int = 11, tokens: Int = 100, write: Bool = true) throws {
        if write { try cache(version: version, tokens: tokens) }
    }

    @discardableResult
    func cache(version: Int = 11, tokens: Int = 100) throws -> URL {
        let rows = (0..<3).map { index in
            CacheFixture.Row(timestampMs: ms("2026-11-06T08:00:00Z") + Double(index), session: index, uncached: tokens, output: 0, key: "k\(index)").json
        }
        var fixture = CacheFixture(sources: [("claude", "/c")], models: ["unpriced-model"], sessions: ["a", "b", "c"],
                                   files: [.init(path: "/c/a.jsonl", kind: "claude", rows: rows)])
        fixture.version = version
        return try fixture.write(to: directory)
    }

    var source: UsageSource { UsageSource(config: EnvironmentConfig(stateDirectory: directory), localPaths: .empty) }

    func engine(_ clock: TestClock) -> UsageEngine {
        UsageEngine(source: source, clock: { clock.now }, timeZone: TimeZone(identifier: "Asia/Ho_Chi_Minh")!)
    }
}

@MainActor
struct UsageMonitorTests {
    private func defaults() -> UserDefaults {
        UserDefaults(suiteName: "usage-tests-\(UUID().uuidString)")!
    }

    private func waitUntil(_ condition: () -> Bool) async {
        for _ in 0..<300 where !condition() { try? await Task.sleep(for: .milliseconds(10)) }
    }

    private func monitor(_ fixture: StateFixture, clock: TestClock = TestClock(StateFixture.now), store: UserDefaults? = nil) -> UsageMonitor {
        UsageMonitor(engine: fixture.engine(clock), defaults: store ?? defaults())
    }

    @Test func opensOnThirtyDaysAndRemembersTheChoice() throws {
        let store = defaults()
        let first = UsageMonitor(source: try StateFixture(write: false).source, defaults: store)
        #expect(first.period == .month)
        first.period = .quarter
        #expect(UsageMonitor(source: try StateFixture(write: false).source, defaults: store).period == .quarter)
    }

    @Test func aStoredChoiceFromTheSevenDayEraIsKeptAndAnUnknownOneFallsBack() throws {
        let store = defaults()
        store.set(7, forKey: UsageMonitor.periodKey)
        #expect(UsageMonitor(source: try StateFixture(write: false).source, defaults: store).period == .week)
        store.set(14, forKey: UsageMonitor.periodKey)
        #expect(UsageMonitor(source: try StateFixture(write: false).source, defaults: store).period == .month)
    }

    @Test func readsOnStartAndPublishesTheReport() async throws {
        let monitor = monitor(try StateFixture())
        monitor.start()
        defer { monitor.stop() }
        await waitUntil { monitor.report != nil }
        #expect(monitor.report?.tokens.processed == 300)
        #expect(monitor.report?.sessions == 3)
        #expect(monitor.reportPeriod == .month)
        #expect(monitor.notice == nil)
    }

    @Test func changingThePeriodRebuildsFromTheDecodedRecordsWithoutReadingTheCacheAgain() async throws {
        let fixture = try StateFixture()
        let engine = fixture.engine(TestClock(StateFixture.now))
        let monitor = UsageMonitor(engine: engine, defaults: defaults())
        monitor.start()
        defer { monitor.stop() }
        await waitUntil { monitor.reportPeriod == .month }
        monitor.period = .day
        await waitUntil { monitor.reportPeriod == .day }
        monitor.period = .quarter
        await waitUntil { monitor.reportPeriod == .quarter }
        #expect(monitor.isStale == false)
        #expect(await engine.cacheReads == 1)
    }

    @Test func nothingIsReadBeforeStartOrAfterStop() async throws {
        let monitor = monitor(try StateFixture())
        monitor.period = .week
        try await Task.sleep(for: .milliseconds(100))
        #expect(monitor.report == nil)

        monitor.start()
        await waitUntil { monitor.report != nil }
        monitor.stop()
        let before = monitor.report
        monitor.period = .day
        try await Task.sleep(for: .milliseconds(100))
        #expect(monitor.report == before)
    }

    @Test func stoppingFreesTheDecodedRecordsAndStartingReadsThemAgain() async throws {
        let fixture = try StateFixture()
        let engine = fixture.engine(TestClock(StateFixture.now))
        let monitor = UsageMonitor(engine: engine, defaults: defaults())
        monitor.start()
        await waitUntil { monitor.report != nil }
        monitor.stop()
        try await Task.sleep(for: .milliseconds(50))
        monitor.start()
        defer { monitor.stop() }
        for _ in 0..<300 where await engine.cacheReads < 2 { try await Task.sleep(for: .milliseconds(10)) }
        #expect(await engine.cacheReads == 2)
    }

    @Test func aMissingCacheTellsTheUserToOpenT3CodesUsagePage() async throws {
        let monitor = monitor(try StateFixture(write: false))
        monitor.start()
        defer { monitor.stop() }
        await waitUntil { monitor.notice != nil }
        #expect(monitor.notice == .missing)
        #expect(monitor.report == nil)
    }

    @Test func aFormatNewerThanThisBuildShowsTheChangedFormatNoticeAndNoNumbers() async throws {
        let monitor = monitor(try StateFixture(version: 12))
        monitor.start()
        defer { monitor.stop() }
        await waitUntil { monitor.notice != nil }
        #expect(monitor.notice == .unsupported(12))
        #expect(monitor.report == nil)
    }

    @Test func anUnreadableAnswerKeepsTheLastGoodReportUntilAFreshOneArrives() throws {
        let monitor = monitor(try StateFixture(write: false))
        var report = UsageReport()
        report.records = 1
        monitor.apply(.report(report), for: .month)
        monitor.apply(.unreadable, for: .month)
        #expect(monitor.notice == .retrying)
        #expect(monitor.report == report)
        monitor.apply(.report(report), for: .month)
        #expect(monitor.notice == nil)
    }

    @Test func aCacheT3IsRewritingIsUnreadableForNowAndRecoversWhenItIsWhole() async throws {
        let fixture = try StateFixture()
        let engine = fixture.engine(TestClock(StateFixture.now))
        guard case .report = await engine.report(for: .month) else { Issue.record("expected a report"); return }

        let url = fixture.directory.appendingPathComponent("usage-scan-cache-v11.json")
        let whole = try Data(contentsOf: url)
        try whole.prefix(whole.count / 2).write(to: url)
        #expect(await engine.report(for: .month) == .unreadable)

        try fixture.cache(tokens: 5_000)
        guard case .report(let recovered) = await engine.report(for: .month) else { Issue.record("expected a report"); return }
        #expect(recovered.tokens.processed == 15_000)
    }

    @Test func aChangedCacheIsPickedUpAndRepeatReadsInTheSameMinuteDoNothing() async throws {
        let fixture = try StateFixture()
        let clock = TestClock(StateFixture.now)
        let engine = fixture.engine(clock)

        guard case .report(let first) = await engine.report(for: .month) else { Issue.record("expected a report"); return }
        #expect(first.tokens.processed == 300)
        #expect(await engine.report(for: .month) == .unchanged)

        clock.now = clock.now.addingTimeInterval(60)
        guard case .report = await engine.report(for: .month) else { Issue.record("a new minute should rebuild"); return }

        try fixture.cache(tokens: 5_000)
        guard case .report(let changed) = await engine.report(for: .month) else { Issue.record("expected a report"); return }
        #expect(changed.tokens.processed == 15_000)
        #expect(await engine.cacheReads == 2)
    }

    @Test func aSnapshotForAnotherPeriodIsMarkedStale() throws {
        let monitor = monitor(try StateFixture(write: false))
        monitor.apply(.report(UsageReport()), for: .week)
        #expect(monitor.isStale)
        monitor.period = .week
        #expect(monitor.isStale == false)
    }
}

struct UsagePeriodTests {
    @Test func periodsAreInT3CodesOrderWithThirtyDaysAsTheDefault() {
        #expect(UsagePeriod.allCases.map(\.days) == [1, 7, 30, 90])
        #expect(UsagePeriod.default == .month)
    }

    @Test func everyPeriodHasAShortLabelAndT3CodesOwn() {
        #expect(UsagePeriod.allCases.map(\.shortLabel) == ["24h", "7d", "30d", "90d"])
        #expect(UsagePeriod.allCases.map(\.label) == ["Past 24h", "7 days", "30 days", "90 days"])
    }

    @Test func pastDayRollsAndTheOthersAreCalendarDays() {
        let zone = TimeZone(identifier: "Asia/Ho_Chi_Minh")!
        let now = date("2026-11-06T09:08:30Z")
        #expect(UsagePeriod.day.window(now: now, timeZone: zone).resolution == .hour)
        for period in [UsagePeriod.week, .month, .quarter] {
            let window = period.window(now: now, timeZone: zone)
            #expect(window.resolution == .day)
            #expect(window.untilDay - window.sinceDay == period.days - 1)
        }
    }
}

struct UsageScreenTextTests {
    @Test func theUpdatedLineIsRelativeToTheCacheTime() {
        let modified = date("2026-11-06T09:00:00Z")
        #expect(UsageView.updatedText(modified: modified, now: date("2026-11-06T09:00:30Z")) == "Updated just now")
        #expect(UsageView.updatedText(modified: modified, now: date("2026-11-06T09:05:00Z")) == "Updated 5 min ago")
        #expect(UsageView.updatedText(modified: modified, now: date("2026-11-06T12:30:00Z")) == "Updated 3 h ago")
        #expect(UsageView.updatedText(modified: modified, now: date("2026-11-09T09:00:00Z")) == "Updated 3 d ago")
        #expect(UsageView.updatedText(modified: modified, now: date("2026-11-06T08:59:00Z")) == "Updated just now")
    }

    @Test func aProviderRowQuotesItsShareOfTokensAndItsCost() {
        #expect(UsageView.providerDetail(tokens: 295, total: 1_000, costUsd: 682.875) == "29.5% · $682.88")
        #expect(UsageView.providerDetail(tokens: 0, total: 0, costUsd: 0) == "0.0% · $0.00")
    }

    @Test func partialDataIsReportedInT3CodesWords() {
        var report = UsageReport()
        #expect(UsageView.partialText(report) == nil)
        report.malformedRecords = 3
        #expect(UsageView.partialText(report) == "3 malformed or inconsistent usage record(s) were skipped; usage may be incomplete.")
        report.rejectedFiles = 2
        #expect(UsageView.partialText(report)?.hasSuffix("2 cache entries could not be read; usage may be incomplete.") == true)
    }

    @Test func eachNoticeExplainsWhatToDo() {
        #expect(UsageView.message(for: .missing)?.contains("Open T3 Code's Usage page") == true)
        #expect(UsageView.message(for: .unsupported(12)) == "T3 Code changed its usage format. Update T3 Code Bar to read it.")
        #expect(UsageView.message(for: .retrying) != nil)
    }
}
