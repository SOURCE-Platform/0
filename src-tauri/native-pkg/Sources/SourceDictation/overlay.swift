import AppKit
import Foundation

/// Session pill: live waveform, running transcript, and a words on/off chip.
/// Stand-in for the full live-words overlay (Phase E): the transcript here
/// is display-only (partial re-transcriptions); the final TRANSCRIPT event
/// still carries the authoritative text. Words visibility persists in a tiny
/// JSON file so rebuilds don't reset it.
final class ListeningIndicator: NSObject, @unchecked Sendable {
    static let shared = ListeningIndicator()

    private var panel: NSPanel?
    private var chipPanel: NSPanel?
    private var dot: NSView?
    private var titleLabel: NSTextField?
    private var waveform: WaveformView?
    private var wordsLabel: NSTextField?
    private var chipButton: NSButton?
    private var wordsVisible = true

    private override init() {
        super.init()
        wordsVisible = Self.loadWordsVisible()
    }

    // MARK: - Session lifecycle

    func show() {
        DispatchQueue.main.async {
            if self.panel == nil {
                self.build()
            }
            self.setTranscript("")
            self.panel?.orderFrontRegardless()
            self.chipPanel?.orderFrontRegardless()
        }
    }

    func hide() {
        DispatchQueue.main.async {
            self.panel?.orderOut(nil)
            self.chipPanel?.orderOut(nil)
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
            self.layoutForWords()
        }
    }

    // MARK: - Words toggle

    func setWordsVisible(_ visible: Bool) {
        wordsVisible = visible
        Self.saveWordsVisible(visible)
        DispatchQueue.main.async {
            self.wordsLabel?.isHidden = !visible
            self.chipButton?.title = visible ? "Words ✓" : "Words"
            self.layoutForWords()
        }
    }

    private func layoutForWords() {
        guard let panel, let pill = panel.contentView else { return }
        let width = panel.frame.width
        let height: CGFloat = wordsVisible ? 108 : 64
        var frame = panel.frame
        let delta = height - frame.height
        frame.origin.y -= delta
        frame.size.height = height
        panel.setFrame(frame, display: true)
        pill.frame = NSRect(x: 0, y: 0, width: width, height: height)

        // Top row: red dot + title.
        dot?.frame = NSRect(x: 18, y: height - 30, width: 14, height: 14)
        titleLabel?.frame = NSRect(x: 40, y: height - 34, width: width - 52, height: 20)
        // Middle: waveform strip. Bottom (optional): running words.
        if wordsVisible {
            waveform?.frame = NSRect(x: 14, y: 36, width: width - 28, height: 24)
            wordsLabel?.frame = NSRect(x: 14, y: 8, width: width - 28, height: 26)
        } else {
            waveform?.frame = NSRect(x: 14, y: 8, width: width - 28, height: 22)
        }
        positionChip()
    }

    private func positionChip() {
        guard let panel, let chip = chipPanel else { return }
        var frame = chip.frame
        frame.origin.x = panel.frame.maxX - frame.width - 8
        frame.origin.y = panel.frame.minY - frame.height - 6
        chip.setFrameOrigin(frame.origin)
    }

    // MARK: - Build

    private func build() {
        let width: CGFloat = 340
        let height: CGFloat = 64
        guard let screen = NSScreen.main else { return }
        let x = screen.frame.midX - width / 2
        let y = screen.frame.maxY - height - 110
        let panel = NSPanel(
            contentRect: NSRect(x: x, y: y, width: width, height: height),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.isFloatingPanel = true
        panel.level = .floating
        panel.backgroundColor = .clear
        panel.isOpaque = false
        panel.hasShadow = true
        panel.ignoresMouseEvents = true
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]

        let pill = NSVisualEffectView(frame: NSRect(x: 0, y: 0, width: width, height: height))
        pill.material = .hudWindow
        pill.state = .active
        pill.wantsLayer = true
        pill.layer?.cornerRadius = 20
        pill.layer?.borderWidth = 2
        pill.layer?.borderColor = NSColor.systemYellow.cgColor

        let dot = NSView(frame: .zero)
        dot.wantsLayer = true
        dot.layer?.cornerRadius = 7
        dot.layer?.backgroundColor = NSColor.systemRed.cgColor
        self.dot = dot

        let title = NSTextField(labelWithString: "Listening…  (Right Option to finish)")
        title.frame = .zero
        title.font = NSFont.systemFont(ofSize: 13, weight: .medium)
        title.textColor = .labelColor
        title.backgroundColor = .clear
        title.isBordered = false
        self.titleLabel = title

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
        words.isHidden = !wordsVisible
        self.wordsLabel = words

        pill.addSubview(dot)
        pill.addSubview(title)
        pill.addSubview(waveform)
        pill.addSubview(words)
        panel.contentView = pill
        self.panel = panel

        buildChip(near: panel)
        layoutForWords()
    }

    private func buildChip(near panel: NSPanel) {
        let chip = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 92, height: 28),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        chip.isFloatingPanel = true
        chip.level = .floating
        chip.backgroundColor = .clear
        chip.isOpaque = false
        chip.hasShadow = true
        chip.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]

        let button = NSButton(title: wordsVisible ? "Words ✓" : "Words", target: nil, action: nil)
        button.frame = NSRect(x: 0, y: 0, width: 92, height: 28)
        button.bezelStyle = .rounded
        button.font = NSFont.systemFont(ofSize: 12, weight: .medium)
        button.target = self
        button.action = #selector(toggleWords)
        chip.contentView?.addSubview(button)
        self.chipButton = button
        self.chipPanel = chip
        positionChip()
    }

    @objc private func toggleWords() {
        setWordsVisible(!wordsVisible)
    }

    // MARK: - Prefs

    private static func prefsURL() -> URL {
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

    private static func saveWordsVisible(_ visible: Bool) {
        let json: [String: Any] = ["wordsVisible": visible]
        guard let data = try? JSONSerialization.data(withJSONObject: json) else { return }
        try? data.write(to: prefsURL())
    }
}

/// Scrolling bar visualizer fed by mic RMS levels (~20 Hz).
final class WaveformView: NSView {
    private var levels: [Float] = Array(repeating: 0, count: 56)

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
        NSColor.systemYellow.setFill()
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
