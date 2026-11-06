import Foundation
import SQLite3

struct LocalUsageSQLite {
    let db: OpaquePointer

    static func read<T>(_ url: URL, _ body: (Self) throws -> T) throws -> T {
        var pointer: OpaquePointer?
        guard sqlite3_open_v2(url.path, &pointer, SQLITE_OPEN_READONLY | SQLITE_OPEN_FULLMUTEX, nil) == SQLITE_OK, let pointer else {
            if let pointer { sqlite3_close(pointer) }
            throw LocalUsageError.sqlite
        }
        defer { sqlite3_close(pointer) }
        sqlite3_busy_timeout(pointer, 100)
        guard sqlite3_exec(pointer, "BEGIN", nil, nil, nil) == SQLITE_OK else { throw LocalUsageError.sqlite }
        return try body(Self(db: pointer))
    }

    func rows(_ sql: String, _ body: (OpaquePointer) throws -> Void) throws {
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK, let statement else { throw LocalUsageError.schema }
        defer { sqlite3_finalize(statement) }
        while true {
            switch sqlite3_step(statement) {
            case SQLITE_ROW: try body(statement)
            case SQLITE_DONE: return
            default: throw LocalUsageError.sqlite
            }
        }
    }

    func tables() throws -> Set<String> {
        var names = Set<String>()
        try rows("SELECT name FROM sqlite_master WHERE type = 'table'") { row in
            if let name = Self.text(row, 0) { names.insert(name) }
        }
        return names
    }

    static func text(_ row: OpaquePointer, _ column: Int32) -> String? {
        guard sqlite3_column_type(row, column) == SQLITE_TEXT else { return nil }
        return sqlite3_column_text(row, column).map { String(cString: $0) }
    }

    static func blob(_ row: OpaquePointer, _ column: Int32) -> Data? {
        guard sqlite3_column_type(row, column) == SQLITE_BLOB else { return nil }
        let count = Int(sqlite3_column_bytes(row, column))
        guard count > 0, let bytes = sqlite3_column_blob(row, column) else { return Data() }
        return Data(bytes: bytes, count: count)
    }
}
