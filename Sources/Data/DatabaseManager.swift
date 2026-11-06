import Foundation
import SQLite3
import os

public enum DatabaseReadError: Error, LocalizedError, Equatable, Sendable {
    case missing, unavailable, incompatibleSchema, busy, invalidData

    public var errorDescription: String? {
        switch self {
        case .missing: return "T3 Code data not found. Open T3 Code or check the configured data directory."
        case .unavailable: return "Cannot read T3 Code data. Check file permissions."
        case .incompatibleSchema: return "This T3 Code database schema is not supported."
        case .busy: return "T3 Code data is busy. Retrying automatically."
        case .invalidData: return "T3 Code data could not be decoded. Retrying automatically."
        }
    }
}

public struct ThreadSnapshot: Sendable {
    public let threads: [ThreadItem]
    public let isServerOnline: Bool
}

/// Owns SQLite and date formatters off the main actor. Never writes to T3 Code's database.
public actor DatabaseManager {
    public static let shared = DatabaseManager()
    private let config: EnvironmentConfig
    private let logger = Logger(subsystem: "com.t3code.bar", category: "database")
    private let isoFormatter = ISO8601DateFormatter()
    private let isoFormatterFractional: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter
    }()

    public init(config: EnvironmentConfig = .shared) {
        self.config = config
    }

    public func snapshot() throws -> ThreadSnapshot {
        ThreadSnapshot(threads: try fetchActiveThreads(), isServerOnline: config.isServerOnline)
    }

    /// The start of the thread's latest assistant reply, for the hover preview.
    ///
    /// Reads only `messages`, through its `(thread_id, created_at)` index: one row, text capped in SQL.
    /// `turn_items` payloads run to a megabyte and are never touched here.
    public func preview(threadId: String) throws -> ThreadPreview? {
        let db = try openDatabase()
        defer { sqlite3_close(db) }

        var statement: OpaquePointer?
        let sql = """
        SELECT substr(json_extract(payload_json, '$.text'), 1, 4000), updated_at
        FROM orchestration_v2_projection_messages
        WHERE thread_id = ?1 AND role = 'assistant'
          AND COALESCE(json_extract(payload_json, '$.text'), '') != ''
        ORDER BY created_at DESC
        LIMIT 1
        """
        let prepared = sqlite3_prepare_v2(db, sql, -1, &statement, nil)
        guard prepared == SQLITE_OK, let statement else {
            throw prepared == SQLITE_ERROR ? DatabaseReadError.incompatibleSchema : readError(code: prepared, db: db)
        }
        defer { sqlite3_finalize(statement) }
        sqlite3_bind_text(statement, 1, threadId, -1, unsafeBitCast(-1, to: sqlite3_destructor_type.self))

        switch sqlite3_step(statement) {
        case SQLITE_ROW:
            let excerpt = PreviewText.excerpt(text(statement, 0) ?? "")
            return excerpt.isEmpty ? nil : ThreadPreview(text: excerpt, updatedAt: parseDate(text(statement, 1)))
        case SQLITE_DONE:
            return nil
        case let code:
            throw readError(code: code, db: db)
        }
    }

    /// Per-turn token totals since the start of `period`, summed in SQL so Swift never decodes the payloads.
    ///
    /// A turn counts as having usage when `turnTokenUsage.inputTokens` is present. Partial turns carry
    /// real, lower-bound counts, so they stay in the sums and are only counted for the footnote.
    /// `started_at` is ISO-8601 with milliseconds and a `Z`, which sorts as text.
    public func usage(period: UsagePeriod, now: Date = Date()) throws -> UsageSnapshot {
        let db = try openDatabase()
        defer { sqlite3_close(db) }

        var statement: OpaquePointer?
        let sql = """
        SELECT driver,
               COUNT(input), COUNT(*) - COUNT(input), COALESCE(SUM(input IS NOT NULL AND status = 'partial'), 0),
               COALESCE(SUM(input), 0), COALESCE(SUM(cached), 0), COALESCE(SUM(creation), 0), COALESCE(SUM(output), 0)
        FROM (
            SELECT json_extract(payload_json, '$.nativeTurnRef.driver') AS driver,
                   json_extract(payload_json, '$.turnTokenUsage.usageStatus') AS status,
                   json_extract(payload_json, '$.turnTokenUsage.inputTokens') AS input,
                   json_extract(payload_json, '$.turnTokenUsage.cachedInputTokens') AS cached,
                   json_extract(payload_json, '$.turnTokenUsage.cacheCreationTokens') AS creation,
                   json_extract(payload_json, '$.turnTokenUsage.outputTokens') AS output
            FROM orchestration_v2_projection_provider_turns
            WHERE started_at >= ?1
        )
        GROUP BY driver
        """
        let prepared = sqlite3_prepare_v2(db, sql, -1, &statement, nil)
        guard prepared == SQLITE_OK, let statement else {
            throw prepared == SQLITE_ERROR ? DatabaseReadError.incompatibleSchema : readError(code: prepared, db: db)
        }
        defer { sqlite3_finalize(statement) }
        let cutoff = isoFormatterFractional.string(from: period.start(before: now))
        sqlite3_bind_text(statement, 1, cutoff, -1, unsafeBitCast(-1, to: sqlite3_destructor_type.self))

        var drivers: [DriverUsage] = []
        while true {
            switch sqlite3_step(statement) {
            case SQLITE_ROW:
                drivers.append(DriverUsage(
                    driver: text(statement, 0),
                    tokens: TokenBreakdown(input: count(statement, 4), cachedInput: count(statement, 5),
                                           cacheCreation: count(statement, 6), output: count(statement, 7)),
                    turns: count(statement, 1), turnsWithoutUsage: count(statement, 2), partialTurns: count(statement, 3)))
            case SQLITE_DONE:
                return UsageSnapshot(period: period, drivers: drivers)
            case let code:
                throw readError(code: code, db: db)
            }
        }
    }

    private func count(_ statement: OpaquePointer, _ column: Int32) -> Int {
        Int(sqlite3_column_int64(statement, column))
    }

    /// Reads `class` and `resetAt` from a `failure` object. Anything else is ignored.
    private func parseFailure(_ json: String?) -> (failureClass: String?, resetAt: Date?) {
        guard let data = json?.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return (nil, nil)
        }
        return (object["class"] as? String, parseDate(object["resetAt"] as? String))
    }

    private func parseDate(_ value: String?) -> Date? {
        guard let value, !value.isEmpty else { return nil }
        return isoFormatterFractional.date(from: value) ?? isoFormatter.date(from: value)
    }

    private func readError(code: Int32, db: OpaquePointer?) -> DatabaseReadError {
        if let db {
            let message = String(cString: sqlite3_errmsg(db))
            logger.error("SQLite read failed (\(code)): \(message, privacy: .private)")
        }
        switch code & 0xff {
        case SQLITE_BUSY, SQLITE_LOCKED: return .busy
        case SQLITE_CANTOPEN, SQLITE_PERM, SQLITE_AUTH, SQLITE_IOERR: return .unavailable
        default: return .invalidData
        }
    }

    private func text(_ statement: OpaquePointer, _ column: Int32) -> String? {
        sqlite3_column_text(statement, column).map { String(cString: $0) }
    }

    private func openDatabase() throws -> OpaquePointer {
        guard FileManager.default.fileExists(atPath: config.stateDbUrl.path) else {
            throw DatabaseReadError.missing
        }
        var db: OpaquePointer?
        let result = sqlite3_open_v2(config.stateDbUrl.path, &db, SQLITE_OPEN_READONLY | SQLITE_OPEN_FULLMUTEX, nil)
        guard result == SQLITE_OK, let validDb = db else {
            let error = readError(code: result, db: db)
            if let db { sqlite3_close(db) }
            throw error
        }
        sqlite3_busy_timeout(validDb, 250)
        return validDb
    }

    /// The `failure` object (class such as `usage_limit`, and `resetAt`) of the latest
    /// executed run, when it failed. Mirrors T3 Code: the newest failed `error` item on
    /// the run's root node.
    private static let latestFailureSQL = """
    (
        SELECT CASE WHEN r.status = 'failed' THEN (
            SELECT json_extract(ti.payload_json, '$.failure')
            FROM orchestration_v2_projection_turn_items ti
            WHERE ti.thread_id = r.thread_id AND ti.run_id = r.run_id
              AND ti.type = 'error' AND ti.status = 'failed'
              AND ti.node_id IS json_extract(r.payload_json, '$.rootNodeId')
            ORDER BY ti.updated_at DESC, ti.ordinal DESC, ti.turn_item_id DESC
            LIMIT 1
        ) END
        FROM orchestration_v2_projection_runs r
        WHERE r.thread_id = t.thread_id
          AND r.status != 'queued'
          AND NOT (r.status = 'cancelled' AND json_extract(r.payload_json, '$.startedAt') IS NULL)
          AND COALESCE(json_extract(r.payload_json, '$.silent'), 0) != 1
        ORDER BY r.completed_at IS NULL DESC, r.completed_at DESC, r.ordinal DESC
        LIMIT 1
    )
    """

    /// A tool call that only waits for another thread. Mirrors T3 Code's `deriveBlockedOnThreadWait`.
    private static let threadWaitToolSQL = """
    ti.type = 'dynamic_tool' AND (
        json_extract(ti.payload_json, '$.toolName') GLOB '*t3_thread_wait'
        OR (json_extract(ti.payload_json, '$.toolName') GLOB '*delegate_task'
            AND json_extract(ti.payload_json, '$.input.mode') = 'wait')
    )
    """

    /// 1 when the active run is running and everything it is executing is a thread wait.
    private static let blockedOnThreadWaitSQL = """
    (
        SELECT CASE WHEN r.status = 'running'
            AND EXISTS (
                SELECT 1 FROM orchestration_v2_projection_turn_items ti
                WHERE ti.thread_id = r.thread_id AND ti.run_id = r.run_id
                  AND ti.status IN ('running', 'waiting') AND \(threadWaitToolSQL))
            AND NOT EXISTS (
                SELECT 1 FROM orchestration_v2_projection_turn_items ti
                WHERE ti.thread_id = r.thread_id AND ti.run_id = r.run_id
                  AND ti.type IN ('command_execution', 'dynamic_tool', 'subagent', 'file_change', 'file_search', 'web_search', 'compaction')
                  AND ti.status IN ('running', 'waiting')
                  AND NOT (ti.type = 'subagent' AND json_extract(ti.payload_json, '$.origin') = 'app_owned')
                  AND NOT (\(threadWaitToolSQL)))
            THEN 1 ELSE 0 END
        FROM orchestration_v2_projection_runs r
        WHERE r.thread_id = t.thread_id
          AND r.status IN ('preparing', 'starting', 'running', 'waiting')
          AND COALESCE(json_extract(r.payload_json, '$.silent'), 0) != 1
        ORDER BY r.ordinal DESC, r.run_id DESC
        LIMIT 1
    )
    """

    public func fetchActiveThreads() throws -> [ThreadItem] {
        let db = try openDatabase()
        defer { sqlite3_close(db) }

        func buildQuery(failure: String, blockedOnThreadWait: String) -> String {
        return """
        SELECT
            t.thread_id,
            t.title,
            t.project_id,
            COALESCE(p.title, 'No Project') as project_title,
            t.created_at,
            t.updated_at,
            (
                SELECT r.status
                FROM orchestration_v2_projection_runs r
                WHERE r.thread_id = t.thread_id
                  AND r.status IN ('preparing', 'starting', 'running', 'waiting')
                  AND COALESCE(json_extract(r.payload_json, '$.silent'), 0) != 1
                ORDER BY r.ordinal DESC, r.run_id DESC
                LIMIT 1
            ) as active_run_status,
            (
                SELECT COALESCE(
                  json_extract(r.payload_json, '$.workStartedAt'),
                  json_extract(r.payload_json, '$.startedAt'),
                  r.requested_at
                )
                FROM orchestration_v2_projection_runs r
                WHERE r.thread_id = t.thread_id
                  AND r.status IN ('preparing', 'starting', 'running', 'waiting')
                  AND COALESCE(json_extract(r.payload_json, '$.silent'), 0) != 1
                ORDER BY r.ordinal DESC, r.run_id DESC
                LIMIT 1
            ) as active_run_started_at,
            (
                SELECT r.status
                FROM orchestration_v2_projection_runs r
                WHERE r.thread_id = t.thread_id
                  AND r.status != 'queued'
                  AND NOT (r.status = 'cancelled' AND json_extract(r.payload_json, '$.startedAt') IS NULL)
                  AND COALESCE(json_extract(r.payload_json, '$.silent'), 0) != 1
                ORDER BY r.completed_at IS NULL DESC, r.completed_at DESC, r.ordinal DESC
                LIMIT 1
            ) as latest_run_status,
            (
                SELECT COALESCE(json_extract(r.payload_json, '$.startedAt'), r.requested_at)
                FROM orchestration_v2_projection_runs r
                WHERE r.thread_id = t.thread_id
                  AND r.status != 'queued'
                  AND NOT (r.status = 'cancelled' AND json_extract(r.payload_json, '$.startedAt') IS NULL)
                  AND COALESCE(json_extract(r.payload_json, '$.silent'), 0) != 1
                ORDER BY r.completed_at IS NULL DESC, r.completed_at DESC, r.ordinal DESC
                LIMIT 1
            ) as latest_run_started_at,
            (
                SELECT r.completed_at
                FROM orchestration_v2_projection_runs r
                WHERE r.thread_id = t.thread_id
                  AND r.status != 'queued'
                  AND NOT (r.status = 'cancelled' AND json_extract(r.payload_json, '$.startedAt') IS NULL)
                  AND COALESCE(json_extract(r.payload_json, '$.silent'), 0) != 1
                ORDER BY r.completed_at IS NULL DESC, r.completed_at DESC, r.ordinal DESC
                LIMIT 1
            ) as latest_run_completed_at,
            (SELECT COUNT(*) FROM orchestration_v2_projection_runtime_requests req WHERE req.thread_id = t.thread_id AND req.status = 'pending' AND req.kind = 'user_input') as pending_user_input,
            (SELECT COUNT(*) FROM orchestration_v2_projection_runtime_requests req WHERE req.thread_id = t.thread_id AND req.status = 'pending' AND req.kind != 'user_input') as pending_approval,
            json_extract(t.payload_json, '$.branch') as branch,
            json_extract(t.payload_json, '$.worktreePath') as worktree_path,
            json_extract(t.payload_json, '$.modelSelection.model') as model_name,
            (SELECT number FROM projection_thread_pull_requests pr WHERE pr.thread_id = t.thread_id LIMIT 1) as pr_number,
            CASE WHEN json_extract(t.payload_json, '$.pinnedAt') IS NOT NULL THEN 1 ELSE 0 END as is_pinned,
            CASE WHEN json_extract(t.payload_json, '$.settledAt') IS NOT NULL THEN 1 ELSE 0 END as is_settled,
            json_extract(t.payload_json, '$.snoozedUntil') as snoozed_until,
            CASE WHEN (SELECT json_extract(pt.payload_json, '$.goal.status') FROM orchestration_v2_projection_provider_threads pt WHERE pt.thread_id = t.thread_id ORDER BY pt.last_run_ordinal DESC LIMIT 1) = 'active' THEN 1 ELSE 0 END as has_goal,
            json_extract(t.payload_json, '$.snoozedAt') as snoozed_at,
            json_extract(t.payload_json, '$.pinOrderKey') as pin_order_key,
            json_extract(t.payload_json, '$.activeOrderKey') as active_order_key,
            (SELECT group_concat(DISTINCT json_extract(r.payload_json, '$.providerInstanceId')) FROM orchestration_v2_projection_runs r WHERE r.thread_id = t.thread_id) as provider_history,
            (SELECT m.updated_at FROM orchestration_v2_projection_messages m WHERE m.thread_id = t.thread_id AND m.role = 'user' ORDER BY m.updated_at DESC LIMIT 1) as latest_user_message_at,
            p.project_icon_json,
            p.favicon_path,
            p.workspace_root,
            json_extract(t.payload_json, '$.unsettledAt') as unsettled_at,
            json_extract(t.payload_json, '$.lastVisitedAt') as last_visited_at,
            (SELECT json_extract(pt.payload_json, '$.status') FROM orchestration_v2_projection_provider_threads pt WHERE pt.thread_id = t.thread_id ORDER BY pt.last_run_ordinal DESC LIMIT 1) as session_status,
            (SELECT json_extract(pt.payload_json, '$.updatedAt') FROM orchestration_v2_projection_provider_threads pt WHERE pt.thread_id = t.thread_id ORDER BY pt.last_run_ordinal DESC LIMIT 1) as session_updated_at,
            \(failure) as latest_failure,
            (SELECT COUNT(*) FROM json_each(t.payload_json, '$.guestShare.messages') g WHERE json_extract(g.value, '$.status') = 'pending') as pending_guest_messages,
            EXISTS (SELECT 1 FROM orchestration_v2_projection_runs r WHERE r.thread_id = t.thread_id) as has_runs,
            COALESCE(\(blockedOnThreadWait), 0) as blocked_on_thread_wait
        FROM orchestration_v2_projection_threads t
        LEFT JOIN projection_projects p ON t.project_id = p.project_id
        WHERE t.thread_id NOT LIKE 'thread:%' AND t.deleted_at IS NULL AND t.archived_at IS NULL
          AND json_extract(t.payload_json, '$.settledAt') IS NULL
          AND COALESCE(json_extract(t.payload_json, '$.lineage.sideChat'), 0) != 1
          AND COALESCE(json_extract(t.payload_json, '$.lineage.relationshipToParent'), '') != 'subagent'
        """
        }

        var statement: OpaquePointer?
        var result = sqlite3_prepare_v2(
            db, buildQuery(failure: Self.latestFailureSQL, blockedOnThreadWait: Self.blockedOnThreadWaitSQL), -1, &statement, nil)
        if result == SQLITE_ERROR {
            // A T3 Code build without turn items can still list threads; only the
            // Limited and Waiting labels need them, so retry without those columns.
            sqlite3_finalize(statement)
            statement = nil
            result = sqlite3_prepare_v2(db, buildQuery(failure: "NULL", blockedOnThreadWait: "NULL"), -1, &statement, nil)
        }
        guard result == SQLITE_OK, let stmt = statement else {
            let error = readError(code: result, db: db)
            throw result == SQLITE_ERROR ? DatabaseReadError.incompatibleSchema : error
        }
        defer { sqlite3_finalize(stmt) }
        var items = try parseThreadItems(from: stmt)

        // Pinned/manual inbox order. Sort the full result before any UI filtering.
        items.sort { a, b in
            if a.isPinned != b.isPinned {
                return a.isPinned && !b.isPinned
            }
            if a.isPinned {
                // Pinned threads with explicit order keys come first.
                if let keyA = a.pinOrderKey, let keyB = b.pinOrderKey {
                    if keyA != keyB { return keyA < keyB }
                } else if a.pinOrderKey != nil && b.pinOrderKey == nil {
                    return true
                } else if a.pinOrderKey == nil && b.pinOrderKey != nil {
                    return false
                }
                if a.createdAt != b.createdAt {
                    return a.createdAt > b.createdAt
                }
                return a.id < b.id
            } else {
                // Unkeyed active threads precede manually ordered ones.
                let keyA = a.activeOrderKey
                let keyB = b.activeOrderKey
                if keyA == nil && keyB != nil {
                    return true
                }
                if keyA != nil && keyB == nil {
                    return false
                }
                if let keyA = keyA, let keyB = keyB {
                    if keyA != keyB { return keyA < keyB }
                } else {
                    // Newly created or unsettled threads come first.
                    let effA = max(a.createdAt.timeIntervalSince1970, a.unsettledAt?.timeIntervalSince1970 ?? 0)
                    let effB = max(b.createdAt.timeIntervalSince1970, b.unsettledAt?.timeIntervalSince1970 ?? 0)
                    if effA != effB {
                        return effA > effB
                    }
                }
                return a.id < b.id
            }
        }

        return items
    }

    private func parseThreadItems(from stmt: OpaquePointer) throws -> [ThreadItem] {
        var results: [ThreadItem] = []
        let now = Date()

        while true {
            let step = sqlite3_step(stmt)
            if step == SQLITE_DONE { break }
            guard step == SQLITE_ROW else {
                throw readError(code: step, db: sqlite3_db_handle(stmt))
            }
            guard let threadId = text(stmt, 0), let title = text(stmt, 1),
                  let projectId = text(stmt, 2), let projectTitle = text(stmt, 3) else {
                throw DatabaseReadError.invalidData
            }

            let createdAtStr = sqlite3_column_text(stmt, 4).map { String(cString: $0) }
            let updatedAtStr = sqlite3_column_text(stmt, 5).map { String(cString: $0) }
            let activeRunStatus = sqlite3_column_text(stmt, 6).map { String(cString: $0) }
            let activeRunStartedAtStr = sqlite3_column_text(stmt, 7).map { String(cString: $0) }
            let latestRunStatus = sqlite3_column_text(stmt, 8).map { String(cString: $0) }
            let latestRunStartedAtStr = sqlite3_column_text(stmt, 9).map { String(cString: $0) }
            let latestRunCompletedAtStr = sqlite3_column_text(stmt, 10).map { String(cString: $0) }

            let pendingUserInput = Int(sqlite3_column_int(stmt, 11))
            let pendingApproval = Int(sqlite3_column_int(stmt, 12))

            let branch = sqlite3_column_text(stmt, 13).map { String(cString: $0) }
            let worktreePath = sqlite3_column_text(stmt, 14).map { String(cString: $0) }
            let modelName = sqlite3_column_text(stmt, 15).map { String(cString: $0) }
            let prNumber: Int? = sqlite3_column_type(stmt, 16) != SQLITE_NULL ? Int(sqlite3_column_int(stmt, 16)) : nil
            let isPinned = sqlite3_column_int(stmt, 17) > 0
            let isSettled = sqlite3_column_int(stmt, 18) > 0
            let snoozedUntil = parseDate(text(stmt, 19))
            let hasGoal = sqlite3_column_int(stmt, 20) > 0
            let snoozedAt = parseDate(text(stmt, 21))
            let pinOrderKey = sqlite3_column_text(stmt, 22).map { String(cString: $0) }
            let activeOrderKey = sqlite3_column_text(stmt, 23).map { String(cString: $0) }
            let providerHistoryStr = sqlite3_column_text(stmt, 24).map { String(cString: $0) }
            let latestUserMessageAtStr = sqlite3_column_text(stmt, 25).map { String(cString: $0) }
            let projectIconJson = sqlite3_column_text(stmt, 26).map { String(cString: $0) }
            let projectFaviconPath = sqlite3_column_text(stmt, 27).map { String(cString: $0) }
            let workspaceRoot = sqlite3_column_text(stmt, 28).map { String(cString: $0) }
            let unsettledAtStr = sqlite3_column_text(stmt, 29).map { String(cString: $0) }
            let lastVisitedAtStr = text(stmt, 30)
            let failure = parseFailure(text(stmt, 33))
            let pendingGuestMessages = Int(sqlite3_column_int(stmt, 34))
            let hasRuns = sqlite3_column_int(stmt, 35) > 0
            let blockedOnThreadWait = sqlite3_column_int(stmt, 36) > 0

            let providers = providerHistoryStr?
                .split(separator: ",")
                .map { String($0).trimmingCharacters(in: .whitespaces) }
                .filter { !$0.isEmpty } ?? modelName.map { [$0] } ?? []

            guard let createdAt = parseDate(createdAtStr), let updatedAt = parseDate(updatedAtStr) else {
                throw DatabaseReadError.invalidData
            }
            let lastVisitedAt = parseDate(lastVisitedAtStr)
            let completedAt = parseDate(latestRunCompletedAtStr)
            let sessionUpdatedAt = parseDate(text(stmt, 32))
            // Match T3 Code's raised-hand precedence without modifying persisted snooze fields.
            let completedWhileSnoozed: Bool
            if latestRunStatus == "completed", let snoozedAt, let completedAt {
                completedWhileSnoozed = completedAt > snoozedAt
            } else {
                completedWhileSnoozed = false
            }
            let freshFailure = text(stmt, 31) == "error"
                && (snoozedAt.map { snooze in sessionUpdatedAt.map { $0 > snooze } ?? false } ?? true)
            let raisedHand = pendingApproval > 0 || pendingUserInput > 0
                || freshFailure || completedWhileSnoozed
            let isSnoozed = snoozedUntil.map { $0 > now && !raisedHand } ?? false
            // Invalid wake dates never hide a thread or create a wake indicator.
            if isSnoozed { continue }
            let wokeAt: Date?
            if let snoozedUntil {
                if raisedHand {
                    wokeAt = completedWhileSnoozed ? completedAt : (sessionUpdatedAt ?? snoozedAt)
                } else {
                    wokeAt = snoozedUntil <= now ? snoozedUntil : nil
                }
            } else {
                wokeAt = nil
            }
            let isWoke = wokeAt.map { wake in lastVisitedAt.map { $0 < wake } ?? true } ?? false
            let item = ThreadItem(
                id: threadId,
                projectId: projectId,
                projectTitle: projectTitle,
                title: title,
                createdAt: createdAt,
                updatedAt: updatedAt,
                latestUserMessageAt: parseDate(latestUserMessageAtStr),
                activeRunStatus: activeRunStatus,
                activeRunStartedAt: parseDate(activeRunStartedAtStr),
                latestRunStatus: latestRunStatus,
                latestRunStartedAt: parseDate(latestRunStartedAtStr),
                latestRunCompletedAt: parseDate(latestRunCompletedAtStr),
                pendingUserInputCount: pendingUserInput,
                pendingApprovalCount: pendingApproval,
                branch: branch,
                worktreePath: worktreePath,
                modelName: modelName,
                providers: providers,
                hasGoal: hasGoal,
                isPinned: isPinned,
                isSettled: isSettled,
                isSnoozed: isSnoozed,
                isWoke: isWoke,
                pinOrderKey: pinOrderKey,
                activeOrderKey: activeOrderKey,
                prNumber: prNumber,
                projectIconJson: projectIconJson,
                projectFaviconPath: projectFaviconPath,
                workspaceRoot: workspaceRoot,
                unsettledAt: parseDate(unsettledAtStr),
                lastVisitedAt: lastVisitedAt,
                latestFailureClass: failure.failureClass,
                usageLimitResetAt: failure.failureClass == "usage_limit" ? failure.resetAt : nil,
                pendingGuestMessageCount: pendingGuestMessages,
                isBlockedOnThreadWait: blockedOnThreadWait,
                hasRuns: hasRuns
            )

            results.append(item)
        }

        return results
    }
}
