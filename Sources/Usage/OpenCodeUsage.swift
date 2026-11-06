import Foundation
import SQLite3

enum OpenCodeUsage {
    private struct Message: Decodable {
        struct Model: Decodable { let id: String?; let modelID: String? }
        struct Time: Decodable { let created: Double? }
        struct Tokens: Decodable {
            struct Cache: Decodable { let read: Double?; let write: Double? }
            let input: Double?; let output: Double?; let reasoning: Double?; let cache: Cache?
        }
        let role: String?
        let id: String?
        let sessionID: String?
        let modelID: String?
        let model: Model?
        let time: Time?
        let tokens: Tokens?
        let cost: Double?
    }

    static func message(_ data: Data, id: String = "", session: String = "", timestamp: Double? = nil) throws -> UsageRecord? {
        let message = try JSONDecoder().decode(Message.self, from: data)
        guard message.role == nil || message.role == "assistant" else { return nil }
        guard let model = [message.model?.id, message.model?.modelID, message.modelID].compactMap({ $0 }).first(where: { !$0.isEmpty }),
              let time = message.time?.created ?? timestamp, time.isFinite, time > 0 else { return nil }
        func token(_ value: Double?) throws -> Int {
            guard let value else { return 0 }
            guard value.isFinite, value >= 0, value < 9e15 else { throw LocalUsageError.malformed }
            return Int(value)
        }
        let reasoning = try token(message.tokens?.reasoning)
        let output = try token(message.tokens?.output) + reasoning
        let key = id.isEmpty ? message.id ?? "" : id
        let cost = message.cost.flatMap { $0.isFinite && $0 > 0 ? $0 : nil }
        let record = UsageRecord(timestampMs: time, model: model, session: session.isEmpty ? message.sessionID ?? "" : session,
                                 uncachedInput: try token(message.tokens?.input), cachedInput: try token(message.tokens?.cache?.read),
                                 cacheCreation: try token(message.tokens?.cache?.write), output: output, reasoning: reasoning,
                                 reportedCostUsd: cost, dedupeKey: key.isEmpty ? nil : "opencode:\(key)")
        return record.processed > 0 ? record : nil
    }

    static func read(_ url: URL) throws -> LocalUsageRead {
        if url.pathExtension == "json" {
            let record = try message(Data(contentsOf: url), id: url.deletingPathExtension().lastPathComponent)
            return .init(records: record.map { [$0] } ?? [], updated: (try url.resourceValues(forKeys: [.contentModificationDateKey])).contentModificationDate)
        }
        return try LocalUsageSQLite.read(url) { db in
            let tables = try db.tables()
            guard tables.contains("message") || tables.contains("session_message") else { throw LocalUsageError.schema }
            var result = LocalUsageRead()
            for table in ["message", "session_message"] where tables.contains(table) {
                var hasTimestamp = false
                try db.rows("PRAGMA table_info(\(table))") { row in
                    if LocalUsageSQLite.text(row, 1) == "time_created" { hasTimestamp = true }
                }
                let predicate = table == "session_message" ? " WHERE type = 'assistant'" : ""
                try db.rows("SELECT id, session_id, data, \(hasTimestamp ? "time_created" : "NULL") FROM \(table)\(predicate) ORDER BY rowid") { row in
                    guard let json = LocalUsageSQLite.text(row, 2) else { result.malformed += 1; return }
                    do {
                        if let record = try message(Data(json.utf8), id: LocalUsageSQLite.text(row, 0) ?? "", session: LocalUsageSQLite.text(row, 1) ?? "",
                                                    timestamp: sqlite3_column_type(row, 3) == SQLITE_NULL ? nil : sqlite3_column_double(row, 3)) {
                            result.records.append(record)
                        }
                    } catch { result.malformed += 1 }
                }
            }
            return result
        }
    }
}
