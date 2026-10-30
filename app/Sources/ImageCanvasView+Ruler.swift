import AppKit

/// The canvas half of the Ruler tool (RulerTool.swift): the press,
/// drag and release `mouseDown` / `mouseDragged` / `mouseUp` ask first, and
/// the line's drawing. A drag on empty canvas draws a new line, a drag on
/// either end moves that end, and ⇧ holds the line to 45° steps. The line
/// draws only while the Ruler is the current tool.
extension ImageCanvasView {
    /// The hit radius of an end point, in screen points.
    private static let rulerHitPoints: CGFloat = 7

    /// The current line, or nil when there is none or it was drawn on a
    /// canvas of another size.
    var rulerMeasurement: RulerMeasurement? {
        rulerSession.canvasSize == bounds.size ? rulerSession.line : nil
    }

    func clearRuler() {
        guard rulerSession.line != nil || rulerSession.drag != nil else { return }
        rulerSession = RulerSession()
    }

    private func rulerClamped(_ point: CGPoint) -> CGPoint {
        CGPoint(x: min(max(point.x, 0), bounds.width), y: min(max(point.y, 0), bounds.height))
    }

    /// True when the press is the Ruler's.
    func rulerMouseDown(_ event: NSEvent) -> Bool {
        guard tool == .ruler else { return false }
        let point = rulerClamped(convert(event.locationInWindow, from: nil))
        let radius = Self.rulerHitPoints / magnification
        var session = rulerSession
        if let line = rulerMeasurement {
            if hypot(line.end.x - point.x, line.end.y - point.y) <= radius {
                session.drag = .end
                rulerSession = session
                return true
            }
            if hypot(line.start.x - point.x, line.start.y - point.y) <= radius {
                session.drag = .start
                rulerSession = session
                return true
            }
        }
        session.line = RulerMeasurement(start: point, end: point)
        session.canvasSize = bounds.size
        session.drag = .end
        rulerSession = session
        return true
    }

    /// A drag tick: the dragged end follows the pointer, clamped to the canvas.
    func rulerMouseDragged(_ raw: CGPoint, _ modifiers: NSEvent.ModifierFlags) -> Bool {
        guard let drag = rulerSession.drag, var line = rulerSession.line else { return false }
        var point = rulerClamped(raw)
        let anchor = drag == .end ? line.start : line.end
        if modifiers.contains(.shift) {
            point = rulerClamped(RulerSession.constrained(point, from: anchor))
        }
        switch drag {
        case .end: line.end = point
        case .start: line.start = point
        }
        rulerSession.line = line
        return true
    }

    /// The release: a press that never became a line (under two screen
    /// points) clears the ruler.
    func rulerMouseUp() -> Bool {
        guard rulerSession.drag != nil else { return false }
        var session = rulerSession
        session.drag = nil
        if let line = session.line, line.length * Double(magnification) < 2 { session.line = nil }
        rulerSession = session
        return true
    }

    /// The line with a dark halo, and a small cross at each end.
    func drawRuler() {
        guard tool == .ruler, let line = rulerMeasurement else { return }
        let scale = magnification
        let path = NSBezierPath()
        path.move(to: line.start)
        path.line(to: line.end)
        let arm = 5 / scale
        for point in [line.start, line.end] {
            path.move(to: CGPoint(x: point.x - arm, y: point.y))
            path.line(to: CGPoint(x: point.x + arm, y: point.y))
            path.move(to: CGPoint(x: point.x, y: point.y - arm))
            path.line(to: CGPoint(x: point.x, y: point.y + arm))
        }
        path.lineWidth = 3 / scale
        NSColor.black.withAlphaComponent(0.55).setStroke()
        path.stroke()
        path.lineWidth = 1 / scale
        NSColor.white.setStroke()
        path.stroke()
    }
}
