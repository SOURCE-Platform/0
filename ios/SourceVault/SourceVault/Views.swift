// SOURCE Vault's screens: not yet paired, locked, the item list, an item,
// and the secure entry sheet. Everything secret is hidden while the screen
// is captured.

import SwiftUI

struct ContentView: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        NavigationStack {
            Group {
                if let recovery = model.recovery {
                    RecoverView(flow: recovery)
                } else {
                switch model.state {
                case "uninitialized":
                    if let flow = model.pairing() {
                        PairingView(flow: flow)
                            .safeAreaInset(edge: .bottom) {
                                Button("Lost every other device? Recover from backup") { model.startRecovery() }.font(.footnote).padding()
                            }
                    } else {
                        ContentUnavailableView("Not paired yet", systemImage: "qrcode", description: Text("Unlock this iPhone to pair it with your Mac."))
                    }
                case "locked", "error":
                    if model.removed != nil { RemovedView() } else { LockedView() }
                default:
                    ItemListView()
                }
                }
            }
            .navigationTitle("SOURCE Vault")
            .toolbar {
                if model.state != "uninitialized" && model.state != "locked" {
                    Button("Lock") { model.lock() }
                }
                if !["uninitialized", "locked", "error"].contains(model.state) && model.removed == nil {
                    NavigationLink(destination: SecurityView()) { Image(systemName: "lock.shield") }.accessibilityLabel("Security")
                }
            }
        }
        .safeAreaInset(edge: .top) {
            if model.state == "uninitialized", let message = model.message {
                Text(message).font(.callout).padding(8).frame(maxWidth: .infinity).background(.yellow.opacity(0.2))
            } else if model.sheetVoid {
                // §22.10: the app was interrupted before the change took effect.
                HStack {
                    Text("A Recovery Key change was interrupted. The new key you saw does not work — your earlier Recovery Key still does.").font(.callout)
                    Button("OK") { model.dismissVoidNotice() }
                }
                .padding(8).frame(maxWidth: .infinity).background(.yellow.opacity(0.2))
            }
        }
        .sheet(item: $model.entry) { SecureEntryView(request: $0) }
        .fullScreenCover(item: $model.sheet) { RecoveryKeyView(request: $0) }
        .overlay { CaptureShield() }
    }
}

struct LockedView: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "lock.fill").font(.largeTitle)
            Button("Unlock with Face ID") { model.unlockWithFaceID() }.buttonStyle(.borderedProminent)
            Button("Use master password") { model.unlockWithMasterPassword() }
            if let message = model.message { Text(message).foregroundStyle(.secondary) }
        }
    }
}

struct ItemListView: View {
    @EnvironmentObject var model: AppModel
    @State private var adding = false

    var body: some View {
        List(model.items) { item in
            NavigationLink(destination: ItemView(item: item)) {
                VStack(alignment: .leading) {
                    Text(item.title)
                    Text(item.username).font(.caption).foregroundStyle(.secondary)
                }
            }
        }
        .overlay { if model.behind { Text("Read-only until this iPhone catches up.").font(.caption) } }
        .safeAreaInset(edge: .bottom) { SyncBar() }
        .toolbar {
            if !model.behind && model.removed == nil {
                Button { adding = true } label: { Image(systemName: "plus") }.accessibilityLabel("Add login")
            }
        }
        .sheet(isPresented: $adding) { AddLoginView() }
    }
}

/// A new login, typed on the iPhone (F.2d step 4). The password field is
/// secure; everything is cleared when the sheet goes, and the sheet covers
/// itself for the app-switcher snapshot.
struct AddLoginView: View {
    @EnvironmentObject var model: AppModel
    @Environment(\.dismiss) private var dismiss
    @Environment(\.scenePhase) private var phase
    @State private var title = ""
    @State private var username = ""
    @State private var password = ""
    @State private var host = ""
    @State private var problem: String?
    @State private var saving = false

    var body: some View {
        NavigationStack {
            Form {
                TextField("Title", text: $title)
                TextField("Username", text: $username).textInputAutocapitalization(.never).autocorrectionDisabled()
                SecureField("Password", text: $password)
                TextField("Website (example.com)", text: $host).textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.URL)
                if let problem { Text(problem).foregroundStyle(.secondary) }
            }
            .navigationTitle("New Login")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") { save() }.disabled(title.isEmpty || password.isEmpty || saving)
                }
            }
        }
        .onDisappear { title = ""; username = ""; password = ""; host = "" }
        .overlay { if phase != .active { Color(.systemBackground).ignoresSafeArea() } }
    }

    private func save() {
        saving = true
        problem = nil
        model.addLogin(title: title, username: username, password: password, host: host) { error in
            saving = false
            if let error { problem = error } else { dismiss() }
        }
    }
}

/// "Sync now" and the last outcome (§22.8 triggers: unlock and this button).
struct SyncBar: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        HStack {
            if model.securityPending && !model.syncing {
                Text("A security change hasn't reached your backup yet. Keep this iPhone online and tap “Sync now”.").font(.caption)
                Spacer()
                Button("Sync now") { model.startSync(byUser: true) }.font(.caption)
            } else if model.accessLost {
                Text("Your backup no longer accepts this iPhone. It may have been removed, or your vault may have been recovered on another device. If you didn't do this, treat it as a security incident. Nothing has been deleted here.").font(.caption)
            } else if model.removed != nil {
                Text("Removal pending — your Mac removed this iPhone. Nothing has been deleted.").font(.caption)
            } else if model.syncing {
                ProgressView().controlSize(.small)
                Text("Syncing…").font(.caption)
            } else {
                Text(model.syncNote ?? model.lastSynced.map { "Synced with your Mac \($0.formatted(date: .omitted, time: .shortened))" } ?? "Not synced yet")
                    .font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button("Sync now") { model.startSync(byUser: true) }.font(.caption)
            }
        }
        .padding(.horizontal).padding(.vertical, 8)
        .background(.bar)
    }
}

/// §22.9: the Mac removed this iPhone — "removal pending" until the
/// backup corroborates it (F.2d). The vault stays readable, nothing is
/// deleted, and nothing here suggests deleting it: this may be the only
/// remaining copy (reviews SEC-I4 / VER-I2).
struct RemovedView: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "iphone.slash").font(.largeTitle)
            Text("Removal pending").font(.headline)
            Text("Your Mac says this iPhone was removed from your vault. You can still open the vault to read it — nothing has been deleted. Keep this iPhone's vault for now: it may hold the only copy of some items.")
                .font(.callout).foregroundStyle(.secondary).multilineTextAlignment(.center)
            Button("Open to read") { model.unlockWithFaceID() }
            RemoveVaultButton()
        }
        .padding()
    }
}

struct ItemView: View {
    @EnvironmentObject var model: AppModel
    @Environment(\.scenePhase) private var phase
    let item: ItemSummary
    @State private var password: String?
    @State private var problem: String?

    var body: some View {
        Form {
            LabeledContent("Username", value: item.username)
            if let password {
                LabeledContent("Password", value: password).privacySensitive()
            } else {
                Button("Show password") {
                    problem = nil
                    model.reveal(item.id) { answer in
                        // The engine's reveal answer: `{ref, kind, secret}` — a
                        // login's `{password}`, a card's `{number, expiry,
                        // cardholder}` (review VER-O3).
                        let secret = answer["secret"] as? [String: Any]
                        password = secret?["password"] as? String
                            ?? [secret?["number"], secret?["expiry"], secret?["cardholder"]].compactMap { $0 as? String }.joined(separator: " · ").nilIfEmpty
                        if password == nil { problem = AppModel.describe(answer["error"] as? String ?? "REFUSED") }
                    }
                }
                if let problem { Text(problem).foregroundStyle(.secondary) }
            }
        }
        .navigationTitle(item.title)
        .onDisappear { password = nil }
        // Gone before the app-switcher snapshot is taken (review SEC-B4).
        .onChange(of: phase) { _, now in if now != .active { password = nil } }
    }
}

/// Secure entry for the engine (catalogue §2): secure fields only; a new
/// master password is typed twice and compared here.
struct SecureEntryView: View {
    let request: EntryRequest
    @Environment(\.dismiss) private var dismiss
    @Environment(\.scenePhase) private var phase
    @State private var first = ""
    @State private var second = ""
    @State private var third = ""

    var body: some View {
        NavigationStack {
            Form {
                switch request.kind {
                case .masterPassword:
                    SecureField("Master password", text: $first)
                case .newMasterPassword:
                    SecureField("New master password", text: $first)
                    SecureField("Repeat it", text: $second)
                case .changeMasterPassword:
                    SecureField("Current master password", text: $first)
                    SecureField("New master password", text: $second)
                    SecureField("Repeat it", text: $third)
                case .recoveryKey:
                    SecureField("Recovery Key (24 words)", text: $first)
                case .adoptMasterPassword:
                    Section {
                        SecureField("Master password", text: $first)
                    } footer: {
                        Text("Another device changed your vault's security. Enter the master password your backup currently uses to apply the change.")
                    }
                }
            }
            .navigationTitle(request.kind == .adoptMasterPassword ? "Apply a Security Change" : "SOURCE Vault")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { request.cancel(); dismiss() } }
                ToolbarItem(placement: .confirmationAction) { Button("OK") { submit() }.disabled(!valid) }
            }
        }
        .interactiveDismissDisabled()
        .onDisappear { first = ""; second = ""; third = "" }
        // The sheet sits above the app's own cover: it covers itself for
        // the app-switcher snapshot too (review VER-O3, 0f5f21b).
        .overlay { if phase != .active { Color(.systemBackground).ignoresSafeArea() } }
        .overlay { CaptureShield() }
    }

    /// The Mac panel's rules: a new master password is at least 8
    /// characters and typed the same twice (the engine refuses others too).
    private var valid: Bool {
        switch request.kind {
        case .newMasterPassword: return first.count >= 8 && first == second
        case .changeMasterPassword: return !first.isEmpty && second.count >= 8 && second == third
        default: return !first.isEmpty
        }
    }

    private func submit() {
        request.kind == .changeMasterPassword ? request.submit(first, second) : request.submit(first)
        dismiss()
    }
}

/// Covers the app while the screen is recorded or mirrored (§14.4).
struct CaptureShield: View {
    @State private var captured = UIScreen.main.isCaptured

    var body: some View {
        Group {
            if captured { Color(.systemBackground).overlay { Text("Hidden while the screen is recorded.") } }
        }
        .onReceive(NotificationCenter.default.publisher(for: UIScreen.capturedDidChangeNotification)) { _ in
            captured = UIScreen.main.isCaptured
        }
    }
}

private extension String {
    var nilIfEmpty: String? { isEmpty ? nil : self }
}
