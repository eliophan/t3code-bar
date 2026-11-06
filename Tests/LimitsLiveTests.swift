import Foundation
import Testing
@testable import T3CodeBar

private final class MemoryLimitsStore: LimitsCredentialStore, @unchecked Sendable {
    private let lock = NSLock()
    private var value: LimitsCredential?
    private var loads = 0
    private var saves = 0
    var loadCount: Int { lock.withLock { loads } }
    var saveCount: Int { lock.withLock { saves } }
    func load() -> LimitsCredential? { lock.withLock { loads += 1; return value } }
    func save(_ credential: LimitsCredential) { lock.withLock { saves += 1; value = credential } }
    func delete() { lock.withLock { value = nil } }
}

private actor MockLimitsSocket: LimitsSocket {
    private var queue: [String] = []
    private var waiter: CheckedContinuation<String, any Error>?
    private(set) var sent: [String] = []
    private(set) var closed = false
    private var blockAck = false
    private var ackWaiter: CheckedContinuation<Void, Never>?
    private(set) var waitingForAck = false
    func suspendAck() { blockAck = true }
    func send(_ text: String) async throws {
        guard !closed else { throw URLError(.networkConnectionLost) }
        sent.append(text)
        if blockAck, text.contains("\"Ack\"") {
            waitingForAck = true
            await withCheckedContinuation { ackWaiter = $0 }
        }
    }
    func receive() async throws -> String {
        if !queue.isEmpty { return queue.removeFirst() }
        if closed { throw URLError(.networkConnectionLost) }
        return try await withCheckedThrowingContinuation { waiter = $0 }
    }
    func push(_ text: String) {
        if let waiter { self.waiter = nil; waiter.resume(returning: text) }
        else { queue.append(text) }
    }
    func close() {
        closed = true
        waiter?.resume(throwing: URLError(.networkConnectionLost))
        waiter = nil
        ackWaiter?.resume()
        ackWaiter = nil
    }
}

private actor MockLimitsServer: LimitsTransport {
    var responses: [(Data, Int)]
    let grantDelay: Duration
    let socketDelay: Duration
    private(set) var socketOpening = false
    private(set) var requests: [URLRequest] = []
    private(set) var sockets: [MockLimitsSocket] = []
    private(set) var socketURLs: [URL] = []
    init(scope: String = "orchestration:read", tokenType: String = "DPoP", status: Int = 200, expires: Double = 3600, grantDelay: Duration = .zero, socketDelay: Duration = .zero) throws {
        self.grantDelay = grantDelay
        self.socketDelay = socketDelay
        responses = [(try JSONSerialization.data(withJSONObject: [
            "access_token": "bar-test-token", "token_type": tokenType,
            "issued_token_type": "urn:ietf:params:oauth:token-type:access_token",
            "scope": scope, "expires_in": expires]), status)]
    }
    func request(_ request: URLRequest) async throws -> (Data, Int) {
        requests.append(request)
        if request.url?.path == "/oauth/token" { try await Task.sleep(for: grantDelay) }
        if !responses.isEmpty { return responses.removeFirst() }
        return (Data("{\"ticket\":\"ticket+&?\",\"expiresAt\":\"2099-01-01T00:00:00Z\"}".utf8), 200)
    }
    func socket(_ url: URL) async -> any LimitsSocket {
        socketOpening = true
        try? await Task.sleep(for: socketDelay) // Simulate a transport that completes despite cancellation.
        let socket = MockLimitsSocket()
        sockets.append(socket)
        socketURLs.append(url)
        return socket
    }
}

private func liveSource(_ accounts: [[String: Any]], error: String? = nil) -> [String: Any] {
    var source: [String: Any] = ["id": "pool", "kind": "cliproxy", "label": "CLI Proxy",
        "checkedAt": "2026-11-06T09:00:00Z", "accounts": accounts]
    if let error { source["error"] = error }
    return source
}
private func liveAccount(_ id: String, driver: String = "codex", email: String? = nil,
                         workspace: String? = nil, used: Double = 20,
                         at: String = "2026-11-06T10:00:00Z") -> [String: Any] {
    var account: [String: Any] = ["id": id, "driver": driver, "plan": "Pro", "usageLimits": [
        "checkedAt": at, "windows": [limitsWindowObject("primary", kind: "session", used: used)]]]
    if let email { account["email"] = email }
    if let workspace { account["workspaceId"] = workspace }
    return account
}
private func liveSnapshot(providers: [[String: Any]] = [], sources: [[String: Any]] = []) -> [String: Any] {
    ["version": 1, "type": "snapshot", "config": ["providers": providers, "usageLimitSources": sources]]
}
private func liveChunk(_ events: [[String: Any]]) throws -> String {
    try LimitsRPC.encode(["_tag": "Chunk", "requestId": 0, "values": events])
}

@MainActor
struct LimitsLiveTests {
    private func waitUntil(_ condition: () async -> Bool) async throws {
        for _ in 0..<300 {
            if await condition() { return }
            try await Task.sleep(for: .milliseconds(5))
        }
        Issue.record("Mock connection did not reach the expected state")
    }

    @Test func openingLimitsDoesNotReadCredentialsOrConnect() async throws {
        let server = try MockLimitsServer()
        let store = MemoryLimitsStore()
        let connection = LimitsLiveConnection(transport: server, store: store)
        connection.resume()
        #expect(store.loadCount == 0)
        #expect(await server.requests.isEmpty)
        #expect(connection.status == .cacheOnly)
    }

    @Test func bearerDowngradeDoesNotStoreOrUseTheToken() async throws {
        let server = try MockLimitsServer(tokenType: "Bearer")
        let store = MemoryLimitsStore()
        let connection = LimitsLiveConnection(transport: server, store: store)
        connection.pair(port: 43111, pairingCredential: "test", remember: true)
        try await waitUntil { if case .failed = connection.status { return true }; return false }
        #expect(store.saveCount == 0)
        #expect(await server.requests.count == 1)
        #expect(await server.sockets.isEmpty)
    }

    @Test func appLaunchReconnectsARememberedBarToken() async throws {
        let server = try MockLimitsServer()
        let store = MemoryLimitsStore()
        let first = LimitsLiveConnection(transport: server, store: store)
        first.pair(port: 43111, pairingCredential: "test", remember: true)
        try await waitUntil { await server.sockets.count == 1 }
        let second = LimitsLiveConnection(transport: server, store: store)
        second.reconnectSavedIfAvailable(port: 43111)
        try await waitUntil { await server.sockets.count == 2 }
        #expect(second.status == .connecting)
        first.disconnect()
        second.disconnect()
    }

    @Test func pairRequestsOnlyReadScopeAndUsesTicketedLoopbackRPC() async throws {
        let server = try MockLimitsServer()
        let store = MemoryLimitsStore()
        let connection = LimitsLiveConnection(transport: server, store: store)
        defer { connection.disconnect() }
        connection.pair(port: 43111, pairingCredential: "bar+pair&secret", remember: false)
        try await waitUntil { await server.sockets.count == 1 }
        let requests = await server.requests
        #expect(requests.count == 2)
        #expect(requests.allSatisfy { $0.url?.host == "127.0.0.1" && $0.url?.port == 43111 })
        #expect(requests[0].url?.path == "/oauth/token")
        #expect(requests[0].httpMethod == "POST")
        let form = String(decoding: requests[0].httpBody ?? Data(), as: UTF8.self)
        #expect(form.contains("subject_token=bar%2Bpair%26secret"))
        #expect(form.contains("scope=orchestration:read"))
        #expect(!form.contains("operate"))
        #expect(requests[1].url?.path == "/api/auth/websocket-ticket/rpc")
        #expect(requests[0].value(forHTTPHeaderField: "DPoP") != nil)
        #expect(requests[1].value(forHTTPHeaderField: "Authorization") == "DPoP bar-test-token")
        #expect(requests[1].value(forHTTPHeaderField: "DPoP") != nil)
        let url = try #require(await server.socketURLs.first)
        #expect(url.scheme == "ws" && url.path == "/ws")
        #expect(URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?.first?.value == "ticket+&?")
        #expect(URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?.contains(URLQueryItem(name: "orchestrationProtocol", value: "2")) == true)
        let socket = try #require(await server.sockets.first)
        try await waitUntil { await socket.sent.count == 1 }
        let sent = try #require(await socket.sent.first)
        let frame = try #require(LimitsRPC.frames(sent).first)
        #expect(frame["tag"] as? String == "subscribeServerConfig")
        #expect(frame["id"] as? Int == 0)
        #expect(frame["headers"] as? [String] == [])
        #expect((frame["payload"] as? [String: Bool]) == ["usageLimitSources": true, "workspaceSnapshotsOnDemand": true])
        #expect(store.saveCount == 0)
    }

    @Test(arguments: [0, -1, 65536, Int.max]) func invalidPortFailsBeforeAnyNetwork(port: Int) async throws {
        let server = try MockLimitsServer()
        let connection = LimitsLiveConnection(transport: server, store: MemoryLimitsStore())
        connection.pair(port: port, pairingCredential: "test", remember: false)
        try await waitUntil { if case .failed = connection.status { return true }; return false }
        #expect(await server.requests.isEmpty)
    }

    @Test(arguments: ["orchestration:read orchestration:operate", "orchestration:operate", ""]) func overbroadOrMissingScopeFailsClosed(scope: String) async throws {
        let server = try MockLimitsServer(scope: scope)
        let store = MemoryLimitsStore()
        let connection = LimitsLiveConnection(transport: server, store: store)
        connection.pair(port: 43111, pairingCredential: "test", remember: true)
        try await waitUntil { if case .failed = connection.status { return true }; return false }
        #expect(await server.sockets.isEmpty)
        #expect(store.saveCount == 0)
    }

    @Test(arguments: [401, 403, 302]) func rejectedAuthDoesNotRetryOrStore(status: Int) async throws {
        let server = try MockLimitsServer(status: status)
        let connection = LimitsLiveConnection(transport: server, store: MemoryLimitsStore(), retryDelay: .milliseconds(10))
        connection.pair(port: 43111, pairingCredential: "test", remember: false)
        try await waitUntil { if case .failed = connection.status { return true }; return false }
        #expect(await server.requests.count == 1)
        #expect(await server.sockets.isEmpty)
    }

    @Test func snapshotIncludesSixCodexTwoClaudeAndAntigravityAndAcknowledgesChunks() async throws {
        let server = try MockLimitsServer()
        let connection = LimitsLiveConnection(transport: server, store: MemoryLimitsStore())
        defer { connection.disconnect() }
        connection.pair(port: 43111, pairingCredential: "test", remember: false)
        try await waitUntil { await server.sockets.count == 1 }
        let socket = try #require(await server.sockets.first)
        let accounts = (0..<6).map { liveAccount("codex-\($0)", email: "user\($0)@example.test") }
            + (0..<2).map { liveAccount("claude-\($0)", driver: "claudeAgent", email: "c\($0)@example.test") }
        let gravity = limitsProviderObject("gravity", driver: "antigravity")
        await socket.push(try liveChunk([liveSnapshot(providers: [gravity], sources: [liveSource(accounts)])]))
        try await waitUntil { connection.status == .live }
        let snapshot = LimitsMerge.snapshot(try #require(connection.data).records)
        #expect(snapshot.accounts.filter { $0.driver == "codex" }.count == 6)
        #expect(Set(snapshot.accounts.filter { $0.driver == "codex" }.map(\.name)).count == 6)
        #expect(snapshot.accounts.filter { $0.driver == "claudeAgent" }.count == 2)
        #expect(snapshot.accounts.filter { $0.driver == "antigravity" }.count == 1)
        #expect(await socket.sent.count == 2)
        #expect(try LimitsRPC.frames(try #require(await socket.sent.last)).first?["_tag"] as? String == "Ack")
    }

    @Test func updatesReplaceSourcesPreserveProvidersAndIgnoreUnrelatedEvents() throws {
        var data = LimitsLiveData()
        _ = try data.apply(liveSnapshot(providers: [limitsProviderObject("gravity", driver: "antigravity")],
                                    sources: [liveSource([liveAccount("old")])]))
        let unrelated: [String: Any] = ["version": 1, "type": "settingsUpdated", "payload": ["settings": [:]]]
        #expect(try data.apply(unrelated) == false)
        _ = try data.apply(["version": 1, "type": "usageLimitSourcesUpdated", "payload": ["sources": [liveSource([liveAccount("new")])]]])
        #expect(data.records.map(\.instanceID) == ["gravity", "source:pool:new"])
        _ = try data.apply(["version": 1, "type": "snapshot", "config": ["providers": [limitsProviderObject("gravity", driver: "antigravity")]]])
        #expect(data.records.map(\.instanceID) == ["gravity", "source:pool:new"])
        _ = try data.apply(["version": 1, "type": "providerStatuses", "payload": ["providers": []]])
        #expect(data.records.count == 1)
        _ = try data.apply(["version": 1, "type": "usageLimitSourcesUpdated", "payload": ["sources": []]])
        #expect(data.records.isEmpty)
    }

    @Test func providersAndSourcesDeduplicateByEmailWorkspaceAndNewestCheck() throws {
        let provider = limitsProviderObject("codex", email: "USER@example.test", workspace: "one", checkedAt: "2026-11-06T09:00:00Z")
        var data = LimitsLiveData()
        _ = try data.apply(liveSnapshot(providers: [provider], sources: [liveSource([
            liveAccount("same", email: "user@example.test", used: 80),
            liveAccount("other", email: "other@example.test", used: 60)])]))
        let snapshot = LimitsMerge.snapshot(data.records)
        #expect(snapshot.accounts.count == 2)
        #expect(snapshot.accounts.first?.windows.first?.usedPercent == 80)
        #expect(snapshot.accounts.first?.checkedAt == limitDate("2026-11-06T10:00:00Z"))
        #expect(snapshot.accounts.first?.id == "codex")
    }

    @Test func ambiguousWorkspaceAndSourceErrorsStayVisible() throws {
        var data = LimitsLiveData()
        _ = try data.apply(liveSnapshot(sources: [liveSource([
            liveAccount("one", email: "same@example.test", workspace: "one"),
            liveAccount("two", email: "same@example.test", workspace: "two"),
            liveAccount("unknown", email: "same@example.test")]), liveSource([], error: "Could not read source")]))
        let snapshot = LimitsMerge.snapshot(data.records, sourceNotices: data.notices)
        #expect(snapshot.accounts.count == 3)
        #expect(snapshot.notices == ["CLI Proxy: Could not read source"])
    }

    @Test func disconnectRetainsLastDataAndReconnectStartsWithFreshSnapshot() async throws {
        let server = try MockLimitsServer()
        let connection = LimitsLiveConnection(transport: server, store: MemoryLimitsStore(), retryDelay: .milliseconds(10))
        defer { connection.disconnect() }
        connection.pair(port: 43111, pairingCredential: "test", remember: false)
        try await waitUntil { await server.sockets.count == 1 }
        let first = try #require(await server.sockets.first)
        await first.push(try liveChunk([liveSnapshot(sources: [liveSource([liveAccount("old")])])]))
        try await waitUntil { connection.status == .live }
        let saved = connection.data
        await first.close()
        try await waitUntil { await server.sockets.count == 2 }
        #expect(connection.status == .retrying)
        #expect(connection.data == saved)
        let second = try #require(await server.sockets.last)
        await second.push(try liveChunk([liveSnapshot(sources: [liveSource([liveAccount("new")])])]))
        try await waitUntil { connection.status == .live }
        #expect(connection.data?.records.map(\.instanceID) == ["source:pool:new"])
        #expect(await server.requests.count == 3) // No repeated bootstrap exchange.
    }

    @Test func malformedStreamFailsClosedKeepingLastRead() async throws {
        let server = try MockLimitsServer()
        let connection = LimitsLiveConnection(transport: server, store: MemoryLimitsStore())
        defer { connection.disconnect() }
        connection.pair(port: 43111, pairingCredential: "test", remember: false)
        try await waitUntil { await server.sockets.count == 1 }
        let socket = try #require(await server.sockets.first)
        await socket.push(try liveChunk([liveSnapshot(sources: [liveSource([liveAccount("old")])])]))
        try await waitUntil { connection.status == .live }
        let saved = connection.data
        await socket.push("{broken")
        try await waitUntil { if case .failed = connection.status { return true }; return false }
        #expect(connection.data == saved)
        #expect(await socket.closed)
    }

    @Test func stopInterruptsClosesAndIgnoresLateUpdates() async throws {
        let server = try MockLimitsServer()
        let connection = LimitsLiveConnection(transport: server, store: MemoryLimitsStore())
        defer { connection.disconnect() }
        connection.pair(port: 43111, pairingCredential: "test", remember: false)
        try await waitUntil { await server.sockets.count == 1 }
        let socket = try #require(await server.sockets.first)
        connection.stop()
        try await waitUntil { await socket.closed }
        await socket.push(try liveChunk([liveSnapshot(sources: [liveSource([liveAccount("late")])])]))
        #expect(connection.status == .paused)
        #expect(connection.data == nil)
        #expect(try LimitsRPC.frames(try #require(await socket.sent.last)).first?["_tag"] as? String == "Interrupt")
    }

    @Test func rememberRequiresExplicitActionAndExpiredSavedTokenDoesNotConnect() async throws {
        let server = try MockLimitsServer()
        let store = MemoryLimitsStore()
        let connection = LimitsLiveConnection(transport: server, store: store)
        connection.pair(port: 43111, pairingCredential: "test", remember: true)
        try await waitUntil { await server.sockets.count == 1 }
        #expect(store.saveCount == 1)
        connection.stop()
        store.save(LimitsCredential(port: 43111, token: "expired", expiresAt: .distantPast))
        connection.reconnectSaved(port: 43111)
        #expect(connection.status == .failed(LimitsConnectionError.expired.message))
        #expect(await server.sockets.count == 1)
        connection.disconnect()
        #expect(store.load() == nil)
    }

    @Test func closingDuringPairingCancelsAuthAndNeverStoresTheToken() async throws {
        let server = try MockLimitsServer(grantDelay: .seconds(1))
        let store = MemoryLimitsStore()
        let connection = LimitsLiveConnection(transport: server, store: store)
        connection.pair(port: 43111, pairingCredential: "test", remember: true)
        try await waitUntil { await server.requests.count == 1 }
        connection.stop()
        try await Task.sleep(for: .milliseconds(20))
        #expect(connection.status == .paused)
        #expect(store.saveCount == 0)
        #expect(await server.sockets.isEmpty)
    }

    @Test(arguments: ["{\"_tag\":\"Exit\",\"requestId\":0,\"exit\":{\"_tag\":\"Failure\"}}",
                       "{\"_tag\":\"Chunk\",\"requestId\":99,\"values\":[]}"])
    func invalidRPCOrSubscriptionExitClosesConnection(frame: String) async throws {
        let server = try MockLimitsServer()
        let connection = LimitsLiveConnection(transport: server, store: MemoryLimitsStore())
        defer { connection.disconnect() }
        connection.pair(port: 43111, pairingCredential: "test", remember: false)
        try await waitUntil { await server.sockets.count == 1 }
        let socket = try #require(await server.sockets.first)
        await socket.push(frame)
        try await waitUntil { if case .failed = connection.status { return true }; return false }
        #expect(await socket.closed)
        #expect(connection.data == nil)
    }

    @Test func monitorUsesLiveAuthorityAndCacheFallbackWithoutDuplicateAccounts() async throws {
        let fixture = CachesFixture()
        try fixture.write("codex.json", object: limitsProviderObject("codex", email: "same@example.test"))
        let server = try MockLimitsServer()
        let connection = LimitsLiveConnection(transport: server, store: MemoryLimitsStore(), retryDelay: .seconds(60))
        let monitor = LimitsMonitor(reader: LimitsReader(cachesDirectory: fixture.directory), live: connection)
        defer { connection.disconnect() }
        await monitor.readOnce()
        #expect(monitor.snapshot?.accounts.count == 1)
        connection.pair(port: 43111, pairingCredential: "test", remember: false)
        try await waitUntil { await server.sockets.count == 1 }
        let socket = try #require(await server.sockets.first)
        await socket.push(try liveChunk([liveSnapshot(providers: [], sources: [liveSource([
            liveAccount("same", email: "same@example.test", used: 80), liveAccount("extra", used: 40)])])]))
        try await waitUntil { connection.status == .live }
        #expect(monitor.snapshot?.accounts.count == 2)
        #expect(monitor.snapshot?.accounts.first?.windows.first?.usedPercent == 80)
        try fixture.write("codex.json", object: limitsProviderObject("codex", email: "same@example.test",
            checkedAt: "2026-11-06T11:00:00Z", windows: [limitsWindowObject("primary", kind: "session", used: 90)]))
        await monitor.readOnce()
        #expect(monitor.snapshot?.accounts.first?.windows.first?.usedPercent == 80) // live authority
        await socket.close()
        try await waitUntil { connection.status == .retrying }
        #expect(monitor.snapshot?.accounts.count == 2)
        #expect(monitor.snapshot?.accounts.first?.windows.first?.usedPercent == 90) // newer cache fallback
        connection.disconnect()
        #expect(monitor.snapshot?.accounts.count == 1)
        #expect(monitor.snapshot?.accounts.first?.windows.first?.usedPercent == 90)
    }

    @Test func savedTokenPortMustMatchTheVisiblePort() async throws {
        let server = try MockLimitsServer()
        let store = MemoryLimitsStore()
        store.save(LimitsCredential(port: 43112, token: "saved", expiresAt: .distantFuture))
        let connection = LimitsLiveConnection(transport: server, store: store)
        connection.reconnectSaved(port: 43111)
        #expect(connection.status == .failed(LimitsConnectionError.destination.message))
        #expect(await server.requests.isEmpty)
    }

    @Test func redirectsAreRejectedBeforeCredentialsCanLeaveLoopback() throws {
        let delegate = LimitsRedirectDelegate()
        let session = URLSession(configuration: .ephemeral)
        defer { session.invalidateAndCancel() }
        let local = try #require(URL(string: "http://127.0.0.1:43111/oauth/token"))
        let remote = try #require(URL(string: "https://example.test/receive"))
        let task = session.dataTask(with: local) // never resumed; no network.
        let response = try #require(HTTPURLResponse(url: local, statusCode: 302, httpVersion: nil, headerFields: nil))
        var called = false
        delegate.urlSession(session, task: task, willPerformHTTPRedirection: response,
                            newRequest: URLRequest(url: remote)) { redirected in
            called = true
            #expect(redirected == nil)
        }
        #expect(called)
    }

    @Test func closingDuringSocketCreationClosesTheLateSocketWithoutSubscribing() async throws {
        let server = try MockLimitsServer(socketDelay: .seconds(1))
        let connection = LimitsLiveConnection(transport: server, store: MemoryLimitsStore())
        connection.pair(port: 43111, pairingCredential: "test", remember: false)
        try await waitUntil { await server.socketOpening }
        connection.stop()
        try await waitUntil { await server.sockets.count == 1 }
        let socket = try #require(await server.sockets.first)
        try await waitUntil { await socket.closed }
        #expect(await socket.sent.isEmpty)
        #expect(connection.status == .paused)
        connection.disconnect()
    }

    @Test func aLateAckCannotPublishLiveDataAfterStop() async throws {
        let server = try MockLimitsServer()
        let connection = LimitsLiveConnection(transport: server, store: MemoryLimitsStore())
        defer { connection.disconnect() }
        connection.pair(port: 43111, pairingCredential: "test", remember: false)
        try await waitUntil { await server.sockets.count == 1 }
        let socket = try #require(await server.sockets.first)
        await socket.suspendAck()
        await socket.push(try liveChunk([liveSnapshot(sources: [liveSource([liveAccount("late")])])]))
        try await waitUntil { await socket.waitingForAck }
        connection.stop()
        try await waitUntil { await socket.closed }
        try await Task.sleep(for: .milliseconds(10))
        #expect(connection.data == nil)
        #expect(connection.status == .paused)
    }

    @Test func codecHandlesArraysAndRejectsInvalidShapes() throws {
        #expect(try LimitsRPC.frames("[{\"_tag\":\"Pong\"}]").count == 1)
        #expect(!LimitsRPC.matches(false, integer: 0))
        #expect(!LimitsRPC.matches(true, integer: 1))
        #expect(throws: LimitsConnectionError.protocolMismatch) { try LimitsRPC.frames("null") }
        var data = LimitsLiveData()
        #expect(throws: LimitsConnectionError.protocolMismatch) { try data.apply(["version": 2, "type": "snapshot"]) }
    }
}
