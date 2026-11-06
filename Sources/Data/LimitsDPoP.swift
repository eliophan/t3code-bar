import CryptoKit
import Foundation

/// A proof binds an access token to Bar's private key. It does not authenticate the server.
struct LimitsDPoP {
    let key: P256.Signing.PrivateKey

    init() { key = P256.Signing.PrivateKey() }
    init(raw: Data) throws { key = try P256.Signing.PrivateKey(rawRepresentation: raw) }

    func proof(method: String, url: URL, token: String? = nil, now: Date = Date()) throws -> String {
        var target = URLComponents(url: url, resolvingAgainstBaseURL: false)!
        target.query = nil
        target.fragment = nil
        let publicKey = key.publicKey.x963Representation
        let header: [String: Any] = ["typ": "dpop+jwt", "alg": "ES256", "jwk": [
            "kty": "EC", "crv": "P-256", "x": Self.base64(Data(publicKey[1..<33])),
            "y": Self.base64(Data(publicKey[33..<65]))]]
        var payload: [String: Any] = ["htm": method, "htu": target.url!.absoluteString,
                                      "jti": UUID().uuidString, "iat": Int(now.timeIntervalSince1970)]
        if let token { payload["ath"] = Self.base64(Data(SHA256.hash(data: Data(token.utf8)))) }
        let encodedHeader = Self.base64(try JSONSerialization.data(withJSONObject: header, options: .sortedKeys))
        let encodedPayload = Self.base64(try JSONSerialization.data(withJSONObject: payload, options: .sortedKeys))
        let body = "\(encodedHeader).\(encodedPayload)"
        let signature = try key.signature(for: Data(body.utf8))
        return body + "." + Self.base64(signature.rawRepresentation)
    }

    private static func base64(_ data: Data) -> String {
        data.base64EncodedString().replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    }
}
