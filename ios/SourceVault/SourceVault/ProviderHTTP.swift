// Provider HTTPS for the engine's provider flows (spec §11.1; catalogue §2
// `http_send`; plan `phase-f2d-plan.md` step 2): standard certificate
// validation (no pinning, D-12), an ephemeral session with no cache, no
// redirects, a time limit, and an answer never larger than the engine's
// buffer. It carries ciphertext, public data and requests the engine
// signed — it never reads them, and never calls back into the engine.

import Foundation

final class ProviderHTTP: NSObject, URLSessionTaskDelegate {
    enum Answer {
        case answered(UInt16, Data, UInt64?)
        case tooLarge
        case unreachable
    }

    static let shared = ProviderHTTP()

    private lazy var session: URLSession = {
        let config = URLSessionConfiguration.ephemeral
        config.urlCache = nil
        config.requestCachePolicy = .reloadIgnoringLocalCacheData
        config.timeoutIntervalForResource = 35 // a backstop; each send has its own deadline
        return URLSession(configuration: config, delegate: self, delegateQueue: nil)
    }()

    /// Blocks the calling (engine) thread until the answer or the deadline:
    /// `timeoutMs` bounds the whole exchange, not only the gaps between
    /// bytes, so a provider that trickles its answer cannot hold the
    /// engine (reviews SEC-I3 / VER-I2).
    func send(origin: String, method: String, path: String, auth: String?, body: Data, timeoutMs: UInt64, maxBody: Int) -> Answer {
        guard let url = URL(string: origin + path), Self.allowed(url) else { return .unreachable }
        let limit = TimeInterval(timeoutMs) / 1000
        var req = URLRequest(url: url, timeoutInterval: limit)
        req.httpMethod = method
        if let auth { req.setValue(auth, forHTTPHeaderField: "Ov0-Auth") }
        if !body.isEmpty {
            req.httpBody = body
            req.setValue("application/octet-stream", forHTTPHeaderField: "Content-Type") // as the Mac sends it
        }
        let done = DispatchSemaphore(value: 0)
        let result = Box()
        let task = Task {
            defer { done.signal() }
            guard let (bytes, response) = try? await session.bytes(for: req), let http = response as? HTTPURLResponse else { return }
            // A refused redirect surfaces as its 3xx: no answer (catalogue §2).
            if (300..<400).contains(http.statusCode) { return }
            if http.expectedContentLength > maxBody { result.set(.tooLarge); return }
            var data = Data()
            do {
                for try await byte in bytes {
                    data.append(byte)
                    if data.count > maxBody { result.set(.tooLarge); return }
                }
            } catch { return }
            let date = (http.value(forHTTPHeaderField: "Date")).flatMap { Self.httpDate.date(from: $0) }.map { UInt64($0.timeIntervalSince1970) }
            result.set(.answered(UInt16(http.statusCode), data, date))
        }
        if done.wait(timeout: .now() + limit) == .timedOut {
            task.cancel()
            return .unreachable
        }
        return result.get()
    }

    /// `https` only; a local test provider in debug builds.
    private static func allowed(_ url: URL) -> Bool {
        #if DEBUG
        if url.scheme == "http" && url.host == "localhost" { return true }
        #endif
        return url.scheme == "https"
    }

    /// The answer, written by the request task and read once by the caller.
    private final class Box: @unchecked Sendable {
        private let lock = NSLock()
        private var answer = Answer.unreachable
        func set(_ a: Answer) { lock.lock(); answer = a; lock.unlock() }
        func get() -> Answer { lock.lock(); defer { lock.unlock() }; return answer }
    }

    private static let httpDate: DateFormatter = {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = TimeZone(identifier: "GMT")
        f.dateFormat = "EEE, dd MMM yyyy HH:mm:ss zzz"
        return f
    }()

    /// Never follow a redirect: it would carry the signed request and its
    /// body to an origin the provider chose (review SEC-O1).
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
        completionHandler(nil)
    }
}
