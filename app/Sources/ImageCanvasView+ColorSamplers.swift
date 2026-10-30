import AppKit

/// One Color Sampler's marker on the canvas: `label` is the 1-based
/// number the Info panel prints, `point` the sampled pixel's centre, and
/// `after` the preview's RGB while a tone sheet is up.
struct ColorSamplerMarker: Equatable {
    let label: Int
    let point: CGPoint
    let after: String?
}

/// A press on a marker with the Color Sampler tool: the drag moves sampler
/// `index`, and a release outside the canvas removes it.
struct ColorSamplerDragSession {
    let index: Int
}

/// The canvas half of the Color Sampler: hit testing, the mouse
/// routing `mouseDown` / `mouseDragged` / `mouseUp` ask first, and the
/// markers. The canvas only reports points; the controller owns the list
/// (`EditorViewController+ColorSamplers.swift`).
extension ImageCanvasView {
    /// The hit radius in screen points, whatever the zoom.
    private static let colorSamplerHitPoints: CGFloat = 7

    /// The marker under `point` (canvas pixels), topmost first.
    func colorSamplerHit(_ point: CGPoint) -> Int? {
        let radius = Self.colorSamplerHitPoints / magnification
        return colorSamplerMarkers.indices.last { index in
            let marker = colorSamplerMarkers[index].point
            return hypot(marker.x - point.x, marker.y - point.y) <= radius
        }
    }

    private func colorSamplerInsideCanvas(_ point: CGPoint) -> Bool {
        point.x >= 0 && point.y >= 0 && point.x < bounds.width && point.y < bounds.height
    }

    /// True when the press is the Color Sampler's: with the tool, ⌥ on a
    /// marker removes it, a press on one starts its drag and any other
    /// press inside the canvas adds a sampler; with the Eyedropper, ⇧-click
    /// adds one. The point is UNCLAMPED, like the eyedropper's.
    func colorSamplerMouseDown(_ event: NSEvent) -> Bool {
        let raw = convert(event.locationInWindow, from: nil)
        if tool == .eyedropper, event.modifierFlags.contains(.shift) {
            if colorSamplerInsideCanvas(raw) { onColorSamplerAdd?(raw) }
            return true
        }
        guard tool == .colorSampler else { return false }
        if let index = colorSamplerHit(raw) {
            if event.modifierFlags.contains(.option) {
                onColorSamplerRemove?(index)
            } else {
                colorSamplerDrag = ColorSamplerDragSession(index: index)
            }
            return true
        }
        if colorSamplerInsideCanvas(raw) { onColorSamplerAdd?(raw) }
        return true
    }

    /// A drag tick: inside the canvas the sampler follows the pointer.
    func colorSamplerMouseDragged(_ raw: CGPoint) -> Bool {
        guard let drag = colorSamplerDrag else { return false }
        if colorSamplerInsideCanvas(raw) { onColorSamplerMove?(drag.index, raw) }
        return true
    }

    /// The release: outside the canvas removes the dragged sampler, inside
    /// it ends the move (the controller commits the dragged list).
    func colorSamplerMouseUp(_ raw: CGPoint) -> Bool {
        guard let drag = colorSamplerDrag else { return false }
        colorSamplerDrag = nil
        if colorSamplerInsideCanvas(raw) {
            onColorSamplerMoveEnd?(drag.index)
        } else {
            onColorSamplerRemove?(drag.index)
        }
        return true
    }

    /// Constant-size markers: a ringed crosshair, the `#n` label and, while a
    /// sheet previews, the after value in a small plate.
    func drawColorSamplers(in context: CGContext) {
        guard !colorSamplerMarkers.isEmpty else { return }
        let scale = magnification
        let radius = 5 / scale
        let arm = 8 / scale
        let font = DS.mono(10 / scale)
        for marker in colorSamplerMarkers {
            let center = marker.point
            let ring = CGRect(x: center.x - radius, y: center.y - radius,
                              width: radius * 2, height: radius * 2)
            let cross = NSBezierPath()
            cross.move(to: CGPoint(x: center.x - arm, y: center.y))
            cross.line(to: CGPoint(x: center.x + arm, y: center.y))
            cross.move(to: CGPoint(x: center.x, y: center.y - arm))
            cross.line(to: CGPoint(x: center.x, y: center.y + arm))
            cross.appendOval(in: ring)
            cross.lineWidth = 3 / scale
            NSColor.black.withAlphaComponent(0.7).setStroke()
            cross.stroke()
            cross.lineWidth = 1 / scale
            NSColor.white.setStroke()
            cross.stroke()

            var text = "#\(marker.label)"
            if let after = marker.after { text += "  \(after)" }
            let attributes: [NSAttributedString.Key: Any] = [
                .font: font, .foregroundColor: NSColor.white,
            ]
            let string = NSAttributedString(string: text, attributes: attributes)
            let size = string.size()
            let origin = CGPoint(x: center.x + arm + 2 / scale, y: center.y + 2 / scale)
            let plate = CGRect(x: origin.x - 2 / scale, y: origin.y - 1 / scale,
                               width: size.width + 4 / scale, height: size.height + 2 / scale)
            NSColor.black.withAlphaComponent(0.65).setFill()
            NSBezierPath(roundedRect: plate, xRadius: 2 / scale, yRadius: 2 / scale).fill()
            string.draw(at: origin)
        }
    }
}
