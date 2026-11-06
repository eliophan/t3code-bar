import Foundation
import Combine

enum LimitsLiveStatus: Equatable {
    case cacheOnly, connecting, live, paused, retrying, failed(String)
    var text: String {
        switch self {
        case .cacheOnly: "Connect to view usage."
        case .connecting: "Connecting…"
        case .live: "Connected"
        case .paused: "Connection paused."
        case .retrying: "Disconnected. Reconnecting…"
        case .failed(let message): message
        }
    }
}

/// A user-started, read-only subscription. T3 may run its own discovery when clients subscribe.
@MainActor
final class LimitsLiveConnection: ObservableObject {
    @Published private(set) var status: LimitsLiveStatus = .cacheOnly
    @Published private(set) var data: LimitsLiveData?
    @Published private(set) var lastUpdatedAt: Date?
    var onChange: (() -> Void)?
    private let transport: any LimitsTransport
    private let store: any LimitsCredentialStore
    private let clock: @Sendable () -> Date
    private let retryDelay: Duration
    private var credential: LimitsCredential?
    private var task: Task<Void, Never>?
    private var activeSocket: (any LimitsSocket)?
    private var generation = 0
    private var lastFrame = Date.distantPast
    private var hasSnapshot = false

    var isConnected: Bool { status == .live }

    init(transport: any LimitsTransport = LimitsURLTransport(), store: any LimitsCredentialStore = LimitsKeychainStore(),
         clock: @escaping @Sendable () -> Date = { Date() }, retryDelay: Duration = .seconds(3)) {
        self.transport = transport
        self.store = store
        self.clock = clock
        self.retryDelay = retryDelay
    }

    func pair(port: Int, pairingCredential: String, remember: Bool) {
        stop()
        credential = nil
        data = nil
        lastUpdatedAt = nil
        generation += 1
        let run = generation
        status = .connecting
        onChange?()
        task = Task { [weak self] in
            guard let self else { return }
            do {
                let endpoint = try LimitsEndpoint(port: port)
                let input = pairingCredential.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !input.isEmpty, input.utf8.count < 16_384 else { throw LimitsConnectionError.authorization }
                let grant = try await exchange(input, endpoint: endpoint)
                try Task.checkCancellation()
                guard generation == run else { return }
                if remember { try store.save(grant) } else { try store.delete() }
                credential = grant
                task = Task { [weak self] in await self?.runConnection(grant, generation: run) }
            } catch {
                guard generation == run, !Task.isCancelled else { return }
                task = nil
                status = .failed((error as? LimitsConnectionError)?.message ?? "Could not connect to local T3 Code. Try connecting again.")
                onChange?()
            }
        }
    }

    /// Uses a long-lived token from T3's CLI directly. No token exchange happens, so there is no DPoP key.
    func connect(port: Int, bearerToken: String, remember: Bool) {
        stop()
        credential = nil
        data = nil
        lastUpdatedAt = nil
        do {
            _ = try LimitsEndpoint(port: port)
            let token = bearerToken.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !token.isEmpty, token.utf8.count <= 16_384 else { throw LimitsConnectionError.invalidToken }
            let grant = LimitsCredential.bearer(port: port, token: token)
            if remember { try store.save(grant) } else { try store.delete() }
            credential = grant
            resume()
        } catch {
            status = .failed((error as? LimitsConnectionError)?.message ?? LimitsConnectionError.storage.message)
        }
        onChange?()
    }

    /// Explicit button action: no Keychain lookup or network request occurs just by opening Limits.
    func reconnectSaved(port: Int) {
        stop()
        credential = nil
        do {
            _ = try LimitsEndpoint(port: port)
            guard let saved = try store.load() else {
                status = .failed("No saved Bar token. Enter a new read-only pairing credential.")
                return
            }
            guard saved.port == port else { throw LimitsConnectionError.destination }
            _ = try LimitsEndpoint(port: saved.port)
            guard saved.expiresAt > clock() else { throw LimitsConnectionError.expired }
            credential = saved
            resume()
        } catch {
            status = .failed((error as? LimitsConnectionError)?.message ?? LimitsConnectionError.storage.message)
        }
    }

    /// Called by the app at launch. Missing credentials keep the onboarding state instead of
    /// showing an error; an expired or revoked credential must be explained to the user.
    func reconnectSavedIfAvailable(port: Int) {
        guard status != .live, status != .connecting else { return }
        do {
            _ = try LimitsEndpoint(port: port)
            guard let saved = try store.load() else { return }
            guard saved.port == port else { throw LimitsConnectionError.destination }
            guard saved.expiresAt > clock() else { throw LimitsConnectionError.expired }
            credential = saved
            resume()
        } catch {
            status = .failed((error as? LimitsConnectionError)?.message ?? LimitsConnectionError.storage.message)
            onChange?()
        }
    }

    func resume() {
        guard task == nil, let credential else { return }
        generation += 1
        let run = generation
        status = .connecting
        task = Task { [weak self] in await self?.runConnection(credential, generation: run) }
    }

    func stop() {
        generation += 1
        task?.cancel()
        task = nil
        let socket = activeSocket
        activeSocket = nil
        if let socket {
            Task {
                // Bound the best-effort Interrupt so a broken send cannot keep the stream open.
                let deadline = Task {
                    do { try await Task.sleep(for: .seconds(1)) } catch { return }
                    await socket.close()
                }
                defer { deadline.cancel() }
                try? await socket.send(LimitsRPC.encode(LimitsRPC.interrupt))
                await socket.close()
            }
        }
        if credential != nil || status == .connecting { status = .paused }
    }

    func disconnect() {
        stop()
        credential = nil
        data = nil
        lastUpdatedAt = nil
        do { try store.delete(); status = .cacheOnly }
        catch { status = .failed(LimitsConnectionError.storage.message) }
        onChange?()
    }

    private func exchange(_ input: String, endpoint: LimitsEndpoint) async throws -> LimitsCredential {
        let signer = LimitsDPoP()
        var request = URLRequest(url: endpoint.url("/oauth/token"))
        request.httpMethod = "POST"
        request.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        request.setValue(try signer.proof(method: "POST", url: request.url!, now: clock()), forHTTPHeaderField: "DPoP")
        // URLComponents percent-encodes separators; '+' must be escaped for form encoding as well.
        var form = URLComponents()
        form.queryItems = [
            URLQueryItem(name: "grant_type", value: "urn:ietf:params:oauth:grant-type:token-exchange"),
            URLQueryItem(name: "subject_token", value: input),
            URLQueryItem(name: "subject_token_type", value: "urn:t3:params:oauth:token-type:environment-bootstrap"),
            URLQueryItem(name: "requested_token_type", value: "urn:ietf:params:oauth:token-type:access_token"),
            URLQueryItem(name: "scope", value: "orchestration:read"),
            URLQueryItem(name: "client_label", value: "T3 Code Bar Limits"),
            URLQueryItem(name: "client_device_type", value: "desktop"), URLQueryItem(name: "client_os", value: "macOS")]
        request.httpBody = Data((form.percentEncodedQuery ?? "").replacingOccurrences(of: "+", with: "%2B").utf8)
        let object = try await http(request)
        guard object["token_type"] as? String == "DPoP",
              object["issued_token_type"] as? String == "urn:ietf:params:oauth:token-type:access_token",
              let scopes = object["scope"] as? String,
              scopes.split(whereSeparator: { $0.isWhitespace }).map(String.init) == ["orchestration:read"],
              let token = object["access_token"] as? String, !token.isEmpty,
              let seconds = object["expires_in"] as? Double, seconds.isFinite, seconds > 0 else {
            throw LimitsConnectionError.authorization
        }
        return LimitsCredential(port: endpoint.port, token: token, expiresAt: clock().addingTimeInterval(seconds),
                                privateKey: signer.key.rawRepresentation)
    }

    private func http(_ request: URLRequest, unauthorized: LimitsConnectionError? = nil) async throws -> [String: Any] {
        let (body, status) = try await transport.request(request)
        guard status == 200 else {
            if let unauthorized, status == 401 || status == 403 { throw unauthorized }
            if (400...499).contains(status) || (300...399).contains(status) { throw LimitsConnectionError.authorization }
            throw URLError(.badServerResponse)
        }
        guard body.count < 1_048_576,
              let object = try? JSONSerialization.jsonObject(with: body) as? [String: Any] else {
            throw LimitsConnectionError.protocolMismatch
        }
        return object
    }

    private func runConnection(_ credential: LimitsCredential, generation run: Int) async {
        defer { if generation == run { task = nil } }
        while !Task.isCancelled, generation == run {
            var socket: (any LimitsSocket)?
            var heartbeat: Task<Void, Never>?
            do {
                guard credential.expiresAt > clock() else { throw LimitsConnectionError.expired }
                let endpoint = try LimitsEndpoint(port: credential.port)
                var request = URLRequest(url: endpoint.url("/api/auth/websocket-ticket/rpc"))
                request.httpMethod = "POST"
                let signer: LimitsDPoP?
                switch credential.kind {
                case .dpop(let privateKey):
                    guard let key = try? LimitsDPoP(raw: privateKey) else { throw LimitsConnectionError.authorization }
                    signer = key
                    request.setValue("DPoP \(credential.token)", forHTTPHeaderField: "Authorization")
                    request.setValue(try key.proof(method: "POST", url: request.url!, token: credential.token, now: clock()), forHTTPHeaderField: "DPoP")
                case .bearer:
                    signer = nil
                    request.setValue("Bearer \(credential.token)", forHTTPHeaderField: "Authorization")
                }
                let issued = try await http(request, unauthorized: signer == nil ? .expired : nil)
                guard let ticket = issued["ticket"] as? String, !ticket.isEmpty,
                      let expires = issued["expiresAt"] as? String, let expiry = LimitsDate.parse(expires), expiry > clock() else {
                    throw LimitsConnectionError.protocolMismatch
                }
                try Task.checkCancellation()
                guard generation == run else { return }
                let proof = try signer?.proof(method: "GET", url: endpoint.url("/ws"), token: ticket, now: clock())
                let connected = await transport.socket(endpoint.url("/ws", ticket: ticket, proof: proof))
                guard !Task.isCancelled, generation == run else { await connected.close(); return }
                socket = connected
                activeSocket = connected
                hasSnapshot = false
                lastFrame = clock()
                let connectedAt = clock()
                try await connected.send(LimitsRPC.encode(LimitsRPC.subscribe))
                heartbeat = Task { [weak self] in
                    while !Task.isCancelled {
                        do { try await Task.sleep(for: .seconds(5)) } catch { return }
                        guard let self, generation == run else { return }
                        if clock() >= credential.expiresAt || clock().timeIntervalSince(lastFrame) > 15
                            || (!hasSnapshot && clock().timeIntervalSince(connectedAt) > 15) {
                            await connected.close()
                            return
                        }
                        do { try await connected.send(LimitsRPC.encode(["_tag": "Ping"])) }
                        catch { await connected.close(); return }
                    }
                }
                var next = LimitsLiveData()
                while !Task.isCancelled, generation == run {
                    let text = try await connected.receive()
                    try Task.checkCancellation()
                    guard generation == run else { break }
                    lastFrame = clock()
                    for frame in try LimitsRPC.frames(text) {
                        guard let tag = frame["_tag"] as? String else { throw LimitsConnectionError.protocolMismatch }
                        if tag == "Pong" { continue }
                        guard tag == "Chunk", LimitsRPC.matches(frame["requestId"], integer: LimitsRPC.requestID),
                              let values = frame["values"] as? [[String: Any]] else { throw LimitsConnectionError.protocolMismatch }
                        var changed = false
                        for value in values {
                            if value["type"] as? String == "snapshot" { hasSnapshot = true }
                            guard hasSnapshot else { throw LimitsConnectionError.protocolMismatch }
                            changed = try next.apply(value) || changed
                        }
                        try await connected.send(LimitsRPC.encode(LimitsRPC.ack))
                        try Task.checkCancellation()
                        guard generation == run else { return }
                        if changed {
                            data = next
                            lastUpdatedAt = clock()
                            status = .live
                            onChange?()
                        }
                    }
                }
            } catch {
                heartbeat?.cancel()
                if let socket { await socket.close() }
                guard !Task.isCancelled, generation == run else { return }
                activeSocket = nil
                if let error = error as? LimitsConnectionError {
                    status = .failed(error.message)
                    onChange?()
                    return
                }
                status = .retrying
                onChange?()
                do { try await Task.sleep(for: retryDelay) } catch { return }
            }
            heartbeat?.cancel()
            if let socket { await socket.close() }
        }
    }
}
