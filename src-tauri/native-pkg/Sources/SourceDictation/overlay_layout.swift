import AppKit
import Foundation

/// The pill's size, surface style, and the frame of everything in it,
/// shared by ListeningIndicator and the live preview
/// (native-pkg/Preview), so layout tweaks reload in the preview exactly
/// as they will ship. A full pill: both ends are half circles, and the
/// pill is exactly as tall as an icon plus equal padding above and below.
/// The settings gear sits centered in the left end, the copy button in the
/// right end. The target-app icon straddles the top edge at the center and
/// the "Copied" bubble straddles the bottom edge, so the panel is taller
/// than the pill by half an icon above and half a bubble below. The waterfall
/// fills the pill edge to edge. Running words, when on, get a strip along
/// the bottom.
enum PillLayout {
    static let width: CGFloat = 172

    private static let iconSize: CGFloat = 24
    /// Padding around each end icon, on every side.
    private static let iconPadding: CGFloat = 10
    private static let baseHeight = iconPadding + iconSize + iconPadding
    /// How far the app icon rises above the pill's top edge.
    private static let appIconOverhang = iconSize / 2
    /// How far the "Copied" bubble (20pt tall) hangs below the bottom edge.
    private static let bubbleOverhang: CGFloat = 10
    /// Words stay clear of the rounded ends.
    private static let wordsInsetX: CGFloat = 30
    private static let wordsHeight: CGFloat = 26
    private static let wordsInset: CGFloat = 6
    private static let topInset: CGFloat = 6

    /// Height of the pill surface itself.
    static func height(wordsVisible: Bool) -> CGFloat {
        wordsVisible ? baseHeight + wordsInset + wordsHeight + 2 : baseHeight
    }

    /// The panel: the pill plus room for the top half of the app icon and
    /// the bottom half of the "Copied" bubble.
    static func panelSize(wordsVisible: Bool) -> NSSize {
        NSSize(width: width, height: bubbleOverhang + height(wordsVisible: wordsVisible) + appIconOverhang)
    }

    /// `surface`, `appIcon` and `copiedCenter` are in panel coordinates;
    /// the rest are in the surface's own coordinates.
    struct Frames {
        var surface: NSRect
        var appIcon: NSRect
        /// Center of the "Copied" bubble: the middle of the bottom edge.
        var copiedCenter: NSPoint
        var gear: NSRect
        var copy: NSRect
        var words: NSRect?
        var waterfall: NSRect
    }

    static func frames(wordsVisible: Bool) -> Frames {
        let height = height(wordsVisible: wordsVisible)
        let iconY = height - iconPadding - iconSize
        // The waterfall's front line sits right on the bottom edge, or
        // just above the words when they are on.
        var bottom: CGFloat = 0
        var words: NSRect?
        if wordsVisible {
            words = NSRect(x: wordsInsetX, y: wordsInset, width: width - wordsInsetX * 2, height: wordsHeight)
            bottom = wordsInset + wordsHeight + 2
        }
        return Frames(
            surface: NSRect(x: 0, y: bubbleOverhang, width: width, height: height),
            appIcon: NSRect(
                x: (width - iconSize) / 2,
                y: bubbleOverhang + height - iconSize / 2,
                width: iconSize,
                height: iconSize
            ),
            copiedCenter: NSPoint(x: width / 2, y: bubbleOverhang),
            gear: NSRect(x: iconPadding, y: iconY, width: iconSize, height: iconSize),
            copy: NSRect(x: width - iconPadding - iconSize, y: iconY, width: iconSize, height: iconSize),
            words: words,
            waterfall: NSRect(x: 0, y: bottom, width: width, height: height - topInset - bottom)
        )
    }

    // MARK: - Surface

    /// Hairline edge; brighter while hovered, so the pill still signals
    /// that it can be dragged.
    static func borderColor(hovered: Bool) -> CGColor {
        NSColor.white.withAlphaComponent(hovered ? 0.45 : 0.14).cgColor
    }

    static func styleSurface(_ surface: NSVisualEffectView) {
        surface.material = .hudWindow
        surface.state = .active
        surface.wantsLayer = true
        surface.layer?.masksToBounds = true
        surface.layer?.borderWidth = 1
        surface.layer?.borderColor = borderColor(hovered: false)
        applyShape(to: surface)
    }

    /// Rounds the surface into a full pill for its current size; call again
    /// whenever its height changes. The layer's corner radius rounds the
    /// border and the content but not the blurred material behind them,
    /// whose square corners showed on light backgrounds, so a mask of the
    /// same shape cuts the material (and so the window shadow) too.
    static func applyShape(to surface: NSVisualEffectView) {
        let size = surface.bounds.size
        let radius = size.height / 2
        surface.layer?.cornerRadius = radius
        surface.maskImage = NSImage(size: size, flipped: false) { rect in
            NSColor.black.setFill()
            NSBezierPath(roundedRect: rect, xRadius: radius, yRadius: radius).fill()
            return true
        }
    }
}

/// Shows the pill's end controls (settings gear, copy button) only while
/// the pointer is over the pill: they fade in on enter and out on leave,
/// so an idle pill is just the waterfall. Owns a tracking area on the
/// surface, which holds its owner weakly, so keep a reference to this.
final class PillHoverReveal: NSResponder {
    private let controls: [NSView]

    init(surface: NSView, controls: [NSView]) {
        self.controls = controls
        super.init()
        surface.addTrackingArea(NSTrackingArea(
            rect: .zero,
            options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect],
            owner: self,
            userInfo: nil
        ))
        setVisible(false, animated: false)
    }

    required init?(coder: NSCoder) {
        nil
    }

    override func mouseEntered(with event: NSEvent) {
        setVisible(true, animated: true)
    }

    override func mouseExited(with event: NSEvent) {
        setVisible(false, animated: true)
    }

    func setVisible(_ visible: Bool, animated: Bool) {
        let alpha: CGFloat = visible ? 1 : 0
        guard animated else {
            controls.forEach { $0.alphaValue = alpha }
            return
        }
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0.2
            controls.forEach { $0.animator().alphaValue = alpha }
        }
    }
}
