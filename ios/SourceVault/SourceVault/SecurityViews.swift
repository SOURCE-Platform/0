// Security on the iPhone (spec v0.5 §22.10, plan F.2d step 5): replace the
// Recovery Key, remove a device, and the Recovery Key sheet itself. A
// Recovery Key change publishes first (§22.7); a device removal does not —
// an ordinary publication would make the removed device's own deliveries
// provider-confirmed and slip them past the cutoff (chunk-2 review
// SEC-B1). Each runs inside a background task, which covers the commit;
// the upload afterwards needs the vault open, and the banner stays until
// it lands. The engine stages the rotation before the sheet.

import SwiftUI
import UIKit

struct DeviceSummary: Identifiable {
    let id: String
    let name: String
    let platform: String
    let isThisDevice: Bool
    let revoked: Bool
}

extension AppModel {
    func loadDevices(done: @escaping ([DeviceSummary]) -> Void) {
        engine?.call(["op": "list_devices"]) { answer in
            let list = answer["devices"] as? [[String: Any]] ?? []
            done(list.map {
                DeviceSummary(id: $0["device_id"] as? String ?? "", name: $0["device_name"] as? String ?? "", platform: $0["platform"] as? String ?? "",
                              isThisDevice: $0["self"] as? Bool == true, revoked: $0["revoked"] as? Bool == true)
            })
        }
    }

    /// A rotating op (`rotate_recovery_key`, `revoke_device`): publish
    /// first (not before a removal), the op, then publish the change.
    @MainActor
    func runSecurityChange(_ request: [String: Any], done: @escaping (String?) -> Void) {
        guard let engine else { return done(Self.describe("BAD_STATE")) }
        var task = UIBackgroundTaskIdentifier.invalid
        let end = { if task != .invalid { UIApplication.shared.endBackgroundTask(task); task = .invalid } }
        task = UIApplication.shared.beginBackgroundTask(withName: "vault-security-change") { end() }
        Task { @MainActor in
            defer { end() }
            if request["op"] as? String != "revoke_device" {
                _ = await engine.callAsync(["op": "provider_publish", "interactive": false])
            }
            let answer = await engine.callAsync(request)
            if answer["ok"] as? Bool == true {
                let published = await engine.callAsync(["op": "provider_publish", "interactive": false])
                done(published["ok"] as? Bool == true ? nil : "Done on this iPhone. It will reach your backup at the next sync — until then, keep this iPhone unlocked and online.")
            } else {
                done(Self.describe(answer["error"] as? String ?? "REFUSED"))
            }
            refresh()
        }
    }

    func dismissVoidNotice() {
        engine?.call(["op": "dismiss_rk_sheet_notice"]) { [weak self] _ in self?.refresh() }
    }
}

struct SecurityView: View {
    @EnvironmentObject var model: AppModel
    @State private var devices: [DeviceSummary] = []
    @State private var note: String?
    @State private var busy = false
    @State private var removing: DeviceSummary?

    var body: some View {
        Form {
            Section {
                Button("Replace Recovery Key") { run(["op": "rotate_recovery_key"]) }.disabled(busy)
            } footer: {
                Text("You'll get a new Recovery Key to save. The old one stops working once the change reaches your backup.")
            }
            Section("Devices") {
                ForEach(devices.filter { !$0.revoked }) { d in
                    HStack {
                        VStack(alignment: .leading) {
                            Text(d.name)
                            Text(d.isThisDevice ? "This iPhone" : d.platform).font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer()
                        if !d.isThisDevice {
                            Button("Remove", role: .destructive) { removing = d }.disabled(busy)
                        }
                    }
                }
            }
            if let note { Section { Text(note).foregroundStyle(.secondary) } }
            Section { RemoveVaultButton() }
        }
        .navigationTitle("Security")
        .onAppear { model.loadDevices { devices = $0 } }
        .confirmationDialog("Remove \(removing?.name ?? "this device")?", isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }), titleVisibility: .visible) {
            Button("Remove", role: .destructive) {
                if let d = removing { run(["op": "revoke_device", "device_id": d.id]) }
            }
        } message: {
            Text("It loses access to your vault. Your vault gets a new key, and you'll get a new Recovery Key to save.")
        }
    }

    private func run(_ request: [String: Any]) {
        busy = true
        note = nil
        model.runSecurityChange(request) { problem in
            busy = false
            note = problem ?? "Done."
            model.loadDevices { devices = $0 }
        }
    }
}

/// The new Recovery Key (§22.10 capture rules): hidden while the screen is
/// recorded and from the app-switcher snapshot; a screenshot warns.
struct RecoveryKeyView: View {
    let request: SheetRequest
    var onConfirmed: () -> Void = {}
    @Environment(\.dismiss) private var dismiss
    @Environment(\.scenePhase) private var phase
    @State private var saved = false
    @State private var screenshot = false
    @State private var answered = false

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Text(request.text).font(.system(.body, design: .monospaced)).privacySensitive()
                } header: {
                    Text(request.reason)
                } footer: {
                    Text("\(request.checkpoint)\n\(request.recovery)")
                }
                if screenshot {
                    Text("A screenshot was taken. Screenshots can be copied to other places — consider replacing this key once you've written it down.").foregroundStyle(.red)
                }
                Toggle("I've written this Recovery Key down and stored it safely", isOn: $saved)
            }
            .navigationTitle("New Recovery Key")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { answer(false) } }
                ToolbarItem(placement: .confirmationAction) { Button("Done") { answer(true) }.disabled(!saved) }
            }
        }
        .interactiveDismissDisabled()
        .onReceive(NotificationCenter.default.publisher(for: UIApplication.userDidTakeScreenshotNotification)) { _ in screenshot = true }
        .onDisappear { if !answered { request.cancel() } }
        .overlay { if phase != .active { Color(.systemBackground).ignoresSafeArea() } }
        // A presented cover sits above the app's own shield (review SEC-B3).
        .overlay { CaptureShield() }
    }

    private func answer(_ ok: Bool) {
        answered = true
        if ok { onConfirmed() }
        ok ? request.acknowledge() : request.cancel()
        dismiss()
    }
}

/// "Remove this vault" (§22.9, F.2d step 6): says first what only this
/// iPhone holds, then removes everything after Face ID.
struct RemoveVaultButton: View {
    @EnvironmentObject var model: AppModel
    @State private var warning: String?
    @State private var problem: String?

    var body: some View {
        Button("Remove This Vault from iPhone…", role: .destructive) { preview() }
            .confirmationDialog("Remove this vault from this iPhone?", isPresented: Binding(get: { warning != nil }, set: { if !$0 { warning = nil } }), titleVisibility: .visible) {
                Button("Remove Vault", role: .destructive) { remove() }
            } message: {
                Text(warning ?? "")
            }
        if let problem { Text(problem).font(.caption).foregroundStyle(.secondary) }
    }

    private func preview() {
        problem = nil
        model.engine?.call(["op": "remove_vault_preview"]) { answer in
            guard answer["ok"] as? Bool == true else { problem = AppModel.describe(answer["error"] as? String ?? "REFUSED"); return }
            let n = answer["local_only"] as? Int ?? 0
            var lines = ["Your vault's files, this iPhone's keys and its pairing are deleted from this iPhone."]
            if answer["unknown"] as? Bool == true {
                lines.append("This iPhone is locked, so it can't check whether it holds changes no other device has.")
            } else if n > 0 {
                lines.append("\(n) change\(n == 1 ? "" : "s") exist only on this iPhone and will be lost.")
            }
            if answer["access_lost"] as? Bool != true {
                lines.append("Your backup hasn't confirmed this iPhone was removed — this may be the only copy of some items.")
            }
            warning = lines.joined(separator: " ")
        }
    }

    private func remove() {
        model.engine?.call(["op": "remove_vault", "confirm": true]) { answer in
            if answer["ok"] as? Bool != true { problem = AppModel.describe(answer["error"] as? String ?? "REFUSED") }
            model.refresh()
        }
    }
}
