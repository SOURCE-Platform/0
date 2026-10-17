// SOURCE Vault (spec v0.5 §22.3, owner decision F2-D2): the vault's own
// iPhone app. The engine locks when the app leaves the foreground and when
// the iPhone locks (catalogue §1, §1.6).

import SwiftUI

@main
struct SourceVaultApp: App {
    @UIApplicationDelegateAdaptor(PushDelegate.self) private var pushDelegate
    @StateObject private var model = AppModel()
    @Environment(\.scenePhase) private var phase

    var body: some Scene {
        WindowGroup {
            if !DeviceFloor.isSupported {
                // §19 row 30: below the measured A15 floor the vault never opens.
                ContentUnavailableView("This iPhone isn't supported",
                    systemImage: "iphone.slash",
                    description: Text("SOURCE Vault needs an iPhone 13 or newer (A15 chip or later). Older models were not tested and cannot be used for the vault."))
            } else {
                ContentView()
                    .environmentObject(model)
                    .onAppear { model.start() }
                    // §22.10: the app-switcher snapshot shows nothing (review
                    // SEC-B4) — covered as soon as the app stops being active.
                    .overlay { if phase != .active { Color(.systemBackground).ignoresSafeArea() } }
            }
        }
        .onChange(of: phase) { _, now in
            guard DeviceFloor.isSupported else { return }
            switch now {
            case .background: model.leaveForeground()
            case .active: model.start(); model.refresh(); model.finishWaitingUpload(); model.checkApprovals()
            default: break
            }
        }
    }
}
