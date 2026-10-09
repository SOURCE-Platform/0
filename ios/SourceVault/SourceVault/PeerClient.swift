// The phone's transport to its Mac (spec v0.5 §22.8; wire annex A.2.1):
// `POST /v1/vault/peer` on the Mac main app's mobile server, TLS pinned to
// the SHA-256 of the server key's SubjectPublicKeyInfo from the pairing
// bundle, the bearer token in the header only, no redirects. The bytes
// are the engine's carriage entries; this file never reads them.

import CryptoKit
import Foundation

final class PeerClient: NSObject, URLSessionTaskDelegate {
    enum Failure: Error {
        /// No host hint answered over a pinned channel.
        case unreachable
        /// An unsigned refusal (401, 403, 413, 429, 503) — the engine
        /// reads it as "unable to verify" or the rate limit.
        case refused(Int)
    }

    private let hosts: [String]
    private let port: Int
    private let pin: String
    private let token: String
    private lazy var session = URLSession(configuration: .ephemeral, delegate: self, delegateQueue: nil)

    /// From the engine's `peer_sync_begin` endpoint (annex A.4).
    init?(endpoint: [String: Any]) {
        guard let hosts = endpoint["host_hints"] as? [String], !hosts.isEmpty,
              let port = endpoint["port"] as? Int, let pin = endpoint["spki_sha256"] as? String,
              let token = endpoint["token"] as? String else { return nil }
        self.hosts = hosts
        self.port = port
        self.pin = pin.lowercased()
        self.token = token
    }

    /// One carriage entry to the Mac; its carriage entry back. Host hints
    /// are untrusted and tried in order — the pin decides.
    func send(_ body: Data) async throws -> Data {
        for host in hosts {
            let h = host.contains(":") ? "[\(host)]" : host
            guard let url = URL(string: "https://\(h):\(port)/v1/vault/peer") else { continue }
            var req = URLRequest(url: url, timeoutInterval: 30)
            req.httpMethod = "POST"
            req.setValue("application/octet-stream", forHTTPHeaderField: "Content-Type")
            req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
            req.httpBody = body
            guard let (data, response) = try? await session.data(for: req), let http = response as? HTTPURLResponse else { continue }
            guard http.statusCode == 200 else { throw Failure.refused(http.statusCode) }
            return data
        }
        throw Failure.unreachable
    }

    func close() {
        session.invalidateAndCancel()
    }

    /// The SPKI DER of an uncompressed P-256 key: the fixed algorithm
    /// header, then the 65-byte point (what `peer_tokens::spki_of_key_pem`
    /// hashes on the Mac). Any other key type is refused.
    static let p256SPKIPrefix = Data([
        0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01,
        0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
    ])

    static func spkiSHA256(of key: SecKey) -> String? {
        guard let attrs = SecKeyCopyAttributes(key) as? [CFString: Any],
              attrs[kSecAttrKeyType] as? String == (kSecAttrKeyTypeECSECPrimeRandom as String),
              attrs[kSecAttrKeySizeInBits] as? Int == 256,
              let raw = SecKeyCopyExternalRepresentation(key, nil) as Data?, raw.count == 65, raw.first == 0x04 else { return nil }
        return SHA256.hash(data: p256SPKIPrefix + raw).map { String(format: "%02x", $0) }.joined()
    }

    func urlSession(_ session: URLSession, didReceive challenge: URLAuthenticationChallenge, completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void) {
        guard challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust,
              let trust = challenge.protectionSpace.serverTrust,
              let leaf = (SecTrustCopyCertificateChain(trust) as? [SecCertificate])?.first,
              let key = SecCertificateCopyKey(leaf), Self.spkiSHA256(of: key) == pin else {
            completionHandler(.cancelAuthenticationChallenge, nil)
            return
        }
        completionHandler(.useCredential, URLCredential(trust: trust))
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
        completionHandler(nil)
    }
}
