// "Sync with your Mac" (spec v0.5 §22.8): the engine builds, signs and
// checks every message; this loop only carries the bytes to the Mac and
// back, one request at a time, and reports the outcome in plain words.
// It runs after unlock and on "Sync now" — never in the background — and
// a lock cancels it.

import Foundation

extension AppModel {
    /// Start a sync unless one is running; a lock cancels it.
    @MainActor
    func startSync() {
        guard syncTask == nil else { return }
        let id = UUID()
        syncID = id
        syncTask = Task { @MainActor [weak self] in
            await self?.syncWithMac()
            // Only this run's slot: a lock may have cancelled it and a
            // newer sync taken its place (reviews SEC-O3 / VER-O4).
            if self?.syncID == id { self?.syncTask = nil }
        }
    }

    @MainActor
    private func syncWithMac() async {
        guard let engine else { return }
        // The engine's own state, not the published one a refresh may not
        // have updated yet.
        let now = await engine.callAsync(["op": "get_state"])
        guard now["state"] as? String == "unlocked", now["removed"] == nil else { return }
        syncing = true
        defer { if !Task.isCancelled { syncing = false } }
        let begun = await engine.callAsync(["op": "peer_sync_begin"])
        guard let first = begun["request"] as? String, let endpoint = begun["endpoint"] as? [String: Any] else {
            if let error = begun["error"] as? String, error != "BAD_STATE" { syncNote = Self.describe(error) }
            return
        }
        guard let client = PeerClient(endpoint: endpoint) else {
            syncNote = "This iPhone has no network address for your Mac yet. Make sure your Mac and iPhone are on the same Wi-Fi."
            return
        }
        defer { client.close() }
        var request = first
        while !Task.isCancelled {
            guard let body = Data(base64URL: request) else { return }
            let step: [String: Any]
            do {
                let answer = try await client.send(body)
                if Task.isCancelled { return }
                step = await deliver(answer, to: engine)
            } catch PeerClient.Failure.refused(let code) {
                step = await engine.callAsync(["op": "peer_sync_step", "refused": code])
                if code == 503 { syncNote = "Your Mac's vault isn't ready. Open SOURCE on your Mac."; return }
            } catch {
                if !Task.isCancelled { syncNote = "Your Mac isn't reachable. Open SOURCE on your Mac, on the same Wi-Fi." }
                return
            }
            if let next = step["request"] as? String {
                request = next
            } else if let done = step["done"] as? [String: Any] {
                report(done)
                refresh()
                return
            } else if step["removed"] != nil {
                refresh()
                return
            } else {
                // A lock in the meantime ends the exchange quietly.
                if step["error"] as? String != "BAD_STATE" { syncNote = Self.describe(step["error"] as? String ?? "PEER_AUTH_INVALID") }
                return
            }
        }
    }

    /// One answer into the engine: inline when it fits one FFI frame, else
    /// streamed in (review SEC-I1).
    private func deliver(_ answer: Data, to engine: VaultEngine) async -> [String: Any] {
        if answer.count <= 40 * 1024 {
            return await engine.callAsync(["op": "peer_sync_step", "response": answer.base64URL])
        }
        let sha = answer.sha256Hex
        let r = await engine.callAsync(["op": "peer_sync_receive", "sha256": sha, "size": answer.count])
        guard let session = r["session"] as? String else { return r }
        let s = await engine.callAsync(["op": "stream_begin", "session": session, "sha256": sha, "size": answer.count])
        guard let stream = s["stream_id"] as? String else { return s }
        let chunk = 24 * 1024
        var offset = 0, seq = 0
        while offset < answer.count {
            let part = answer[offset..<min(offset + chunk, answer.count)]
            let w = await engine.callAsync(["op": "stream_write", "session": session, "stream_id": stream, "seq": seq, "offset": offset, "data": part.base64URL])
            guard w["ok"] as? Bool == true else { return w }
            offset += chunk
            seq += 1
        }
        let end = await engine.callAsync(["op": "stream_end", "session": session, "stream_id": stream])
        guard end["ok"] as? Bool == true else { return end }
        return await engine.callAsync(["op": "peer_sync_step", "session": session])
    }

    /// "Synced" only when the exchange completed and the Mac's signed
    /// status was checked; otherwise say what is still open (§22.5).
    @MainActor
    private func report(_ done: [String: Any]) {
        let n = { (k: String) in done[k] as? Int ?? 0 }
        if done["checked"] as? Bool != true || done["mac_behind"] as? Bool == true {
            syncNote = "Your Mac needs to catch up first. Try again later."
            return
        }
        lastSynced = Date()
        if n("waiting") > 0 {
            syncNote = "Some changes from your Mac can't be opened here yet. They'll arrive once this iPhone catches up."
        } else if n("unavailable") > 0 || done["limited"] as? Bool == true {
            syncNote = "Some items couldn't come from your Mac directly. They'll arrive through your backup."
        } else {
            syncNote = n("admitted") > 0 ? "Updated from your Mac." : nil
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
