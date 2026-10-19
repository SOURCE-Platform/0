import AppKit
import AVFoundation

/// Live preview of the dictation pill — the spectrum waterfall, layout
/// and surface — on the real microphone, without SOURCE.
/// `npm run pill:preview` (scripts/pill-preview.sh) recompiles and
/// relaunches this whenever overlay_visualizers.swift, band_meter.swift
/// overlay_layout.swift or this file is saved, so visual tweaks show up
/// in seconds. It draws with the shipped view classes and PillLayout, so
/// it matches the real pill (words hidden).
/// Pass `--demo` (or deny the mic) for simulated speech instead.
@main
final class PillPreview: NSObject, NSApplicationDelegate, NSWindowDelegate {
    private static let width = PillLayout.width
    private static let size = PillLayout.panelSize(wordsVisible: false)
    private static let defaults = UserDefaults(suiteName: "source.pill-preview")

    private var window: NSPanel?
    private var hoverReveal: PillHoverReveal?
    private let spectrum = SpectrumSlots()
    private let horizon = PerspectiveWaveformView(frame: .zero)
    private let engine = AVAudioEngine()
    private let meter = BandMeter()
    private var lastBandsAt = Date.distantPast
    private var demoClock: Double = 0

    static func main() {
        let app = NSApplication.shared
        let delegate = PillPreview()
        app.delegate = delegate
        app.setActivationPolicy(.accessory)
        app.run()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        buildWindow()
        startFrames()
        if CommandLine.arguments.contains("--demo") {
            startDemo("--demo flag")
            return
        }
        AVCaptureDevice.requestAccess(for: .audio) { granted in
            DispatchQueue.main.async {
                granted ? self.startMic() : self.startDemo("microphone access denied")
            }
        }
    }

    // MARK: - Window

    private func buildWindow() {
        let panel = NSPanel(
            contentRect: NSRect(origin: savedOrigin(), size: Self.size),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.level = .floating
        panel.backgroundColor = .clear
        panel.isOpaque = false
        panel.hasShadow = true
        panel.isMovableByWindowBackground = true
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.delegate = self

        let frames = PillLayout.frames(wordsVisible: false)
        let container = NSView(frame: NSRect(origin: .zero, size: Self.size))
        let pill = NSVisualEffectView(frame: frames.surface)
        PillLayout.styleSurface(pill)
        container.addSubview(pill)

        horizon.frame = frames.waterfall
        pill.addSubview(horizon)

        // The real gear and copy controls, so hover states match the pill.
        let gear = GearControl(frame: frames.gear)
        gear.onClick = { print("pill preview: gear clicked") }
        pill.addSubview(gear)
        let copy = CopyControl(frame: frames.copy)
        copy.onCopy = {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString("Copied from the pill preview.", forType: .string)
            return true
        }
        pill.addSubview(copy)
        copy.bubbleHost = container
        copy.bubbleCenter = frames.copiedCenter
        hoverReveal = PillHoverReveal(surface: pill, controls: [gear, copy])

        // Small tag so the preview is never mistaken for the real pill.
        let label = NSTextField(labelWithString: "preview")
        label.font = NSFont.systemFont(ofSize: 8)
        label.textColor = .tertiaryLabelColor
        label.alignment = .center
        label.frame = NSRect(x: 40, y: frames.appIcon.minY - 11, width: Self.width - 80, height: 10)
        pill.addSubview(label)

        let appIcon = NSImageView(frame: frames.appIcon)
        appIcon.image = NSWorkspace.shared.icon(forFile: "/System/Library/CoreServices/Finder.app")
        appIcon.imageScaling = .scaleProportionallyDown
        container.addSubview(appIcon)

        panel.contentView = container
        panel.orderFrontRegardless()
        panel.invalidateShadow()
        window = panel
    }

    /// Remembered across relaunches, so each reload lands in the same spot.
    private func savedOrigin() -> NSPoint {
        if let x = Self.defaults?.object(forKey: "x") as? Double,
           let y = Self.defaults?.object(forKey: "y") as? Double {
            return NSPoint(x: x, y: y)
        }
        let screen = NSScreen.main?.frame ?? .zero
        return NSPoint(x: screen.midX - Self.width / 2, y: screen.maxY - 220)
    }

    func windowDidMove(_ notification: Notification) {
        guard let origin = window?.frame.origin else { return }
        Self.defaults?.set(Double(origin.x), forKey: "x")
        Self.defaults?.set(Double(origin.y), forKey: "y")
    }

    // MARK: - Frames (mirrors ListeningIndicator.startFrames)

    private func startFrames() {
        let timer = Timer(timeInterval: 1.0 / 30.0, repeats: true) { [weak self] _ in
            guard let self else { return }
            self.spectrum.tick()
            self.horizon.pushSnapshot(self.spectrum.levels)
        }
        RunLoop.main.add(timer, forMode: .common)
    }

    // MARK: - Input

    /// Same tap size and ~20 Hz throttle as MicSessionRecorder, so the
    /// bands arrive at the rate the real pill sees.
    private func startMic() {
        let input = engine.inputNode
        let format = input.outputFormat(forBus: 0)
        input.installTap(onBus: 0, bufferSize: 4096, format: format) { [weak self] buffer, _ in
            guard let self else { return }
            let now = Date()
            guard now.timeIntervalSince(self.lastBandsAt) > 0.05 else { return }
            self.lastBandsAt = now
            guard let bands = self.meter.bands(for: buffer) else { return }
            let copy = bands
            DispatchQueue.main.async { self.spectrum.pushBands(copy) }
        }
        do {
            try engine.start()
            print("pill preview: listening on the microphone")
        } catch {
            startDemo("mic failed to start: \(error.localizedDescription)")
        }
    }

    /// Simulated speech: words with pauses, voiced lows, a sibilant burst.
    private func startDemo(_ reason: String) {
        print("pill preview: simulated speech (\(reason))")
        Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
            guard let self else { return }
            self.demoClock += 0.1
            let t = self.demoClock.truncatingRemainder(dividingBy: 2.4)
            let word = (t > 0.25 && t < 0.95) || (t > 1.3 && t < 1.85)
            let envelope = word ? 0.55 + 0.45 * abs(sin(t * 2 * .pi * 2.6)) : 0
            let hiss = word && t > 1.55 && t < 1.75 ? 0.9 : 0.0
            let bands: [Float] = (0..<BandMeter.bandCount).map { band in
                let voiced = envelope * pow(0.7, Double(band)) * (0.8 + 0.2 * sin(t * 9 + Double(band)))
                return Float(min(1, max(voiced, hiss * Double(band) / 5)))
            }
            self.spectrum.pushBands(bands)
        }
    }
}

/// The helper's logger, which the shared controls call for hover/click
/// debug lines; the preview has no supervisor to send them to.
func writeDictationLine(_ line: String) {}
