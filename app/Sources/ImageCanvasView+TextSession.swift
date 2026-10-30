import AppKit

// `TextSessionHostView` is a flipped canvas subview at the session origin holding the NSTextView at
// (0, 0), its frame `scale ×` the view's size, its bounds the view's size and its `frameRotation`
// the angle; `textSessionPresentation` is the ONE placement rule (a transformed layer whose quad
// overlaps the canvas opens exactly at its anchor, everything else at the upright clamp),
// transformed point text never wraps (`fitUnwrappedTextSession`), and the commit reads
// `textSessionOrigin`, never the text view's frame.

/// The on-canvas text editor's host: a flipped canvas subview
/// whose frame sits at the session's origin and turns and scales the
/// NSTextView inside it by the layer's `TextSessionPlacement`, so a rotated
/// or scaled text layer is edited where its glyphs are. The text view sits
/// at (0, 0) in the host; the host's frame is `scale ×` the text view's
/// size, its bounds the text view's size, and its `frameRotation` the
/// placement's angle about the origin — the layer's anchor.
final class TextSessionHostView: NSView {
    let placement: TextSessionPlacement
    let sessionOrigin: CGPoint
    /// Point text under a non-identity placement: the editor never wraps it
    /// (`fitUnwrappedTextSession`), because the commit lays point text out
    /// unwrapped.
    let unwrapped: Bool

    init(origin: CGPoint, placement: TextSessionPlacement, unwrapped: Bool = false) {
        self.placement = placement
        self.sessionOrigin = origin
        self.unwrapped = unwrapped
        super.init(frame: NSRect(origin: origin, size: .zero))
    }

    override var isFlipped: Bool { true }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("TextSessionHostView does not support NSCoder") }
}

extension ImageCanvasView {
    /// The ONE placement rule for a new session of `size` at `point`. A
    /// transformed layer whose editor quad overlaps the canvas opens exactly
    /// at its anchor, turned and scaled. Everything else — a new session, an
    /// untransformed or mirrored layer, a transformed one entirely off the
    /// canvas — opens upright at the original clamp: shifted left so the box
    /// does not overhang the right edge, and for a re-edit (`editing`) also
    /// clamped vertically.
    func textSessionPresentation(
        at point: CGPoint, size: CGSize, placement: TextSessionPlacement, editing: Bool
    ) -> (origin: CGPoint, placement: TextSessionPlacement) {
        if !placement.isIdentity,
           TextSessionPlacement.convexQuad(
               placement.quad(origin: point, size: size), overlaps: bounds) {
            return (point, placement)
        }
        let x = max(0, min(point.x, bounds.width - size.width))
        let y = editing ? max(0, min(point.y, bounds.height - size.height)) : point.y
        return (CGPoint(x: x, y: y), .identity)
    }

    /// Puts `textView` at (0, 0) in a new host over the canvas and sizes the
    /// host around it.
    func installTextSessionHost(
        _ textView: CanvasTextView, origin: CGPoint, placement: TextSessionPlacement,
        unwrapped: Bool = false
    ) {
        let host = TextSessionHostView(origin: origin, placement: placement, unwrapped: unwrapped)
        textView.setFrameOrigin(.zero)
        if unwrapped {
            textView.textContainer?.widthTracksTextView = false
        }
        host.addSubview(textView)
        addSubview(host)
        textSessionHost = host
        syncTextSessionHost()
    }

    /// Re-fits the host to its text view after the view's size changed.
    func syncTextSessionHost() {
        guard let host = textSessionHost, let textView = host.subviews.first else { return }
        let size = textView.frame.size
        let scale = host.placement.scale
        host.frame = NSRect(
            origin: host.sessionOrigin,
            size: NSSize(width: size.width * scale, height: size.height * scale))
        host.bounds = NSRect(origin: .zero, size: size)
        // AppKit reads 30° back as 29.999999999999996; don't re-assign it.
        if abs(host.frameRotation - host.placement.rotationDegrees) > 1e-9 {
            host.frameRotation = host.placement.rotationDegrees
        }
        needsDisplay = true
    }

    /// An unwrapped session's box is exactly as wide as its widest line: the
    /// string is measured at the point-text width (`TextLayer.unboundedWidth`,
    /// what the commit lays point text out at), then the container and the
    /// view are fitted to it, so alignment still acts within the block.
    func fitUnwrappedTextSession(_ textView: NSTextView) {
        guard textSessionHost?.unwrapped == true,
              let manager = textView.layoutManager, let container = textView.textContainer
        else { return }
        container.size = NSSize(width: TextLayer.unboundedWidth, height: container.size.height)
        manager.ensureLayout(for: container)
        // The widest line fragment's USED width: the container's used rect
        // can report the whole unbounded width inside a text view.
        var widest: CGFloat = 0
        var glyph = 0
        while glyph < manager.numberOfGlyphs {
            var line = NSRange(location: 0, length: 0)
            let used = manager.lineFragmentUsedRect(forGlyphAt: glyph, effectiveRange: &line)
            if used.width.isFinite { widest = max(widest, used.width) }
            glyph = max(NSMaxRange(line), glyph + 1)
        }
        let width = max(widest.rounded(.up) + 1, 1)
        container.size = NSSize(width: width, height: container.size.height)
        textView.minSize.width = width
        textView.maxSize.width = width
        if textView.frame.width != width {
            textView.setFrameSize(NSSize(width: width, height: textView.frame.height))
        }
    }

    /// The session's anchor in canvas space; nil without a session.
    var textSessionOrigin: CGPoint? { textSessionHost?.sessionOrigin }

    /// The session border's corners (TL, TR, BR, BL) in canvas space: the
    /// text view's bounds outset by 2 canvas pixels, through the host.
    func textSessionBorderQuad() -> [CGPoint]? {
        guard let host = textSessionHost, let textView = host.subviews.first else { return nil }
        let outset = 2 / host.placement.scale
        let rect = textView.bounds.insetBy(dx: -outset, dy: -outset)
        return [
            CGPoint(x: rect.minX, y: rect.minY), CGPoint(x: rect.maxX, y: rect.minY),
            CGPoint(x: rect.maxX, y: rect.maxY), CGPoint(x: rect.minX, y: rect.maxY),
        ].map { convert($0, from: textView) }
    }

    /// Dashed hairline border (selection style) around the active text
    /// session, through the quad's corners in `NSBezierPath(rect:)`'s order.
    func drawTextSessionBorder(_ quad: [CGPoint]) {
        guard quad.count == 4 else { return }
        let scale = magnification
        let lineWidth = 1 / scale
        let dashPattern: [CGFloat] = [4 / scale, 4 / scale]
        func outline() -> NSBezierPath {
            let path = NSBezierPath()
            path.move(to: quad[0])
            for corner in quad.dropFirst() {
                path.line(to: corner)
            }
            path.close()
            path.lineWidth = lineWidth
            return path
        }

        let blackPath = outline()
        blackPath.setLineDash(dashPattern, count: dashPattern.count, phase: 0)
        NSColor.black.setStroke()
        blackPath.stroke()

        let whitePath = outline()
        whitePath.setLineDash(dashPattern, count: dashPattern.count, phase: 4 / scale)
        NSColor.white.setStroke()
        whitePath.stroke()
    }
}
