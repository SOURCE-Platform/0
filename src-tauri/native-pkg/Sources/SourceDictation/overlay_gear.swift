import AppKit
import Foundation

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
