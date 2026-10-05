// Pairing this iPhone with the Mac (spec §5.1, §22.10): scan the Mac's QR,
// send the hello over the pinned channel, show the code to compare, wait
// for the Mac's confirm, stream the bundle into the engine (which checks
// it and asks for Face ID), send the ACK. Any failure removes everything
// the attempt created.

import CryptoKit
import Foundation
import UIKit

@MainActor
final class PairingFlow: ObservableObject {
    enum Step: Equatable {
        case scan, contacting, compare(String), waiting, finishing, done, failed(String)
    }

    @Published var step: Step = .scan
    private var client: PairingClient?
    /// The bundle, fetched while the code stays on screen; used only once
    /// the user has tapped "They match" (review SEC-I2, 0f5f21b).
    private var bundle: Task<Any?, Never>?
    private let engine: VaultEngine
    private let onDone: () -> Void
    private let onFailed: (String) -> Void

    init(engine: VaultEngine, onDone: @escaping () -> Void, onFailed: @escaping (String) -> Void) {
        self.engine = engine
        self.onDone = onDone
        self.onFailed = onFailed
    }

    func scanned(_ text: String) {
        guard step == .scan, let qr = (try? JSONSerialization.jsonObject(with: Data(text.utf8))) as? [String: Any], qr["v"] as? Int == 2 else { return }
        step = .contacting
        // The screen stays on while the user confirms on the Mac: a lock
        // would end the attempt (review VER-I7).
        UIApplication.shared.isIdleTimerDisabled = true
        Task { await begin(qr) }
    }

    private func begin(_ qr: [String: Any]) async {
        let begun = await engine.callAsync(["op": "join_begin", "qr": qr, "name": UIDevice.current.name])
        guard begun["ok"] as? Bool == true, let hello = begun["hello"] as? [String: Any],
              let host = begun["host"] as? String, let port = begun["port"] as? Int, let fp = begun["fp"] as? String,
              let client = PairingClient(host: host, port: port, fp: fp) else {
            return await fail(begun["error"] as? String ?? "That code is not a SOURCE Vault code.")
        }
        self.client = client
        do {
            let answer = try await client.post("v1/vault/enroll/hello", hello)
            let sas = await engine.callAsync(["op": "join_hello", "reply": answer["reply"] ?? [:]])
            guard let code = sas["sas"] as? String, let proof = sas["proof"] as? String else { return await fail("The Mac's answer did not check out.") }
            client.proof = proof
            step = .compare(code)
            // The Mac sends the bundle only after the user confirms in its
            // Source Vault window; fetch it now, use it only after the tap.
            bundle = Task { try? await client.get("v1/vault/enroll/bundle", timeout: 310)["bundle"] }
            // The code lives as long as the Mac's 300 s session, no longer
            // (review SEC-O1, 1e83714).
            Task { [weak self] in
                try? await Task.sleep(nanoseconds: 290 * 1_000_000_000)
                guard let self, case .compare = self.step else { return }
                await self.fail("Code expired — generate a new one on your Mac.")
            }
        } catch {
            await fail("Secure channel could not be established.")
        }
    }

    /// The user compared this code with the Source Vault window's.
    func codesMatch(_ match: Bool) {
        guard case .compare = step else { return }
        if match {
            step = .waiting
            Task { await receive() }
        } else {
            Task { await fail("Codes don't match — do not pair.") }
        }
    }

    private func receive() async {
        guard let client, let fetch = bundle else { return await fail("Connection lost — start over.") }
        guard let bundle = await fetch.value else { return await fail("The Mac sent nothing — start over.") }
        do {
            step = .finishing
            let bytes = try JSONSerialization.data(withJSONObject: bundle)
            let done = await stream(bytes)
            guard let ack = done["ack"] as? [String: Any] else {
                return await fail(AppModel.describe(done["error"] as? String ?? "REFUSED"))
            }
            _ = try await client.post("v1/vault/enroll/ack", ack)
            let finished = await engine.callAsync(["op": "join_finish"])
            client.close()
            guard finished["ok"] as? Bool == true else { return await fail("iPhone didn't finish pairing — try again.") }
            UIApplication.shared.isIdleTimerDisabled = false
            step = .done
            onDone()
        } catch {
            await fail("iPhone didn't finish pairing — try again.")
        }
    }

    /// The bundle into the engine's join session, in stream chunks that fit
    /// the FFI's request cap, then the §22.10 checks (`join_complete`).
    private func stream(_ bytes: Data) async -> [String: Any] {
        let sha = bytes.sha256Hex
        let begun = await engine.callAsync(["op": "join_bundle_begin", "sha256": sha, "size": bytes.count])
        guard let session = begun["session"] as? String else { return begun }
        let s = await engine.callAsync(["op": "stream_begin", "session": session, "sha256": sha, "size": bytes.count])
        guard let stream = s["stream_id"] as? String else { return s }
        let chunk = 24 * 1024
        var offset = 0
        var seq = 0
        while offset < bytes.count {
            let part = bytes[offset..<min(offset + chunk, bytes.count)]
            let w = await engine.callAsync(["op": "stream_write", "session": session, "stream_id": stream, "seq": seq, "offset": offset, "data": part.base64URL])
            guard w["ok"] as? Bool == true else { return w }
            offset += chunk
            seq += 1
        }
        _ = await engine.callAsync(["op": "stream_end", "session": session, "stream_id": stream])
        return await engine.callAsync(["op": "join_complete", "session": session])
    }

    private func fail(_ message: String) async {
        UIApplication.shared.isIdleTimerDisabled = false
        bundle?.cancel()
        bundle = nil
        client?.close()
        client = nil
        _ = await engine.callAsync(["op": "join_abort"])
        step = .failed(message)
        // Shown wherever the app is now, even if the pairing screen has
        // already been replaced (review SEC-I1, 1e83714).
        onFailed(message)
    }

    func restart() {
        step = .scan
    }

    /// Leaving the pairing screen: the screen may sleep again (SEC-O2).
    func leave() {
        UIApplication.shared.isIdleTimerDisabled = false
    }
}

extension Data {
    var sha256Hex: String {
        SHA256.hash(data: self).map { String(format: "%02x", $0) }.joined()
    }

    /// The engine's encoding (`vault_proto::b64`): base64url without
    /// padding — standard base64 is refused (review VER-B1).
    var base64URL: String {
        base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    }
}
