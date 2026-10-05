// SOURCE Vault's screens: not yet paired, locked, the item list, an item,
// and the secure entry sheet. Everything secret is hidden while the screen
// is captured.

import SwiftUI

struct ContentView: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        NavigationStack {
            Group {
                switch model.state {
                case "uninitialized":
                    if let flow = model.pairing() {
                        PairingView(flow: flow)
                    } else {
                        ContentUnavailableView("Not paired yet", systemImage: "qrcode", description: Text("Unlock this iPhone to pair it with your Mac."))
                    }
                case "locked", "error":
                    LockedView()
                default:
                    ItemListView()
                }
            }
            .navigationTitle("SOURCE Vault")
            .toolbar {
                if model.state != "uninitialized" && model.state != "locked" {
                    Button("Lock") { model.lock() }
                }
            }
        }
        .safeAreaInset(edge: .top) {
            if model.state == "uninitialized", let message = model.message {
                Text(message).font(.callout).padding(8).frame(maxWidth: .infinity).background(.yellow.opacity(0.2))
            }
        }
        .sheet(item: $model.entry) { SecureEntryView(request: $0) }
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
    }
}

struct ItemView: View {
    @EnvironmentObject var model: AppModel
    @Environment(\.scenePhase) private var phase
    let item: ItemSummary
    @State private var password: String?

    var body: some View {
        Form {
            LabeledContent("Username", value: item.username)
            if let password {
                LabeledContent("Password", value: password).privacySensitive()
            } else {
                Button("Show password") {
                    model.reveal(item.id) { answer in
                        let record = answer["record"] as? [String: Any]
                        password = record?["password"] as? String ?? answer["password"] as? String
                    }
                }
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
                }
            }
            .navigationTitle("SOURCE Vault")
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
