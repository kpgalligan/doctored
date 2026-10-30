import AppKit

// The Ruler tool: a measuring line on the canvas, and Straighten
// Layer, which rotates the layer selection so the line lies along the nearer
// axis. The line is view state: it is never saved, has no undo step and is
// not in an Action; only Straighten Layer edits the document, as ONE
// "Straighten Layer" step recorded as `straighten_layers` (its MCP twin).
// The canvas half is `ImageCanvasView+Ruler.swift`, the controller half
// `EditorViewController+Ruler.swift`.

/// One ruler line in canvas pixels (y down, like every canvas coordinate).
struct RulerMeasurement: Equatable {
    var start: CGPoint
    var end: CGPoint

    /// The line's direction in degrees, measured the way a protractor reads
    /// it on screen: counter-clockwise from the +x axis with y UP, in
    /// (−180, 180]. 0 for a zero-length line.
    var angleDegrees: Double {
        let dx = Double(end.x - start.x), dy = Double(end.y - start.y)
        guard dx != 0 || dy != 0 else { return 0 }
        let degrees = atan2(-dy, dx) * 180 / .pi
        return degrees == -180 ? 180 : degrees
    }

    /// The line's length in pixels.
    var length: Double {
        Double(hypot(end.x - start.x, end.y - start.y))
    }

    /// The rotation, in degrees counter-clockwise on screen, that brings the
    /// line parallel to the nearer axis: the smallest one, so |ρ| ≤ 45. nil
    /// when the line has no length or already lies along an axis.
    func straightenRotation() -> Double? {
        guard length > 0 else { return nil }
        let angle = angleDegrees
        let rotation = (angle / 90).rounded() * 90 - angle
        // Below a millionth of a degree nothing would move a sample.
        return abs(rotation) < 1e-6 ? nil : rotation
    }

    /// `straightenRotation` as a canvas-space matrix about the canvas
    /// centre — the `transform_layers` matrix Straighten Layer commits. The
    /// canvas has y down, where a positive `CGAffineTransform` angle turns
    /// clockwise on screen, so the angle is −ρ.
    func straightenMatrix(canvasWidth: Int, canvasHeight: Int) -> CGAffineTransform? {
        guard let rotation = straightenRotation() else { return nil }
        let center = CGPoint(x: CGFloat(canvasWidth) / 2, y: CGFloat(canvasHeight) / 2)
        return CGAffineTransform(translationX: center.x, y: center.y)
            .rotated(by: CGFloat(-rotation * .pi / 180))
            .translatedBy(x: -center.x, y: -center.y)
    }
}

/// The canvas's ruler state: the line, the canvas size it was drawn on (a
/// line from another canvas size is stale and reads as none) and the drag in
/// flight.
struct RulerSession: Equatable {
    /// What a drag moves, decided at mouse-down.
    enum Drag: Equatable {
        /// The end point follows the pointer: a new line, or an existing
        /// line's end.
        case end
        /// An existing line's start point follows the pointer.
        case start
    }

    var line: RulerMeasurement?
    var canvasSize: CGSize = .zero
    var drag: Drag?

    /// `point` with ⇧ held: on the nearest 45° ray from `anchor`, at the
    /// pointer's distance along it.
    static func constrained(_ point: CGPoint, from anchor: CGPoint) -> CGPoint {
        let dx = point.x - anchor.x, dy = point.y - anchor.y
        let distance = hypot(dx, dy)
        guard distance > 0 else { return point }
        let step = CGFloat.pi / 4
        let angle = (atan2(dy, dx) / step).rounded() * step
        return CGPoint(x: anchor.x + cos(angle) * distance, y: anchor.y + sin(angle) * distance)
    }
}
