import AppKit
import Foundation

/// Session pill: live waveform on top, running words beneath, bottom row
/// with a settings gear (opens O's dictation settings) and an elapsed
/// ticker. Words visibility lives in O's dictation settings and is read
/// fresh at every session start, so no pill UI is needed for it.
final class ListeningIndicator: NSObject, @unchecked Sendable {
    static let shared = ListeningIndicator()

    var onOpenSettings: (() -> Void)?

    private var panel: NSPanel?
    private var waveform: WaveformView?
    private var wordsLabel: NSTextField?
    private var elapsedLabel: NSTextField?
    private var appIconView: NSImageView?
    private var elapsedTimer: Timer?
    private var sessionStart: Date?
    private var wordsVisible = true

    private override init() {
        super.init()
    }

    // MARK: - Session lifecycle

    func show(focusTarget: CapturedFocusTarget? = nil) {
        wordsVisible = Self.loadWordsVisible()
        sessionStart = Date()
        DispatchQueue.main.async {
            if self.panel == nil {
                self.build()
            }
            self.setTranscript("")
            self.setTargetApp(focusTarget)
            self.updateElapsed()
            self.startTicker()
            self.panel?.orderFrontRegardless()
        }
    }

    func hide() {
        DispatchQueue.main.async {
            self.elapsedTimer?.invalidate()
            self.elapsedTimer = nil
            self.sessionStart = nil
            if let pillBackground = self.panel?.contentView as? PillBackgroundView {
                pillBackground.stopCursorForcing()
            }
            if let origin = self.panel?.frame.origin {
                Self.savePanelOrigin(origin)
            }
            self.panel?.orderOut(nil)
        }
    }

    func setLevel(_ level: Float) {
        let clamped = level
        DispatchQueue.main.async {
            self.waveform?.pushLevel(clamped)
        }
    }

    func setTranscript(_ text: String) {
        DispatchQueue.main.async {
            guard self.wordsVisible else { return }
            self.wordsLabel?.stringValue = text
        }
    }

    /// The app that owned the text field when dictation began — the same
    /// target insertion restores. Its icon sits center-bottom so a mid-
    /// dictation app switch never leaves doubt about where words land.
    /// Falls back to the frontmost app when AX capture found nothing, so
    /// a missing accessibility grant hides information instead of the icon.
    private func setTargetApp(_ target: CapturedFocusTarget?) {
        let selfPID = ProcessInfo.processInfo.processIdentifier
        let captured = target.flatMap { NSRunningApplication(processIdentifier: $0.pid) }
        let frontmost = NSWorkspace.shared.frontmostApplication
        let app = (captured ?? frontmost).flatMap {
            $0.processIdentifier == selfPID ? nil : $0
        }
        guard let app, let icon = app.icon else {
            writeDictationLine(
                "DEBUG pill target icon unavailable (captured pid: \(target.map(String.init(describing:)) ?? "none"))"
            )
            appIconView?.isHidden = true
            return
        }
        writeDictationLine(
            "DEBUG pill target icon: \(app.localizedName ?? "?") pid \(app.processIdentifier) bundle \(app.bundleIdentifier ?? "?")"
        )
        appIconView?.image = icon
        appIconView?.toolTip = app.localizedName.map { "Dictating into \($0)" }
        appIconView?.isHidden = false
    }

    // MARK: - Ticker

    private func startTicker() {
        elapsedTimer?.invalidate()
        elapsedTimer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { [weak self] _ in
            self?.updateElapsed()
        }
    }

    private func updateElapsed() {
        guard let start = sessionStart else {
            elapsedLabel?.stringValue = "0:00"
            return
        }
        let seconds = max(0, Int(Date().timeIntervalSince(start)))
        elapsedLabel?.stringValue = String(format: "%d:%02d", seconds / 60, seconds % 60)
    }

    // MARK: - Layout

    private func relayout() {
        guard let panel, let pill = panel.contentView else { return }
        let width = panel.frame.width
        // Waveform 26 + words 28 (optional) + bottom row 30 + paddings.
        let height: CGFloat = wordsVisible ? 116 : 88
        var frame = panel.frame
        let delta = height - frame.height
        frame.origin.y -= delta
        frame.size.height = height
        panel.setFrame(frame, display: true)
        pill.frame = NSRect(x: 0, y: 0, width: width, height: height)

        var cursor = height - 8
        waveform?.frame = NSRect(x: 14, y: cursor - 26, width: width - 28, height: 26)
        cursor -= 26 + 6
        if wordsVisible {
            wordsLabel?.isHidden = false
            wordsLabel?.frame = NSRect(x: 14, y: cursor - 26, width: width - 28, height: 26)
            cursor -= 26 + 6
        } else {
            wordsLabel?.isHidden = true
        }
        gearView?.frame = NSRect(x: 14, y: 8, width: 24, height: 24)
        appIconView?.frame = NSRect(x: (width - 24) / 2, y: 8, width: 24, height: 24)
        elapsedLabel?.frame = NSRect(x: width - 14 - 120, y: 11, width: 120, height: 17)
    }

    private var gearView: GearControl?

    // MARK: - Build

    private func build() {
        let width: CGFloat = 340
        guard let screen = NSScreen.main else { return }
        let defaultOrigin = NSPoint(x: screen.frame.midX - width / 2, y: screen.frame.maxY - 220)
        var origin = defaultOrigin
        if let saved = Self.loadPanelOrigin(), Self.originIsOnScreen(saved, size: NSSize(width: width, height: 88)) {
            origin = saved
        }
        let panel = NSPanel(
            contentRect: NSRect(origin: origin, size: NSSize(width: width, height: 88)),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.isFloatingPanel = true
        panel.level = .floating
        panel.backgroundColor = .clear
        panel.isOpaque = false
        panel.hasShadow = true
        panel.ignoresMouseEvents = false
        panel.acceptsMouseMovedEvents = true
        panel.isMovable = true
        panel.isMovableByWindowBackground = true
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]

        let pill = PillBackgroundView(frame: NSRect(x: 0, y: 0, width: width, height: 88))
        pill.material = .hudWindow
        pill.state = .active
        pill.wantsLayer = true
        pill.layer?.cornerRadius = 20
        pill.layer?.borderWidth = 2
        pill.layer?.borderColor = NSColor.systemGray.cgColor

        let waveform = WaveformView(frame: .zero)
        self.waveform = waveform

        let words = NSTextField(labelWithString: "")
        words.frame = .zero
        words.font = NSFont.systemFont(ofSize: 12)
        words.textColor = .secondaryLabelColor
        words.backgroundColor = .clear
        words.isBordered = false
        words.maximumNumberOfLines = 2
        words.cell?.wraps = true
        words.cell?.isScrollable = false
        self.wordsLabel = words

        let gear = GearControl(frame: .zero)
        gear.onClick = { [weak self] in self?.openSettings() }
        self.gearView = gear

        let elapsed = NSTextField(labelWithString: "0:00")
        elapsed.frame = .zero
        elapsed.alignment = .right
        elapsed.font = NSFont.monospacedDigitSystemFont(ofSize: 12, weight: .regular)
        elapsed.textColor = .secondaryLabelColor
        elapsed.backgroundColor = .clear
        elapsed.isBordered = false
        self.elapsedLabel = elapsed

        let appIcon = NSImageView(frame: .zero)
        appIcon.imageScaling = .scaleProportionallyDown
        appIcon.isHidden = true
        self.appIconView = appIcon

        pill.addSubview(waveform)
        pill.addSubview(words)
        pill.addSubview(gear)
        pill.addSubview(appIcon)
        pill.addSubview(elapsed)
        panel.contentView = pill
        self.panel = panel
        relayout()
    }

    func openSettings() {
        writeDictationLine("DEBUG gear clicked")
        onOpenSettings?()
    }

    // MARK: - Prefs (shared with O's dictation settings)

    static func prefsURL() -> URL {
        let home = FileManager.default.homeDirectoryForCurrentUser
        return home
            .appendingPathComponent(".observer_data", isDirectory: true)
            .appendingPathComponent("dictation-pill.json")
    }

    private static func loadWordsVisible() -> Bool {
        guard let data = try? Data(contentsOf: prefsURL()),
            let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            let value = json["wordsVisible"] as? Bool
        else {
            return true
        }
        return value
    }

    /// Pill position persists across helper restarts in the same prefs file
    /// O's settings already merges keys into (never overwrites it).
    private static func loadPanelOrigin() -> NSPoint? {
        guard let data = try? Data(contentsOf: prefsURL()),
            let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            let x = json["originX"] as? NSNumber,
            let y = json["originY"] as? NSNumber
        else {
            return nil
        }
        return NSPoint(x: x.doubleValue, y: y.doubleValue)
    }

    private static func savePanelOrigin(_ origin: NSPoint) {
        var json: [String: Any] =
            (try? Data(contentsOf: prefsURL()))
            .flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] } ?? [:]
        json["originX"] = origin.x
        json["originY"] = origin.y
        if let data = try? JSONSerialization.data(withJSONObject: json) {
            try? data.write(to: prefsURL())
        }
    }

    private static func originIsOnScreen(_ origin: NSPoint, size: NSSize) -> Bool {
        let rect = NSRect(origin: origin, size: size)
        return NSScreen.screens.contains { $0.frame.intersects(rect) }
    }
}

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
/// mouse delta, so grabs work from any empty area (waveform, words,
/// background) while subviews like the gear keep their own clicks.
/// Hover uses an explicit tracking area (same pattern as GearControl):
/// cursor rects proved unreliable on this borderless panel.
final class PillBackgroundView: NSVisualEffectView {
    private var dragStartMouse: NSPoint?
    private var dragStartOrigin: NSPoint?
    private var trackingAreaRef: NSTrackingArea?
    private var cursorTimer: Timer?
    private var lastMovedDiagAt: TimeInterval = 0

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
        layer?.borderColor = NSColor.white.cgColor
        pillDiag("entered rects=\(window?.areCursorRectsEnabled ?? false) moved=\(window?.acceptsMouseMovedEvents ?? false) \(pillCursorState("after-set"))")
    }

    override func mouseExited(with event: NSEvent) {
        _ = event
        writeDictationLine("DEBUG pill hover exited")
        stopCursorForcing()
        NSCursor.arrow.set()
        layer?.borderColor = NSColor.systemGray.cgColor
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

/// AppKit-native control for the gear. The two image layers crossfade so
/// the hover tint animates reliably even though NSImage tint is not animatable.
final class GearControl: NSView {
    var onClick: (() -> Void)?

    private let normalImage = NSImageView(frame: .zero)
    private let hoverImage = NSImageView(frame: .zero)
    private var trackingAreaRef: NSTrackingArea?
    private var lastClickAt = Date.distantPast

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        let symbol = NSImage(systemSymbolName: "gearshape", accessibilityDescription: "Dictation settings")
        for imageView in [normalImage, hoverImage] {
            imageView.image = symbol
            imageView.imageScaling = .scaleProportionallyUpOrDown
            addSubview(imageView)
        }
        normalImage.contentTintColor = .secondaryLabelColor
        hoverImage.contentTintColor = .white
        hoverImage.alphaValue = 0
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
        setAccessibilityLabel("Dictation settings")
    }

    required init?(coder: NSCoder) {
        nil
    }

    override func layout() {
        super.layout()
        let glyphFrame = bounds.insetBy(dx: 3, dy: 3)
        normalImage.frame = glyphFrame
        hoverImage.frame = glyphFrame
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

    override func resetCursorRects() {
        addCursorRect(bounds, cursor: .pointingHand)
    }

    override func mouseEntered(with event: NSEvent) {
        writeDictationLine("DEBUG gear hover entered")
        setHovered(true)
    }

    override func mouseExited(with event: NSEvent) {
        writeDictationLine("DEBUG gear hover exited")
        setHovered(false)
    }

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool {
        true
    }

    override func mouseDown(with event: NSEvent) {
        let now = Date()
        guard now.timeIntervalSince(lastClickAt) >= 1 else { return }
        lastClickAt = now
        onClick?()
    }

    private func setHovered(_ hovered: Bool) {
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0.2
            normalImage.animator().alphaValue = hovered ? 0 : 1
            hoverImage.animator().alphaValue = hovered ? 1 : 0
        }
    }
}

/// Scrolling bar visualizer fed by mic RMS levels (~20 Hz).
final class WaveformView: NSView {
    private var levels: [Float] = Array(repeating: 0, count: 112)

    func pushLevel(_ level: Float) {
        levels.removeFirst()
        levels.append(min(1.0, max(0, level)))
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let context = NSGraphicsContext.current?.cgContext else { return }
        context.clear(dirtyRect)
        let count = levels.count
        let slot = bounds.width / CGFloat(count)
        let barWidth = max(2, slot - 2)
        NSColor.white.setFill()
        for (index, level) in levels.enumerated() {
            let height = max(2, CGFloat(level) * bounds.height)
            let x = CGFloat(index) * slot + 1
            let y = (bounds.height - height) / 2
            let bar = NSBezierPath(
                roundedRect: NSRect(x: x, y: y, width: barWidth, height: height),
                xRadius: 1.5,
                yRadius: 1.5
            )
            bar.fill()
        }
    }
}
