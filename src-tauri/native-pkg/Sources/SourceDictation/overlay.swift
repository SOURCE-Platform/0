import AppKit
import Foundation

/// Session pill: a perspective waterfall of the live mic spectrum
/// (snapshots receding from a full-width front line into the distance),
/// the settings gear (opens O's dictation settings) in the left end, a
/// copy button for the last finished dictation in the right end, the
/// target-app icon straddling the top edge, and optional running words
/// along the bottom. Layout lives in PillLayout. Words visibility lives in
/// O's dictation settings and is read fresh at every session start, so no
/// pill UI is needed for it.
final class ListeningIndicator: NSObject, @unchecked Sendable {
    static let shared = ListeningIndicator()

    var onOpenSettings: (() -> Void)?

    private var panel: NSPanel?
    /// The pill body; the panel's content view is a clear container that
    /// also holds the app icon, which overhangs the pill's top edge.
    private var surface: PillBackgroundView?
    private var horizon: PerspectiveWaveformView?
    private let spectrum = SpectrumSlots()
    private var wordsLabel: NSTextField?
    private var appIconView: NSImageView?
    private var frameTimer: Timer?
    private var wordsVisible = true
    /// What the copy button copies: the last finished dictation, else the
    /// live words of the current one. Main thread only.
    private var lastFinishedText = ""
    private var liveText = ""

    private override init() {
        super.init()
    }

    // MARK: - Session lifecycle

    func show(focusTarget: CapturedFocusTarget? = nil) {
        wordsVisible = Self.loadWordsVisible()
        DispatchQueue.main.async {
            if self.panel == nil {
                self.build()
            }
            self.horizon?.reset()
            self.spectrum.reset()
            self.setTranscript("")
            self.setTargetApp(focusTarget)
            self.relayout()
            self.startFrames()
            self.panel?.orderFrontRegardless()
        }
    }

    func hide() {
        DispatchQueue.main.async {
            self.frameTimer?.invalidate()
            self.frameTimer = nil
            self.surface?.stopCursorForcing()
            if let origin = self.panel?.frame.origin {
                Self.savePanelOrigin(origin)
            }
            self.panel?.orderOut(nil)
        }
    }

    /// Per-band spectrum levels (0..1) from the mic's FFT meter, pushed on
    /// the audio tap thread. The frame timer turns them into display.
    func setBands(_ bands: [Float]) {
        DispatchQueue.main.async {
            self.spectrum.pushBands(bands)
        }
    }

    func setTranscript(_ text: String) {
        DispatchQueue.main.async {
            self.liveText = text
            guard self.wordsVisible else { return }
            self.wordsLabel?.stringValue = text
        }
    }

    /// The final text of a finished dictation (the transcript, then the
    /// inserted text when that arrives), kept for the copy button.
    func setLastFinished(_ text: String) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        DispatchQueue.main.async {
            self.lastFinishedText = trimmed
        }
    }

    private func copyLastDictation() -> Bool {
        let text = lastFinishedText.isEmpty ? liveText : lastFinishedText
        guard !text.isEmpty else { return false }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        writeDictationLine("DEBUG pill copied last dictation (\(text.count) chars)")
        return true
    }

    /// The app that owned the text field when dictation began — the same
    /// target insertion restores. Its icon sits on the top edge so a mid-
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

    // MARK: - Frames

    /// 30 fps display clock: ease the spectrum, then hand it to the
    /// waterfall as one snapshot. Common run-loop mode keeps it running
    /// while the pill is being dragged.
    private func startFrames() {
        frameTimer?.invalidate()
        let timer = Timer(timeInterval: 1.0 / 30.0, repeats: true) { [weak self] _ in
            guard let self else { return }
            self.spectrum.tick()
            self.horizon?.pushSnapshot(self.spectrum.levels)
        }
        RunLoop.main.add(timer, forMode: .common)
        frameTimer = timer
    }

    // MARK: - Layout

    private func relayout() {
        guard let panel, let container = panel.contentView, let surface else { return }
        // Grow or shrink downward: the top edge stays put.
        let size = PillLayout.panelSize(wordsVisible: wordsVisible)
        var frame = panel.frame
        frame.origin.y -= size.height - frame.height
        frame.size = size
        panel.setFrame(frame, display: true)
        container.frame = NSRect(origin: .zero, size: size)

        let frames = PillLayout.frames(wordsVisible: wordsVisible)
        surface.frame = frames.surface
        PillLayout.applyShape(to: surface)
        appIconView?.frame = frames.appIcon
        gearView?.frame = frames.gear
        copyView?.frame = frames.copy
        copyView?.bubbleHost = container
        copyView?.bubbleCenter = frames.copiedCenter
        wordsLabel?.isHidden = frames.words == nil
        wordsLabel?.frame = frames.words ?? .zero
        horizon?.frame = frames.waterfall
        panel.invalidateCursorRects(for: surface)
        panel.invalidateShadow()
    }

    private var gearView: GearControl?
    private var copyView: CopyControl?

    // MARK: - Build

    private func build() {
        let width = PillLayout.width
        guard let screen = NSScreen.main else { return }
        let defaultOrigin = NSPoint(x: screen.frame.midX - width / 2, y: screen.frame.maxY - 220)
        var origin = defaultOrigin
        if let saved = Self.loadPanelOrigin(), Self.originIsOnScreen(saved, size: PillLayout.panelSize(wordsVisible: true)) {
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

        let container = NSView(frame: NSRect(x: 0, y: 0, width: width, height: 88))
        let pill = PillBackgroundView(frame: NSRect(x: 0, y: 0, width: width, height: 88))
        PillLayout.styleSurface(pill)
        self.surface = pill

        let horizon = PerspectiveWaveformView(frame: .zero)
        self.horizon = horizon

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

        let copy = CopyControl(frame: .zero)
        copy.onCopy = { [weak self] in self?.copyLastDictation() ?? false }
        self.copyView = copy

        let appIcon = NSImageView(frame: .zero)
        appIcon.imageScaling = .scaleProportionallyDown
        appIcon.isHidden = true
        self.appIconView = appIcon

        pill.addSubview(horizon)
        pill.addSubview(words)
        pill.addSubview(gear)
        pill.addSubview(copy)
        container.addSubview(pill)
        container.addSubview(appIcon)
        panel.contentView = container
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
