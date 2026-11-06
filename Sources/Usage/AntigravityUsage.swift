import Foundation
import SQLite3

enum AntigravityUsage {
    struct Candidate: Sendable {
        var record: UsageRecord
        let identities: [String]
        var timestampQuality: Int
    }
    private struct Entry {
        let index: Int
        let model: String
        let timestamp: Double?
        let usages: [Proto]
    }
    private static let modelIDs: [Int: String] = [
        246: "gemini-2.5-pro", 312: "gemini-2.5-flash", 313: "gemini-2.5-flash-thinking", 329: "gemini-2.5-flash-thinking",
        330: "gemini-2.5-flash-lite", 281: "claude-sonnet-4", 282: "claude-sonnet-4", 290: "claude-opus-4", 291: "claude-opus-4",
        333: "claude-sonnet-4-5", 334: "claude-sonnet-4-5", 340: "claude-haiku-4-5", 341: "claude-haiku-4-5",
        1026: "claude-opus-4-6", 1035: "claude-sonnet-4-6", 1016: "gemini-3.1-pro", 1036: "gemini-3.1-pro", 1037: "gemini-3.1-pro",
        1018: "gemini-3-flash-preview", 1084: "gemini-3-flash-preview", 1047: "gemini-3-flash-preview"
    ]

    private static func model(_ name: String, id: Int) -> String {
        if name.isEmpty { return modelIDs[id] ?? (id > 0 ? "antigravity-model-\(id)" : "") }
        var normalized = name.lowercased().replacingOccurrences(of: "\\s*\\([^)]*\\)\\s*$", with: "", options: .regularExpression).replacingOccurrences(of: " ", with: "-")
        if normalized.hasPrefix("claude-") {
            normalized = normalized.replacingOccurrences(of: "^claude-(4(?:\\.\\d+)?)-(sonnet|opus|haiku)", with: "claude-$2-$1", options: .regularExpression).replacingOccurrences(of: ".", with: "-")
        }
        return normalized
    }

    private static func entry(_ blob: Data, index: Int, step: Bool) throws -> Entry {
        let root = try Proto(blob)
        if !step, root.bytes(1) == nil { throw LocalUsageError.malformed }
        let data = step ? root : try root.nested(1)
        let modelData = step ? try data.nested(24) : data
        var usages: [Proto] = []
        if let usage = data.bytes(step ? 9 : 4) { usages.append(try Proto(usage)) }
        for retry in data.values[step ? 28 : 17] ?? [] {
            guard case .bytes(let bytes) = retry else { throw LocalUsageError.malformed }
            if let usage = try Proto(bytes).bytes(2) { usages.append(try Proto(usage)) }
        }
        let name = try modelData.text(step ? 12 : 19)
        let alternate = try modelData.text(step ? 8 : 21)
        let timestamp: Double?
        if step { timestamp = try data.nested(8).timestamp ?? data.nested(1).timestamp }
        else { timestamp = try data.nested(9).nested(4).timestamp }
        return Entry(index: index, model: model(name.isEmpty ? alternate : name, id: modelData.number(step ? 1 : 3)), timestamp: timestamp, usages: usages)
    }

    static func read(_ url: URL) throws -> LocalUsageRead {
        let fallback = try url.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate ?? Date(timeIntervalSince1970: 0)
        return try LocalUsageSQLite.read(url) { db in
            let tables = try db.tables()
            guard tables.contains("steps") || tables.contains("gen_metadata") else { throw LocalUsageError.schema }
            var result = LocalUsageRead()
            func entries(_ table: String, column: String, step: Bool) throws -> [Entry] {
                guard tables.contains(table) else { return [] }
                var entries: [Entry] = []
                try db.rows("SELECT idx, \(column) FROM \(table) ORDER BY rowid") { row in
                    if step, sqlite3_column_type(row, 1) == SQLITE_NULL { return }
                    guard sqlite3_column_type(row, 0) == SQLITE_INTEGER, let blob = LocalUsageSQLite.blob(row, 1) else { result.malformed += 1; return }
                    do { entries.append(try entry(blob, index: Int(sqlite3_column_int64(row, 0)), step: step)) }
                    catch { result.malformed += 1 }
                }
                return entries.sorted { $0.index < $1.index }
            }
            let generations = try entries("gen_metadata", column: "data", step: false)
            let steps = try entries("steps", column: "metadata", step: true)
            var trajectory: Double?
            if tables.contains("trajectory_metadata_blob") {
                try db.rows("SELECT data FROM trajectory_metadata_blob") { row in
                    guard let blob = LocalUsageSQLite.blob(row, 0) else { result.malformed += 1; return }
                    do { if trajectory == nil { trajectory = try Proto(blob).nested(2).timestamp } }
                    catch { result.malformed += 1 }
                }
            }
            var generationModels: [Int: String] = [:]
            for generation in generations { generationModels[generation.index] = generation.model }
            let session = url.deletingPathExtension().lastPathComponent
            for (source, entries) in [("step", steps), ("generation", generations)] {
                for (index, entry) in entries.enumerated() {
                    for (usageIndex, usage) in entry.usages.enumerated() {
                        let reasoning = usage.number(9)
                        let output = max(usage.number(3), reasoning + usage.number(10))
                        let identities: [String]
                        do { identities = try [11, 12, 7].compactMap { field in
                            let value = try usage.text(field)
                            return value.isEmpty ? nil : "antigravity:\(field):\(value)"
                        } } catch { result.malformed += 1; continue }
                        let name = modelIDs[usage.number(1)] ?? [entry.model, source == "step" ? generationModels[entry.index] ?? "" : "", model("", id: usage.number(1))].first(where: { !$0.isEmpty }) ?? "antigravity-unknown"
                        let timestamp = entry.timestamp ?? trajectory ?? fallback.timeIntervalSince1970 * 1000
                        let record = UsageRecord(timestampMs: timestamp, model: name,
                                                 rateModel: name.replacingOccurrences(of: "-(?:low|medium|high)$", with: "", options: .regularExpression), session: session,
                                                 uncachedInput: usage.number(2), cachedInput: usage.number(5), cacheCreation: usage.number(4),
                                                 output: output, reasoning: min(output, reasoning),
                                                 dedupeKey: identities.first ?? "antigravity:\(session):\(source):\(index):\(usageIndex)")
                        if record.processed > 0 {
                            result.candidates.append(.init(record: record, identities: identities, timestampQuality: entry.timestamp != nil ? 2 : trajectory != nil ? 1 : 0))
                        }
                    }
                }
            }
            return result
        }
    }

    static func merge(_ input: [(Candidate, Int)]) -> [(UsageRecord, Int)] {
        var candidates = input.map(\.0)
        var owners = Array(input.indices)
        var parents = Array(input.indices)
        var keys: [String: Int] = [:]
        func root(_ index: Int) -> Int {
            var current = index
            while parents[current] != current { current = parents[current] }
            return current
        }
        for index in candidates.indices {
            for key in candidates[index].identities {
                if let previous = keys[key] {
                    let left = root(index), right = root(previous)
                    if left != right {
                        let first = owners[left] < owners[right] ? left : right
                        let other = first == left ? right : left
                        let best = candidates[left].timestampQuality > candidates[right].timestampQuality ? left : candidates[left].timestampQuality < candidates[right].timestampQuality ? right : candidates[left].record.timestampMs <= candidates[right].record.timestampMs ? left : right
                        var record = candidates[first].record
                        if record.model == "antigravity-unknown" { record.model = candidates[other].record.model; record.rateModel = candidates[other].record.rateModel }
                        record.timestampMs = candidates[best].record.timestampMs
                        record.uncachedInput = max(candidates[left].record.uncachedInput, candidates[right].record.uncachedInput)
                        record.cachedInput = max(candidates[left].record.cachedInput, candidates[right].record.cachedInput)
                        record.cacheCreation = max(candidates[left].record.cacheCreation, candidates[right].record.cacheCreation)
                        record.output = max(candidates[left].record.output, candidates[right].record.output)
                        record.reasoning = max(candidates[left].record.reasoning, candidates[right].record.reasoning)
                        candidates[left].record = record
                        candidates[left].timestampQuality = candidates[best].timestampQuality
                        owners[left] = owners[first]
                        parents[right] = left
                    }
                }
                keys[key] = index
            }
        }
        return candidates.indices.compactMap { index in parents[index] == index ? (candidates[index].record, input[owners[index]].1) : nil }
    }

    private struct Proto {
        enum Value { case number(Int), bytes(Data) }
        var values: [Int: [Value]] = [:]
        init(_ data: Data) throws {
            let bytes = [UInt8](data)
            var offset = 0
            func varint() throws -> UInt64 {
                var value: UInt64 = 0
                for shift in stride(from: 0, through: 63, by: 7) {
                    guard offset < bytes.count else { throw LocalUsageError.malformed }
                    let byte = bytes[offset]; offset += 1
                    if shift == 63, byte > 1 { throw LocalUsageError.malformed }
                    value |= UInt64(byte & 127) << shift
                    if byte < 128 { return value }
                }
                throw LocalUsageError.malformed
            }
            while offset < bytes.count {
                let tag = try varint()
                guard tag > 7, tag < UInt64(Int.max) else { throw LocalUsageError.malformed }
                let field = Int(tag / 8), wire = tag % 8
                switch wire {
                case 0:
                    let value = try varint()
                    values[field, default: []].append(.number(value <= 9_007_199_254_740_991 ? Int(value) : 0))
                case 1, 2, 5:
                    let length = wire == 2 ? try varint() : wire == 1 ? 8 : 4
                    guard length <= bytes.count - offset else { throw LocalUsageError.malformed }
                    if wire == 2 { values[field, default: []].append(.bytes(Data(bytes[offset..<offset + Int(length)]))) }
                    offset += Int(length)
                default: throw LocalUsageError.malformed
                }
            }
        }
        func number(_ field: Int) -> Int { if case .number(let value) = values[field]?.first { return value }; return 0 }
        func bytes(_ field: Int) -> Data? { if case .bytes(let value) = values[field]?.first { return value }; return nil }
        func nested(_ field: Int) throws -> Self { try Self(bytes(field) ?? Data()) }
        func text(_ field: Int) throws -> String {
            guard let bytes = bytes(field) else { return "" }
            guard let value = String(data: bytes, encoding: .utf8) else { throw LocalUsageError.malformed }
            return value.trimmingCharacters(in: .whitespacesAndNewlines)
        }
        var timestamp: Double? { number(1) > 0 ? Double(number(1)) * 1000 + Double(number(2) / 1_000_000) : nil }
    }
}
