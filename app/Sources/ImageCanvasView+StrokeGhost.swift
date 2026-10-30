import AppKit

/// How an in-progress stroke's overlay is ghosted over the canvas.
enum StrokeGhostStyle {
    /// The overlay's own sampled pixels (a typed clone/heal).
    case sampled
    /// The overlay's alpha flooded with one colour.
    case wash(color: CGColor, alpha: CGFloat)
}

/// A finished typed stroke's ghost, kept up while its job runs: the
/// projection does not move until the job publishes, so without the pin the
/// stroke would vanish at mouse-up and reappear when the job ends.
struct StrokeGhostPin {
    let overlay: CGImage
    let style: StrokeGhostStyle
}

extension ImageCanvasView {
    /// The live stroke's ghost style; nil = this stroke draws no ghost.
    func liveStrokeGhostStyle() -> StrokeGhostStyle? {
        guard strokeOnMask || strokeHealKind == .spotHeal || precisionStrokePreview else { return nil }
        if precisionStrokePreview, tool == .clone || tool == .heal { return .sampled }
        let onCoverage = channelDisplay?.shows(paintTarget) == true
        // A spot-heal footprint ghosts BLUE: not coverage being painted, but
        // the region the inpaint replaces at mouse-up.
        let color: NSColor = strokeHealKind == .spotHeal ? .systemBlue
            : (onCoverage ? (tool == .eraser ? .black : .white) : .systemRed)
        // A coverage ghost has to read over a 50% wash, so it is drawn more
        // opaque than the ghost over a plain picture.
        return .wash(color: color.cgColor, alpha: onCoverage ? 0.85 : 0.5)
    }

    /// `endStroke`'s latch, just before the overlay is cleared: a typed
    /// stroke keeps its ghost until its job ends.
    func latchStrokeGhostPin() {
        guard precisionStrokePreview, let overlay = overlayContext?.makeImage(),
              let style = liveStrokeGhostStyle() else { return }
        strokeGhostPin = StrokeGhostPin(overlay: overlay, style: style)
    }

    /// Quick-mask-style ghost of an in-progress MASK stroke: a translucent
    /// red wash wherever the overlay has coverage. Painting a mask changes
    /// coverage, not color, so previewing the overlay's own white/black
    /// pixels would read as paint; and re-compositing the masked projection
    /// on every tick is deliberately not attempted. The true result appears
    /// when the stroke commits on mouse-up.
    func drawStrokeGhost(in context: CGContext) {
        let overlay: CGImage
        let style: StrokeGhostStyle
        if strokeActive {
            guard let live = overlayContext?.makeImage(), let liveStyle = liveStrokeGhostStyle() else { return }
            overlay = live
            style = liveStyle
        } else {
            guard let pin = strokeGhostPin else { return }
            overlay = pin.overlay
            style = pin.style
        }
        // The ghost's colour depends on what is underneath. Over the PICTURE
        // (a mask stroke with no wash on, the default) red at half alpha is
        // just "you painted here". But when the canvas is already showing the
        // very coverage being painted — a channel's rubylith, or that plane
        // or channel drawn as the grayscale base — red is the wash's own
        // colour, and a red stroke there reads as MORE mask exactly where the
        // brush is ADDING coverage. So the ghost is then drawn as the
        // coverage it paints: white for the brush, black for the eraser, the
        // vocabulary this whole feature uses. It sits over the wash rather
        // than under it, and commits into it at mouse-up.
        switch style {
        case .sampled:
            context.saveGState()
            context.setAlpha(0.65)
            drawFlipped(overlay, in: context)
            context.restoreGState()
        case .wash(let color, let alpha):
            context.saveGState()
            context.translateBy(x: 0, y: bounds.height)
            context.scaleBy(x: 1, y: -1)
            let rect = CGRect(origin: .zero, size: bounds.size)
            context.setAlpha(alpha)
            // Draw the stroke, then flood its alpha with the colour: sourceIn
            // keeps the fill only where the overlay covered something.
            context.beginTransparencyLayer(auxiliaryInfo: nil)
            context.draw(overlay, in: rect)
            context.setBlendMode(.sourceIn)
            context.setFillColor(color)
            context.fill(rect)
            context.endTransparencyLayer()
            context.restoreGState()
        }
    }
}
