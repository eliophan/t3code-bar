import Foundation
import Testing
@testable import T3CodeBar

struct ProviderKindTests {
    /// Every provider id that appears in a real T3 Code database, plus a custom instance.
    @Test(arguments: [
        ("codex", ProviderKind.openAI),
        ("codex_6a18363d-d561-4725-8387-64fce804a18d", .openAI),
        ("claudeAgent", .claude),
        ("antigravity", .antigravity),
        ("opencode", .openCode),
        ("muse", .muse),
        ("cursor", .cursor),
        ("grok", .grok),
        ("kiro", .kiro),
        ("fx", .fx),
        ("pi", .pi),
    ])
    func driversGetTheirOwnLogo(_ id: String, _ expected: ProviderKind) {
        #expect(ProviderKind.resolve(id) == expected)
    }

    @Test(arguments: [
        ("claude-sonnet-4-5", ProviderKind.claude),
        ("gemini-2.5-pro", .gemini),
        ("gpt-5.5", .openAI),
        ("o3-mini", .openAI),
    ])
    func modelNamesStillResolveWhenThereIsNoRunHistory(_ name: String, _ expected: ProviderKind) {
        #expect(ProviderKind.resolve(name) == expected)
    }

    @Test func unknownProvidersAreNotShownAsOpenAI() {
        #expect(ProviderKind.resolve("brand-new-agent") == .unknown)
        // A word that merely contains a short driver name is not that driver.
        #expect(ProviderKind.resolve("pipeline") == .unknown)
        #expect(ProviderKind.resolve("prefix") == .unknown)
    }

    @Test func kindsAreDistinctInFirstSeenOrder() {
        #expect(ProviderKind.resolveUnique(["codex", "opencode", "codex_abc", "claudeAgent"]) == [.openAI, .openCode, .claude])
        #expect(ProviderKind.resolveUnique([]) == [.openAI])
    }

    @Test func everyKindWithALogoFileShipsIt() throws {
        let names = ["grok", "cursor", "opencode", "fx", "kiro", "pi", "muse"]
        for name in names {
            #expect(AppResources.url(forResource: "ProviderLogo-\(name)", withExtension: "svg") != nil, "missing \(name)")
        }
        #expect(AppResources.url(forResource: "ProviderLogo-antigravity", withExtension: "png") != nil)
    }
}

struct LimitedStatusTests {
    @Test func usageLimitIsLimitedNotFailed() {
        var thread = makeThread(latest: "failed")
        #expect(thread.status == .failed)
        thread.latestFailureClass = "provider_error"
        #expect(thread.status == .failed)
        thread.latestFailureClass = "usage_limit"
        #expect(thread.status == .limited)
        #expect(thread.status.label == "Limited")
        #expect(thread.status != .failed)
    }

    @Test func aLimitOnlyMattersWhileTheLatestRunFailed() {
        var thread = makeThread(latest: "completed")
        thread.latestFailureClass = "usage_limit"
        #expect(thread.status != .limited)
    }

    @Test func limitedNeedsAttentionLikeFailed() {
        #expect(ThreadStatus.limited.isAttention)
        #expect(ThreadStatus.limited.showsLabelInMenuBar)
        #expect(AttentionTracker.needsAlert(.limited))
    }

    private func fixtureWithTurnItems() throws -> DatabaseFixture {
        let fixture = try DatabaseFixture()
        try fixture.execute("""
        CREATE TABLE orchestration_v2_projection_turn_items (
          turn_item_id TEXT, thread_id TEXT, run_id TEXT, node_id TEXT, ordinal INTEGER,
          type TEXT, status TEXT, updated_at TEXT, payload_json TEXT);
        """)
        return fixture
    }

    private func errorItem(_ fixture: DatabaseFixture, id: String, thread: String, node: String,
                           failureClass: String, status: String = "failed", updatedAt: String = "2026-11-04T00:00:01Z") throws {
        try fixture.execute("INSERT INTO orchestration_v2_projection_turn_items VALUES (?, ?, ?, ?, 1, 'error', ?, ?, ?)",
                            values: [id, thread, "run-\(thread)-1", node, status, updatedAt, #"{"failure":{"class":"\#(failureClass)"}}"#])
    }

    @Test func databaseReportsLimitedFromTheRootErrorItem() async throws {
        let fixture = try fixtureWithTurnItems()
        try fixture.thread("limited")
        try fixture.thread("other")
        try fixture.thread("child")
        try fixture.thread("newer")
        for id in ["limited", "other", "child", "newer"] {
            try fixture.run(id, status: "failed", payload: #"{"rootNodeId":"root","startedAt":"2026-11-04T00:00:00Z"}"#, completedAt: "2026-11-04T00:00:02Z")
        }
        try errorItem(fixture, id: "a", thread: "limited", node: "root", failureClass: "usage_limit")
        try errorItem(fixture, id: "b", thread: "other", node: "root", failureClass: "provider_error")
        // A limit hit by a child node does not make the thread's own run a limited one.
        try errorItem(fixture, id: "c", thread: "child", node: "sub", failureClass: "usage_limit")
        // The newest root error wins, as in T3 Code.
        try errorItem(fixture, id: "d1", thread: "newer", node: "root", failureClass: "usage_limit", updatedAt: "2026-11-04T00:00:01Z")
        try errorItem(fixture, id: "d2", thread: "newer", node: "root", failureClass: "provider_error", updatedAt: "2026-11-04T00:00:09Z")

        let threads = try await DatabaseManager(config: fixture.config).fetchActiveThreads()
        let byId = Dictionary(uniqueKeysWithValues: threads.map { ($0.id, $0.status) })
        #expect(byId["limited"] == .limited)
        #expect(byId["other"] == .failed)
        #expect(byId["child"] == .failed)
        #expect(byId["newer"] == .failed)
    }

    @Test func missingTurnItemsTableStillListsFailedThreads() async throws {
        let fixture = try DatabaseFixture()
        try fixture.thread("t")
        try fixture.run("t", status: "failed", completedAt: "2026-11-04T00:00:02Z")
        let threads = try await DatabaseManager(config: fixture.config).fetchActiveThreads()
        #expect(threads.first?.status == .failed)
    }
}
