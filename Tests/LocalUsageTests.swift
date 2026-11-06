import Foundation
import SQLite3
import Testing
@testable import T3CodeBar

private final class LocalDB {
    let url: URL
    var db: OpaquePointer?
    init(_ url: URL, schema: String) throws {
        self.url = url
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        guard sqlite3_open(url.path, &db) == SQLITE_OK else { throw LocalUsageError.sqlite }
        try sql(schema)
    }
    deinit { sqlite3_close(db) }
    func sql(_ sql: String) throws {
        guard sqlite3_exec(db, sql, nil, nil, nil) == SQLITE_OK else { throw LocalUsageError.sqlite }
    }
    func blob(table: String = "steps", column: String = "metadata", index: Int = 0, bytes: Data) throws {
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, "INSERT INTO \(table) VALUES (?, ?)", -1, &statement, nil) == SQLITE_OK else { throw LocalUsageError.sqlite }
        defer { sqlite3_finalize(statement) }
        sqlite3_bind_int(statement, 1, Int32(index))
        _ = bytes.withUnsafeBytes { sqlite3_bind_blob(statement, 2, $0.baseAddress, Int32($0.count), unsafeBitCast(-1, to: sqlite3_destructor_type.self)) }
        guard sqlite3_step(statement) == SQLITE_DONE else { throw LocalUsageError.sqlite }
    }
}

private final class LocalClock: @unchecked Sendable {
    private let lock = NSLock()
    private var value: Date
    init(_ date: Date) { value = date }
    var now: Date {
        get { lock.withLock { value } }
        set { lock.withLock { value = newValue } }
    }
}

struct LocalUsageTests {
    private let now = date("2026-11-06T09:08:00Z")
    private let timestamp = ms("2026-11-05T10:00:00Z")
    private let zone = TimeZone(secondsFromGMT: 0)!
    private let schema = "CREATE TABLE message(id TEXT, session_id TEXT, time_created INTEGER, data TEXT)"
    private var openJSON: String { """
    {"role":"assistant","modelID":"glm-5.3-flash","time":{"created":\(Int(timestamp))},"tokens":{"input":100,"output":20,"reasoning":5,"cache":{"read":800,"write":0}},"cost":0.003}
    """ }
    private func openMessage(_ db: LocalDB, id: String = "m1", json: String? = nil) throws {
        let escaped = (json ?? openJSON).replacingOccurrences(of: "'", with: "''")
        try db.sql("INSERT INTO message VALUES ('\(id)', 's1', \(Int(timestamp)), '\(escaped)')")
    }
    private func report(_ snapshot: LocalUsageSnapshot) -> UsageReport {
        UsageReportBuilder.build(dataset: snapshot.merging(nil), window: .calendarDays(7, now: now, timeZone: zone), prices: PriceBook())
    }
    private func varint(_ n: Int) -> Data {
        var value = UInt64(n), result = Data()
        repeat {
            let byte = UInt8(value & 127)
            value >>= 7
            result.append(byte | (value > 0 ? 128 : 0))
        } while value > 0
        return result
    }
    private func number(_ field: Int, _ value: Int) -> Data { varint(field * 8) + varint(value) }
    private func bytes(_ field: Int, _ data: Data) -> Data { varint(field * 8 + 2) + varint(data.count) + data }
    private func text(_ field: Int, _ value: String) -> Data { bytes(field, Data(value.utf8)) }
    private func usage(input: Int = 100, output: Int = 20, id: String = "response", secondID: String? = nil) -> Data {
        number(1, 1035) + number(2, input) + number(3, output) + number(5, 800) + number(9, 5) + text(11, id) + (secondID.map { text(12, $0) } ?? Data())
    }
    private func step(_ usage: Data, timestamp: Double? = nil, retry: Data? = nil) -> Data {
        bytes(9, usage) + bytes(24, text(12, "Claude 4.5 Sonnet (Thinking)"))
            + (timestamp.map { bytes(8, number(1, Int($0 / 1000))) } ?? Data())
            + (retry.map { bytes(28, bytes(2, $0)) } ?? Data())
    }

    @Test func openCodeCountsReasoningOnceAndDeduplicatesSQLiteAndLegacyCopies() throws {
        let root = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let db = try LocalDB(root.appendingPathComponent("opencode.db"), schema: schema)
        try openMessage(db)
        let legacy = root.appendingPathComponent("storage/message/s1")
        try FileManager.default.createDirectory(at: legacy, withIntermediateDirectories: true)
        try Data(openJSON.utf8).write(to: legacy.appendingPathComponent("m1.json"))
        var scanner = LocalUsageScanner()
        let snapshot = scanner.scan(paths: .init(openCode: [root, root]), now: now)
        let report = report(snapshot)
        #expect(report.processedTokens == 925)
        #expect(report.tokens.output == 25)
        #expect(report.tokens.reasoning == 5)
        #expect(report.costUsd == 0.003)
        #expect(report.sessions == 1)
        #expect(report.duplicatesDropped == 1)
        #expect(snapshot.statuses.first?.state == .ready)
        #expect(scanner.reads == 2)
        _ = scanner.scan(paths: .init(openCode: [root]), now: now)
        #expect(scanner.reads == 2)
    }

    @Test func legacySessionMessageSchemaAndWALChangesAreVisibleWithoutWrites() throws {
        let root = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let db = try LocalDB(root.appendingPathComponent("opencode.db"), schema: "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE session_message(id TEXT, session_id TEXT, type TEXT, data TEXT)")
        let escaped = openJSON.replacingOccurrences(of: "'", with: "''")
        try db.sql("INSERT INTO session_message VALUES ('m1', 's1', 'assistant', '\(escaped)')")
        var scanner = LocalUsageScanner()
        let first = scanner.scan(paths: .init(openCode: [root]), now: now)
        #expect(report(first).processedTokens == 925)
        let dbBefore = try Data(contentsOf: db.url)
        try db.sql("INSERT INTO session_message VALUES ('m2', 's1', 'assistant', '\(escaped)')")
        let second = scanner.scan(paths: .init(openCode: [root]), now: now)
        #expect(report(second).processedTokens == 1850)
        #expect(scanner.reads == 2)
        #expect(try Data(contentsOf: db.url) == dbBefore)
    }

    @Test func failureKeepsLastReadableDataAndRecoversWithExplicitState() throws {
        let root = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let db = try LocalDB(root.appendingPathComponent("opencode.db"), schema: schema)
        try openMessage(db)
        var scanner = LocalUsageScanner()
        _ = scanner.scan(paths: .init(openCode: [root]), now: now)
        try db.sql("ALTER TABLE message RENAME TO broken")
        let failed = scanner.scan(paths: .init(openCode: [root]), now: now)
        #expect(failed.statuses.first?.state == .partial)
        #expect(failed.statuses.first?.failedFiles == 1)
        #expect(report(failed).processedTokens == 925)
        try db.sql("ALTER TABLE broken RENAME TO message")
        #expect(scanner.scan(paths: .init(openCode: [root]), now: now).statuses.first?.state == .ready)
    }

    @Test func malformedRowsArePartialAndUnrecognizedDatabaseIsUnreadable() throws {
        let root = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let db = try LocalDB(root.appendingPathComponent("opencode.db"), schema: schema)
        try openMessage(db)
        try openMessage(db, id: "bad", json: "{")
        var scanner = LocalUsageScanner()
        let partial = scanner.scan(paths: .init(openCode: [root]), now: now)
        #expect(partial.statuses.first?.state == .partial)
        #expect(report(partial).malformedRecords == 1)
        #expect(report(partial).processedTokens == 925)
        try db.sql("DROP TABLE message")
        var fresh = LocalUsageScanner()
        #expect(fresh.scan(paths: .init(openCode: [root]), now: now).statuses.first?.state == .unreadable)
    }

    @Test func antigravityMergesTransitiveIdentitiesAndUsesBestTimestampAndMaximumTokens() throws {
        let root = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let a = try LocalDB(root.appendingPathComponent("conversations/a.db"), schema: "CREATE TABLE steps(idx INTEGER, metadata BLOB)")
        let b = try LocalDB(root.appendingPathComponent("conversations/b.db"), schema: "CREATE TABLE steps(idx INTEGER, metadata BLOB)")
        try a.blob(bytes: step(usage(input: 100, id: "one"), retry: usage(input: 10, output: 40, id: "two")))
        try b.blob(bytes: step(usage(input: 200, id: "one", secondID: "bridge"), timestamp: timestamp))
        try b.blob(index: 1, bytes: step(usage(input: 150, output: 80, id: "two", secondID: "bridge"), timestamp: timestamp + 1000))
        var scanner = LocalUsageScanner()
        let snapshot = scanner.scan(paths: .init(antigravity: [root, root.appendingPathComponent("conversations")]), now: now)
        let records = snapshot.files.flatMap(\.records)
        #expect(records.count == 1)
        let record = try #require(records.first)
        #expect(record.model == "claude-sonnet-4-6")
        #expect(record.timestampMs == timestamp)
        #expect(record.uncachedInput == 200)
        #expect(record.cachedInput == 800)
        #expect(record.output == 80)
        #expect(record.session == "a")
        #expect(report(snapshot).processedTokens == 1080)
    }

    @Test func antigravityGenerationMetadataAndUnknownModelsRemainUnpriced() throws {
        let root = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let db = try LocalDB(root.appendingPathComponent("c.db"), schema: "CREATE TABLE gen_metadata(idx INTEGER, data BLOB)")
        let unknownUsage = number(1, 9999) + number(2, 15) + number(9, 7) + number(10, 3) + text(11, "unknown")
        let generation = number(3, 9999) + bytes(4, unknownUsage) + bytes(9, bytes(4, number(1, Int(timestamp / 1000))))
        try db.blob(table: "gen_metadata", bytes: bytes(1, generation))
        var scanner = LocalUsageScanner()
        let snapshot = scanner.scan(paths: .init(antigravity: [root]), now: now)
        let record = try #require(snapshot.files.first?.records.first)
        #expect(record.model == "antigravity-model-9999")
        #expect(record.output == 10)
        #expect(record.reasoning == 7)
        #expect(report(snapshot).unpricedRecords == 1)
        #expect(report(snapshot).processedTokens == 25)
    }

    @Test func malformedProtobufIsPartialAndSymlinksAreSkipped() throws {
        let root = temporaryDirectory(), other = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root); try? FileManager.default.removeItem(at: other) }
        let db = try LocalDB(root.appendingPathComponent("valid.db"), schema: "CREATE TABLE steps(idx INTEGER, metadata BLOB)")
        let secret = try LocalDB(other.appendingPathComponent("secret.db"), schema: "CREATE TABLE steps(idx INTEGER, metadata BLOB)")
        try secret.blob(bytes: step(usage(), timestamp: timestamp))
        try db.blob(bytes: Data([0x4a, 0xff, 0xff]))
        try FileManager.default.createSymbolicLink(at: root.appendingPathComponent("copy.db"), withDestinationURL: secret.url)
        var scanner = LocalUsageScanner()
        let snapshot = scanner.scan(paths: .init(antigravity: [root]), now: now)
        #expect(snapshot.files.count == 1)
        #expect(snapshot.files.first?.malformed == 1)
        #expect(snapshot.statuses.first(where: { $0.kind == .antigravity })?.state == .partial)
    }

    @Test func cursorCacheUsesReportedCostModelAliasAndOldestAccountFreshness() throws {
        let root = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let url = root.appendingPathComponent("usage-cursor-account-cache-v1.json")
        let json = """
        {"version":1,"strings":["claude-4.5-sonnet-thinking","conv1"],"accounts":{"keychain":{"accountKey":"abc","sinceMs":0,"untilMs":\(Int(timestamp)),"fetchedAtMs":\(Int(timestamp)),"records":[[\(Int(timestamp)),0,1,100,2000,0,300,0.42,"cursor-account:abc:k:0"]]}}}
        """
        try Data(json.utf8).write(to: url)
        var scanner = LocalUsageScanner()
        let snapshot = scanner.scan(paths: .init(cursor: url), now: now)
        let record = try #require(snapshot.files.first?.records.first)
        #expect(record.rateModel == "claude-4.5-sonnet")
        #expect(report(snapshot).costUsd == 0.42)
        #expect(report(snapshot).processedTokens == 2400)
        let status = try #require(snapshot.statuses.first(where: { $0.kind == .cursor }))
        #expect(status.dataUpdatedAt == Date(timeIntervalSince1970: timestamp / 1000))
        #expect(UsageView.localSourceText(status, now: now).contains("Saved account data"))
        try Data("{\"version\":2}".utf8).write(to: url)
        #expect(scanner.scan(paths: .init(cursor: url), now: now).statuses.last?.state == .partial)
    }

    @Test func cursorAccountsKeepSeparateSessionCountsEvenWhenConversationNamesMatch() throws {
        let root = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let url = root.appendingPathComponent("usage-cursor-account-cache-v1.json")
        let json = """
        {"version":1,"strings":["unknown","same-conversation"],"accounts":{"a":{"fetchedAtMs":\(Int(timestamp)),"records":[[\(Int(timestamp)),0,1,1,0,0,0,0.1,"a:1"]]},"b":{"fetchedAtMs":\(Int(timestamp+1000)),"records":[[\(Int(timestamp)),0,1,2,0,0,0,0.2,"b:1"]]}}}
        """
        try Data(json.utf8).write(to: url)
        var scanner = LocalUsageScanner()
        let snapshot = scanner.scan(paths: .init(cursor: url), now: now)
        #expect(report(snapshot).sessions == 2)
        #expect(report(snapshot).processedTokens == 3)
        #expect(snapshot.sources.count == 2)
        #expect(snapshot.statuses.last?.dataUpdatedAt == Date(timeIntervalSince1970: timestamp / 1000))
    }

    @Test func missingSourcesHaveNoFabricatedProviderRows() {
        var scanner = LocalUsageScanner()
        let snapshot = scanner.scan(paths: .empty, now: now)
        #expect(snapshot.statuses.allSatisfy { $0.state == .missing })
        #expect(report(snapshot).providers.isEmpty)
    }

    @Test func engineRefreshesLocalSourcesWithoutAChangedTranscriptCache() async throws {
        let root = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let db = try LocalDB(root.appendingPathComponent("opencode.db"), schema: schema)
        try openMessage(db)
        let source = UsageSource(config: EnvironmentConfig(stateDirectory: root), localPaths: .init(openCode: [root]))
        let engine = UsageEngine(source: source, clock: { date("2026-11-06T09:08:00Z") }, timeZone: zone)
        guard case .report(let report) = await engine.report(for: .week) else { Issue.record("local usage works without a transcript cache"); return }
        #expect(report.scanCacheMissing)
        #expect(report.isPartial)
        #expect(report.processedTokens == 925)
        #expect(await engine.report(for: .week) == .unchanged)
    }

    @Test func firstReportIncludesLocalProvidersEvenWithoutPrewarm() async throws {
        let root = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let db = try LocalDB(root.appendingPathComponent("opencode.db"), schema: schema)
        try openMessage(db)
        try CacheFixture().write(to: root)
        let source = UsageSource(config: EnvironmentConfig(stateDirectory: root), localPaths: .init(openCode: [root]))
        let engine = UsageEngine(source: source, clock: { date("2026-11-06T09:08:00Z") }, timeZone: zone)
        guard case .report(let report) = await engine.report(for: .week, includeLocal: false) else {
            Issue.record("expected a complete first report"); return
        }
        #expect(report.providers.contains { $0.kind == .opencode && $0.tokens.processed == 925 })
    }

    @Test func enginePicksUpWALWritesAfterThirtySecondsWithAnUnchangedTranscriptCache() async throws {
        let root = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let db = try LocalDB(root.appendingPathComponent("opencode.db"), schema: schema)
        try openMessage(db)
        try CacheFixture().write(to: root)
        let clock = LocalClock(now)
        let engine = UsageEngine(source: UsageSource(config: EnvironmentConfig(stateDirectory: root), localPaths: .init(openCode: [root])), clock: { clock.now }, timeZone: zone)
        guard case .report(let first) = await engine.report(for: .week) else { Issue.record("expected initial report"); return }
        #expect(first.processedTokens == 925)
        try openMessage(db, id: "m2")
        clock.now = now.addingTimeInterval(29)
        #expect(await engine.report(for: .week) == .unchanged)
        clock.now = now.addingTimeInterval(31)
        guard case .report(let fresh) = await engine.report(for: .week) else { Issue.record("expected refreshed report"); return }
        #expect(fresh.processedTokens == 1850)
        #expect(await engine.cacheReads == 1)
    }

    @Test func unknownLargeProtobufIntegersDoNotDropUsage() throws {
        let root = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let db = try LocalDB(root.appendingPathComponent("large.db"), schema: "CREATE TABLE steps(idx INTEGER, metadata BLOB)")
        let largeInteger = Data([0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01])
        try db.blob(bytes: step(usage(), timestamp: timestamp) + varint(50 * 8) + largeInteger)
        var scanner = LocalUsageScanner()
        let snapshot = scanner.scan(paths: .init(antigravity: [root]), now: now)
        #expect(snapshot.statuses.first(where: { $0.kind == .antigravity })?.state == .ready)
        #expect(report(snapshot).processedTokens == 920)
    }

    @Test func combinedSourcesRejectTokenTotalsThatWouldOverflow() {
        var local = LocalUsageSnapshot()
        local.sources = [.init(kind: .opencode, directory: "/local")]
        local.files = [
            .init(path: "/local/one", kind: .opencode, modifiedMs: timestamp, malformed: 0,
                  records: [.init(timestampMs: timestamp, model: "unknown", uncachedInput: Int.max)]),
            .init(path: "/local/two", kind: .opencode, modifiedMs: timestamp, malformed: 0,
                  records: [.init(timestampMs: timestamp, model: "unknown", uncachedInput: 1)])
        ]
        let merged = local.merging(nil)
        #expect(merged.files.count == 1)
        #expect(merged.rejectedFiles == 1)
    }

    @Test func discoveryUsesEnvironmentAndOnlyAntigravityInstanceIDsFromSettings() throws {
        let home = temporaryDirectory(), state = home.appendingPathComponent("state")
        defer { try? FileManager.default.removeItem(at: home) }
        try FileManager.default.createDirectory(at: state, withIntermediateDirectories: true)
        try Data("{\"providerInstances\":{\"instance\":{\"driver\":\"antigravity\",\"environment\":{\"TOKEN\":\"do-not-decode\"}}}}".utf8).write(to: state.appendingPathComponent("settings.json"))
        let paths = LocalUsagePaths.discover(state: state, home: home, environment: ["OPENCODE_DATA_DIR": "/tmp/oc-a,/tmp/oc-b", "ANTIGRAVITY_DATA_DIR": "/tmp/ag"])
        #expect(paths.openCode.map(\.path) == ["/tmp/oc-a", "/tmp/oc-b"])
        #expect(paths.antigravity.count == 2)
        #expect(paths.antigravity.last?.path.hasSuffix("/antigravity-acp") == true)
        #expect(paths.cursor?.lastPathComponent == "usage-cursor-account-cache-v1.json")
    }
}
