import Foundation
import SQLite3
import Testing
@testable import T3CodeBar

final class DatabaseFixture {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    let config: EnvironmentConfig
    private(set) var db: OpaquePointer?

    init(schema: Bool = true) throws {
        config = EnvironmentConfig(stateDirectory: directory)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let result = sqlite3_open(config.stateDbUrl.path, &db)
        #expect(result == SQLITE_OK)
        if schema {
            try execute("""
            CREATE TABLE orchestration_v2_projection_threads (
              thread_id TEXT PRIMARY KEY, title TEXT NOT NULL, project_id TEXT NOT NULL,
              created_at TEXT NOT NULL, updated_at TEXT NOT NULL, payload_json TEXT NOT NULL,
              interaction_mode TEXT, deleted_at TEXT, archived_at TEXT);
            CREATE TABLE projection_projects (
              project_id TEXT PRIMARY KEY, title TEXT, project_icon_json TEXT, favicon_path TEXT, workspace_root TEXT);
            CREATE TABLE orchestration_v2_projection_runs (
              thread_id TEXT, status TEXT, payload_json TEXT, ordinal INTEGER, run_id TEXT,
              requested_at TEXT, completed_at TEXT);
            CREATE TABLE orchestration_v2_projection_runtime_requests (thread_id TEXT, status TEXT, kind TEXT);
            CREATE TABLE projection_thread_pull_requests (thread_id TEXT, number INTEGER);
            CREATE TABLE orchestration_v2_projection_provider_threads (thread_id TEXT, payload_json TEXT, last_run_ordinal INTEGER);
            CREATE TABLE orchestration_v2_projection_messages (thread_id TEXT, role TEXT, created_at TEXT, updated_at TEXT, payload_json TEXT);
            CREATE TABLE orchestration_v2_projection_provider_turns (
              provider_turn_id TEXT PRIMARY KEY, thread_id TEXT, started_at TEXT, payload_json TEXT NOT NULL);
            """)
        }
    }

    deinit {
        sqlite3_close(db)
        try? FileManager.default.removeItem(at: directory)
    }

    func execute(_ sql: String, values: [String] = []) throws {
        if values.isEmpty {
            guard sqlite3_exec(db, sql, nil, nil, nil) == SQLITE_OK else {
                throw FixtureError.sql
            }
            return
        }
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK, let statement else {
            throw FixtureError.sql
        }
        defer { sqlite3_finalize(statement) }
        for (index, value) in values.enumerated() {
            let result = value.withCString {
                sqlite3_bind_text(statement, Int32(index + 1), $0, -1, unsafeBitCast(-1, to: sqlite3_destructor_type.self))
            }
            guard result == SQLITE_OK else { throw FixtureError.sql }
        }
        guard sqlite3_step(statement) == SQLITE_DONE else { throw FixtureError.sql }
    }

    func thread(_ id: String, payload: String = "{}", mode: String = "default") throws {
        try execute("INSERT INTO orchestration_v2_projection_threads VALUES (?, ?, 'p', '2026-11-04T00:00:00Z', '2026-11-04T00:00:00Z', ?, ?, NULL, NULL)",
                    values: [id, "Thread \(id)", payload, mode])
    }

    func run(_ thread: String, status: String, ordinal: Int = 1, payload: String = "{}", completedAt: String = "") throws {
        try execute("INSERT INTO orchestration_v2_projection_runs VALUES (?, ?, ?, ?, ?, '2026-11-04T00:00:00Z', NULLIF(?, ''))",
                    values: [thread, status, payload, String(ordinal), "run-\(thread)-\(ordinal)", completedAt])
    }

    func message(_ thread: String, role: String = "assistant", text: String, createdAt: String) throws {
        let payload = (try? JSONSerialization.data(withJSONObject: ["text": text])).flatMap { String(data: $0, encoding: .utf8) } ?? "{}"
        try execute("INSERT INTO orchestration_v2_projection_messages VALUES (?, ?, ?, ?, ?)",
                    values: [thread, role, createdAt, createdAt, payload])
    }

    /// A provider turn. `usage` is the `turnTokenUsage` object, or nil for a turn that reported none.
    func turn(driver: String?, startedAt: String, usage: [String: Any]? = nil) throws {
        var payload: [String: Any] = [:]
        if let driver { payload["nativeTurnRef"] = ["driver": driver] }
        if let usage { payload["turnTokenUsage"] = usage }
        let json = String(data: try JSONSerialization.data(withJSONObject: payload), encoding: .utf8) ?? "{}"
        turnCount += 1
        try execute("INSERT INTO orchestration_v2_projection_provider_turns VALUES (?, 't', ?, ?)",
                    values: ["turn-\(turnCount)", startedAt, json])
    }

    private var turnCount = 0

    enum FixtureError: Error { case sql }
}

func makeThread(id: String = "t", active: String? = nil, latest: String? = nil, approvals: Int = 0,
                inputs: Int = 0, goal: Bool = false, woke: Bool = false,
                lastVisitedAt: Date? = Date(timeIntervalSince1970: 0)) -> ThreadItem {
    ThreadItem(id: id, projectId: "p", projectTitle: "Project", title: "Thread \(id)",
               createdAt: Date(timeIntervalSince1970: 0), updatedAt: Date(timeIntervalSince1970: 0),
               latestUserMessageAt: nil, activeRunStatus: active, activeRunStartedAt: nil,
               latestRunStatus: latest, latestRunStartedAt: nil,
               latestRunCompletedAt: latest == "completed" ? Date(timeIntervalSince1970: 1) : nil,
               pendingUserInputCount: inputs, pendingApprovalCount: approvals, branch: nil, worktreePath: nil,
               modelName: nil, providers: [], hasGoal: goal, isPinned: false, isSettled: false,
               isSnoozed: false, isWoke: woke, pinOrderKey: nil, activeOrderKey: nil, prNumber: nil,
               projectIconJson: nil, projectFaviconPath: nil, workspaceRoot: nil, unsettledAt: nil,
               lastVisitedAt: lastVisitedAt)
}
