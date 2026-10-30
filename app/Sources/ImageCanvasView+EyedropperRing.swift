import AppKit

/// The Eyedropper's sampling ring: shown around the pointer from the
/// press to the release of an eyedropper gesture (the tool itself, or ⌥
/// borrowing it from the brush, fill or gradient tool) when the options
/// bar's Show sampling ring is on. The controller sets and clears it
/// (`EditorViewController+Eyedropper.swift`).
struct EyedropperRing {
    /// The sampled point, in canvas pixels.
    var point: CGPoint
    /// The colour under the pointer: the ring's upper half.
    var picked: NSColor
    /// The foreground as it was before the press: the lower half.
    var previous: NSColor
}

extension ImageCanvasView {
    /// A two-tone ring of constant screen size: the new colour above, the
    /// old one below, between a light and a dark hairline so it reads on
    /// any picture.
    func drawEyedropperRing() {
        guard let ring = eyedropperRing else { return }
        let scale = magnification
        let outer = 34 / scale, inner = 22 / scale
        let center = ring.point
        func half(_ color: NSColor, upper: Bool) {
            // The canvas is flipped (y down): the upper half on screen is
            // the arc through smaller y, angles 180° → 360°.
            let path = NSBezierPath()
            path.appendArc(withCenter: center, radius: outer,
                           startAngle: upper ? 180 : 0, endAngle: upper ? 360 : 180)
            path.appendArc(withCenter: center, radius: inner,
                           startAngle: upper ? 360 : 180, endAngle: upper ? 180 : 0,
                           clockwise: true)
            path.close()
            color.setFill()
            path.fill()
        }
        half(ring.picked.withAlphaComponent(1), upper: true)
        half(ring.previous.withAlphaComponent(1), upper: false)
        for (radius, color) in [(outer, NSColor.white), (inner, NSColor.black)] {
            let edge = NSBezierPath(ovalIn: CGRect(
                x: center.x - radius, y: center.y - radius, width: radius * 2, height: radius * 2))
            edge.lineWidth = 1 / scale
            color.withAlphaComponent(0.8).setStroke()
            edge.stroke()
        }
    }
}
