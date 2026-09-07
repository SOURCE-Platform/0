import AppKit
import Foundation

/// Modifier-only Right Option toggle via a CGEvent tap.
/// keyCode 61 == Right Option. Fires on release after a clean press.
final class RightOptionHotkey {
    private let onToggle: () -> Void
    private var eventTap: CFMachPort?
    private var runLoopSource: CFRunLoopSource?
    private var rightOptionDown = false
    private var interrupted = false
    private var retryTimer: Timer?
    private var reportedUnavailable = false

    init(onToggle: @escaping () -> Void) {
        self.onToggle = onToggle
    }

    func start() {
        installTap()
        retryTimer = Timer.scheduledTimer(withTimeInterval: 5.0, repeats: true) { [weak self] _ in
            guard let self else { return }
            if self.eventTap == nil {
                self.installTap()
            }
        }
    }

    private func installTap() {
        if let tap = eventTap {
            CGEvent.tapEnable(tap: tap, enable: true)
            return
        }
        let mask = (1 << CGEventType.flagsChanged.rawValue)
            | (1 << CGEventType.keyDown.rawValue)
            | (1 << CGEventType.leftMouseDown.rawValue)
            | (1 << CGEventType.rightMouseDown.rawValue)
            | (1 << CGEventType.tapDisabledByTimeout.rawValue)
            | (1 << CGEventType.tapDisabledByUserInput.rawValue)
        guard let tap = CGEvent.tapCreate(
            tap: .cgSessionEventTap,
            place: .headInsertEventTap,
            options: .defaultTap,
            eventsOfInterest: CGEventMask(mask),
            callback: { _, _, event, refcon in
                guard let refcon else { return Unmanaged.passRetained(event) }
                let hotkey = Unmanaged<RightOptionHotkey>.fromOpaque(refcon).takeUnretainedValue()
                hotkey.handle(event: event)
                return Unmanaged.passRetained(event)
            },
            userInfo: Unmanaged.passUnretained(self).toOpaque()
        ) else {
            if !reportedUnavailable {
                reportedUnavailable = true
                writeDictationLine(
                    "ERROR {\"message\":\"event tap unavailable; grant Accessibility permission\", \"trusted\":\(AXIsProcessTrusted())}"
                )
            }
            return
        }
        reportedUnavailable = false
        eventTap = tap
        runLoopSource = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0)
        if let source = runLoopSource {
            CFRunLoopAddSource(CFRunLoopGetCurrent(), source, .commonModes)
        }
        CGEvent.tapEnable(tap: tap, enable: true)
        writeDictationLine("DEBUG tap installed")
    }

    private func handle(event: CGEvent) {
        if event.getIntegerValueField(.eventSourceUserData) == synthesizedEventTag {
            return
        }
        if event.type == .tapDisabledByTimeout || event.type == .tapDisabledByUserInput {
            if let tap = eventTap {
                CGEvent.tapEnable(tap: tap, enable: true)
                writeDictationLine("DEBUG tap re-enabled")
            }
            return
        }
        let type = event.type
        if type == .keyDown || type == .leftMouseDown || type == .rightMouseDown {
            interrupted = true
            return
        }
        guard type == .flagsChanged else { return }
        let keyCode = event.getIntegerValueField(.keyboardEventKeycode)
        let flags = event.flags
        if keyCode == 58 || keyCode == 61 {
            writeDictationLine("DEBUG key flagsChanged keyCode=\(keyCode) alt=\(flags.contains(.maskAlternate))")
        }
        if keyCode == 61 {
            if flags.contains(.maskAlternate) {
                rightOptionDown = true
                interrupted = false
            } else if rightOptionDown {
                rightOptionDown = false
                if !interrupted {
                    onToggle()
                }
                interrupted = false
            }
        } else if rightOptionDown {
            interrupted = true
        }
    }
}
