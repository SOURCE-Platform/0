// Phase G (spec §7.2): this iPhone's APNs token, registered with the
// vault's provider through a request the engine builds and signs (the
// token and nothing else). A push only says "Approval request — open
// Source to review"; tapping it opens the app, which fetches the request
// itself (§7.1).
//
// Needs the `aps-environment` entitlement, which a free Apple account
// cannot sign: until the owner's paid account is set up (Phase J),
// registration fails quietly and approvals are seen when the app opens.

import UIKit
import UserNotifications

final class PushDelegate: NSObject, UIApplicationDelegate {
    static var engine: VaultEngine?

    func application(_ application: UIApplication, didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data) {
        let token = deviceToken.map { String(format: "%02x", $0) }.joined()
        guard let engine = Self.engine else { return }
        engine.call(["op": "push_register_request", "token": token]) { r in
            guard let origin = r["origin"] as? String, let method = r["method"] as? String, let path = r["path"] as? String,
                  let auth = r["auth"] as? String, let body = r["body"] as? String else { return }
            DispatchQueue.global().async {
                _ = ProviderHTTP.shared.send(origin: origin, method: method, path: path, auth: auth, body: Data(body.utf8), timeoutMs: 30_000, maxBody: 4096)
            }
        }
    }

    func application(_ application: UIApplication, didFailToRegisterForRemoteNotificationsWithError error: Error) {
        // No entitlement yet (free account): approvals still arrive when the app opens.
    }

    /// Ask once the vault is paired; alerts only (no silent or VoIP pushes).
    static func register(engine: VaultEngine) {
        self.engine = engine
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { granted, _ in
            guard granted else { return }
            DispatchQueue.main.async { UIApplication.shared.registerForRemoteNotifications() }
        }
    }
}
