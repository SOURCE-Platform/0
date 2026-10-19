import AppKit
import Foundation

/// Temporary diagnostic: file logging (helper stdout never reaches the
/// unified log from a GUI launch). Records hover/drag events plus whether
/// the requested cursor is actually current afterwards.
func pillDiag(_ line: String) {
    let url = URL(fileURLWithPath: "/tmp/pillcursor.log")
    let entry = "\(Date().timeIntervalSince1970) \(line)\n"
    guard let data = entry.data(using: .utf8) else { return }
    if FileManager.default.fileExists(atPath: url.path) {
        if let handle = try? FileHandle(forWritingTo: url) {
            try? handle.seekToEnd()
            try? handle.write(contentsOf: data)
            try? handle.close()
        }
    } else {
        try? data.write(to: url)
    }
}

func pillCursorState(_ context: String) -> String {
    let current = NSCursor.currentSystem
    let name =
        current === NSCursor.openHand ? "openHand"
        : current === NSCursor.closedHand ? "closedHand"
        : current === NSCursor.arrow ? "arrow"
        : current === NSCursor.pointingHand ? "pointingHand" : "other"
    return "\(context) cursor=\(name)"
}

/// Pill surface: open-hand cursor on hover to signal draggability,
/// closed-hand cursor for the drag itself. Dragging moves the panel by
/// mouse delta, so grabs work from any empty area (visualizers, words,
/// background) while subviews like the gear keep their own clicks.
/// Hover uses an explicit tracking area (same pattern as GearControl):
/// cursor rects proved unreliable on this borderless panel.
final class PillBackgroundView: NSVisualEffectView {
    private var dragStartMouse: NSPoint?
    private var dragStartOrigin: NSPoint?
    private var trackingAreaRef: NSTrackingArea?
    private var cursorTimer: Timer?
    private var lastMovedDiagAt: TimeInterval = 0

    /// Belt and suspenders: the rect covers steady-state hover evaluated
    /// by AppKit on every move; the explicit sets below cover drag states.
    override func resetCursorRects() {
        super.resetCursorRects()
        addCursorRect(bounds, cursor: .openHand)
    }

    override func updateTrackingAreas() {
        if let trackingAreaRef {
            removeTrackingArea(trackingAreaRef)
        }
        let area = NSTrackingArea(
            rect: .zero,
            options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect],
            owner: self,
            userInfo: nil
        )
        addTrackingArea(area)
        trackingAreaRef = area
        super.updateTrackingAreas()
    }

    override func mouseEntered(with event: NSEvent) {
        _ = event
        writeDictationLine("DEBUG pill hover entered")
        startCursorForcing(.openHand)
        layer?.borderColor = PillLayout.borderColor(hovered: true)
        pillDiag("entered rects=\(window?.areCursorRectsEnabled ?? false) moved=\(window?.acceptsMouseMovedEvents ?? false) \(pillCursorState("after-set"))")
    }

    override func mouseExited(with event: NSEvent) {
        _ = event
        writeDictationLine("DEBUG pill hover exited")
        stopCursorForcing()
        NSCursor.arrow.set()
        layer?.borderColor = PillLayout.borderColor(hovered: false)
        pillDiag("exited \(pillCursorState("after-set"))")
    }

    /// Something on this panel reverts explicitly-set cursors (the border
    /// reacts to hover, so events arrive — the change just won't stick).
    /// Re-assert on every move plus a backstop timer while held/hovering.
    private func startCursorForcing(_ cursor: NSCursor) {
        stopCursorForcing()
        cursor.set()
        cursorTimer = Timer.scheduledTimer(withTimeInterval: 0.12, repeats: true) { _ in
            cursor.set()
        }
    }

    func stopCursorForcing() {
        cursorTimer?.invalidate()
        cursorTimer = nil
    }

    override func mouseMoved(with event: NSEvent) {
        _ = event
        if dragStartMouse != nil {
            NSCursor.closedHand.set()
        } else {
            NSCursor.openHand.set()
        }
        let now = Date().timeIntervalSince1970
        if now - lastMovedDiagAt > 3 {
            lastMovedDiagAt = now
            pillDiag("moved \(pillCursorState("after-set"))")
        }
    }

    override func mouseDown(with event: NSEvent) {
        _ = event
        writeDictationLine("DEBUG pill drag started")
        startCursorForcing(.closedHand)
        dragStartMouse = NSEvent.mouseLocation
        dragStartOrigin = window?.frame.origin
    }

    override func mouseDragged(with event: NSEvent) {
        _ = event
        guard let startMouse = dragStartMouse, let startOrigin = dragStartOrigin else { return }
        let current = NSEvent.mouseLocation
        window?.setFrameOrigin(NSPoint(
            x: startOrigin.x + current.x - startMouse.x,
            y: startOrigin.y + current.y - startMouse.y
        ))
    }

    override func mouseUp(with event: NSEvent) {
        dragStartMouse = nil
        dragStartOrigin = nil
        stopCursorForcing()
        let point = convert(event.locationInWindow, from: nil)
        NSCursor.openHand.set()
        if !bounds.contains(point) {
            NSCursor.arrow.set()
        }
    }
}
