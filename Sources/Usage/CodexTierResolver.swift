import Foundation
import SQLite3

/// Codex turns whose service tier T3 Code had not worked out when it wrote the cache.
///
/// T3 Code asks its own database on every scan, so this does the same, read-only. The tier decides whether
/// a turn is billed at the fast price. Without the database, turns keep the standard speed the cache holds.
enum CodexTierResolver {
    private static let sql = """
    SELECT json_extract(attempt.payload_json, '$.nativeThreadId'), json_extract(turn.payload_json, '$.nativeTurnRef.nativeId'),
      (SELECT json_extract(event.payload_json, '$.modelSelection') FROM orchestration_events AS event
       WHERE event.aggregate_kind = 'thread' AND event.application_event_version = 2 AND event.stream_id = attempt.thread_id
         AND event.event_type = 'run.updated' AND json_extract(event.payload_json, '$.id') = attempt.run_id
         AND json_extract(event.payload_json, '$.activeAttemptId') = attempt.attempt_id
         AND json_extract(event.payload_json, '$.status') = 'running' ORDER BY event.sequence LIMIT 1)
    FROM orchestration_v2_projection_provider_turns AS turn
    LEFT JOIN orchestration_v2_projection_run_attempts AS attempt
      ON attempt.attempt_id = turn.run_attempt_id AND attempt.thread_id = turn.thread_id AND attempt.provider_thread_id = turn.provider_thread_id
    WHERE json_extract(turn.payload_json, '$.nativeTurnRef.driver') = 'codex'
      AND json_extract(turn.payload_json, '$.nativeTurnRef.nativeId') IN (SELECT value FROM json_each(?))
    """

    private struct TurnRow {
        let thread: String?
        let selection: String?
    }

    private struct ModelSelection: Decodable {
        struct Option: Decodable {
            enum Value: Decodable {
                case text(String), flag(Bool), other
                init(from decoder: Decoder) throws {
                    let container = try decoder.singleValueContainer()
                    if let text = try? container.decode(String.self) { self = .text(text) }
                    else if let flag = try? container.decode(Bool.self) { self = .flag(flag) }
                    else { self = .other }
                }
            }
            let id: String?
            let value: Value?
        }
        let model: String?
        let options: [Option]?
    }

    /// The speed T3 Code assigns a selection, or nil when it assigns none.
    static func speed(ofSelection json: String) -> (model: String?, speed: UsageSpeed)? {
        guard let data = json.data(using: .utf8), let selection = try? JSONDecoder().decode(ModelSelection.self, from: data) else { return nil }
        let options = selection.options ?? []
        var tier: String?
        for option in options where option.id == "serviceTier" {
            guard case .text(let text) = option.value else { return nil }
            tier = tier ?? text
        }
        if tier == nil, options.contains(where: { option in
            if option.id == "fastMode", case .flag(true) = option.value { return true }
            return false
        }) { tier = "fast" }
        switch tier {
        case "fast", "priority": return (selection.model, .fast)
        case "ultrafast": return (selection.model, .ultrafast)
        case nil, "default", "standard": return (selection.model, .standard)
        default: return nil
        }
    }

    /// Looks the pending turns up in `database`. Absent, locked or unreadable databases resolve nothing.
    static func resolve(_ dataset: UsageDataset, database: URL) -> UsageDataset {
        var turns = Set<String>()
        for file in dataset.files where file.kind == .codex {
            for record in file.records { if let turn = record.unresolvedTierTurn { turns.insert(turn) } }
        }
        guard !turns.isEmpty, let rows = query(turns: turns, database: database) else { return dataset }

        var resolved = dataset
        resolved.files = dataset.files.map { file in
            guard file.kind == .codex else { return file }
            var records = file.records
            for index in records.indices {
                guard let turn = records[index].unresolvedTierTurn, let row = rows[turn] ?? nil,
                      row.thread == records[index].session, let selection = row.selection,
                      let found = speed(ofSelection: selection), found.model == records[index].model else { continue }
                records[index].speed = found.speed
                records[index].unresolvedTierTurn = nil
            }
            return UsageDataset.File(path: file.path, kind: file.kind, modifiedMs: file.modifiedMs, malformed: file.malformed, records: records)
        }
        return resolved
    }

    /// Turn id to its row. A turn id that appears twice is ambiguous and maps to nil, as in T3 Code.
    private static func query(turns: Set<String>, database: URL) -> [String: TurnRow?]? {
        var db: OpaquePointer?
        guard sqlite3_open_v2(database.path, &db, SQLITE_OPEN_READONLY | SQLITE_OPEN_FULLMUTEX, nil) == SQLITE_OK, let db else {
            if let db { sqlite3_close(db) }
            return nil
        }
        defer { sqlite3_close(db) }
        sqlite3_busy_timeout(db, 250)
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK, let statement else { return nil }
        defer { sqlite3_finalize(statement) }
        guard let list = try? JSONSerialization.data(withJSONObject: turns.sorted()), let json = String(data: list, encoding: .utf8) else { return nil }
        sqlite3_bind_text(statement, 1, json, -1, unsafeBitCast(-1, to: sqlite3_destructor_type.self))

        func text(_ column: Int32) -> String? { sqlite3_column_text(statement, column).map { String(cString: $0) } }
        var rows: [String: TurnRow?] = [:]
        while true {
            switch sqlite3_step(statement) {
            case SQLITE_ROW:
                guard let turn = text(1) else { continue }
                rows[turn] = rows.keys.contains(turn) ? .some(nil) : .some(TurnRow(thread: text(0), selection: text(2)))
            case SQLITE_DONE:
                return rows
            default:
                return nil
            }
        }
    }
}
