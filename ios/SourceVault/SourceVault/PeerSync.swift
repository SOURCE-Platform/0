// "Sync with your Mac" (spec v0.5 §22.8): the engine builds, signs and
// checks every message; this loop only carries the bytes to the Mac and
// back, one request at a time, and reports the outcome in plain words.

import Foundation

extension AppModel {
    @MainActor
    func syncWithMac() async {
        guard let engine, !syncing else { return }
        // The engine's own state, not the published one a refresh may not
        // have updated yet.
        let now = await engine.callAsync(["op": "get_state"])
        guard now["state"] as? String == "unlocked", now["removed"] == nil else { return }
        syncing = true
        defer { syncing = false }
        let begun = await engine.callAsync(["op": "peer_sync_begin"])
        guard let first = begun["request"] as? String, let endpoint = begun["endpoint"] as? [String: Any],
              let client = PeerClient(endpoint: endpoint) else {
            if let error = begun["error"] as? String, error != "BAD_STATE" { syncNote = Self.describe(error) }
            return
        }
        defer { client.close() }
        var request = first
        while true {
            guard let body = Data(base64URL: request) else { return }
            let step: [String: Any]
            do {
                let answer = try await client.send(body)
                step = await engine.callAsync(["op": "peer_sync_step", "response": answer.base64URL])
            } catch PeerClient.Failure.refused(let code) {
                step = await engine.callAsync(["op": "peer_sync_step", "refused": code])
            } catch {
                syncNote = "Your Mac isn't reachable. Open SOURCE on your Mac, on the same Wi-Fi."
                return
            }
            if let next = step["request"] as? String {
                request = next
            } else if let done = step["done"] as? [String: Any] {
                lastSynced = Date()
                let added = done["admitted"] as? Int ?? 0
                syncNote = added > 0 ? "Updated from your Mac." : (done["mac_behind"] as? Bool == true ? "Your Mac needs to catch up first." : nil)
                refresh()
                return
            } else if step["removed"] != nil {
                refresh()
                return
            } else {
                syncNote = Self.describe(step["error"] as? String ?? "PEER_AUTH_INVALID")
                return
            }
        }
    }
}

extension Data {
    /// `vault_proto::b64`: base64url without padding.
    init?(base64URL s: String) {
        guard !s.contains(where: { "+/=".contains($0) }) else { return nil }
        var t = s.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        while t.count % 4 != 0 { t += "=" }
        self.init(base64Encoded: t)
    }
}
