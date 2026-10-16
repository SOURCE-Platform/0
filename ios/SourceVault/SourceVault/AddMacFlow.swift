// Adding a replacement Mac (spec §22.13; design `phase-f2e-design.md`
// revision 2): scan the Mac's QR (v:3), send the hello over the pinned
// channel, compare the code with the Mac's "Join a Vault" window, confirm
// with Face ID and the master password, send the bundle, keep the Mac's
// acknowledgement, tell the Mac it is done, publish. Only after the old
// Mac was removed (the engine refuses otherwise).

import SwiftUI
import UIKit

@MainActor
final class AddMacFlow: ObservableObject {
    enum Step: Equatable {
        case scan, contacting, compare(String), sending, done, failed(String)
    }

    @Published var step: Step = .scan
    private var client: PairingClient?
    private let engine: VaultEngine

    init(engine: VaultEngine) {
        self.engine = engine
    }

    func scanned(_ text: String) {
        guard step == .scan, let qr = (try? JSONSerialization.jsonObject(with: Data(text.utf8))) as? [String: Any], qr["v"] as? Int == 3 else { return }
        step = .contacting
        UIApplication.shared.isIdleTimerDisabled = true // a lock would end it
        Task { await begin(qr) }
    }

    private func begin(_ qr: [String: Any]) async {
        let begun = await engine.callAsync(["op": "authorize_begin", "qr": qr])
        guard begun["ok"] as? Bool == true, let hello = begun["hello"] as? [String: Any], let host = begun["host"] as? String,
              let port = begun["port"] as? Int, let fp = begun["fp"] as? String, let client = PairingClient(host: host, port: port, fp: fp) else {
            return fail(AddMacFlow.describe(begun["error"] as? String ?? "INVALID_INPUT"))
        }
        self.client = client
        do {
            let answer = try await client.post("v1/vault/renroll/hello", hello)
            let code = await engine.callAsync(["op": "authorize_reply", "reply": answer["reply"] ?? [:]])
            guard let sas = code["sas"] as? String else { return fail("The Mac's answer did not check out. Start again on the Mac.") }
            step = .compare(sas)
        } catch {
            fail("Couldn't reach the Mac. Keep this iPhone on the same Wi-Fi, next to the Mac, and try again.")
        }
    }

    /// "They match": Face ID and the master password, then the bundle.
    func confirm() {
        guard case .compare = step, let client else { return }
        step = .sending
        Task {
            let c = await engine.callAsync(["op": "authorize_confirm"])
            guard c["ok"] as? Bool == true, let session = c["session"] as? String, let sha = c["stream"] as? String,
                  let size = c["size"] as? Int, let proof = c["proof"] as? String else {
                return fail(AddMacFlow.describe(c["error"] as? String ?? "REFUSED"))
            }
            client.proof = proof
            var bundle = Data()
            while bundle.count < size {
                let r = await engine.callAsync(["op": "stream_read", "session": session, "sha256": sha, "offset": bundle.count])
                guard let chunk = (r["data"] as? String).flatMap({ Data(base64URL: $0) }), !chunk.isEmpty else { return fail("Couldn't prepare your vault for the Mac.") }
                bundle.append(chunk)
            }
            do {
                // The Mac answers after its window: the code compared and
                // the master password typed there.
                let acked = try await client.postData("v1/vault/renroll/bundle", bundle, timeout: 300)
                bundle.resetBytes(in: 0..<bundle.count)
                let finished = await engine.callAsync(["op": "authorize_finish", "ack": acked["ack"] ?? [:], "peer_endpoint": acked["peer_endpoint"] ?? NSNull()])
                guard finished["ok"] as? Bool == true, let head = finished["registry_head"] as? String else {
                    return fail(AddMacFlow.describe(finished["error"] as? String ?? "REFUSED"))
                }
                _ = try await client.post("v1/vault/renroll/done", ["registry_head": head])
                _ = await engine.callAsync(["op": "provider_publish", "interactive": false])
                step = .done
                end()
            } catch {
                fail("The Mac didn't finish. Nothing was added — start again on the Mac.")
            }
        }
    }

    func cancel() {
        Task { _ = await engine.callAsync(["op": "authorize_abort"]) }
        end()
    }

    private func fail(_ message: String) {
        Task { _ = await engine.callAsync(["op": "authorize_abort"]) }
        step = .failed(message)
        end()
    }

    private func end() {
        client?.close()
        client = nil
        UIApplication.shared.isIdleTimerDisabled = false
    }

    static func describe(_ error: String) -> String {
        switch error {
        case "REPLACEMENT_ONLY": return "Remove your old Mac first (in the Devices list below), wait for it to reach your backup, then add the new Mac."
        case "NOT_FRESH": return "Tap “Sync now” first, so your new Mac gets an up-to-date vault."
        case "INVALID_INPUT": return "That code isn't from a Mac next to you on the same network."
        case "PROTOCOL_VIOLATION": return "That isn't a SOURCE \"Join from your iPhone\" code."
        default: return AppModel.describe(error)
        }
    }
}

struct AddMacView: View {
    @StateObject var flow: AddMacFlow

    var body: some View {
        VStack(spacing: 20) {
            switch flow.step {
            case .scan:
                Text("On the new Mac: open SOURCE, go to the Vault tab and choose “Show code for my iPhone”. Then scan it here.")
                    .multilineTextAlignment(.center)
                QRScanner { flow.scanned($0) }.frame(height: 320).clipShape(RoundedRectangle(cornerRadius: 16))
            case .contacting:
                ProgressView("Contacting the Mac…")
            case .compare(let code):
                Text("The Mac's Source Vault window shows a code. It must be exactly this one:").multilineTextAlignment(.center)
                Text(code).font(.system(.largeTitle, design: .monospaced)).bold()
                HStack {
                    Button("Codes don't match", role: .destructive) { flow.cancel() }
                    Button("They match") { flow.confirm() }.buttonStyle(.borderedProminent)
                }
            case .sending:
                ProgressView("Sending your vault to the Mac… Enter your master password in the Mac's window too.")
            case .done:
                Label("Mac added", systemImage: "checkmark.seal")
            case .failed(let message):
                Text(message).multilineTextAlignment(.center)
            }
        }
        .padding()
        .navigationTitle("Add a Mac")
        .onDisappear { if flow.step != .done { flow.cancel() } }
    }
}
