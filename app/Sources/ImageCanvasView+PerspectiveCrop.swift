import AppKit

/// The canvas half of the Perspective Crop tool (PerspectiveCrop.swift):
/// the gesture routing `mouseDown` / `mouseDragged` / `mouseUp` / `keyDown`
/// ask first, and the overlay. With no quadrilateral a drag draws a
/// rectangle; with one, a press on a corner drags that corner, a press inside
/// moves the whole quadrilateral, and a double-click or Return commits while
/// Escape removes it. The corners are deliberately UNCLAMPED once placed: the
/// part of the quadrilateral past the canvas edge comes out transparent.
extension ImageCanvasView {
    /// A corner's hit radius, in screen points.
    private static let perspectiveCropHitPoints: CGFloat = 8

    private func perspectiveCropClamped(_ point: CGPoint) -> CGPoint {
        CGPoint(x: min(max(point.x, 0), bounds.width), y: min(max(point.y, 0), bounds.height))
    }

    /// True when the press is the Perspective Crop's.
    func perspectiveCropMouseDown(_ event: NSEvent) -> Bool {
        guard tool == .perspectiveCrop else { return false }
        let raw = convert(event.locationInWindow, from: nil)
        var session = perspectiveCropSession
        guard let corners = session.corners, corners.count == 4 else {
            session.drag = .creating(anchor: perspectiveCropClamped(raw))
            perspectiveCropSession = session
            return true
        }
        if event.clickCount >= 2 {
            onPerspectiveCropCommit?()
            return true
        }
        let radius = Self.perspectiveCropHitPoints / magnification
        if let index = corners.indices.min(by: {
            hypot(corners[$0].x - raw.x, corners[$0].y - raw.y)
                < hypot(corners[$1].x - raw.x, corners[$1].y - raw.y)
        }), hypot(corners[index].x - raw.x, corners[index].y - raw.y) <= radius {
            session.drag = .corner(index)
        } else {
            // Inside moves the quadrilateral; outside does the same, so a
            // press never silently discards the corners already placed.
            session.drag = .move(grab: raw, start: corners)
        }
        perspectiveCropSession = session
        return true
    }

    /// A drag tick.
    func perspectiveCropMouseDragged(_ raw: CGPoint) -> Bool {
        var session = perspectiveCropSession
        guard let drag = session.drag else { return false }
        switch drag {
        case .creating(let anchor):
            let point = perspectiveCropClamped(raw)
            let rect = CGRect(
                x: min(anchor.x, point.x), y: min(anchor.y, point.y),
                width: abs(point.x - anchor.x), height: abs(point.y - anchor.y))
            // Under two screen points it is still a click, not a rectangle.
            guard rect.width * magnification >= 2, rect.height * magnification >= 2 else {
                return true
            }
            session.corners = [
                CGPoint(x: rect.minX, y: rect.minY), CGPoint(x: rect.maxX, y: rect.minY),
                CGPoint(x: rect.maxX, y: rect.maxY), CGPoint(x: rect.minX, y: rect.maxY),
            ]
        case .corner(let index):
            guard var corners = session.corners, corners.indices.contains(index) else {
                return true
            }
            corners[index] = raw
            session.corners = corners
        case .move(let grab, let start):
            session.corners = start.map {
                CGPoint(x: $0.x + raw.x - grab.x, y: $0.y + raw.y - grab.y)
            }
        }
        perspectiveCropSession = session
        return true
    }

    /// The release ends the drag; the quadrilateral stays for the next one.
    func perspectiveCropMouseUp() -> Bool {
        guard perspectiveCropSession.drag != nil else { return false }
        perspectiveCropSession.drag = nil
        return true
    }

    /// Return commits, Escape removes the quadrilateral. Inert in Quick
    /// Mask, like the Crop tool's keys: a commit is a document edit.
    func perspectiveCropKeyDown(_ event: NSEvent) -> Bool {
        guard tool == .perspectiveCrop, perspectiveCropSession.corners != nil, !quickMaskActive
        else { return false }
        switch event.keyCode {
        case 53:
            onPerspectiveCropCancel?()
            return true
        case 36, 76:
            onPerspectiveCropCommit?()
            return true
        default:
            return false
        }
    }

    /// Everything outside the quadrilateral dimmed, its outline (red when it
    /// is not convex and so cannot commit) and a handle on each corner.
    func drawPerspectiveCrop() {
        guard tool == .perspectiveCrop, let corners = perspectiveCropSession.corners,
              corners.count == 4 else { return }
        let outline = NSBezierPath()
        outline.move(to: corners[0])
        for corner in corners.dropFirst() { outline.line(to: corner) }
        outline.close()

        let convex = PerspectiveCropSession.isConvex(corners)
        if convex {
            let dim = NSBezierPath(rect: bounds)
            dim.append(outline)
            dim.windingRule = .evenOdd
            NSColor.black.withAlphaComponent(0.5).setFill()
            dim.fill()
        }
        let scale = magnification
        outline.lineWidth = 3 / scale
        NSColor.black.withAlphaComponent(0.45).setStroke()
        outline.stroke()
        outline.lineWidth = 1 / scale
        (convex ? NSColor.white.withAlphaComponent(0.95) : NSColor.systemRed).setStroke()
        outline.stroke()

        let size = ImageCanvasView.transformHandleSize / scale
        for point in corners {
            let square = NSBezierPath(
                rect: CGRect(
                    x: point.x - size / 2, y: point.y - size / 2, width: size, height: size))
            NSColor.white.setFill()
            square.fill()
            square.lineWidth = 1 / scale
            NSColor.black.withAlphaComponent(0.65).setStroke()
            square.stroke()
        }
    }
}
