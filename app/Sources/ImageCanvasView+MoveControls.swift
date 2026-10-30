import AppKit

/// The Move tool's Show Transform Controls: a box with eight handles
/// around the layers the Move tool would move, drawn while the Move tool is
/// current, there is no pixel selection and no Free Transform is open. A
/// press on a handle is reported by its index (TL, TR, BR, BL, T, R, B, L —
/// `transformHandlePoints`' order); the controller opens Free Transform and
/// hands it that handle (`EditorViewController+MoveControls.swift`).
extension ImageCanvasView {
    private func moveControlsCorners(_ rect: CGRect) -> [CGPoint] {
        [
            CGPoint(x: rect.minX, y: rect.minY), CGPoint(x: rect.maxX, y: rect.minY),
            CGPoint(x: rect.maxX, y: rect.maxY), CGPoint(x: rect.minX, y: rect.maxY),
        ]
    }

    /// The box, when it is showing.
    private var visibleMoveControls: CGRect? {
        guard tool == .move, !isTransforming, selection == nil, !quickMaskActive else { return nil }
        return moveControlsRect
    }

    /// True when the press took one of the box's handles.
    func moveControlsMouseDown(_ event: NSEvent) -> Bool {
        guard let rect = visibleMoveControls else { return false }
        let point = convert(event.locationInWindow, from: nil)
        let slop = Self.transformHandleSize / magnification
        let handles = Self.transformHandlePoints(moveControlsCorners(rect))
        guard let index = handles.firstIndex(where: {
            abs(point.x - $0.x) <= slop && abs(point.y - $0.y) <= slop
        }) else { return false }
        onMoveControlsHandle?(index, event.modifierFlags)
        return true
    }

    /// The Free Transform box's own look, so the handles read the same.
    func drawMoveControls() {
        guard let rect = visibleMoveControls else { return }
        let scale = magnification
        let box = NSBezierPath(rect: rect)
        box.lineWidth = 3 / scale
        NSColor.black.withAlphaComponent(0.45).setStroke()
        box.stroke()
        box.lineWidth = 1 / scale
        NSColor.white.withAlphaComponent(0.95).setStroke()
        box.stroke()
        let size = Self.transformHandleSize / scale
        for point in Self.transformHandlePoints(moveControlsCorners(rect)) {
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
