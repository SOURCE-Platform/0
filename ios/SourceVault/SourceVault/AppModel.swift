// SOURCE Vault's state (spec v0.5 §22.3): the engine, what it reports,
// the secure entry on screen, and the item list. Locks on background and
// when protected data goes away (catalogue §1); the engine itself enforces
// the auto-lock window.

import Foundation
import UIKit

struct ItemSummary: Identifiable {
    let id: String
    let title: String
    let username: String
}

final class AppModel: ObservableObject {
    @Published var state = "uninitialized"
    @Published var behind = false
    @Published var items: [ItemSummary] = []
    @Published var entry: EntryRequest?
    @Published var message: String?
    /// §22.9: the Mac removed this iPhone (`published`: among its
    /// provider-committed entries); nil while still a vault device.
    @Published var removed: Bool?
    @Published var syncing = false
    @Published var lastSynced: Date?
    @Published var syncNote: String?

    private let services = VaultServices()
    private(set) var engine: VaultEngine?

    init() {
        services.onEntry = { [weak self] request in self?.entry = request }
        services.onEvent = { [weak self] event in self?.handle(event) }
        NotificationCenter.default.addObserver(forName: UIApplication.protectedDataWillBecomeUnavailableNotification, object: nil, queue: .main) { [weak self] _ in
            self?.lock()
        }
    }

    /// The vault's directory: Application Support, complete file protection.
    static var vaultDir: URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("Vault", isDirectory: true)
    }

    /// Open the engine once protected data is available (catalogue §1).
    func start() {
        guard engine == nil, UIApplication.shared.isProtectedDataAvailable else { return }
        var dir = Self.vaultDir
        // §22.10: CompleteUnlessOpen (an open database finishes its write
        // after the phone locks), and never in iCloud or Finder backups.
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true, attributes: [.protectionKey: FileProtectionType.completeUnlessOpen])
        try? FileManager.default.setAttributes([.protectionKey: FileProtectionType.completeUnlessOpen], ofItemAtPath: dir.path)
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try? dir.setResourceValues(values)
        engine = VaultEngine(dir: dir, services: services)
        refresh()
    }

    /// A pairing flow on this engine; ends by refreshing the state.
    @MainActor func pairing() -> PairingFlow? {
        guard let engine else { return nil }
        return PairingFlow(engine: engine, onDone: { [weak self] in self?.refresh() }, onFailed: { [weak self] message in
            self?.items = []
            self?.message = message
            self?.refresh()
        })
    }

    func lock() {
        entry?.cancel()
        entry = nil
        engine?.lock()
        items = []
    }

    func refresh() {
        engine?.call(["op": "get_state"]) { [weak self] answer in
            self?.state = answer["state"] as? String ?? "error"
            self?.behind = answer["behind"] as? Bool ?? false
            self?.removed = (answer["removed"] as? [String: Any]).map { $0["published"] as? Bool ?? false }
            if answer["vault_open"] as? Bool == true { self?.loadItems() }
        }
    }

    /// Face ID opens this iPhone's envelope; the master password is the
    /// fallback the engine offers when Face ID cannot be used (§22.4).
    func unlockWithFaceID() {
        run(["op": "unlock"]) { [weak self] answer in
            if answer["error"] as? String == "DEVICE_NOT_AUTHORIZED" { self?.unlockWithMasterPassword() }
            if answer["ok"] as? Bool == true { self?.syncAfterUnlock() }
        }
    }

    func unlockWithMasterPassword() {
        run(["op": "begin_recovery_unlock", "kind": "mp"]) { [weak self] answer in
            if answer["ok"] as? Bool == true { self?.syncAfterUnlock() }
        }
    }

    /// §22.8 triggers: unlock and "Sync now" — never in the background
    /// (the app locks when it leaves the foreground).
    private func syncAfterUnlock() {
        Task { @MainActor in await syncWithMac() }
    }

    func loadItems() {
        engine?.call(["op": "list_items"]) { [weak self] answer in
            let list = answer["items"] as? [[String: Any]] ?? []
            self?.items = list.map { ItemSummary(id: $0["ref"] as? String ?? "", title: $0["title"] as? String ?? "", username: $0["username"] as? String ?? "") }
        }
    }

    /// One record's secret fields (crossing d), handed straight to the view.
    func reveal(_ ref: String, done: @escaping ([String: Any]) -> Void) {
        engine?.call(["op": "reveal", "ref": ref], done: done)
    }

    private func run(_ request: [String: Any], then: (([String: Any]) -> Void)? = nil) {
        message = nil
        engine?.call(request) { [weak self] answer in
            if answer["ok"] as? Bool != true, let error = answer["error"] as? String {
                self?.message = Self.describe(error)
            }
            then?(answer)
            self?.refresh()
        }
    }

    private func handle(_ event: [String: Any]) {
        switch event["event"] as? String {
        case "locked":
            // Catalogue §1: dismiss any secure entry still on screen.
            entry?.cancel()
            entry = nil
            items = []
            state = "locked"
        case "state":
            state = event["state"] as? String ?? state
        default:
            break
        }
    }

    static func describe(_ error: String) -> String {
        switch error {
        case "PEER_AUTH_INVALID": return "Couldn't verify the answer from your Mac. Nothing was changed."
        case "PEER_NOT_PERMITTED": return "Your Mac no longer lets this iPhone sync."
        case "PEER_LIMIT": return "Your Mac is busy — try again in a minute."
        case "WRONG_CREDENTIAL": return "That password is not right."
        case "PRESENCE_DENIED": return "Face ID was cancelled."
        case "PANEL_CANCELLED": return "Cancelled."
        case "VAULT_BEHIND": return "This iPhone's vault is out of date; it opens read-only until it catches up."
        default: return "Something went wrong (\(error))."
        }
    }
}
