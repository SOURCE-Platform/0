// The services the engine calls back (catalogue §2): secure entry and the
// Recovery Key sheet (shown by the UI, the engine's thread waiting), the
// presence check, the capture check and events. Secrets cross only into
// the engine's own buffers, and the copies made here are zeroed.

import Foundation
import LocalAuthentication
import UIKit

/// What the UI is asked to collect (catalogue §2 kinds).
enum EntryKind: UInt8 {
    case newMasterPassword = 0, masterPassword = 1, changeMasterPassword = 2, recoveryKey = 3
    /// Adopting a key change made on another device (§2.7, F.2d).
    case adoptMasterPassword = 4
}

/// One secure-entry request waiting on the user.
final class EntryRequest: Identifiable {
    let id = UUID()
    let kind: EntryKind
    fileprivate let done = DispatchSemaphore(value: 0)
    fileprivate var first = Data()
    fileprivate var second = Data()
    fileprivate var submitted = false

    init(kind: EntryKind) { self.kind = kind }

    /// Called by the UI once, with the field contents as UTF-8.
    func submit(_ a: String, _ b: String = "") {
        first = Data(a.utf8)
        second = Data(b.utf8)
        submitted = true
        done.signal()
    }

    func cancel() {
        done.signal()
    }
}

final class VaultServices: @unchecked Sendable {
    /// Main-queue hooks set by the app model.
    var onEntry: (EntryRequest) -> Void = { $0.cancel() }
    var onEvent: ([String: Any]) -> Void = { _ in }
    /// Kept current from `UIScreen.capturedDidChangeNotification` so the
    /// engine's thread never has to wait on the main queue.
    private let captured = NSLock()
    private var isCaptured = false

    init() {
        NotificationCenter.default.addObserver(forName: UIScreen.capturedDidChangeNotification, object: nil, queue: .main) { [weak self] _ in
            self?.setCaptured(UIScreen.main.isCaptured)
        }
        DispatchQueue.main.async { self.setCaptured(UIScreen.main.isCaptured) }
    }

    private func setCaptured(_ value: Bool) {
        captured.lock(); isCaptured = value; captured.unlock()
    }

    func captureSuppressed() -> Bool {
        captured.lock(); defer { captured.unlock() }
        return !isCaptured
    }

    func secureEntry(kind: UInt8, timeoutMs: UInt64, a: UnsafeMutablePointer<UInt8>?, aLen: UnsafeMutablePointer<Int>?, b: UnsafeMutablePointer<UInt8>?, bLen: UnsafeMutablePointer<Int>?, cap: Int) -> Int32 {
        guard let kind = EntryKind(rawValue: kind), let a, let aLen, let b, let bLen else { return 1 }
        let request = EntryRequest(kind: kind)
        DispatchQueue.main.async { self.onEntry(request) }
        let waited = request.done.wait(timeout: .now() + .milliseconds(Int(min(timeoutMs, 600_000))))
        defer {
            request.first.resetBytes(in: 0..<request.first.count)
            request.second.resetBytes(in: 0..<request.second.count)
        }
        guard waited == .success, request.submitted, request.first.count <= cap, request.second.count <= cap else { return 1 }
        request.first.copyBytes(to: a, count: request.first.count)
        request.second.copyBytes(to: b, count: request.second.count)
        aLen.pointee = request.first.count
        bLen.pointee = request.second.count
        return 0
    }

    /// No F.2b phone op issues a Recovery Key (catalogue §4); a sheet that
    /// cannot be shown is never acknowledged.
    func recoverySheet(words: UnsafePointer<UInt8>?, len: Int, checkpoint: UnsafePointer<CChar>?, recovery: UnsafePointer<CChar>?, reason: UnsafePointer<CChar>?, timeoutMs: UInt64) -> Int32 {
        1
    }

    func presence(reason: String) -> Bool {
        let context = LAContext()
        let done = DispatchSemaphore(value: 0)
        var ok = false
        context.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason) { success, _ in
            ok = success
            done.signal()
        }
        done.wait()
        return ok
    }

    /// Never waits: the engine may call this from the thread that locks.
    func event(_ json: Data) {
        guard let event = (try? JSONSerialization.jsonObject(with: json)) as? [String: Any] else { return }
        DispatchQueue.main.async { self.onEvent(event) }
    }
}
