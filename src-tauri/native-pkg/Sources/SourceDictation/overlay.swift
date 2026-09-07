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
    private var elapsedTimer: Timer?
    private var sessionStart: Date?
    private var wordsVisible = true

    private override init() {
        super.init()
    }

    // MARK: - Session lifecycle

    func show() {
        wordsVisible = Self.loadWordsVisible()
        sessionStart = Date()
        DispatchQueue.main.async {
            if self.panel == nil {
                self.build()
            }
            self.setTranscript("")
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
        gearView?.frame = NSRect(x: 17, y: 11, width: 18, height: 18)
        elapsedLabel?.frame = NSRect(x: width - 14 - 120, y: 11, width: 120, height: 17)
        // Generous click target in screen coordinates for the event-tap
        // click detector (AppKit mouse delivery to this panel is dead).
        Self.updateGearRect(NSRect(
            x: panel.frame.minX + 14, y: panel.frame.minY + 8, width: 24, height: 24
        ))
    }

    /// Screen-space click target of the gear, refreshed on every layout.
    /// Read by the event tap: a left-click inside it while a session is
    /// active opens O's dictation settings. Lock-guarded: written on the
    /// main thread, read on the tap thread.
    // Manually synchronized via gearLock (main thread writes, tap thread
    // reads), so this opts out of the concurrency checker explicitly.
    nonisolated(unsafe) private static var storedGearRect: NSRect?
    nonisolated(unsafe) private static let gearLock = NSLock()

    static func updateGearRect(_ rect: NSRect) {
        gearLock.lock()
        storedGearRect = rect
        gearLock.unlock()
    }

    static func currentGearRect() -> NSRect? {
        gearLock.lock()
        defer { gearLock.unlock() }
        return storedGearRect
    }

    private var gearView: NSImageView?

    // MARK: - Build

    private func build() {
        let width: CGFloat = 340
        guard let screen = NSScreen.main else { return }
        let panel = NSPanel(
            contentRect: NSRect(x: screen.frame.midX - width / 2, y: screen.frame.maxY - 220, width: width, height: 88),
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
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]

        let pill = NSVisualEffectView(frame: NSRect(x: 0, y: 0, width: width, height: 88))
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

        let gear = NSImageView(frame: .zero)
        gear.image = NSImage(systemSymbolName: "gearshape", accessibilityDescription: "Dictation settings")
        gear.imageScaling = .scaleProportionallyUpOrDown
        self.gearView = gear

        let elapsed = NSTextField(labelWithString: "0:00")
        elapsed.frame = .zero
        elapsed.alignment = .right
        elapsed.font = NSFont.monospacedDigitSystemFont(ofSize: 12, weight: .regular)
        elapsed.textColor = .secondaryLabelColor
        elapsed.backgroundColor = .clear
        elapsed.isBordered = false
        self.elapsedLabel = elapsed

        pill.addSubview(waveform)
        pill.addSubview(words)
        pill.addSubview(gear)
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
