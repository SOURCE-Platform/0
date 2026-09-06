import AppKit
import Foundation

/// Minimal session indicator: a small floating pill while dictation is
/// active. Temporary stand-in for the full live-words overlay (Phase E):
/// it answers "is it listening?" with zero transcription dependency.
/// All UI work hops to the main queue; the unchecked conformance covers
/// the shared singleton.
final class ListeningIndicator: @unchecked Sendable {
    static let shared = ListeningIndicator()

    private var panel: NSPanel?
    private var label: NSTextField?

    private init() {}

    func show() {
        DispatchQueue.main.async {
            if self.panel == nil {
                self.build()
            }
            self.panel?.orderFrontRegardless()
        }
    }

    func hide() {
        DispatchQueue.main.async {
            self.panel?.orderOut(nil)
        }
    }

    private func build() {
        let width: CGFloat = 300
        let height: CGFloat = 44
        guard let screen = NSScreen.main else { return }
        let x = screen.frame.midX - width / 2
        let y = screen.frame.maxY - height - 120
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
        pill.layer?.cornerRadius = 22
        pill.layer?.borderWidth = 2
        pill.layer?.borderColor = NSColor.systemYellow.cgColor

        let dot = NSView(frame: NSRect(x: 20, y: 14, width: 16, height: 16))
        dot.wantsLayer = true
        dot.layer?.cornerRadius = 8
        dot.layer?.backgroundColor = NSColor.systemRed.cgColor

        let label = NSTextField(labelWithString: "Listening…  (Right Option to finish)")
        label.frame = NSRect(x: 44, y: 0, width: width - 56, height: height)
        label.font = NSFont.systemFont(ofSize: 13, weight: .medium)
        label.textColor = .labelColor
        label.backgroundColor = .clear
        label.isBordered = false

        pill.addSubview(dot)
        pill.addSubview(label)
        panel.contentView = pill

        self.panel = panel
        self.label = label
    }
}
