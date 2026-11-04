import Foundation
import Testing
@testable import T3CodeBar

struct WaitingRequestModelTests {
    @Test func waitingAndRequestAreTheirOwnStates() {
        var blocked = makeThread(active: "running")
        blocked.isBlockedOnThreadWait = true
        #expect(blocked.status == .waiting)
        #expect(blocked.status.label == "Waiting")

        var guest = makeThread(active: "running")
        guest.pendingGuestMessageCount = 2
        #expect(guest.status == .request)
        #expect(guest.status.label == "Request")
    }

    @Test func requestRanksBelowApprovalAndInputButAboveWork() {
        var thread = makeThread(active: "running", approvals: 1)
        thread.pendingGuestMessageCount = 1
        #expect(thread.status == .approval)
        thread = makeThread(active: "running", inputs: 1)
        thread.pendingGuestMessageCount = 1
        #expect(thread.status == .input)
    }

    @Test func onlyARunningRunCanBeBlockedOnAWait() {
        var thread = makeThread(active: "waiting")
        thread.isBlockedOnThreadWait = true
        // A run in the `waiting` state is still Working in T3 Code.
        #expect(thread.status.label == "Working")
        thread = makeThread(active: "running")
        #expect(thread.status.label == "Working")
    }

    @Test func aThreadThatNeverRanIsWaitingUnlessSomethingMoreUrgentApplies() {
        var thread = makeThread()
        thread.hasRuns = false
        #expect(thread.status == .waiting)
        thread = makeThread(woke: true)
        thread.hasRuns = false
        #expect(thread.status == .woke)
        #expect(makeThread().status == .settled)
    }

    @Test func waitingNeverCountsAsRunningWork() {
        #expect(!ThreadStatus.waiting.isRunning)
        #expect(!ThreadStatus.waiting.isAttention)
        #expect(!ThreadStatus.waiting.showsLabelInMenuBar)
        #expect(ThreadStatus.request.isAttention)
        #expect(ThreadStatus.request.showsLabelInMenuBar)
        #expect(AttentionTracker.needsAlert(.request))
        #expect(!AttentionTracker.needsAlert(.waiting))
    }

    @Test func notificationsUseT3CodesWording() {
        #expect(AttentionNotifier.title(for: .request) == "Guest request")
        #expect(AttentionNotifier.title(for: .limited) == "Usage limit reached")
        #expect(AttentionNotifier.title(for: .approval) == "Approval needed")
    }

    @Test func timeUntilRoundsUpAndIgnoresThePast() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        func until(_ seconds: TimeInterval) -> String? { ThreadRowView.timeUntil(now.addingTimeInterval(seconds), now: now) }
        #expect(until(-5) == nil)
        #expect(until(0) == nil)
        #expect(until(20) == "1m")
        #expect(until(45 * 60) == "45m")
        #expect(until(59 * 60 + 30) == "1h")
        #expect(until(2 * 3600) == "2h")
        #expect(until(2 * 3600 + 600) == "3h")
        #expect(until(30 * 3600) == "2d")
    }
}

struct WaitingRequestDatabaseTests {
    private func fixture(turnItems: Bool = true) throws -> DatabaseFixture {
        let fixture = try DatabaseFixture()
        if turnItems {
            try fixture.execute("""
            CREATE TABLE orchestration_v2_projection_turn_items (
              turn_item_id TEXT, thread_id TEXT, run_id TEXT, node_id TEXT, ordinal INTEGER,
              type TEXT, status TEXT, updated_at TEXT, payload_json TEXT);
            """)
        }
        return fixture
    }

    private func item(_ fixture: DatabaseFixture, _ id: String, thread: String, type: String, status: String = "running", payload: String = "{}") throws {
        try fixture.execute("INSERT INTO orchestration_v2_projection_turn_items VALUES (?, ?, ?, 'root', 1, ?, ?, '2026-11-04T00:00:01Z', ?)",
                            values: [id, thread, "run-\(thread)-1", type, status, payload])
    }

    private func statuses(_ fixture: DatabaseFixture) async throws -> [String: ThreadStatus] {
        let threads = try await DatabaseManager(config: fixture.config).fetchActiveThreads()
        return Dictionary(uniqueKeysWithValues: threads.map { ($0.id, $0.status) })
    }

    @Test func guestMessagesMakeARequestOnlyWhilePending() async throws {
        let fixture = try fixture()
        try fixture.thread("pending", payload: #"{"guestShare":{"messages":[{"status":"sent"},{"status":"pending"},{"status":"pending"}]}}"#)
        try fixture.thread("handled", payload: #"{"guestShare":{"messages":[{"status":"sent"},{"status":"dismissed"}]}}"#)
        try fixture.thread("none")
        for id in ["pending", "handled", "none"] {
            try fixture.run(id, status: "completed", payload: #"{"startedAt":"2026-11-04T00:00:00Z"}"#, completedAt: "2026-11-04T00:00:02Z")
        }
        let threads = try await DatabaseManager(config: fixture.config).fetchActiveThreads()
        let byId = Dictionary(uniqueKeysWithValues: threads.map { ($0.id, $0) })
        #expect(byId["pending"]?.status == .request)
        #expect(byId["pending"]?.pendingGuestMessageCount == 2)
        #expect(byId["handled"]?.status != .request)
        #expect(byId["none"]?.status != .request)
    }

    @Test func aThreadWithNoRunsIsWaitingAndOneWithRunsIsNot() async throws {
        let fixture = try fixture()
        try fixture.thread("empty")
        try fixture.thread("ran")
        try fixture.run("ran", status: "completed", payload: #"{"startedAt":"2026-11-04T00:00:00Z"}"#, completedAt: "2026-11-04T00:00:02Z")
        let byId = try await statuses(fixture)
        #expect(byId["empty"] == .waiting)
        #expect(byId["ran"] != .waiting)
    }

    @Test func runningRunThatOnlyWaitsOnThreadsIsWaiting() async throws {
        let fixture = try fixture()
        let running = #"{"rootNodeId":"root","startedAt":"2026-11-04T00:00:00Z"}"#
        for id in ["wait", "waitAndWork", "delegateWait", "delegateAsync", "working", "waitTool"] {
            try fixture.thread(id)
            try fixture.run(id, status: "running", payload: running)
        }
        try item(fixture, "1", thread: "wait", type: "dynamic_tool", payload: #"{"toolName":"t3-code.t3_thread_wait"}"#)
        // Any other running work means the run is working, not waiting.
        try item(fixture, "2", thread: "waitAndWork", type: "dynamic_tool", payload: #"{"toolName":"mcp__t3-code__t3_thread_wait"}"#)
        try item(fixture, "3", thread: "waitAndWork", type: "command_execution")
        try item(fixture, "4", thread: "delegateWait", type: "dynamic_tool", payload: #"{"toolName":"t3-code.delegate_task","input":{"mode":"wait"}}"#)
        try item(fixture, "5", thread: "delegateAsync", type: "dynamic_tool", payload: #"{"toolName":"t3-code.delegate_task","input":{"mode":"async"}}"#)
        try item(fixture, "6", thread: "working", type: "command_execution")
        // Waiting on a background task (t3_background_wait) is not a thread wait.
        try item(fixture, "7", thread: "waitTool", type: "dynamic_tool", payload: #"{"toolName":"mcp__t3-code__t3_background_wait"}"#)

        let byId = try await statuses(fixture)
        #expect(byId["wait"] == .waiting)
        #expect(byId["waitAndWork"]?.label == "Working")
        #expect(byId["delegateWait"] == .waiting)
        #expect(byId["delegateAsync"]?.label == "Working")
        #expect(byId["working"]?.label == "Working")
        #expect(byId["waitTool"]?.label == "Working")
    }

    @Test func finishedWaitItemsDoNotBlockTheRun() async throws {
        let fixture = try fixture()
        try fixture.thread("t")
        try fixture.run("t", status: "running", payload: #"{"rootNodeId":"root","startedAt":"2026-11-04T00:00:00Z"}"#)
        try item(fixture, "1", thread: "t", type: "dynamic_tool", status: "completed", payload: #"{"toolName":"t3-code.t3_thread_wait"}"#)
        #expect(try await statuses(fixture)["t"]?.label == "Working")
    }

    @Test func withoutTurnItemsARunningThreadStillShowsWorking() async throws {
        let fixture = try fixture(turnItems: false)
        try fixture.thread("t")
        try fixture.run("t", status: "running", payload: #"{"startedAt":"2026-11-04T00:00:00Z"}"#)
        #expect(try await statuses(fixture)["t"]?.label == "Working")
    }

    @Test func usageLimitResetTimeIsReadOnlyForUsageLimits() async throws {
        let fixture = try fixture()
        try fixture.thread("limited")
        try fixture.thread("providerError")
        try fixture.thread("noReset")
        for id in ["limited", "providerError", "noReset"] {
            try fixture.run(id, status: "failed", payload: #"{"rootNodeId":"root","startedAt":"2026-11-04T00:00:00Z"}"#, completedAt: "2026-11-04T00:00:02Z")
        }
        try item(fixture, "a", thread: "limited", type: "error", status: "failed", payload: #"{"failure":{"class":"usage_limit","resetAt":"2026-11-05T03:00:00.000Z"}}"#)
        try item(fixture, "b", thread: "providerError", type: "error", status: "failed", payload: #"{"failure":{"class":"provider_error","resetAt":"2026-11-05T03:00:00.000Z"}}"#)
        try item(fixture, "c", thread: "noReset", type: "error", status: "failed", payload: #"{"failure":{"class":"usage_limit"}}"#)

        let threads = try await DatabaseManager(config: fixture.config).fetchActiveThreads()
        let byId = Dictionary(uniqueKeysWithValues: threads.map { ($0.id, $0) })
        #expect(byId["limited"]?.status == .limited)
        #expect(byId["limited"]?.usageLimitResetAt == ISO8601DateFormatter().date(from: "2026-11-05T03:00:00Z"))
        #expect(byId["providerError"]?.status == .failed)
        #expect(byId["providerError"]?.usageLimitResetAt == nil)
        #expect(byId["noReset"]?.status == .limited)
        #expect(byId["noReset"]?.usageLimitResetAt == nil)
    }
}
