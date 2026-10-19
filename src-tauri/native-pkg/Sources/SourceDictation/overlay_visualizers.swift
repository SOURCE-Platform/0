import AppKit
import Foundation

/// The live spectrum as a row of levels — the six FFT bands from
/// BandMeter spread over `count` slots mirrored around the center (lows
/// in the middle, highs at the edges), interpolating between neighboring
/// bands so the row reads smooth. Bands arrive at the mic tap's rate;
/// each `tick()` eases every slot toward them, so the 30 fps snapshots
/// stay smooth even when the mic delivers less often.
final class SpectrumSlots {
    static let count = 25
    private var bands = [Float](repeating: 0, count: BandMeter.bandCount)
    private(set) var levels = [CGFloat](repeating: 0, count: count)

    func pushBands(_ newBands: [Float]) {
        for index in bands.indices where index < newBands.count {
            bands[index] = min(1, max(0, newBands[index]))
        }
    }

    /// One display frame: ease every slot halfway toward its target.
    func tick() {
        for slot in levels.indices {
            levels[slot] += (target(for: slot) - levels[slot]) * 0.5
        }
    }

    func reset() {
        bands = [Float](repeating: 0, count: bands.count)
        levels = [CGFloat](repeating: 0, count: levels.count)
    }

    /// A slot's distance from the center, scaled into band space (center =
    /// lowest band, edge = highest), linearly interpolated between the two
    /// nearest bands. Mirrored slots sit at equal distances, so symmetry
    /// holds at any slot count.
    private func target(for slot: Int) -> CGFloat {
        let center = CGFloat(Self.count - 1) / 2
        let bandSpan = CGFloat(BandMeter.bandCount - 1)
        let position = abs(CGFloat(slot) - center) / center * bandSpan
        let low = min(Int(position.rounded(.down)), BandMeter.bandCount - 1)
        let high = min(BandMeter.bandCount - 1, low + 1)
        let blend = position - CGFloat(Int(position.rounded(.down)))
        let a = CGFloat(bands[low])
        let b = CGFloat(bands[high])
        return a + (b - a) * blend
    }
}

/// Spectrum waterfall receding into the distance: 30 times a second the
/// spectrum row is snapshotted as one contour line. New lines span the
/// view edge to edge along its bottom, then travel up toward the top
/// center, narrowing to nothing, shrinking in height, and fading out — so the last second or so of speech reads as a landscape of
/// its spectrum. A line is as bright as its sound, its quiet stretches
/// dimmer, and silence draws nothing. Equal time steps bunch toward the back.
final class PerspectiveWaveformView: NSView {
    /// ~1.6 s of history at 30 fps.
    private static let historyCount = 48
    /// Tallest peak on the front line, as a fraction of the view height.
    private static let peakHeight: CGFloat = 0.42
    /// How far the front line starts below the bottom edge, as a fraction
    /// of the tallest peak. Hides the empty dark band under the nearest
    /// lines, so the view starts on the lines themselves.
    private static let sinkBelowEdge: CGFloat = 0.5
    /// Levels below `quiet` draw nothing; at `loud` and above, full white.
    private static let quiet: CGFloat = 0.04
    private static let loud: CGFloat = 0.3
    /// Quiet stretches of a line that has sound somewhere still show at
    /// this share of the line's brightness, so every line reaches both
    /// edges and the waterfall rises from the bottom corners.
    private static let quietFloor: CGFloat = 0.35
    private var snapshots = [[CGFloat]](
        repeating: [CGFloat](repeating: 0, count: SpectrumSlots.count),
        count: historyCount
    )

    func pushSnapshot(_ levels: [CGFloat]) {
        snapshots.removeFirst()
        snapshots.append(levels)
        needsDisplay = true
    }

    func reset() {
        snapshots = [[CGFloat]](
            repeating: [CGFloat](repeating: 0, count: SpectrumSlots.count),
            count: Self.historyCount
        )
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let context = NSGraphicsContext.current?.cgContext else { return }
        context.clear(dirtyRect)
        context.setLineCap(.butt)
        context.setLineJoin(.round)

        // Slot x positions on the front line: left edge to right edge.
        let last = CGFloat(SpectrumSlots.count - 1)
        let foregroundX = (0..<SpectrumSlots.count).map { bounds.width * CGFloat($0) / last }
        let amplitude = bounds.height * Self.peakHeight
        let sink = amplitude * Self.sinkBelowEdge
        let topY = bounds.height - 2

        // Oldest (back) first so nearer lines paint over. Depth 0 is the
        // foreground; depth 1 is the far end, where lines have faded out.
        let oldest = CGFloat(snapshots.count - 1)
        for (index, levels) in snapshots.enumerated() {
            let depth = 1 - CGFloat(index) / oldest
            let closeness = 1 - depth
            let fade = pow(closeness, 1.3)
            guard fade > 0.01, (levels.max() ?? 0) > Self.quiet else { continue }
            // The front line starts `sink` below the bottom edge (clipped
            // by the view); farther lines climb toward the top.
            let baseY = -sink + (topY + sink) * (1 - pow(closeness, 1.8))
            let scale = pow(closeness, 1.15)
            let points = levels.enumerated().map { slot, level in
                CGPoint(
                    x: bounds.midX + (foregroundX[slot] - bounds.midX) * scale,
                    y: baseY + level * amplitude * scale
                )
            }
            context.setLineWidth(max(0.5, 1.25 * scale))
            let floor = Self.quietFloor * Self.presence(levels.max() ?? 0)
            func step(_ slot: Int) -> CGFloat {
                (max(Self.presence(levels[slot]), floor) * 12).rounded()
            }
            // Brightness steps in twelfths; neighboring pieces on the same
            // step stroke as one path, so joins show no antialiasing seams.
            var slot = 0
            while slot < levels.count {
                let current = step(slot)
                var end = slot
                while end + 1 < levels.count, step(end + 1) == current {
                    end += 1
                }
                let alpha = fade * current / 12
                if alpha > 0.01 {
                    context.setStrokeColor(NSColor.white.withAlphaComponent(alpha).cgColor)
                    for piece in slot...end {
                        Self.addSegment(around: piece, of: points, to: context, continuing: piece > slot)
                    }
                    context.strokePath()
                }
                slot = end + 1
            }
        }
    }

    /// 0 at or below `quiet`, 1 at or above `loud`, eased between.
    private static func presence(_ level: CGFloat) -> CGFloat {
        let t = min(1, max(0, (level - quiet) / (loud - quiet)))
        return t * t * (3 - 2 * t)
    }

    /// The piece of the smoothed contour belonging to one bar top: a
    /// quadratic from the midpoint with its left neighbor to the midpoint
    /// with its right one, the top itself as control point. Consecutive
    /// pieces join into one smooth curve, each with its own brightness.
    private static func addSegment(
        around slot: Int,
        of points: [CGPoint],
        to context: CGContext,
        continuing: Bool
    ) {
        let point = points[slot]
        let start = slot > 0 ? midpoint(points[slot - 1], point) : point
        let end = slot < points.count - 1 ? midpoint(point, points[slot + 1]) : point
        if !continuing {
            context.move(to: start)
        }
        context.addQuadCurve(to: end, control: point)
    }

    private static func midpoint(_ a: CGPoint, _ b: CGPoint) -> CGPoint {
        CGPoint(x: (a.x + b.x) / 2, y: (a.y + b.y) / 2)
    }
}
