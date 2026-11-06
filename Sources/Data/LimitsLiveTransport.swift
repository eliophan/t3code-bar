import Foundation
import Security

protocol LimitsSocket: Sendable {
    func send(_ text: String) async throws
    func receive() async throws -> String
    func close() async
}
protocol LimitsTransport: Sendable {
    func request(_ request: URLRequest) async throws -> (Data, Int)
    func socket(_ url: URL) async -> any LimitsSocket
}

/// Ephemeral, no cookies/cache/proxies; redirects are rejected before a credential can leave loopback.
final class LimitsURLTransport: LimitsTransport, @unchecked Sendable {
    private let session: URLSession
    init() {
        let config = URLSessionConfiguration.ephemeral
        config.httpCookieStorage = nil
        config.httpShouldSetCookies = false
        config.urlCache = nil
        config.urlCredentialStorage = nil
        config.connectionProxyDictionary = [:]
        config.timeoutIntervalForRequest = 15
        config.timeoutIntervalForResource = 30
        session = URLSession(configuration: config, delegate: LimitsRedirectDelegate(), delegateQueue: nil)
    }
    deinit { session.invalidateAndCancel() }
    func request(_ request: URLRequest) async throws -> (Data, Int) {
        let (data, response) = try await session.data(for: request)
        guard let response = response as? HTTPURLResponse else { throw LimitsConnectionError.protocolMismatch }
        return (data, response.statusCode)
    }
    func socket(_ url: URL) async -> any LimitsSocket {
        let task = session.webSocketTask(with: url)
        task.maximumMessageSize = 8 * 1024 * 1024
        task.resume()
        return LimitsURLSocket(task: task)
    }
}

final class LimitsRedirectDelegate: NSObject, URLSessionTaskDelegate {
    func urlSession(_ session: URLSession, task: URLSessionTask, didReceive challenge: URLAuthenticationChallenge,
                    completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void) {
        completionHandler(.cancelAuthenticationChallenge, nil)
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
        completionHandler(nil)
    }
}
private struct LimitsURLSocket: LimitsSocket {
    let task: URLSessionWebSocketTask
    func send(_ text: String) async throws { try await task.send(.string(text)) }
    func receive() async throws -> String {
        switch try await task.receive() {
        case .string(let text): return text
        case .data(let data):
            guard let text = String(data: data, encoding: .utf8) else { throw LimitsConnectionError.protocolMismatch }
            return text
        @unknown default: throw LimitsConnectionError.protocolMismatch
        }
    }
    func close() async { task.cancel(with: .goingAway, reason: nil) }
}

enum LimitsCredentialKind: Sendable {
    /// Short-lived token from the pairing exchange, bound to this key.
    case dpop(privateKey: Data)
    /// Long-lived token issued by T3's CLI. Valid for whoever holds it.
    case bearer
}

struct LimitsCredential: Codable, Sendable {
    let port: Int
    let token: String
    let expiresAt: Date
    /// Stored shape stays flat so credentials saved before Bearer support still decode.
    let privateKey: Data?

    init(port: Int, token: String, expiresAt: Date, privateKey: Data? = nil) {
        self.port = port
        self.token = token
        self.expiresAt = expiresAt
        self.privateKey = privateKey
    }

    var kind: LimitsCredentialKind { privateKey.map { .dpop(privateKey: $0) } ?? .bearer }

    /// T3 session tokens look like `<base64url JSON>.<signature>` with `exp` in milliseconds.
    /// Anything else is treated as non-expiring and the server's 401 is the signal.
    static func bearer(port: Int, token: String) -> LimitsCredential {
        LimitsCredential(port: port, token: token, expiresAt: bearerExpiry(token) ?? .distantFuture)
    }

    static func bearerExpiry(_ token: String) -> Date? {
        guard let payload = token.split(separator: ".", omittingEmptySubsequences: false).first else { return nil }
        var base64 = payload.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        base64 += String(repeating: "=", count: (4 - base64.count % 4) % 4)
        guard let data = Data(base64Encoded: base64),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let exp = object["exp"] as? NSNumber, CFGetTypeID(exp) != CFBooleanGetTypeID(),
              exp.doubleValue.isFinite, exp.doubleValue > 0 else { return nil }
        return Date(timeIntervalSince1970: exp.doubleValue / 1000)
    }
}
protocol LimitsCredentialStore: Sendable {
    func load() throws -> LimitsCredential?
    func save(_ credential: LimitsCredential) throws
    func delete() throws
}
/// Only Bar's own exchanged token is stored. No T3 Code Keychain items are queried.
struct LimitsKeychainStore: LimitsCredentialStore {
    private var query: [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: "com.t3codebar.limits", kSecAttrAccount as String: "local-read-only"]
    }
    func load() throws -> LimitsCredential? {
        var attributes = query
        attributes[kSecReturnData as String] = true
        attributes[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(attributes as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = result as? Data,
              let credential = try? JSONDecoder().decode(LimitsCredential.self, from: data) else {
            throw LimitsConnectionError.storage
        }
        return credential
    }
    func save(_ credential: LimitsCredential) throws {
        try delete()
        var attributes = query
        attributes[kSecValueData as String] = try JSONEncoder().encode(credential)
        attributes[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        guard SecItemAdd(attributes as CFDictionary, nil) == errSecSuccess else { throw LimitsConnectionError.storage }
    }
    func delete() throws {
        let status = SecItemDelete(query as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw LimitsConnectionError.storage }
    }
}
