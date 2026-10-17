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
    /// `login`, `card` or `secure_note` (Phase I, §10.8 Choice A).
    var kind = "login"
}

final class AppModel: ObservableObject {
    @Published var state = "uninitialized"
    @Published var behind = false
    @Published var items: [ItemSummary] = []
    @Published var entry: EntryRequest?
    /// A new Recovery Key on screen (F.2d step 5).
    @Published var sheet: SheetRequest?
    /// §22.10: a Recovery Key sheet was shown for a change that never
    /// took effect; the earlier key still works.
    @Published var sheetVoid = false
    /// §22.10: a security change made here still waits to reach the
    /// backup; the banner stays until it does (review SEC-I3).
    @Published var securityPending = false
    /// §22.11: a fully staged upload is waiting; it finishes at the next
    /// foreground even while the vault is locked (F.2d chunk 3).
    func finishWaitingUpload() {
        engine?.call(["op": "get_state"]) { [weak self] answer in
            guard answer["upload_waiting"] as? Bool == true, answer["vault_open"] as? Bool != true else { return }
            self?.engine?.call(["op": "provider_publish", "interactive": false, "resume": true]) { _ in self?.refresh() }
        }
    }

    /// Total-loss recovery onto this iPhone, while it runs (F.2d chunk 3).
    @Published var recovery: RecoveryFlow?
    /// Phase G: requests from the Mac waiting for this iPhone's approval.
    @Published var showApprovals = false
    private(set) var approvals: ApprovalCenter?

    /// At every foreground: anything waiting for approval opens the sheet
    /// (a push only brings the app forward, §7.2).
    @MainActor func checkApprovals() {
        if approvals == nil, let engine {
            approvals = ApprovalCenter(engine: engine)
            if state != "uninitialized" { PushDelegate.register(engine: engine) }
        }
        guard let center = approvals, state != "uninitialized" else { return }
        Task {
            await center.check()
            if !center.waiting.isEmpty { showApprovals = true }
        }
    }
    @Published var message: String?
    /// §22.9: the Mac removed this iPhone (`published`: among its
    /// provider-committed entries); nil while still a vault device.
    @Published var removed: Bool?
    /// §11.3.2 corroborated: the backup no longer accepts this iPhone.
    @Published var accessLost = false
    @Published var syncing = false
    @Published var lastSynced: Date?
    @Published var syncNote: String?
    /// The sync in progress; a lock cancels it (review VER-O4).
    var syncTask: Task<Void, Never>?
    var syncID: UUID?

    private let services = VaultServices()
    private(set) var engine: VaultEngine?

    init() {
        services.onEntry = { [weak self] request in self?.entry = request }
        services.onSheet = { [weak self] request in self?.sheet = request }
        services.onSheetEnded = { [weak self] request in
            if self?.sheet?.id == request.id { self?.sheet = nil }
            self?.refresh()
        }
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

    @MainActor func startRecovery() {
        guard let engine else { return }
        recovery = RecoveryFlow(engine: engine, onFinishing: { [weak self] busy in self?.recoveryFinishing(busy) }) { [weak self] in
            self?.recovery = nil
            self?.refresh()
        }
    }

    // Owner decision 2026-10-13: once the new Recovery Key of a recovery
    // is confirmed, leaving the app lets the finalize finish (at most 30 s)
    // and locks right after, so the user always learns whether that key is
    // in effect (chunk-3 reviews VER-B2 / SEC-I1). Before the confirmation,
    // and when the iPhone itself locks, the vault locks at once.
    private var finishing = false
    private var holdLock = false
    private var deferredLock: DispatchWorkItem?
    private var backgroundTask = UIBackgroundTaskIdentifier.invalid

    private func recoveryFinishing(_ busy: Bool) {
        finishing = busy
        if busy { return }
        holdLock = false
        if backgroundTask != .invalid { UIApplication.shared.endBackgroundTask(backgroundTask); backgroundTask = .invalid }
        if let pending = deferredLock {
            pending.cancel()
            deferredLock = nil
            lock()
        }
    }

    /// The recovery's Recovery Key sheet was confirmed.
    func sheetConfirmed() {
        guard finishing else { return }
        holdLock = true
        backgroundTask = UIApplication.shared.beginBackgroundTask(withName: "vault-recovery-finalize") { [weak self] in self?.lock() }
    }

    /// The app left the foreground.
    func leaveForeground() {
        guard holdLock else { return lock() }
        guard deferredLock == nil else { return }
        let item = DispatchWorkItem { [weak self] in
            self?.deferredLock = nil
            self?.lock()
        }
        deferredLock = item
        DispatchQueue.main.asyncAfter(deadline: .now() + 30, execute: item)
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
        syncTask?.cancel()
        syncTask = nil
        syncID = nil
        syncing = false
        // Lock first: a sheet dismissed by the lock is then recorded as
        // void (§22.10), never taken for the user's own cancel.
        engine?.lock()
        entry?.cancel()
        entry = nil
        sheet?.cancel()
        sheet = nil
        items = []
    }

    func refresh() {
        engine?.call(["op": "get_state"]) { [weak self] answer in
            // LOCKED with a staged upload reports BACKING_UP (§13): for the
            // screens it is the lock screen — no key is resident.
            let open = answer["vault_open"] as? Bool == true
            let reported = answer["state"] as? String ?? "error"
            self?.state = reported == "backing_up" && !open ? "locked" : reported
            self?.behind = answer["behind"] as? Bool ?? false
            self?.removed = (answer["removed"] as? [String: Any]).map { $0["published"] as? Bool ?? false }
            self?.accessLost = answer["access_lost"] as? Bool == true
            self?.sheetVoid = answer["rk_sheet_void"] as? Bool == true
            if answer["vault_open"] as? Bool == true {
                self?.engine?.call(["op": "remote_update_status"]) { status in
                    let ops = status["ops"] as? [String] ?? []
                    self?.securityPending = status["pending"] as? Bool == true && ops.contains { ["revocation", "rk_replacement", "mp_change"].contains($0) }
                }
            }
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
        Task { @MainActor in startSync() }
    }

    func loadItems() {
        engine?.call(["op": "list_items"]) { [weak self] answer in
            let list = answer["items"] as? [[String: Any]] ?? []
            self?.items = list.map { ItemSummary(id: $0["ref"] as? String ?? "", title: $0["title"] as? String ?? "", username: $0["username"] as? String ?? "", kind: $0["kind"] as? String ?? "login") }
        }
    }

    /// One record's secret fields (crossing d), handed straight to the view.
    func reveal(_ ref: String, done: @escaping ([String: Any]) -> Void) {
        engine?.call(["op": "reveal", "ref": ref], done: done)
    }

    /// A new login (§1.5 `add_item`; Face ID confirms it in the engine),
    /// then a sync that publishes it — the user is here, so it may ask for
    /// the master password to apply a security change first (§2.7).
    func addLogin(title: String, username: String, password: String, host: String, done: @escaping (String?) -> Void) {
        var item: [String: Any] = ["op": "add_item", "kind": "login", "title": title, "username": username, "password": password]
        if !host.isEmpty { item["hosts"] = [host] }
        engine?.call(item) { [weak self] answer in
            guard answer["ok"] as? Bool == true else { return done(Self.describe(answer["error"] as? String ?? "REFUSED")) }
            done(nil)
            self?.loadItems()
            Task { @MainActor in self?.startSync(byUser: true) }
        }
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
            sheet?.cancel()
            sheet = nil
            items = []
            state = "locked"
        case "state":
            // BACKING_UP or RECOVERING say nothing about a resident key:
            // ask the engine (review SEC-O3 / VER-O1).
            let s = event["state"] as? String ?? state
            if s == "backing_up" || s == "recovering" { refresh() } else { state = s }
        default:
            break
        }
    }

    static func describe(_ error: String) -> String {
        switch error {
        case "PEER_AUTH_INVALID": return "Couldn't verify the answer from your Mac, so it was ignored. Try again later."
        case "PEER_NOT_PERMITTED": return "This iPhone can't sync with your Mac right now."
        case "PEER_LIMIT": return "Your Mac is busy — try again in a minute."
        case "FORMAT_INVALID": return "Your Mac didn't understand this iPhone's request. Update both apps."
        case "MP_ADOPTION_REQUIRED": return "Another device changed your vault's security. Tap “Sync now” and enter your master password to apply it."
        case "BACKUP_UNAVAILABLE": return "Your backup can't be reached right now. Your vault keeps working on this iPhone."
        case "RECORD_CORRUPT": return "A saved item is damaged and can't be decrypted, so this security change was stopped and nothing was lost. Restore or delete that item on your Mac, then try again."
        case "ROTATION_PENDING": return "Your last security change hasn't reached your backup yet. Tap “Sync now”, then try again."
        case "BACKUP_CONFLICT": return "Two devices changed the vault at once. Tap “Sync now” to try again."
        case "DEVICE_NOT_AUTHORIZED": return "This iPhone's key isn't available. Use your master password."
        case "INVALID_INPUT": return "Check the fields and try again."
        case "WRONG_CREDENTIAL": return "That password is not right."
        case "PRESENCE_DENIED": return "Face ID was cancelled."
        case "PANEL_CANCELLED": return "Cancelled."
        case "VAULT_BEHIND": return "This iPhone's vault is out of date; it opens read-only until it catches up."
        default: return "Something went wrong (\(error))."
        }
    }
}
