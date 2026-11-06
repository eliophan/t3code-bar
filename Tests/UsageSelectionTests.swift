import Foundation
import Testing
@testable import T3CodeBar

struct UsageSelectionTests {
    private let zone = TimeZone(identifier: "Asia/Ho_Chi_Minh")!
    private let now = date("2026-11-06T09:08:00Z")
    private let recent = ms("2026-11-06T03:00:00Z")

    private var week: UsageWindow { .calendarDays(7, now: now, timeZone: zone) }

    private func row(_ ts: Double, session: Int = 0, model: Int = 0, uncached: Int = 1, output: Int = 0, key: String? = nil) -> String {
        CacheFixture.Row(timestampMs: ts, model: model, session: session, uncached: uncached, output: output, key: key).json
    }

    private func cache(_ files: [CacheFixture.FileEntry], sources: [(String, String)] = [("claude", "/c"), ("codex", "/x")]) -> CacheFixture {
        CacheFixture(sources: sources, models: ["m0", "m1"], sessions: ["s0", "s1", "s2"], files: files)
    }

    private func counted(_ fixture: CacheFixture, window: UsageWindow? = nil) throws
        -> (records: [UsageRecord], selection: UsageDataset.Selection) {
        var records: [UsageRecord] = []
        let selection = try fixture.decoded().select(in: window ?? week) { records.append($0.record) }
        return (records, selection)
    }

    @Test func aKeyRepeatedInsideAFileCountsOnce() throws {
        let file = CacheFixture.FileEntry(path: "/c/a.jsonl", kind: "claude", rows: [row(recent, uncached: 5, key: "k"), row(recent + 1, uncached: 6, key: "k")])
        let result = try counted(cache([file]))
        #expect(result.records.map(\.uncachedInput) == [5])
        #expect(result.selection.duplicatesDropped == 1)
    }

    @Test func aKeyRepeatedAcrossFilesAndSourcesKeepsTheFirstOneInReaderOrder() throws {
        let claude = CacheFixture.FileEntry(path: "/c/a.jsonl", kind: "claude", rows: [row(recent, uncached: 1, key: "k")])
        let codex = CacheFixture.FileEntry(path: "/x/a.jsonl", kind: "codex", rows: [row(recent, uncached: 2, key: "k")])
        // The cache lists codex first, but T3 Code walks its readers claude, codex, grok, pi, muse.
        let result = try counted(cache([codex, claude], sources: [("codex", "/x"), ("claude", "/c")]))
        #expect(result.records.map(\.uncachedInput) == [1])
    }

    @Test func filesWithinOneSourceKeepTheirCacheOrder() throws {
        let first = CacheFixture.FileEntry(path: "/c/z.jsonl", kind: "claude", rows: [row(recent, uncached: 1, key: "k")])
        let second = CacheFixture.FileEntry(path: "/c/a.jsonl", kind: "claude", rows: [row(recent, uncached: 2, key: "k")])
        #expect(try counted(cache([first, second])).records.map(\.uncachedInput) == [1])
    }

    @Test func anOutOfWindowCopyHidesAnInWindowOne() throws {
        let old = CacheFixture.FileEntry(path: "/c/old.jsonl", kind: "claude", rows: [row(ms("2026-09-01T00:00:00Z"), uncached: 1, key: "k")])
        let new = CacheFixture.FileEntry(path: "/c/new.jsonl", kind: "claude", rows: [row(recent, uncached: 2, key: "k")])
        let result = try counted(cache([old, new]))
        #expect(result.records.isEmpty)
        #expect(result.selection.outOfWindow == 1)
        #expect(result.selection.duplicatesDropped == 1)
    }

    @Test func recordsWithoutAKeyAreNeverDeduplicated() throws {
        let file = CacheFixture.FileEntry(path: "/x/a.jsonl", kind: "codex", rows: [row(recent), row(recent)])
        #expect(try counted(cache([file])).records.count == 2)
    }

    @Test func aCodexSessionInTwoFilesCountsItsRepeatedEventsOnlyOnce() throws {
        let events = [row(recent, session: 1, uncached: 10), row(recent, session: 1, uncached: 10), row(recent + 5, session: 1, uncached: 20)]
        let parent = CacheFixture.FileEntry(path: "/x/parent.jsonl", kind: "codex", rows: events)
        let fork = CacheFixture.FileEntry(path: "/x/fork.jsonl", kind: "codex", rows: events + [row(recent + 9, session: 1, uncached: 30)])
        let result = try counted(cache([parent, fork]))
        #expect(result.records.map(\.uncachedInput) == [10, 10, 20, 30])
        #expect(result.selection.duplicatesDropped == 3)
    }

    @Test func aCodexForkWithAThirdCopyOfARepeatedEventKeepsTheExtraOne() throws {
        let twice = [row(recent, session: 1, uncached: 10), row(recent, session: 1, uncached: 10)]
        let parent = CacheFixture.FileEntry(path: "/x/parent.jsonl", kind: "codex", rows: twice)
        let fork = CacheFixture.FileEntry(path: "/x/fork.jsonl", kind: "codex", rows: twice + [row(recent, session: 1, uncached: 10)])
        #expect(try counted(cache([parent, fork])).records.count == 3)
    }

    @Test func aCodexSessionInOneFileOnlyIsLeftAlone() throws {
        let file = CacheFixture.FileEntry(path: "/x/a.jsonl", kind: "codex", rows: [row(recent, session: 1), row(recent, session: 1)])
        let other = CacheFixture.FileEntry(path: "/x/b.jsonl", kind: "codex", rows: [row(recent, session: 2)])
        #expect(try counted(cache([file, other])).records.count == 3)
    }

    @Test func theSharedSessionRuleDoesNotApplyToOtherProviders() throws {
        let rows = [row(recent, session: 1, uncached: 10)]
        let a = CacheFixture.FileEntry(path: "/c/a.jsonl", kind: "claude", rows: rows)
        let b = CacheFixture.FileEntry(path: "/c/b.jsonl", kind: "claude", rows: rows)
        #expect(try counted(cache([a, b])).records.count == 2)
    }

    @Test func eventsAtDifferentTimesOrSizesInASharedSessionAreDistinct() throws {
        let parent = CacheFixture.FileEntry(path: "/x/parent.jsonl", kind: "codex", rows: [row(recent, session: 1, uncached: 10)])
        let fork = CacheFixture.FileEntry(path: "/x/fork.jsonl", kind: "codex",
                                          rows: [row(recent + 1, session: 1, uncached: 10), row(recent, session: 1, uncached: 11)])
        #expect(try counted(cache([parent, fork])).records.count == 3)
    }

    @Test func sessionsAreCountedPerSourceAndAnEmptySessionIsNotASession() throws {
        let claude = CacheFixture.FileEntry(path: "/c/a.jsonl", kind: "claude", rows: [row(recent, session: 1), row(recent + 1, session: 1), row(recent + 2, session: 2)])
        let codex = CacheFixture.FileEntry(path: "/x/a.jsonl", kind: "codex", rows: [row(recent, session: 1)])
        var fixture = cache([claude, codex])
        fixture.sessions = ["", "s1", "s2"]
        let selection = try counted(fixture).selection
        #expect(selection.sources.map(\.distinctSessions) == [2, 1])
        #expect(selection.sessions == 3)
    }

    @Test func aSessionOnlyCountsWhenOneOfItsRecordsWasCounted() throws {
        let file = CacheFixture.FileEntry(path: "/c/a.jsonl", kind: "claude",
                                          rows: [row(recent, session: 1), row(ms("2026-09-01T00:00:00Z"), session: 2)])
        #expect(try counted(cache([file])).selection.sessions == 1)
    }

    @Test func filesOutsideTheirSourceDirectoryOrTooOldAreNotRead() throws {
        let inside = CacheFixture.FileEntry(path: "/c/a.jsonl", kind: "claude", rows: [row(recent)])
        let stray = CacheFixture.FileEntry(path: "/elsewhere/a.jsonl", kind: "claude", rows: [row(recent)])
        let sibling = CacheFixture.FileEntry(path: "/cache/a.jsonl", kind: "claude", rows: [row(recent)])
        var stale = CacheFixture.FileEntry(path: "/c/stale.jsonl", kind: "claude", rows: [row(recent)])
        stale.modifiedMs = ms("2026-10-01T00:00:00Z")
        #expect(try counted(cache([inside, stray, sibling, stale])).records.count == 1)
    }

    @Test func aCodexFileMustBelongToACodexSource() throws {
        let file = CacheFixture.FileEntry(path: "/c/a.jsonl", kind: "codex", rows: [row(recent)])
        #expect(try counted(cache([file])).records.isEmpty)
    }

    @Test func pathsWithDotSegmentsAreMatchedAfterNormalizing() {
        #expect(UsageDataset.isWithin("/h/./claude/../claude/a.jsonl", "/h/claude"))
        #expect(!UsageDataset.isWithin("/h/claude/../codex/a.jsonl", "/h/claude"))
        #expect(UsageDataset.isWithin("/h//claude/a.jsonl", "/h/claude"))
    }

    @Test func aKindThisBuildDoesNotKnowIsStillCountedAfterTheKnownOnes() throws {
        let known = CacheFixture.FileEntry(path: "/c/a.jsonl", kind: "claude", rows: [row(recent, uncached: 1)])
        let future = CacheFixture.FileEntry(path: "/n/a.jsonl", kind: "newcomer", rows: [row(recent, uncached: 2)])
        let fixture = cache([future, known], sources: [("newcomer", "/n"), ("claude", "/c")])
        var kinds: [UsageProviderKind] = []
        try fixture.decoded().select(in: week) { kinds.append($0.kind) }
        #expect(kinds == [.claude, .other("newcomer")])
    }

    @Test func eachCountedRecordCarriesItsSlotAndSource() throws {
        let file = CacheFixture.FileEntry(path: "/x/a.jsonl", kind: "codex", rows: [row(ms("2026-11-06T16:30:00Z"))])
        var slots: [UsageWindow.Slot] = []
        var sourceIndexes: [Int] = []
        try cache([file]).decoded().select(in: week) { slots.append($0.slot); sourceIndexes.append($0.sourceIndex) }
        #expect(slots == [.init(day: dayNumber("2026-11-06"), hour: -1)])
        #expect(sourceIndexes == [1])
    }
}
