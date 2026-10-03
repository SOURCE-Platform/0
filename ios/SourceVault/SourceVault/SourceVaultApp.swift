// SOURCE Vault (spec v0.5 §22.3, owner decision F2-D2): the vault's own
// iPhone app. The engine locks when the app leaves the foreground and when
// the iPhone locks (catalogue §1, §1.6).

import SwiftUI

@main
struct SourceVaultApp: App {
    @StateObject private var model = AppModel()
    @Environment(\.scenePhase) private var phase

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(model)
                .onAppear { model.start() }
        }
        .onChange(of: phase) { _, now in
            switch now {
            case .background: model.lock()
            case .active: model.start(); model.refresh()
            default: break
            }
        }
    }
}
