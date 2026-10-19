import AppKit
import Foundation

/// Copy button for the pill's right end: a backup way to get the last
/// finished dictation onto the clipboard (say it landed in the wrong app).
/// `onCopy` does the copying and returns whether there was anything to
/// copy; a "Copied" bubble centered on `bubbleCenter` in `bubbleHost`
/// (the pill's bottom edge, in the panel) confirms either way,
/// and the glyph flips to a checkmark on success. Hover tint matches
/// GearControl.
final class CopyControl: NSView {
    var onCopy: (() -> Bool)?
    /// Where the bubble shows: a view that is not clipped to the pill, and
    /// the bubble's center in its coordinates.
    weak var bubbleHost: NSView?
    var bubbleCenter: NSPoint = .zero

    private static let copySymbol = makeCopyGlyph()
    private static let doneSymbol = NSImage(systemSymbolName: "checkmark", accessibilityDescription: "Copied")

    private let normalImage = NSImageView(frame: .zero)
    private let hoverImage = NSImageView(frame: .zero)
    private var trackingAreaRef: NSTrackingArea?
    private var bubble: CopiedBubble?
    private var resetWork: DispatchWorkItem?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        for imageView in [normalImage, hoverImage] {
            imageView.image = Self.copySymbol
            imageView.imageScaling = .scaleProportionallyUpOrDown
            addSubview(imageView)
        }
        normalImage.contentTintColor = .secondaryLabelColor
        hoverImage.contentTintColor = .white
        hoverImage.alphaValue = 0
        toolTip = "Copy last dictation"
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
        setAccessibilityLabel("Copy last dictation")
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
        setHovered(true)
    }

    override func mouseExited(with event: NSEvent) {
        setHovered(false)
    }

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool {
        true
    }

    override func mouseDown(with event: NSEvent) {
        let copied = onCopy?() ?? false
        showFeedback(copied: copied)
    }

    private func setHovered(_ hovered: Bool) {
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0.2
            normalImage.animator().alphaValue = hovered ? 0 : 1
            hoverImage.animator().alphaValue = hovered ? 1 : 0
        }
    }

    // MARK: - Feedback

    private func showFeedback(copied: Bool) {
        if copied {
            normalImage.image = Self.doneSymbol
            hoverImage.image = Self.doneSymbol
        }
        showBubble(copied ? "Copied" : "Nothing to copy yet")
        resetWork?.cancel()
        let work = DispatchWorkItem { [weak self] in
            guard let self else { return }
            self.normalImage.image = Self.copySymbol
            self.hoverImage.image = Self.copySymbol
            NSAnimationContext.runAnimationGroup { context in
                context.duration = 0.25
                self.bubble?.animator().alphaValue = 0
            }
        }
        resetWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.2, execute: work)
    }

    /// The white capsule, centered on `bubbleCenter`.
    private func showBubble(_ text: String) {
        guard let host = bubbleHost ?? superview else { return }
        let bubble = self.bubble.flatMap { $0.superview === host ? $0 : nil } ?? makeBubble(in: host)
        bubble.text = text
        let size = bubble.capsuleSize
        bubble.frame = NSRect(
            x: (bubbleCenter.x - size.width / 2).rounded(),
            y: (bubbleCenter.y - size.height / 2).rounded(),
            width: size.width,
            height: size.height
        )
        bubble.alphaValue = 1
    }

    private func makeBubble(in host: NSView) -> CopiedBubble {
        let bubble = CopiedBubble(frame: .zero)
        bubble.alphaValue = 0
        host.addSubview(bubble)
        self.bubble = bubble
        return bubble
    }

    /// Copy glyph from SF Symbols (the gear's family, so the stroke weight
    /// matches): one portrait sheet over another, mirrored so the back
    /// sheet's top and right edges show. A template image, so the tint
    /// colors apply like a plain symbol's.
    private static func makeCopyGlyph() -> NSImage? {
        guard let symbol = NSImage(
            systemSymbolName: "rectangle.portrait.on.rectangle.portrait",
            accessibilityDescription: "Copy last dictation"
        ) else {
            return NSImage(systemSymbolName: "doc.on.doc", accessibilityDescription: "Copy last dictation")
        }
        let image = NSImage(size: symbol.size, flipped: false) { rect in
            let mirror = NSAffineTransform()
            mirror.translateX(by: rect.width, yBy: 0)
            mirror.scaleX(by: -1, yBy: 1)
            mirror.concat()
            symbol.draw(in: rect)
            return true
        }
        image.isTemplate = true
        image.accessibilityDescription = "Copy last dictation"
        return image
    }
}

/// White capsule with its text centered both ways (a text field label
/// would sit high: it top-aligns its text).
private final class CopiedBubble: NSView {
    var text = "" {
        didSet { needsDisplay = true }
    }

    private static let attributes: [NSAttributedString.Key: Any] = [
        .font: NSFont.systemFont(ofSize: 11, weight: .medium),
        .foregroundColor: NSColor.black,
    ]

    var capsuleSize: NSSize {
        let textSize = (text as NSString).size(withAttributes: Self.attributes)
        return NSSize(width: ceil(textSize.width) + 18, height: 20)
    }

    override func draw(_ dirtyRect: NSRect) {
        NSColor.white.withAlphaComponent(0.92).setFill()
        let radius = bounds.height / 2
        NSBezierPath(roundedRect: bounds, xRadius: radius, yRadius: radius).fill()
        let textSize = (text as NSString).size(withAttributes: Self.attributes)
        (text as NSString).draw(
            at: NSPoint(x: (bounds.width - textSize.width) / 2, y: (bounds.height - textSize.height) / 2),
            withAttributes: Self.attributes
        )
    }
}
