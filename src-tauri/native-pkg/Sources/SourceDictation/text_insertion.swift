import AppKit
import ApplicationServices

/// Tag on synthesized key events so the hotkey tap ignores its own typing.
let synthesizedEventTag: Int64 = 0x4656_5353

enum InsertionMethod: String {
    case accessibility
    case pasteboard
    case hid
}

struct InsertionOutcome {
    let ok: Bool
    let method: InsertionMethod?
    let message: String
}

/// Types text into the previously captured target.
/// Order: Accessibility insertion → clipboard paste → HID char fallback.
/// Pasteboard is captured and restored so the user's clipboard survives.
final class TextInserter {
    func insert(text: String, target: CapturedFocusTarget?) -> InsertionOutcome {
        guard !text.isEmpty else {
            return InsertionOutcome(ok: false, method: nil, message: "empty text")
        }
        guard AXIsProcessTrusted() else {
            return InsertionOutcome(ok: false, method: nil, message: "accessibility not trusted")
        }
        waitForModifiersToRelease(timeoutSeconds: 2.0)
        if let target {
            restoreFocusTarget(target)
            Thread.sleep(forTimeInterval: target.bundleID == "com.mitchellh.ghostty" ? 0.2 : 0.08)
        }
        if tryAccessibilityInsert(text: text) {
            return InsertionOutcome(ok: true, method: .accessibility, message: "ok")
        }
        if tryPasteboardInsert(text: text, pid: target?.pid) {
            return InsertionOutcome(ok: true, method: .pasteboard, message: "ok")
        }
        if tryHIDInsert(text: text) {
            return InsertionOutcome(ok: true, method: .hid, message: "ok")
        }
        return InsertionOutcome(ok: false, method: nil, message: "all methods failed")
    }

    // MARK: - Accessibility

    private func tryAccessibilityInsert(text: String) -> Bool {
        guard let element = focusedElement() else { return false }
        if insertViaSelectedRange(element: element, text: text) { return true }
        if setViaValue(element: element, text: text) { return true }
        return setViaSelectedText(element: element, text: text)
    }

    private func focusedElement() -> AXUIElement? {
        let systemWide = AXUIElementCreateSystemWide()
        var appRef: CFTypeRef?
        guard AXUIElementCopyAttributeValue(
            systemWide, kAXFocusedApplicationAttribute as CFString, &appRef
        ) == .success,
            let app = appRef as! AXUIElement?
        else {
            return nil
        }
        var elementRef: CFTypeRef?
        guard AXUIElementCopyAttributeValue(
            app, kAXFocusedUIElementAttribute as CFString, &elementRef
        ) == .success
        else {
            return nil
        }
        return (elementRef as! AXUIElement?)
    }

    private func insertViaSelectedRange(element: AXUIElement, text: String) -> Bool {
        var rangeRef: CFTypeRef?
        guard AXUIElementCopyAttributeValue(
            element, kAXSelectedTextRangeAttribute as CFString, &rangeRef
        ) == .success,
            let rangeValue = rangeRef as! AXValue?,
            AXValueGetType(rangeValue) == .cfRange
        else {
            return false
        }
        var range = CFRange()
        AXValueGetValue(rangeValue, .cfRange, &range)
        guard let selected = selectedText(element: element) else { return false }
        let head = String(selected.prefix(range.location))
        let tail = String(selected.dropFirst(range.location + range.length))
        let merged = head + text + tail
        guard setAttribute(element: element, attribute: kAXValueAttribute, value: merged as CFTypeRef) else {
            return false
        }
        var newRange = CFRange(location: range.location + (text as NSString).length, length: 0)
        guard let newValue = AXValueCreate(.cfRange, &newRange) else { return false }
        AXUIElementSetAttributeValue(
            element, kAXSelectedTextRangeAttribute as CFString, newValue
        )
        return true
    }

    private func selectedText(element: AXUIElement) -> String? {
        var textRef: CFTypeRef?
        guard AXUIElementCopyAttributeValue(
            element, kAXSelectedTextAttribute as CFString, &textRef
        ) == .success
        else {
            // Fall back to the full value when nothing is selected.
            var valueRef: CFTypeRef?
            guard AXUIElementCopyAttributeValue(
                element, kAXValueAttribute as CFString, &valueRef
            ) == .success
            else {
                return nil
            }
            return valueRef as? String
        }
        return textRef as? String
    }

    private func setViaValue(element: AXUIElement, text: String) -> Bool {
        setAttribute(element: element, attribute: kAXValueAttribute, value: text as CFTypeRef)
    }

    private func setViaSelectedText(element: AXUIElement, text: String) -> Bool {
        setAttribute(element: element, attribute: kAXSelectedTextAttribute, value: text as CFTypeRef)
    }

    private func setAttribute(element: AXUIElement, attribute: String, value: CFTypeRef) -> Bool {
        AXUIElementSetAttributeValue(element, attribute as CFString, value) == .success
    }

    // MARK: - Pasteboard

    private func tryPasteboardInsert(text: String, pid: pid_t?) -> Bool {
        let board = NSPasteboard.general
        let saved = board.pasteboardItems?.compactMap { item -> (types: [String], strings: [String: String])? in
            var strings: [String: String] = [:]
            for type in item.types {
                if let value = item.string(forType: type) {
                    strings[type.rawValue] = value
                }
            }
            return (item.types.map(\.rawValue), strings)
        }
        board.clearContents()
        board.setString(text, forType: .string)
        let pasted = postPasteKeystroke(pid: pid)
        // Restore after the target has had a chance to read the pasteboard.
        DispatchQueue.global().asyncAfter(deadline: .now() + 5.0) {
            board.clearContents()
            for entry in saved ?? [] {
                let item = NSPasteboardItem()
                for (type, value) in entry.strings {
                    item.setString(value, forType: NSPasteboard.PasteboardType(type))
                }
                board.writeObjects([item])
            }
        }
        return pasted
    }

    private func postPasteKeystroke(pid: pid_t?) -> Bool {
        guard let source = CGEventSource(stateID: .hidSystemState) else { return false }
        source.userData = synthesizedEventTag
        // Virtual key 9 == V with the command flag == Cmd+V.
        guard let down = CGEvent(keyboardEventSource: source, virtualKey: 9, keyDown: true),
            let up = CGEvent(keyboardEventSource: source, virtualKey: 9, keyDown: false)
        else {
            return false
        }
        down.flags = .maskCommand
        up.flags = .maskCommand
        down.setIntegerValueField(.eventSourceUserData, value: synthesizedEventTag)
        up.setIntegerValueField(.eventSourceUserData, value: synthesizedEventTag)
        if let pid {
            down.postToPid(pid)
            up.postToPid(pid)
        } else {
            down.post(tap: .cghidEventTap)
            up.post(tap: .cghidEventTap)
        }
        return true
    }

    // MARK: - HID fallback

    private func tryHIDInsert(text: String) -> Bool {
        guard let source = CGEventSource(stateID: .hidSystemState) else { return false }
        source.userData = synthesizedEventTag
        for scalar in text.unicodeScalars {
            var utf16 = Array(String(scalar).utf16)
            utf16.withUnsafeMutableBufferPointer { buffer in
                let event = CGEvent(
                    keyboardEventSource: source,
                    virtualKey: 0,
                    keyDown: true
                )
                event?.keyboardSetUnicodeString(
                    stringLength: buffer.count,
                    unicodeString: buffer.baseAddress!
                )
                event?.setIntegerValueField(.eventSourceUserData, value: synthesizedEventTag)
                event?.post(tap: .cghidEventTap)
                let release = CGEvent(
                    keyboardEventSource: source,
                    virtualKey: 0,
                    keyDown: false
                )
                release?.post(tap: .cghidEventTap)
            }
            Thread.sleep(forTimeInterval: 0.002)
        }
        return true
    }

    // MARK: - Helpers

    private func waitForModifiersToRelease(timeoutSeconds: TimeInterval) {
        let deadline = Date().addingTimeInterval(timeoutSeconds)
        while Date() < deadline {
            let flags = NSEvent.modifierFlags
            if flags.intersection([.command, .control, .option, .shift, .function]).isEmpty {
                return
            }
            Thread.sleep(forTimeInterval: 0.05)
        }
    }
}
