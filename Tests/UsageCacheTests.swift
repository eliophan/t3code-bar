import Foundation
import Testing
@testable import T3CodeBar

struct UsageCacheDecodeTests {
    private let t0 = ms("2026-11-05T12:00:00Z")

    private func fixture(_ rows: [String], kind: String = "claude", models: [String] = ["claude-sonnet-5-5", "<synthetic>"]) -> CacheFixture {
        CacheFixture(sources: [(kind, "/h/\(kind)")], models: models, sessions: ["s1", "s2"],
                     files: [.init(path: "/h/\(kind)/a.jsonl", kind: kind, rows: rows)])
    }

    @Test func rowsResolveTheirModelSessionAndOptionalFields() throws {
        let row = CacheFixture.Row(timestampMs: t0, model: 0, session: 1, uncached: 3, cached: 4, creation: 5, output: 6, reasoning: 2,
                                   key: "msg:req", cost: 0.25, speed: 1, rateModel: 1, turn: "turn-9")
        let dataset = try fixture([row.json], kind: "codex").decoded()
        let record = try #require(dataset.files.first?.records.first)
        #expect(record == UsageRecord(timestampMs: t0, model: "claude-sonnet-5-5", rateModel: "<synthetic>", session: "s2",
                                      uncachedInput: 3, cachedInput: 4, cacheCreation: 5, output: 6, reasoning: 2,
                                      reportedCostUsd: 0.25, speed: .fast, dedupeKey: "msg:req", unresolvedTierTurn: "turn-9"))
        #expect(record.processed == 18)
    }

    @Test func trailingLineRecordsAreCountedToo() throws {
        var cache = fixture([CacheFixture.Row(timestampMs: t0, uncached: 1).json])
        cache.files[0].tail = [CacheFixture.Row(timestampMs: t0 + 1, uncached: 2).json]
        #expect(try cache.decoded().files[0].records.map(\.uncachedInput) == [1, 2])
    }

    @Test func aBadRowDropsTheWholeFileEntryAndOthersSurvive() throws {
        var cache = fixture([CacheFixture.Row(timestampMs: t0, uncached: 1).json, "[1,2,3]"])
        cache.files.append(.init(path: "/h/claude/b.jsonl", kind: "claude", rows: [CacheFixture.Row(timestampMs: t0, uncached: 7).json]))
        let dataset = try cache.decoded()
        #expect(dataset.files.map(\.path) == ["/h/claude/b.jsonl"])
        #expect(dataset.rejectedFiles == 1)
    }

    @Test func overflowingTokenTotalsRejectTheFileAndKeepOtherFiles() throws {
        let large = CacheFixture.Row(timestampMs: t0, uncached: 8_999_999_999_999_999).json
        var cache = fixture([CacheFixture.Row(timestampMs: t0, uncached: 7).json])
        cache.files.append(.init(path: "/h/claude/large.jsonl", kind: "claude", rows: Array(repeating: large, count: 1_025)))
        let dataset = try cache.decoded()
        #expect(dataset.files.count == 1)
        #expect(dataset.files[0].records[0].processed == 7)
        #expect(dataset.rejectedFiles == 1)
    }

    @Test(arguments: [
        "[1,5,0,1,1,1,1,1,null,null,0,null]",
        "[1,0,0,1,1,1,1,1,null,null,3,null]",
        "[1,0,0,1,1,1,1,1,null,null,0,9]",
        "[1,0,0,\"x\",1,1,1,1,null,null,0,null]",
        "[null,0,0,1,1,1,1,1,null,null,0,null]",
        "[1,0,0,1,1,1,1,1,null,null,0,\"claude\"]",
    ])
    func eachKindOfInvalidRowRejectsItsFile(row: String) throws {
        let dataset = try fixture([row]).decoded()
        #expect(dataset.files.isEmpty)
        #expect(dataset.rejectedFiles == 1)
    }

    @Test func anOutOfRangeSessionIndexIsAnEmptySessionNotAnError() throws {
        let dataset = try fixture(["[1,0,99,1,1,1,1,1,null,null,0,null]"]).decoded()
        #expect(dataset.files.first?.records.first?.session == "")
    }

    @Test func elevenItemRowsAreValid() throws {
        let dataset = try fixture(["[1,0,0,1,1,1,1,1,null,null,0]"]).decoded()
        #expect(dataset.files.first?.records.count == 1)
        #expect(dataset.files.first?.records.first?.rateModel == nil)
    }

    @Test func syntheticClaudeRowsAreDroppedButOtherProvidersKeepThem() throws {
        let synthetic = CacheFixture.Row(timestampMs: t0, model: 1, uncached: 9).json
        let real = CacheFixture.Row(timestampMs: t0, model: 0, uncached: 1).json
        #expect(try fixture([synthetic, real]).decoded().files[0].records.map(\.uncachedInput) == [1])
        #expect(try fixture([synthetic, real], kind: "codex").decoded().files[0].records.map(\.uncachedInput) == [9, 1])
    }

    @Test func aProviderKindThisBuildDoesNotKnowIsKeptWithItsRawName() throws {
        let dataset = try fixture([CacheFixture.Row(timestampMs: t0, uncached: 4).json], kind: "newcomer").decoded()
        #expect(dataset.files.first?.kind == .other("newcomer"))
        #expect(dataset.sources == [.init(kind: .other("newcomer"), directory: "/h/newcomer")])
    }

    @Test func theTierTurnIsIgnoredBeforeVersionSeven() throws {
        let row = CacheFixture.Row(timestampMs: t0, turn: "turn-1").json
        var old = fixture([row], kind: "codex")
        old.version = 6
        #expect(try old.decoded().files[0].records[0].unresolvedTierTurn == nil)
        old.version = 7
        #expect(try old.decoded().files[0].records[0].unresolvedTierTurn == "turn-1")
    }

    @Test func keyOrderDoesNotMatterAndEscapesDecode() throws {
        var cache = fixture([CacheFixture.Row(timestampMs: t0, uncached: 5).json])
        cache.files[0].path = "/h/claude/caf\u{e9} \"q\" \u{1F600}.jsonl"
        cache.filesFirst = true
        let dataset = try cache.decoded()
        #expect(dataset.files.first?.path == "/h/claude/caf\u{e9} \"q\" \u{1F600}.jsonl")
        #expect(dataset.files.first?.records.count == 1)
        #expect(dataset.sources.first?.directory == "/h/claude")
    }

    @Test func filesKeepTheOrderTheyHaveInTheCache() throws {
        var cache = fixture([])
        cache.files = ["z", "a", "m"].map { .init(path: "/h/claude/\($0).jsonl", kind: "claude", rows: [CacheFixture.Row(timestampMs: 1).json]) }
        #expect(try cache.decoded().files.map(\.path) == ["/h/claude/z.jsonl", "/h/claude/a.jsonl", "/h/claude/m.jsonl"])
    }

    @Test(arguments: [(3, false), (4, true), (11, true), (12, false)])
    func onlyVersionsFourToElevenDecode(version: Int, supported: Bool) throws {
        var cache = fixture([CacheFixture.Row(timestampMs: t0).json])
        cache.version = version
        if supported {
            #expect(try cache.decoded().version == version)
        } else {
            #expect(throws: UsageCacheError.unsupportedVersion(version)) { try cache.decoded() }
        }
    }

    @Test func aTruncatedFileIsUnreadableNotUnsupported() {
        let json = fixture([CacheFixture.Row(timestampMs: t0).json]).json
        for cut in [json.count - 1, json.count / 2, json.count - 40] {
            let truncated = Data(json.prefix(cut).utf8)
            #expect(throws: UsageCacheError.unreadable) {
                try truncated.withUnsafeBytes { try UsageCacheDecoder.decode($0.bindMemory(to: UInt8.self)) }
            }
        }
    }
}

struct UsageCacheLocatorTests {
    private func write(_ version: Int, name: String? = nil, in directory: URL, modified: String) throws {
        var cache = CacheFixture(sources: [("claude", "/h")], models: ["m"], sessions: ["s"],
                                 files: [.init(path: "/h/a.jsonl", kind: "claude", rows: [CacheFixture.Row(timestampMs: 1).json])])
        cache.version = version
        let url = try cache.write(to: directory, name: name)
        try FileManager.default.setAttributes([.modificationDate: date(modified)], ofItemAtPath: url.path)
    }

    private func located(_ directory: URL) -> UsageCacheLocation { UsageCacheLocator(directory: directory).locate() }

    private func availableName(_ location: UsageCacheLocation) -> String? {
        if case .available(let file) = location { return file.url.lastPathComponent }
        return nil
    }

    @Test func aMissingDirectoryOrNoCacheIsMissing() throws {
        #expect(located(temporaryDirectory()) == .missing)
        let empty = temporaryDirectory()
        try FileManager.default.createDirectory(at: empty, withIntermediateDirectories: true)
        try Data("{}".utf8).write(to: empty.appendingPathComponent("usage-model-rates.json"))
        #expect(located(empty) == .missing)
    }

    @Test func theHighestGenerationWinsEvenWhenAnOlderFileIsNewer() throws {
        let directory = temporaryDirectory()
        try write(10, in: directory, modified: "2026-11-06T09:00:00Z")
        try write(11, in: directory, modified: "2026-11-01T09:00:00Z")
        try write(9, in: directory, modified: "2026-11-05T09:00:00Z")
        #expect(availableName(located(directory)) == "usage-scan-cache-v11.json")
    }

    @Test func theUnversionedLegacyNameIsUsedOnlyWhenNothingElseIs() throws {
        let directory = temporaryDirectory()
        try write(5, name: "usage-scan-cache.json", in: directory, modified: "2026-11-06T09:00:00Z")
        #expect(availableName(located(directory)) == "usage-scan-cache.json")
        try write(7, in: directory, modified: "2026-11-01T09:00:00Z")
        #expect(availableName(located(directory)) == "usage-scan-cache-v7.json")
    }

    @Test func otherFilesInTheDirectoryAreNotCaches() throws {
        let directory = temporaryDirectory()
        try write(11, name: "usage-scan-cache-v11.json.tmp", in: directory, modified: "2026-11-06T09:00:00Z")
        try write(11, name: "usage-scan-cache-vx.json", in: directory, modified: "2026-11-06T09:00:00Z")
        #expect(located(directory) == .missing)
    }

    @Test func aNewerFormatThanThisBuildKnowsIsReportedNotSkipped() throws {
        let directory = temporaryDirectory()
        try write(11, in: directory, modified: "2026-11-01T09:00:00Z")
        try write(12, in: directory, modified: "2026-11-06T09:00:00Z")
        #expect(located(directory) == .unsupported(12))
    }

    @Test func aLeftoverNewerFormatOlderThanTheCurrentCacheIsIgnored() throws {
        let directory = temporaryDirectory()
        try write(12, in: directory, modified: "2026-11-01T09:00:00Z")
        try write(11, in: directory, modified: "2026-11-06T09:00:00Z")
        #expect(availableName(located(directory)) == "usage-scan-cache-v11.json")
    }

    @Test func aCacheTooOldToReadIsUnsupported() throws {
        let directory = temporaryDirectory()
        try write(3, in: directory, modified: "2026-11-06T09:00:00Z")
        #expect(located(directory) == .unsupported(3))
    }

    @Test func theReaderDecodesTheLocatedFileAndRecordsItsSizeAndTime() throws {
        let directory = temporaryDirectory()
        try write(11, in: directory, modified: "2026-11-06T09:00:00Z")
        guard case .available(let file) = located(directory) else { Issue.record("no cache located"); return }
        let dataset = try UsageCacheReader().load(file)
        #expect(dataset.size == file.size)
        #expect(dataset.modified == date("2026-11-06T09:00:00Z"))
        #expect(dataset.files.count == 1)
    }

    @Test func aFileThatDisappearsBetweenLocateAndLoadIsUnreadable() throws {
        let directory = temporaryDirectory()
        try write(11, in: directory, modified: "2026-11-06T09:00:00Z")
        guard case .available(let file) = located(directory) else { Issue.record("no cache located"); return }
        try FileManager.default.removeItem(at: file.url)
        #expect(throws: UsageCacheError.unreadable) { try UsageCacheReader().load(file) }
    }
}
