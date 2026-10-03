// The phone's half of the Mac's ephemeral enrollment server (spec §5.2):
// HTTPS to the QR's host and port, accepted only when the server's leaf
// certificate hashes (SHA-256 of its DER) to the QR's `fp` — the hostname
// is ignored (decision D6). Three routes: hello, bundle (waits while the
// user confirms on the Mac), ack.

import CryptoKit
import Foundation

final class PairingClient: NSObject, URLSessionTaskDelegate {
    private let base: URL
    /// The hello's route proof (review SEC-I3): sent with the bundle and
    /// ACK requests, which the Mac answers only for the device that said
    /// hello.
    var proof: String?
    private let fp: String
    private lazy var session = URLSession(configuration: .ephemeral, delegate: self, delegateQueue: nil)

    init?(host: String, port: Int, fp: String) {
        // An IPv6 literal needs brackets in a URL.
        let h = host.contains(":") ? "[\(host)]" : host
        guard let url = URL(string: "https://\(h):\(port)") else { return nil }
        base = url
        self.fp = fp.lowercased()
    }

    func post(_ path: String, _ body: [String: Any]) async throws -> [String: Any] {
        var req = URLRequest(url: base.appendingPathComponent(path), timeoutInterval: 30)
        req.httpMethod = "POST"
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        req.httpBody = try JSONSerialization.data(withJSONObject: body)
        return try await send(req)
    }

    /// The bundle route waits up to the 300 s session for the Mac's confirm.
    func get(_ path: String, timeout: TimeInterval) async throws -> [String: Any] {
        try await send(URLRequest(url: base.appendingPathComponent(path), timeoutInterval: timeout))
    }

    private func send(_ req: URLRequest) async throws -> [String: Any] {
        var req = req
        if let proof { req.setValue(proof, forHTTPHeaderField: "X-Ov0-Proof") }
        let (data, _) = try await session.data(for: req)
        guard let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else {
            throw PairingError.refused("MALFORMED")
        }
        guard json["ok"] as? Bool == true else { throw PairingError.refused(json["error"] as? String ?? "REFUSED") }
        return json
    }

    /// The pin: the presented leaf certificate must be the one on the
    /// Mac's screen. Anything else ends the connection before any frame.
    func urlSession(_ session: URLSession, didReceive challenge: URLAuthenticationChallenge, completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void) {
        guard challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust,
              let trust = challenge.protectionSpace.serverTrust,
              let leaf = (SecTrustCopyCertificateChain(trust) as? [SecCertificate])?.first else {
            completionHandler(.cancelAuthenticationChallenge, nil)
            return
        }
        let der = SecCertificateCopyData(leaf) as Data
        let hash = SHA256.hash(data: der).map { String(format: "%02x", $0) }.joined()
        if hash == fp {
            completionHandler(.useCredential, URLCredential(trust: trust))
        } else {
            completionHandler(.cancelAuthenticationChallenge, nil)
        }
    }

    /// Never follow a redirect: it could carry the hello somewhere else
    /// (review SEC-O2).
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
        completionHandler(nil)
    }

    func close() {
        session.invalidateAndCancel()
    }
}

enum PairingError: Error {
    case refused(String)
}
