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
        case preview(items: Int, created: Date?)
        case failed(String)
    }

    @Published var step = Step.enter
    private let engine: VaultEngine
    private let onDone: () -> Void

    init(engine: VaultEngine, onDone: @escaping () -> Void) {
        self.engine = engine
        self.onDone = onDone
    }

    func start(handle: String, withRecoveryKey: Bool) {
        step = .working("Finding your vault…")
        Task {
            let r = await engine.callAsync(["op": "provider_recover_start", "handle": handle, "kind": withRecoveryKey ? "rk" : "mp"])
            guard r["ok"] as? Bool == true else { return fail(r) }
            let p = await engine.callAsync(["op": "recovery_preview"])
            let preview = p["preview"] as? [String: Any] ?? [:]
            let created = (preview["created_at"] as? Double).map { Date(timeIntervalSince1970: $0) }
            step = .preview(items: preview["item_count"] as? Int ?? 0, created: created)
        }
    }

    func finish() {
        step = .working("Recovering your vault…")
        Task {
            let r = await engine.callAsync(["op": "provider_recover_finish"])
            guard r["ok"] as? Bool == true else { return fail(r) }
            onDone()
        }
    }

    func cancel() {
        engine.lock() // ends a recovery in progress; nothing was uploaded
        onDone()
    }

    private func fail(_ answer: [String: Any]) {
        step = .failed(AppModel.describe(answer["error"] as? String ?? "REFUSED"))
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
                    TextField("Recovery handle (email)", text: $handle).textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.emailAddress)
                    Picker("Unlock with", selection: $withRecoveryKey) {
                        Text("Master password").tag(false)
                        Text("Recovery Key").tag(true)
                    }
                } footer: {
                    Text("Use this only if you've lost every other device. Your vault gets a new key and a new Recovery Key, and the lost devices are locked out. A Mac can't join a vault recovered on an iPhone yet.")
                }
                if case .failed(let message) = flow.step { Text(message).foregroundStyle(.secondary) }
                Button("Find My Vault") { flow.start(handle: handle, withRecoveryKey: withRecoveryKey) }.disabled(handle.isEmpty)
            case .working(let what):
                ProgressView(what)
            case .preview(let items, let created):
                Section("Found your vault") {
                    LabeledContent("Items", value: "\(items)")
                    if let created { LabeledContent("Created", value: created.formatted(date: .abbreviated, time: .omitted)) }
                }
                Button("Recover This Vault") { flow.finish() }.buttonStyle(.borderedProminent)
            }
            Button("Cancel", role: .cancel) { flow.cancel() }
        }
        .navigationTitle("Recover Vault")
    }
}
