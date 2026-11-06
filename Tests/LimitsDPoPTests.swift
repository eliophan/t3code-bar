import CryptoKit
import Foundation
import Testing
@testable import T3CodeBar

struct LimitsDPoPTests {
    private func decode(_ value: Substring) -> Data {
        let text = value.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        return Data(base64Encoded: text + String(repeating: "=", count: (4 - text.count % 4) % 4))!
    }

    @Test func proofIsSignedAndBoundToMethodURLAndToken() throws {
        let signer = LimitsDPoP()
        let url = URL(string: "http://127.0.0.1:43111/ws?ticket=private")!
        let jwt = try signer.proof(method: "GET", url: url, token: "ticket", now: Date(timeIntervalSince1970: 1000))
        let parts = jwt.split(separator: ".")
        #expect(parts.count == 3)
        let payload = try #require(JSONSerialization.jsonObject(with: decode(parts[1])) as? [String: Any])
        #expect(payload["htm"] as? String == "GET")
        #expect(payload["htu"] as? String == "http://127.0.0.1:43111/ws")
        #expect(payload["iat"] as? Int == 1000)
        let expected = Data(SHA256.hash(data: Data("ticket".utf8))).base64EncodedString()
            .replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
        #expect(payload["ath"] as? String == expected)
        let signature = try P256.Signing.ECDSASignature(rawRepresentation: decode(parts[2]))
        #expect(signer.key.publicKey.isValidSignature(signature, for: Data("\(parts[0]).\(parts[1])".utf8)))
        #expect(try signer.proof(method: "GET", url: url, token: "ticket") != jwt)
    }
}
