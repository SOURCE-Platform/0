// iPhone remote approval (spec §6.5, §6.6, §7.1; Phase G design
// `phase-g-design.md`): when the Mac cannot ask for Touch ID itself, it
// asks this iPhone. The phone fetches the waiting requests from its Mac
// over the authenticated peer channel, shows exactly what it would approve
// (the site and the action), and on "Approve" asks for Face ID and signs
// that request — nothing else. A push only opens the app; the request is
// always fetched here.

import SwiftUI

struct ApprovalRequest: Identifiable {
    let id: String
    let origin: String
    let action: String
    /// The saved login it releases ("title — username"), when this phone's
    /// vault is open; and the Mac asking (final review SPEC-I1).
    var account: String?
    var mac: String?

    var label: String {
        switch action {
        case "fill_password": return "Fill a password"
        case "fill_card": return "Fill a card"
        case "reveal": return "Show a password"
        case "unlock_vault": return "Unlock the vault on your Mac"
        case "update_password": return "Save a changed password"
        default: return action
        }
    }
}

@MainActor
final class ApprovalCenter: ObservableObject {
    @Published var waiting: [ApprovalRequest] = []
    @Published var note: String?
    private let engine: VaultEngine

    init(engine: VaultEngine) {
        self.engine = engine
    }

    /// One `peer_approvals` exchange with the Mac.
    func check() async {
        let begun = await engine.callAsync(["op": "approvals_fetch_begin"])
        guard let request = begun["request"] as? String, let endpoint = begun["endpoint"] as? [String: Any] else { return }
        guard let answer = await send(request, endpoint) else { return }
        let r = await engine.callAsync(["op": "approvals_fetch_step", "response": answer.base64URL])
        let list = r["approvals"] as? [[String: Any]] ?? []
        waiting = list.compactMap { a in
            guard let id = a["request_id"] as? String, let origin = a["origin"] as? String, let action = a["action"] as? String else { return nil }
            let acct = a["account"] as? [String: Any]
            let title = acct?["title"] as? String
            let user = (acct?["username"] as? String).flatMap { $0.isEmpty ? nil : $0 }
            let account = title.map { t in user.map { "\(t) — \($0)" } ?? t }
            return ApprovalRequest(id: id, origin: origin, action: action, account: account, mac: a["mac"] as? String)
        }
    }

    /// Approve (Face ID, then the signed approval) or deny.
    func decide(_ request: ApprovalRequest, approve: Bool) async {
        note = nil
        let d = await engine.callAsync(["op": "approval_decide", "request_id": request.id, "approve": approve])
        guard let req = d["request"] as? String, let endpoint = d["endpoint"] as? [String: Any] else {
            note = AppModel.describe(d["error"] as? String ?? "REFUSED")
            return
        }
        guard let answer = await send(req, endpoint) else { return }
        let r = await engine.callAsync(["op": "approval_submit_step", "response": answer.base64URL])
        waiting.removeAll { $0.id == request.id }
        if let error = r["error"] as? String {
            note = error == "APPROVAL_EXPIRED" ? "That request expired. Try again on your Mac." : AppModel.describe(error)
        } else {
            note = approve ? "Approved." : "Denied."
        }
    }

    private func send(_ request: String, _ endpoint: [String: Any]) async -> Data? {
        guard let body = Data(base64URL: request), let client = PeerClient(endpoint: endpoint) else { return nil }
        defer { client.close() }
        do {
            return try await client.send(body)
        } catch {
            note = "Your Mac isn't reachable. Use the same Wi-Fi as your Mac."
            return nil
        }
    }
}

struct ApprovalSheet: View {
    @ObservedObject var center: ApprovalCenter
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                ForEach(center.waiting) { r in
                    Section {
                        LabeledContent("Website", value: r.origin)
                        LabeledContent("Request", value: r.label)
                        if r.action != "unlock_vault" {
                            LabeledContent("Account", value: r.account ?? "One saved item (unlock this app to see which)")
                        }
                        LabeledContent("From", value: r.mac ?? "Your Mac")
                        HStack {
                            Button("Deny", role: .destructive) { Task { await center.decide(r, approve: false) } }
                            Spacer()
                            Button("Approve with Face ID") { Task { await center.decide(r, approve: true) } }.buttonStyle(.borderedProminent)
                        }
                    } footer: {
                        Text("Approve only if you just asked for this on your Mac.")
                    }
                }
                if let note = center.note { Text(note).foregroundStyle(.secondary) }
                if center.waiting.isEmpty && center.note == nil { Text("Nothing is waiting for your approval.") }
            }
            .navigationTitle("Approval Requests")
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
            .refreshable { await center.check() }
        }
    }
}
