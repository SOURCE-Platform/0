// The engine behind SOURCE Vault (spec v0.5 §22.2): the five catalogued
// entry points of `vault-ffi`, called only from here. Ops run on one
// background queue (they block while waiting on the user, catalogue §1);
// lock and close may be called from any thread.

import Foundation

final class VaultEngine: @unchecked Sendable {
    private let handle: OpaquePointer
    private let services: VaultServices
    private let queue = DispatchQueue(label: "com.racker.source-vault.engine")

    /// Opens the engine on `dir`. Call only while protected data is
    /// available (catalogue §1).
    init?(dir: URL, services: VaultServices) {
        // The engine copies the table at open; `services` (its ctx) lives
        // as long as this object.
        var table = Ov0Callbacks(
            ctx: Unmanaged.passUnretained(services).toOpaque(),
            secure_entry: vaultSecureEntry,
            recovery_sheet: vaultRecoverySheet,
            presence: vaultPresence,
            capture_suppressed: vaultCaptureSuppressed,
            event: vaultEvent
        )
        guard let h = dir.path.withCString({ path in ov0_engine_open(path, &table) }) else { return nil }
        handle = h
        self.services = services
    }

    /// One §1.5 op; the answer arrives on the main queue.
    func call(_ request: [String: Any], done: @escaping ([String: Any]) -> Void) {
        queue.async { [self] in
            let answer = callNow(request)
            DispatchQueue.main.async { done(answer) }
        }
    }

    private func callNow(_ request: [String: Any]) -> [String: Any] {
        guard var bytes = try? JSONSerialization.data(withJSONObject: request) else { return ["ok": false, "error": "INVALID_INPUT"] }
        defer { bytes.resetBytes(in: 0..<bytes.count) } // a request may carry a record's fields
        var out: UnsafeMutablePointer<UInt8>?
        var len = 0
        let rc = bytes.withUnsafeBytes { raw in
            ov0_engine_call(handle, raw.bindMemory(to: UInt8.self).baseAddress, raw.count, &out, &len)
        }
        guard rc == 0, let out else { return ["ok": false, "error": "INVALID_INPUT"] }
        defer { ov0_engine_free(out) } // zeroed by the engine as it frees
        let data = Data(bytesNoCopy: out, count: len, deallocator: .none)
        return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] ?? ["ok": false]
    }

    /// Locks at once, whatever op is waiting (§1.6, catalogue §1).
    func lock() {
        ov0_engine_lock(handle)
    }

    deinit {
        ov0_engine_close(handle)
    }
}

// MARK: - C callbacks (catalogue §2): each finds the services through ctx.

private func services(_ ctx: UnsafeMutableRawPointer?) -> VaultServices {
    Unmanaged<VaultServices>.fromOpaque(ctx!).takeUnretainedValue()
}

private let vaultSecureEntry: @convention(c) (UnsafeMutableRawPointer?, UInt8, UInt64, UnsafeMutablePointer<UInt8>?, UnsafeMutablePointer<Int>?, UnsafeMutablePointer<UInt8>?, UnsafeMutablePointer<Int>?, Int) -> Int32 = { ctx, kind, timeout, a, aLen, b, bLen, cap in
    services(ctx).secureEntry(kind: kind, timeoutMs: timeout, a: a, aLen: aLen, b: b, bLen: bLen, cap: cap)
}

private let vaultRecoverySheet: @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<UInt8>?, Int, UnsafePointer<CChar>?, UnsafePointer<CChar>?, UnsafePointer<CChar>?, UInt64) -> Int32 = { ctx, words, len, checkpoint, recovery, reason, timeout in
    services(ctx).recoverySheet(words: words, len: len, checkpoint: checkpoint, recovery: recovery, reason: reason, timeoutMs: timeout)
}

private let vaultPresence: @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<CChar>?) -> Bool = { ctx, reason in
    services(ctx).presence(reason: reason.map { String(cString: $0) } ?? "SOURCE Vault")
}

private let vaultCaptureSuppressed: @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<CChar>?) -> Bool = { ctx, _ in
    services(ctx).captureSuppressed()
}

private let vaultEvent: @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<UInt8>?, Int) -> Void = { ctx, json, len in
    guard let json else { return }
    services(ctx).event(Data(bytes: json, count: len))
}
