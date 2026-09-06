import AppKit
import ApplicationServices

/// Remembers where dictation should land so focus can be restored
/// before typing. Mirrors FluidVoice TypingService's capture/restore.
struct CapturedFocusTarget {
    let pid: pid_t
    let bundleID: String?
    let isSecureField: Bool
}

func captureFocusedTarget() -> CapturedFocusTarget? {
    let systemWide = AXUIElementCreateSystemWide()
    var appRef: CFTypeRef?
    guard AXUIElementCopyAttributeValue(
        systemWide, kAXFocusedApplicationAttribute as CFString, &appRef
    ) == .success,
        let appElement = appRef as! AXUIElement?
    else {
        return nil
    }
    var pid: pid_t = 0
    guard AXUIElementGetPid(appElement, &pid) == .success, pid != 0 else {
        return nil
    }
    let bundleID = NSRunningApplication(processIdentifier: pid)?.bundleIdentifier
    return CapturedFocusTarget(
        pid: pid,
        bundleID: bundleID,
        isSecureField: focusedElementIsSecure(appElement: appElement)
    )
}

private func focusedElementIsSecure(appElement: AXUIElement) -> Bool {
    var focusedRef: CFTypeRef?
    guard AXUIElementCopyAttributeValue(
        appElement, kAXFocusedUIElementAttribute as CFString, &focusedRef
    ) == .success,
        let element = focusedRef as! AXUIElement?
    else {
        return false
    }
    var roleRef: CFTypeRef?
    guard AXUIElementCopyAttributeValue(
        element, kAXRoleAttribute as CFString, &roleRef
    ) == .success,
        let role = roleRef as? String
    else {
        return false
    }
    return role == "AXSecureTextField"
}

/// Best-effort refocus of the app that owned the field when dictation began.
@discardableResult
func restoreFocusTarget(_ target: CapturedFocusTarget) -> Bool {
    guard let app = NSRunningApplication(processIdentifier: target.pid) else {
        return false
    }
    if app.processIdentifier == ProcessInfo.processInfo.processIdentifier {
        return true
    }
    return app.activate()
}
