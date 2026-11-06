import Foundation
import Testing
@testable import T3CodeBar

private final class TestClock: @unchecked Sendable {
    private let lock = NSLock()
    private var value: Date
    init(_ value: Date) { self.value = value }
    var date: Date { lock.withLock { value } }
    func advance(_ seconds: TimeInterval) { lock.withLock { value = value.addingTimeInterval(seconds) } }
}

@MainActor
struct LimitsMonitorTests {
    private func monitor(_ fixture: CachesFixture, clock: TestClock = TestClock(limitDate("2026-11-06T10:00:00Z")))
        -> (LimitsMonitor, LimitsReader) {
        let reader = LimitsReader(cachesDirectory: fixture.directory)
        return (LimitsMonitor(reader: reader, clock: { clock.date }), reader)
    }

    private func codex(used: Double, checkedAt: String = "2026-11-06T09:00:00Z") -> [String: Any] {
        limitsProviderObject("codex", checkedAt: checkedAt, windows: [limitsWindowObject("primary", kind: "session", used: used)])
    }

    private func waitUntil(_ condition: () -> Bool) async {
        for _ in 0..<300 where !condition() { try? await Task.sleep(for: .milliseconds(10)) }
    }

    @Test func aChangedFileIsPickedUp() async throws {
        let fixture = CachesFixture()
        let (monitor, _) = monitor(fixture)
        try fixture.write("codex.json", object: codex(used: 5))
        await monitor.readOnce()
        #expect(monitor.snapshot?.accounts.first?.windows.first?.usedPercent == 5)
        try fixture.write("codex.json", object: codex(used: 45.5, checkedAt: "2026-11-06T09:05:00Z"))
        await monitor.readOnce()
        #expect(monitor.snapshot?.accounts.first?.windows.first?.usedPercent == 45.5)
    }

    @Test func anUnchangedFileIsNotDecodedAgain() async throws {
        let fixture = CachesFixture()
        let (monitor, reader) = monitor(fixture)
        try fixture.write("codex.json", object: codex(used: 5))
        try fixture.write("grok.json", object: limitsProviderObject("grok", driver: "grok"))
        await monitor.readOnce()
        await monitor.readOnce()
        await monitor.readOnce()
        #expect(await reader.parseCount == 2)
        try fixture.write("codex.json", object: codex(used: 45.5))
        await monitor.readOnce()
        #expect(await reader.parseCount == 3)
    }

    @Test func aCorruptRewriteKeepsTheLastGoodSnapshot() async throws {
        let fixture = CachesFixture()
        let (monitor, reader) = monitor(fixture)
        try fixture.write("codex.json", object: codex(used: 5))
        await monitor.readOnce()
        let good = monitor.snapshot
        try fixture.write("codex.json", "{\"driver\":\"codex\",")
        await monitor.readOnce()
        #expect(monitor.snapshot == good)
        #expect(monitor.snapshot?.skippedFiles == 0)
        await monitor.readOnce()
        #expect(await reader.parseCount == 2)
        try fixture.write("codex.json", object: codex(used: 70))
        await monitor.readOnce()
        #expect(monitor.snapshot?.accounts.first?.windows.first?.usedPercent == 70)
    }

    @Test func aFileThatNeverDecodedIsCountedAsSkipped() async throws {
        let fixture = CachesFixture()
        let (monitor, _) = monitor(fixture)
        try fixture.write("bad.json", "nope")
        await monitor.readOnce()
        #expect(monitor.snapshot?.skippedFiles == 1)
    }

    @Test func aDeletedFileLeavesTheSnapshot() async throws {
        let fixture = CachesFixture()
        let (monitor, _) = monitor(fixture)
        try fixture.write("codex.json", object: codex(used: 5))
        await monitor.readOnce()
        try fixture.remove("codex.json")
        await monitor.readOnce()
        #expect(monitor.snapshot?.accounts.isEmpty == true)
        #expect(monitor.snapshot?.state == .noLimitsReported)
    }

    @Test func aFailedReadKeepsTheLastGoodSnapshotAndRecordsTheError() async throws {
        let fixture = CachesFixture()
        let (monitor, _) = monitor(fixture)
        try fixture.write("codex.json", object: codex(used: 5))
        await monitor.readOnce()
        let good = monitor.snapshot
        monitor.apply(.failure(.unreadable))
        #expect(monitor.snapshot == good)
        #expect(monitor.readError == .unreadable)
        await monitor.readOnce()
        #expect(monitor.readError == nil)
    }

    @Test func theNowTickFollowsTheClockOnEveryRead() async throws {
        let fixture = CachesFixture()
        let clock = TestClock(limitDate("2026-11-06T10:00:00Z"))
        let (monitor, _) = monitor(fixture, clock: clock)
        try fixture.write("codex.json", object: codex(used: 5))
        await monitor.readOnce()
        let first = monitor.now
        clock.advance(5)
        await monitor.readOnce()
        #expect(monitor.now == first.addingTimeInterval(5))
    }

    @Test func nothingIsReadUntilStartAndNothingAfterStop() async throws {
        let fixture = CachesFixture()
        let (monitor, reader) = monitor(fixture)
        try fixture.write("codex.json", object: codex(used: 5))
        monitor.refresh()
        try await Task.sleep(for: .milliseconds(50))
        #expect(monitor.snapshot == nil)
        #expect(await reader.parseCount == 0)
        monitor.start()
        await waitUntil { monitor.snapshot != nil }
        #expect(monitor.snapshot?.accounts.count == 1)
        monitor.stop()
        try fixture.write("codex.json", object: codex(used: 45.5))
        monitor.refresh()
        try await Task.sleep(for: .milliseconds(50))
        #expect(monitor.snapshot?.accounts.first?.windows.first?.usedPercent == 5)
    }

    @Test func aMissingDirectoryShowsThatStateInsteadOfAnError() async {
        let reader = LimitsReader(cachesDirectory: FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString))
        let monitor = LimitsMonitor(reader: reader)
        await monitor.readOnce()
        #expect(monitor.snapshot?.state == .noCacheDirectory)
        #expect(monitor.readError == nil)
    }
}
