import AppKit

// Edit ▸ Puppet Warp, the canvas half: mouse and key routing into the session
// (`PuppetWarpSession`, which owns the pins and the solve), the base the canvas draws instead
// of the projection while the session is open (the stack below the layer, the layer's proxy
// through the pose at the layer's opacity, the stack above) and the mesh and pin overlay. The
// frozen `ImageCanvasView.swift` only calls in here.

extension ImageCanvasView {
    /// A press while the session is open belongs to it, whatever the tool.
    func puppetWarpMouseDown(_ event: NSEvent) -> Bool {
        guard puppetWarpSession != nil else { return false }
        window?.makeFirstResponder(self)
        let point = convert(event.locationInWindow, from: nil)
        do {
            try puppetWarpSession?.press(at: point, modifiers: event.modifierFlags,
                                         scale: magnification)
        } catch { NSSound.beep() }
        onPuppetWarpChange?()
        return true
    }

    func puppetWarpMouseDragged(_ event: NSEvent) -> Bool {
        guard puppetWarpSession != nil else { return false }
        do {
            try puppetWarpSession?.dragged(to: convert(event.locationInWindow, from: nil))
        } catch { NSSound.beep() }
        onPuppetWarpChange?()
        return true
    }

    func puppetWarpMouseUp(_ event: NSEvent) -> Bool {
        guard puppetWarpSession != nil else { return false }
        puppetWarpSession?.released()
        onPuppetWarpChange?()
        return true
    }

    /// Return/Enter commits, Esc cancels, Delete removes the selected pins;
    /// every other key is swallowed while the session is open.
    func puppetWarpKeyDown(_ event: NSEvent) -> Bool {
        guard puppetWarpSession != nil else { return false }
        switch event.keyCode {
        case 53: onPuppetWarpCancel?()
        case 36, 76: onPuppetWarpCommit?()
        case 51, 117:
            do { try puppetWarpSession?.removeSelected() } catch { NSSound.beep() }
            onPuppetWarpChange?()
        default: break
        }
        return true
    }

    /// `image` drawn upright over the canvas rect `rect` in this flipped view.
    func drawUpright(_ image: CGImage, in rect: CGRect, context: CGContext,
                             alpha: CGFloat, hole: CGImage? = nil) {
        guard rect.width > 0, rect.height > 0 else { return }
        context.saveGState()
        context.setAlpha(alpha)
        context.interpolationQuality = .high
        context.translateBy(x: rect.minX, y: rect.maxY)
        context.scaleBy(x: 1, y: -1)
        let local = CGRect(origin: .zero, size: rect.size)
        if let hole {
            // The float form's layer: its pixels less the floated part,
            // a·(1−c), cut out of a transparency layer of its own.
            context.beginTransparencyLayer(auxiliaryInfo: nil)
            context.draw(image, in: local)
            context.saveGState()
            context.clip(to: local, mask: hole)
            context.setBlendMode(.destinationOut)
            context.setFillColor(NSColor.black.cgColor)
            context.fill(local)
            context.restoreGState()
            context.endTransparencyLayer()
        } else {
            context.draw(image, in: local)
        }
        context.restoreGState()
    }

    /// The session's base (`drawMeshWarpBase`).
    func drawPuppetWarpBase(_ session: PuppetWarpSession, in context: CGContext) {
        drawMeshWarpBase(session.workspace, preview: session.preview, at: session.previewRect,
                         in: context)
    }

    /// A mesh warp session's base (Puppet Warp's and Perspective Warp's):
    /// the stack below, the layer (unwarped with no preview; with the float
    /// form, the layer less the float), the warped proxy at the layer's
    /// opacity, the stack above.
    func drawMeshWarpBase(_ workspace: MeshWarpWorkspace, preview: CGImage?, at previewRect: CGRect,
                          in context: CGContext) {
        if let below = workspace.below { drawFlipped(below, in: context) }
        if let preview {
            if workspace.isFloat, let layer = workspace.proxyImage {
                drawUpright(layer, in: workspace.layerRect, context: context,
                            alpha: workspace.opacity, hole: workspace.coverageMask)
            }
            drawUpright(preview, in: previewRect, context: context, alpha: workspace.opacity)
        } else if let layer = workspace.proxyImage {
            drawUpright(layer, in: workspace.layerRect, context: context, alpha: workspace.opacity)
        }
        if let above = workspace.above { drawFlipped(above, in: context) }
    }

    /// The deformed mesh (with Show Mesh) and the pins, at a constant screen
    /// size: a selected pin is filled dark, the others light.
    func drawPuppetWarpOverlay() {
        guard let session = puppetWarpSession else { return }
        let scale = magnification
        if session.options.showMesh {
            let path = NSBezierPath()
            for triangle in session.mesh.triangles {
                let p = triangle.map { session.positions[$0] }
                path.move(to: p[0])
                path.line(to: p[1])
                path.line(to: p[2])
                path.close()
            }
            path.lineWidth = 1 / scale
            NSColor.black.withAlphaComponent(0.35).setStroke()
            path.stroke()
            path.lineWidth = 0.5 / scale
            NSColor.white.withAlphaComponent(0.7).setStroke()
            path.stroke()
        }
        let radius = 4 / scale
        for (index, pin) in session.pins.enumerated() {
            let ring = NSBezierPath(ovalIn: CGRect(x: pin.to.x - radius, y: pin.to.y - radius,
                                                   width: radius * 2, height: radius * 2))
            (session.selected.contains(index) ? NSColor.black : NSColor.systemYellow).setFill()
            ring.fill()
            ring.lineWidth = 1.5 / scale
            NSColor.white.setStroke()
            ring.stroke()
        }
    }
}
