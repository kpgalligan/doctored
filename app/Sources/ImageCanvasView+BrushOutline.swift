import AppKit

// `BrushOutline` geometry mirrors `SoftBrush.clipDab`'s transform, drawn as a canvas overlay (never
// an `NSCursor` image) while the system pointer is hidden through the identity-guarded
// `brushOutlineCursor` + `resetCursorRects` pattern; the crosshair returns below 6 screen points
// and for ⌥ on brush/clone/heal. `brushOutlineViewportDidChange` (called from
// `magnificationDidChange` and at the end of `setViewRotation`, since a turn need not scroll)
// re-derives the hover point when the canvas scrolls, zooms or turns under a still pointer.

/// The dab's full footprint at the pointer, in canvas units. Pure geometry.
///
/// The brush-tip cursor, drawn as a canvas overlay rather than as an
/// `NSCursor` image: a 200 px tip at 32× zoom would be a 6400 pt cursor.
struct BrushOutline: Equatable {
    /// Below this on-screen diameter the crosshair stays and no outline is drawn.
    static let minimumScreenDiameter: CGFloat = 6
    let center: CGPoint
    let diameter: CGFloat
    let angleDegrees: CGFloat
    let roundness: CGFloat
    let magnification: CGFloat

    /// nil when `diameter * magnification < minimumScreenDiameter`.
    init?(
        center: CGPoint, diameter: CGFloat, angleDegrees: CGFloat, roundness: CGFloat,
        magnification: CGFloat
    ) {
        guard diameter * magnification >= Self.minimumScreenDiameter else { return nil }
        self.center = center
        self.diameter = diameter
        self.angleDegrees = angleDegrees
        self.roundness = roundness
        self.magnification = magnification
    }

    /// Ellipse in canvas coordinates through SoftBrush.clipDab's exact transform:
    /// translate(center) · rotate(-angleDegrees·π/180) · scale(1, max(roundness, 0.01));
    /// a round tip (roundness ≥ 0.995) is the plain centred circle.
    var path: NSBezierPath {
        let squash = max(roundness, 0.01)
        let rect = CGRect(
            x: -diameter / 2, y: -diameter / 2, width: diameter, height: diameter)
        guard squash < 0.995 else {
            return NSBezierPath(ovalIn: rect.offsetBy(dx: center.x, dy: center.y))
        }
        let transform = CGAffineTransform(translationX: center.x, y: center.y)
            .rotated(by: -angleDegrees * .pi / 180)
            .scaledBy(x: 1, y: squash)
        let path = NSBezierPath(ovalIn: rect)
        path.transform(
            using: AffineTransform(
                m11: transform.a, m12: transform.b, m21: transform.c, m22: transform.d,
                tX: transform.tx, tY: transform.ty))
        return path
    }

    /// The centred square of side `diameter`, outset by 3 / magnification (halo + slack).
    var dirtyRect: CGRect {
        let outset = 3 / magnification
        return CGRect(
            x: center.x - diameter / 2, y: center.y - diameter / 2,
            width: diameter, height: diameter
        ).insetBy(dx: -outset, dy: -outset)
    }
}

extension ImageCanvasView {
    /// Tools whose ⌥-press needs a one-pixel pointer: brush (eyedropper),
    /// clone and heal (source point). Eraser, dodge and spotHeal have no ⌥ meaning.
    static let brushOutlineOptionPrecisionTools: Set<EditorTool> = [.brush, .clone, .heal]

    /// 1×1 fully transparent image, hotspot (0,0); built once. A stable
    /// instance keeps the identity-guarded cursor-rect invalidation cheap.
    static let brushOutlineHiddenCursor: NSCursor = {
        let image = NSImage(size: NSSize(width: 1, height: 1), flipped: false) { _ in true }
        return NSCursor(image: image, hotSpot: NSPoint(x: 0, y: 0))
    }()

    /// nil unless tool.usesBrushTip, hoverPoint != nil, !isTransforming, and the size gate passes.
    /// Independent of modifiers: the ring keeps drawing while ⌥ is held.
    var brushOutline: BrushOutline? {
        guard tool.usesBrushTip, let hoverPoint, !isTransforming else { return nil }
        return BrushOutline(
            center: hoverPoint, diameter: brushSize, angleDegrees: brushAngle,
            roundness: brushRoundness, magnification: magnification)
    }

    func brushOutlineDirtyRect() -> CGRect? {
        brushOutline?.dirtyRect
    }

    /// Redraws only the outline's old and new squares — `setHoverPoint`
    /// runs at pointer rate, so never a whole-canvas repaint.
    func invalidateBrushOutline(from before: CGRect?) {
        let after = brushOutlineDirtyRect()
        guard before != after else { return }
        if let before { setNeedsDisplay(before) }
        if let after { setNeedsDisplay(after) }
    }

    /// Strokes the outline in the clone source marker's two-pass style:
    /// a dark halo, then a light hairline, both constant in screen points.
    func drawBrushOutline() {
        guard let outline = brushOutline else { return }
        let path = outline.path
        path.lineWidth = 2.5 / outline.magnification
        NSColor.black.withAlphaComponent(0.45).setStroke()
        path.stroke()
        path.lineWidth = 1 / outline.magnification
        NSColor.white.withAlphaComponent(0.95).setStroke()
        path.stroke()
    }

    /// The hidden pointer while the outline stands in for it; nil (the
    /// tool's crosshair) when the footprint is too small to see or ⌥ asks
    /// for a single-pixel pick. Hover cursors survive only through
    /// `resetCursorRects`, so this follows `updateGuideHoverCursor`: a
    /// stored cursor, invalidated ONLY when its identity changes.
    func updateBrushOutlineCursor() {
        let wanted: NSCursor? =
            tool.usesBrushTip
                && brushSize * magnification >= BrushOutline.minimumScreenDiameter
                && !(selectionCursorModifiers.contains(.option)
                    && Self.brushOutlineOptionPrecisionTools.contains(tool))
            ? Self.brushOutlineHiddenCursor : nil
        guard wanted !== brushOutlineCursor else { return }
        brushOutlineCursor = wanted
        window?.invalidateCursorRects(for: self)
    }

    /// The canvas moved under a possibly stationary pointer (scroll, zoom,
    /// pinch): no mouse event arrives, so re-derive the cursor gate and the
    /// latched hover point from the pointer's current window location.
    func brushOutlineViewportDidChange() {
        updateBrushOutlineCursor()
        if let window {
            refreshHoverPoint(windowLocation: window.mouseLocationOutsideOfEventStream)
        }
    }
}
