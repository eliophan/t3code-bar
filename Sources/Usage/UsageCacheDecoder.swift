import Foundation

/// T3 Code's decoded `usage-scan-cache-vN.json`: transcript providers' records, grouped by file.
public struct UsageDataset: Sendable {
    public struct Source: Equatable, Sendable {
        public let kind: UsageProviderKind
        public let directory: String
    }

    public struct File: Sendable {
        public let path: String
        public let kind: UsageProviderKind
        public let modifiedMs: Double
        public let malformed: Int
        public internal(set) var records: [UsageRecord]
    }

    public let version: Int
    public let sources: [Source]
    public internal(set) var files: [File]
    /// File entries dropped because one of their rows could not be decoded.
    public let rejectedFiles: Int
    public var size = 0
    /// When T3 Code last wrote the cache, which is the last time its Usage page scanned.
    public var modified = Date(timeIntervalSince1970: 0)
}

public enum UsageCacheError: Error, Equatable, Sendable {
    /// The file is not valid JSON, which happens briefly while T3 Code rewrites it.
    case unreadable
    case unsupportedVersion(Int?)
}

enum UsageCacheDecoder {
    static let supportedVersions = 4...11

    /// The `version` field, found without decoding the file.
    static func version(in bytes: UnsafeBufferPointer<UInt8>) -> Int? {
        var scanner = JSONScanner(bytes)
        guard (try? scanner.openObject()) != nil, !scanner.closesObject() else { return nil }
        repeat {
            guard let key = try? scanner.objectKey() else { return nil }
            if key == "version" { return (try? scanner.number()).flatMap { Int(exactly: $0) } }
            guard (try? scanner.skipValue()) != nil else { return nil }
        } while (try? scanner.nextMember()) == true
        return nil
    }

    static func decode(_ bytes: UnsafeBufferPointer<UInt8>) throws -> UsageDataset {
        do {
            return try decodeDocument(bytes)
        } catch is JSONScanner.Malformed {
            throw UsageCacheError.unreadable
        }
    }

    private static func decodeDocument(_ bytes: UnsafeBufferPointer<UInt8>) throws -> UsageDataset {
        var scanner = JSONScanner(bytes)
        var sources: [UsageDataset.Source] = []
        var version: Int?
        var models: [String] = []
        var sessions: [String] = []
        var filesStart: Int?

        try scanner.openObject()
        if !scanner.closesObject() {
            repeat {
                switch try scanner.objectKey() {
                case "sources": sources = try decodeSources(&scanner)
                case "version": version = Int(exactly: try scanner.number())
                case "models": models = try decodeStrings(&scanner)
                case "sessions": sessions = try decodeStrings(&scanner)
                case "files":
                    scanner.skipWhitespace()
                    filesStart = scanner.position
                    try scanner.skipValue()
                default: try scanner.skipValue()
                }
            } while try scanner.nextMember()
        }
        guard let version, supportedVersions.contains(version) else { throw UsageCacheError.unsupportedVersion(version) }
        guard let filesStart else { throw UsageCacheError.unreadable }

        var rows = RowDecoder(models: models, sessions: sessions, version: version)
        var files: [UsageDataset.File] = []
        var rejected = 0
        var tokenBudget = 0
        var scanner2 = JSONScanner(bytes, at: filesStart)
        try scanner2.openObject()
        if !scanner2.closesObject() {
            repeat {
                let path = try scanner2.objectKey()
                if let file = try decodeFile(&scanner2, path: path, rows: &rows),
                   let total = tokenTotal(file.records, addingTo: tokenBudget) {
                    files.append(file)
                    tokenBudget = total
                } else {
                    rejected += 1
                }
            } while try scanner2.nextMember()
        }
        return UsageDataset(version: version, sources: sources, files: files, rejectedFiles: rejected)
    }

    private static func tokenTotal(_ records: [UsageRecord], addingTo initial: Int) -> Int? {
        var total = initial
        for record in records {
            for value in [record.uncachedInput, record.cachedInput, record.cacheCreation, record.output, record.reasoning] {
                guard value >= 0 else { return nil }
                let (sum, overflow) = total.addingReportingOverflow(value)
                guard !overflow else { return nil }
                total = sum
            }
        }
        return total
    }

    private static func decodeSources(_ scanner: inout JSONScanner) throws -> [UsageDataset.Source] {
        var sources: [UsageDataset.Source] = []
        try scanner.openObject()
        if scanner.closesObject() { return sources }
        repeat {
            let key = try scanner.objectKey()
            var directory: String?
            try scanner.openObject()
            if !scanner.closesObject() {
                repeat {
                    if try scanner.objectKey() == "dir", scanner.isString() {
                        directory = try scanner.string()
                    } else {
                        try scanner.skipValue()
                    }
                } while try scanner.nextMember()
            }
            if let directory {
                let kind = key.split(separator: "\u{0}", maxSplits: 1, omittingEmptySubsequences: false).first.map(String.init) ?? key
                sources.append(.init(kind: UsageProviderKind(raw: kind), directory: directory))
            }
        } while try scanner.nextMember()
        return sources
    }

    private static func decodeStrings(_ scanner: inout JSONScanner) throws -> [String] {
        var strings: [String] = []
        try scanner.openArray()
        if scanner.closesArray() { return strings }
        repeat {
            if scanner.isNull() {
                try scanner.consumeNull()
                strings.append("")
            } else {
                strings.append(try scanner.string())
            }
        } while try scanner.nextItem()
        return strings
    }

    /// Nil when the entry must be dropped: T3 Code drops a whole file when any of its rows is bad.
    private static func decodeFile(_ scanner: inout JSONScanner, path: String, rows: inout RowDecoder) throws -> UsageDataset.File? {
        var provider: String?
        var modified: Double?
        var malformed = 0
        var records: [UsageRecord] = []
        var valid = true
        var deferredRows: [Int] = []

        try scanner.openObject()
        if !scanner.closesObject() {
            repeat {
                switch try scanner.objectKey() {
                case "p":
                    if scanner.isString() { provider = try scanner.string() } else { try scanner.skipValue() }
                case "m":
                    if scanner.isNumber() { modified = try scanner.number() } else { try scanner.skipValue() }
                case "mr":
                    if scanner.isNumber() { malformed = Int(exactly: try scanner.number()) ?? 0 } else { try scanner.skipValue() }
                case "r", "t":
                    if let provider {
                        valid = try rows.decode(&scanner, isClaude: provider == "claude", into: &records) && valid
                    } else {
                        scanner.skipWhitespace()
                        deferredRows.append(scanner.position)
                        try scanner.skipValue()
                    }
                default: try scanner.skipValue()
                }
            } while try scanner.nextMember()
        }
        guard let provider, let modified else { return nil }
        for start in deferredRows {
            var rowScanner = scanner
            rowScanner.rewind(to: start)
            valid = try rows.decode(&rowScanner, isClaude: provider == "claude", into: &records) && valid
        }
        guard valid else { return nil }
        return .init(path: path, kind: UsageProviderKind(raw: provider), modifiedMs: modified, malformed: malformed, records: records)
    }
}

/// Decodes the 11 to 13 item row tuples of SPEC section 2.1 against the cache's model and session tables.
private struct RowDecoder {
    let models: [String]
    let sessions: [String]
    let version: Int
    private var numbers = [Double](repeating: 0, count: 13)

    private static let requiredNumbers: UInt16 = [0, 1, 3, 4, 5, 6, 7, 10].reduce(0) { $0 | 1 << $1 }

    init(models: [String], sessions: [String], version: Int) {
        self.models = models
        self.sessions = sessions
        self.version = version
    }

    /// False when any row is invalid. The array is always consumed to its end so the caller can carry on.
    mutating func decode(_ scanner: inout JSONScanner, isClaude: Bool, into records: inout [UsageRecord]) throws -> Bool {
        var allValid = true
        try scanner.openArray()
        if scanner.closesArray() { return true }
        repeat {
            var count = 0
            var numeric: UInt16 = 0
            var nulls: UInt16 = 0
            var key: String?
            var turn: String?
            try scanner.openArray()
            if !scanner.closesArray() {
                repeat {
                    let bit: UInt16 = count < 13 ? 1 << UInt16(count) : 0
                    if scanner.isString(), count == 8 || count == 12 {
                        let text = try scanner.string()
                        if count == 8 { key = text } else { turn = text }
                    } else if scanner.isNull() {
                        try scanner.consumeNull()
                        nulls |= bit
                    } else if scanner.isNumber(), count < 13 {
                        numbers[count] = try scanner.number()
                        numeric |= bit
                    } else {
                        try scanner.skipValue()
                    }
                    count += 1
                } while try scanner.nextItem()
            }
            guard let record = makeRecord(count: count, numeric: numeric, nulls: nulls, key: key, turn: turn) else {
                allValid = false
                continue
            }
            if isClaude, record.model == "<synthetic>" { continue }
            records.append(record)
        } while try scanner.nextItem()
        return allValid
    }

    private func makeRecord(count: Int, numeric: UInt16, nulls: UInt16, key: String?, turn: String?) -> UsageRecord? {
        guard count >= 11, numeric & Self.requiredNumbers == Self.requiredNumbers,
              let model = index(numbers[1], in: models.count),
              let speed = index(numbers[10], in: UsageSpeed.allCases.count).flatMap(UsageSpeed.init(rawValue:)),
              let uncached = integer(numbers[3]), let cached = integer(numbers[4]), let creation = integer(numbers[5]),
              let output = integer(numbers[6]), let reasoning = integer(numbers[7]) else { return nil }
        var rateModel: String?
        if count > 11 {
            if numeric & 1 << 11 != 0 {
                guard let rate = index(numbers[11], in: models.count) else { return nil }
                rateModel = models[rate]
            } else if nulls & 1 << 11 == 0 {
                return nil
            }
        }
        let session = numeric & 1 << 2 != 0 ? index(numbers[2], in: sessions.count).map { sessions[$0] } ?? "" : ""
        return UsageRecord(
            timestampMs: numbers[0], model: models[model], rateModel: rateModel, session: session,
            uncachedInput: uncached, cachedInput: cached, cacheCreation: creation, output: output, reasoning: reasoning,
            reportedCostUsd: numeric & 1 << 9 != 0 ? numbers[9] : nil, speed: speed,
            dedupeKey: key, unresolvedTierTurn: version >= 7 ? turn : nil)
    }

    private func integer(_ value: Double) -> Int? {
        abs(value) < 9e15 ? Int(value) : nil
    }

    private func index(_ value: Double, in count: Int) -> Int? {
        guard let integer = Int(exactly: value), integer >= 0, integer < count else { return nil }
        return integer
    }
}
