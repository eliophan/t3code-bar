import Foundation

/// Only the loopback port is accepted, never a URL supplied by runtime metadata or a pairing link.
struct LimitsEndpoint: Equatable, Sendable {
    let port: Int
    init(port: Int) throws {
        guard (1...65535).contains(port) else { throw LimitsConnectionError.destination }
        self.port = port
    }
    func url(_ path: String, ticket: String? = nil, proof: String? = nil) -> URL {
        var parts = URLComponents()
        parts.scheme = ticket == nil ? "http" : "ws"
        parts.host = "127.0.0.1"
        parts.port = port
        parts.path = path
        if let ticket {
            parts.queryItems = [URLQueryItem(name: "wsTicket", value: ticket),
                                URLQueryItem(name: "orchestrationProtocol", value: "2")]
            if let proof { parts.queryItems?.append(URLQueryItem(name: "dpop", value: proof)) }
        }
        return parts.url!
    }
}

enum LimitsConnectionError: Error, Equatable {
    case destination, authorization, invalidToken, protocolMismatch, expired, storage
    var message: String {
        switch self {
        case .destination: "Enter a local T3 Code port from 1 to 65535."
        case .authorization: "Pairing failed. Create a new read-only code."
        case .invalidToken: "Enter a T3 Code access token of up to 16 KiB."
        case .protocolMismatch: "This T3 Code version returned an unsupported Limits stream."
        case .expired: "The Bar access token expired or was revoked. Connect again with a new pairing code or access token."
        case .storage: "Could not save the Bar access token in Keychain. Connect again without Remember."
        }
    }
}

struct LimitsLiveData: Equatable, Sendable {
    var providers: [LimitsRecord] = []
    var sources: [LimitsRecord] = []
    var notices: [String] = []
    var records: [LimitsRecord] { providers + sources }

    mutating func apply(_ event: [String: Any]) throws -> Bool {
        guard LimitsRPC.matches(event["version"], integer: 1), let type = event["type"] as? String else {
            throw LimitsConnectionError.protocolMismatch
        }
        switch type {
        case "snapshot":
            guard let config = event["config"] as? [String: Any],
                  let raw = config["providers"] as? [[String: Any]] else { throw LimitsConnectionError.protocolMismatch }
            providers = decodeProviders(raw)
            if let sources = config["usageLimitSources"] { try decodeSources(sources) }
        case "providerStatuses":
            guard let payload = event["payload"] as? [String: Any],
                  let raw = payload["providers"] as? [[String: Any]] else { throw LimitsConnectionError.protocolMismatch }
            providers = decodeProviders(raw)
        case "usageLimitSourcesUpdated":
            guard let payload = event["payload"] as? [String: Any], let raw = payload["sources"] else {
                throw LimitsConnectionError.protocolMismatch
            }
            try decodeSources(raw)
        case "keybindingsUpdated", "settingsUpdated", "environmentThemesUpdated": return false
        default: throw LimitsConnectionError.protocolMismatch
        }
        return true
    }

    private func decodeProviders(_ raw: [[String: Any]]) -> [LimitsRecord] {
        raw.compactMap { object in
            guard let data = try? JSONSerialization.data(withJSONObject: object) else { return nil }
            return LimitsReader.decode(data, fallbackID: "provider")
        }
    }

    private mutating func decodeSources(_ value: Any) throws {
        guard let raw = value as? [[String: Any]] else { throw LimitsConnectionError.protocolMismatch }
        var next: [LimitsRecord] = []
        var warnings: [String] = []
        for source in raw {
            guard let id = source["id"] as? String, let label = source["label"] as? String,
                  let accounts = source["accounts"] as? [[String: Any]] else { continue }
            if let error = source["error"] as? String { warnings.append("\(label): \(error)") }
            else if accounts.isEmpty { warnings.append("\(label): No accounts reported.") }
            for account in accounts {
                guard let accountID = account["id"] as? String, let driver = account["driver"] as? String,
                      let limits = account["usageLimits"] as? [String: Any] else { continue }
                let workspace = account["workspaceId"] as? String
                let email = account["email"] as? String
                let name = accountID.lowercased().hasSuffix(".json") ? String(accountID.dropLast(5)) : accountID
                next.append(LimitsRecord(instanceID: "source:\(id):\(accountID)\(workspace.map { ":\($0)" } ?? "")",
                    driver: driver, enabled: true, installed: true, available: true,
                    displayName: email == nil ? name : nil, authenticated: true,
                    plan: account["plan"] as? String, email: email, workspaceID: workspace,
                    limits: LimitsReader.decodeLimits(limits), isSource: true))
            }
        }
        sources = next
        notices = warnings
    }
}

/// Effect RPC's JSON socket codec accepts one frame or a frame array. Numeric request IDs match T3.
enum LimitsRPC {
    static let requestID = 0
    static func matches(_ value: Any?, integer: Int) -> Bool {
        guard let number = value as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID() else { return false }
        return number.doubleValue == Double(integer)
    }
    static var subscribe: [String: Any] { ["_tag": "Request", "id": requestID,
        "tag": "subscribeServerConfig", "payload": ["usageLimitSources": true, "workspaceSnapshotsOnDemand": true],
        "headers": []] }
    static var ack: [String: Any] { ["_tag": "Ack", "requestId": requestID] }
    static var interrupt: [String: Any] { ["_tag": "Interrupt", "requestId": requestID, "interruptors": []] }
    static func encode(_ frame: [String: Any]) throws -> String {
        String(decoding: try JSONSerialization.data(withJSONObject: frame), as: UTF8.self)
    }
    static func frames(_ text: String) throws -> [[String: Any]] {
        guard text.utf8.count <= 8 * 1024 * 1024,
              let raw = try? JSONSerialization.jsonObject(with: Data(text.utf8)) else { throw LimitsConnectionError.protocolMismatch }
        if let frames = raw as? [[String: Any]] { return frames }
        if let frame = raw as? [String: Any] { return [frame] }
        throw LimitsConnectionError.protocolMismatch
    }
}
