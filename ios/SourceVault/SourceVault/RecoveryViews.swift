// Total-loss recovery onto this iPhone (spec §11.5, §11.8; plan F.2d
// chunk 3): every other device is lost. The engine locates the vault by
// its recovery handle, asks for the master password or Recovery Key in its
// own secure entry, shows what it found (FR-01), and — only after the user
// confirms — re-keys the vault, shows a new Recovery Key and finalizes.
// Stated: a Mac cannot join a vault recovered on an iPhone until F.2e.

import SwiftUI

@MainActor
final class RecoveryFlow: ObservableObject {
    enum Step {
        case enter
        case working(String)
        case preview(items: Int, date: Date?, version: Int, check: String)
        case failed(String)
        /// The finish failed. `keyShown`: after the new Recovery Key was
        /// confirmed — the engine keeps the attempt, so it is sent again
        /// unchanged, and the recovery may already have taken effect.
        case finishFailed(String, keyShown: Bool)
    }

    @Published var step = Step.enter
    private let engine: VaultEngine
    private let onDone: () -> Void
    private let onFinishing: (Bool) -> Void

    init(engine: VaultEngine, onFinishing: @escaping (Bool) -> Void, onDone: @escaping () -> Void) {
        self.engine = engine
        self.onFinishing = onFinishing
        self.onDone = onDone
    }

    func start(handle: String, withRecoveryKey: Bool) {
        step = .working("Finding your vault…")
        Task {
            let r = await engine.callAsync(["op": "provider_recover_start", "handle": handle, "kind": withRecoveryKey ? "rk" : "mp"])
            guard r["ok"] as? Bool == true else {
                // Back to a clean start, so a second try is possible.
                engine.lock()
                return fail(r)
            }
            let p = await engine.callAsync(["op": "recovery_preview"])
            let preview = p["preview"] as? [String: Any] ?? [:]
            let date = (preview["created_at"] as? Double).map { Date(timeIntervalSince1970: $0) }
            step = .preview(items: preview["item_count"] as? Int ?? 0, date: date, version: preview["generation"] as? Int ?? 0, check: preview["registry_head_prefix"] as? String ?? "")
        }
    }

    func finish() {
        step = .working("Recovering your vault…")
        onFinishing(true)
        Task {
            let r = await engine.callAsync(["op": "provider_recover_finish"])
            onFinishing(false)
            guard r["ok"] as? Bool == true else {
                let error = r["error"] as? String ?? "REFUSED"
                // Refused before the sheet was confirmed: nothing was sent.
                step = .finishFailed(Self.describe(error), keyShown: !["PANEL_CANCELLED", "CAPTURE_UNSAFE", "BAD_STATE"].contains(error))
                return
            }
            onDone()
        }
    }

    func cancel() {
        engine.lock() // ends a recovery in progress here (§13.2)
        onDone()
    }

    private func fail(_ answer: [String: Any]) {
        step = .failed(Self.describe(answer["error"] as? String ?? "REFUSED"))
    }

    /// Recovery's own wording (the Mac's, `vaultErrors.ts`).
    static func describe(_ error: String) -> String {
        switch error {
        case "RECOVERY_THROTTLED": return "Too many recovery attempts — try again later, or recover with your Recovery Key."
        case "AUTH_INVALID", "PROVIDER_401": return "No backup matched that recovery name and password or Recovery Key."
        case "BACKUP_UNAVAILABLE": return "Your backup can't be reached right now. Check the connection and try again."
        case "DEVICE_NOT_AUTHORIZED": return "This iPhone needs Face ID turned on to hold your vault."
        case "CAPTURE_UNSAFE": return "Stop screen recording or mirroring, then try again."
        default: return AppModel.describe(error)
        }
    }
}

struct RecoverView: View {
    @ObservedObject var flow: RecoveryFlow
    @State private var handle = ""
    @State private var withRecoveryKey = false

    var body: some View {
        Form {
            switch flow.step {
            case .enter, .failed:
                Section {
                    TextField("Recovery name", text: $handle).textInputAutocapitalization(.never).autocorrectionDisabled()
                    Picker("Unlock with", selection: $withRecoveryKey) {
                        Text("Master password").tag(false)
                        Text("Recovery Key").tag(true)
                    }
                } footer: {
                    Text("Use this only if you've lost every other device. Your vault gets a new key and the lost devices are locked out. A Mac can't join a vault recovered on an iPhone yet.")
                }
                if case .failed(let message) = flow.step { Text(message).foregroundStyle(.secondary) }
                Button("Find My Vault") { flow.start(handle: handle, withRecoveryKey: withRecoveryKey) }.disabled(handle.isEmpty)
            case .working(let what):
                ProgressView(what)
            case .preview(let items, let date, let version, let check):
                Section {
                    Text("This backup is from \(date?.formatted(date: .abbreviated, time: .shortened) ?? "an unknown date") and contains \(items) \(items == 1 ? "item" : "items").")
                    LabeledContent("Version", value: "\(version)")
                    LabeledContent("Check code", value: check).font(.system(.body, design: .monospaced))
                } header: {
                    Text("Found your vault")
                } footer: {
                    // §11.7: on a fresh device the user is the only check
                    // against an old backup (chunk-3 reviews SEC-B2 / VER-B1).
                    Text("Compare these with your printed Recovery Key sheet. An older version than your sheet means this backup is out of date — don't continue. Finishing replaces the vault key, removes every other device, and may show you a new Recovery Key.")
                }
                Button("Recover This Vault") { flow.finish() }.buttonStyle(.borderedProminent)
            case .finishFailed(let message, let keyShown):
                Text(message).foregroundStyle(.secondary)
                if keyShown {
                    Text("Keep the new Recovery Key you just saved: the recovery may already have taken effect. Try again to finish it.").font(.callout)
                }
                Button("Try Again") { flow.finish() }.buttonStyle(.borderedProminent)
            }
            if case .working = flow.step {} else {
                Button("Cancel", role: .cancel) { flow.cancel() }
            }
        }
        .navigationTitle("Recover Vault")
    }
}
