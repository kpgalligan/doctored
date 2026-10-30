import AppKit

// Edit ▸ Perspective Warp, the canvas half: mouse and key routing into the session
// (`PerspectiveWarpSession`, which owns the quads and the core calls), the base the canvas draws
// instead of the projection while the session is open (Puppet Warp's `drawMeshWarpBase`), and
// the quad, pin and edge overlay. The frozen `ImageCanvasView.swift` only calls in here.

extension ImageCanvasView {
    /// A press while the session is open belongs to it, whatever the tool.
    func perspectiveWarpMouseDown(_ event: NSEvent) -> Bool {
        guard perspectiveWarpSession != nil else { return false }
        window?.makeFirstResponder(self)
        let point = convert(event.locationInWindow, from: nil)
        if perspectiveWarpSession?.press(at: point, modifiers: event.modifierFlags,
                                         scale: magnification) == true {
            NSSound.beep()
        }
        onPerspectiveWarpChange?()
        return true
    }

    func perspectiveWarpMouseDragged(_ event: NSEvent) -> Bool {
        guard perspectiveWarpSession != nil else { return false }
        if perspectiveWarpSession?.dragged(to: convert(event.locationInWindow, from: nil)) == true {
            NSSound.beep()
        }
        onPerspectiveWarpChange?()
        return true
    }

    func perspectiveWarpMouseUp(_ event: NSEvent) -> Bool {
        guard perspectiveWarpSession != nil else { return false }
        if perspectiveWarpSession?.released(scale: magnification) == true { NSSound.beep() }
        onPerspectiveWarpChange?()
        return true
    }

    /// Return/Enter commits, Esc cancels, Delete removes the selected quad;
    /// every other key is swallowed while the session is open.
    func perspectiveWarpKeyDown(_ event: NSEvent) -> Bool {
        guard perspectiveWarpSession != nil else { return false }
        switch event.keyCode {
        case 53: onPerspectiveWarpCancel?()
        case 36, 76: onPerspectiveWarpCommit?()
        case 51, 117:
            if perspectiveWarpSession?.removeSelected() == true { NSSound.beep() }
            onPerspectiveWarpChange?()
        default: break
        }
        return true
    }

    /// The session's base: the stack, the layer (unwarped in Layout) and the
    /// warped proxy (`drawMeshWarpBase`).
    func drawPerspectiveWarpBase(_ session: PerspectiveWarpSession, in context: CGContext) {
        drawMeshWarpBase(session.workspace, preview: session.preview, at: session.previewRect,
                         in: context)
    }

    /// The quads (rest corners in Layout, warped in Warp), the shared edges
    /// in thick blue, the straightened edges in yellow, the corners or pins,
    /// and Layout's rubber band, at a constant screen size.
    func drawPerspectiveWarpOverlay() {
        guard let session = perspectiveWarpSession else { return }
        let scale = magnification
        let layout = session.layout
        let warped = session.mode == .warp
        let point = { (v: Int) in warped ? layout.vertices[v].to : layout.vertices[v].from }
        for (index, ids) in layout.quads.enumerated() {
            let path = NSBezierPath()
            path.move(to: point(ids[0]))
            for v in ids.dropFirst() { path.line(to: point(v)) }
            path.close()
            if !warped, session.selectedQuad == index {
                NSColor.systemBlue.withAlphaComponent(0.12).setFill()
                path.fill()
            }
            path.lineWidth = 2 / scale
            NSColor.black.withAlphaComponent(0.45).setStroke()
            path.stroke()
            path.lineWidth = 1 / scale
            NSColor.white.setStroke()
            path.stroke()
        }
        let segment = { (a: Int, b: Int, color: NSColor, width: CGFloat) in
            let path = NSBezierPath()
            path.move(to: point(a))
            path.line(to: point(b))
            path.lineWidth = width / scale
            color.setStroke()
            path.stroke()
        }
        for pair in layout.sharedEdges where pair.allSatisfy({ layout.vertices.indices.contains($0) }) {
            segment(pair[0], pair[1], NSColor.systemBlue, 3)
        }
        for entry in layout.straighten where layout.quads.indices.contains(entry.quad) {
            let (a, b) = layout.edge(entry.quad, entry.edge)
            segment(a, b, NSColor.systemYellow, 2)
        }
        let radius = 4 / scale
        for v in layout.vertices.indices {
            let p = point(v)
            let box = CGRect(x: p.x - radius, y: p.y - radius, width: radius * 2, height: radius * 2)
            let mark = warped ? NSBezierPath(ovalIn: box) : NSBezierPath(rect: box)
            (warped && session.selectedPins.contains(v) ? NSColor.black : NSColor.white).setFill()
            mark.fill()
            mark.lineWidth = 1.5 / scale
            NSColor.black.setStroke()
            mark.stroke()
        }
        if let rect = session.creatingRect {
            let band = NSBezierPath(rect: rect)
            band.lineWidth = 1 / scale
            band.setLineDash([4 / scale, 3 / scale], count: 2, phase: 0)
            NSColor.systemBlue.setStroke()
            band.stroke()
        }
    }
}
