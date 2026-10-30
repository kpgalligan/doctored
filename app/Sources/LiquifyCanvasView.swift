import AppKit

/// The Liquify workspace's picture: one proxy pixel per point at 100%,
/// with nearest display sampling; scrolling and magnification belong to the
/// surrounding NSScrollView. It draws what the sheet hands it and forwards
/// drags and pointer moves through the `on*` closures. Points are in proxy
/// pixels, row 0 at the top.
final class LiquifyCanvasView: NSView {
    /// The proxy through the field.
    var image: CGImage? { didSet { needsDisplay = true } }
    /// With a selection: the untouched proxy, shown where the selection is not.
    var baseImage: CGImage? { didSet { needsDisplay = true } }
    /// With a selection: its coverage as a gray image, white = selected.
    var clipMask: CGImage? { didSet { needsDisplay = true } }
    /// Show Mesh: the grid through the field.
    var meshImage: CGImage? { didSet { needsDisplay = true } }
    /// Show Mask: the freeze coverage as a gray image, white = frozen.
    var freezeImage: CGImage? { didSet { needsDisplay = true } }
    /// The brush outline at the pointer.
    var brush: (center: CGPoint, radius: CGFloat)? { didSet { needsDisplay = true } }

    /// A drag began at this point with these modifiers; false leaves it to the view.
    var onStrokeBegin: ((CGPoint, NSEvent.ModifierFlags) -> Bool)?
    var onStrokeMove: ((CGPoint) -> Void)?
    var onStrokeEnd: (() -> Void)?
    /// The pointer is at this point, or (nil) has left the view.
    var onHover: ((CGPoint?) -> Void)?

    private var stroking = false
    private var tracking: NSTrackingArea?
    override var isFlipped: Bool { true }

    override init(frame: NSRect) { super.init(frame: frame) }
    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("LiquifyCanvasView does not support NSCoder") }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(rect: .zero,
                                  options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect],
                                  owner: self, userInfo: nil)
        addTrackingArea(area)
        tracking = area
    }

    override func mouseMoved(with event: NSEvent) { onHover?(convert(event.locationInWindow, from: nil)) }
    override func mouseExited(with event: NSEvent) { onHover?(nil) }

    override func mouseDown(with event: NSEvent) {
        stroking = onStrokeBegin?(convert(event.locationInWindow, from: nil), event.modifierFlags) ?? false
        if !stroking { super.mouseDown(with: event) }
    }

    override func mouseDragged(with event: NSEvent) {
        guard stroking else { super.mouseDragged(with: event); return }
        let point = convert(event.locationInWindow, from: nil)
        onHover?(point)
        onStrokeMove?(point)
    }

    override func mouseUp(with event: NSEvent) {
        guard stroking else { super.mouseUp(with: event); return }
        stroking = false
        onStrokeEnd?()
    }

    override func draw(_ dirtyRect: NSRect) {
        let visible = dirtyRect.intersection(bounds)
        DS.checkerA.setFill(); visible.fill()
        let side: CGFloat = 12
        let firstX = Int(floor(visible.minX / side)), lastX = Int(ceil(visible.maxX / side))
        let firstY = Int(floor(visible.minY / side)), lastY = Int(ceil(visible.maxY / side))
        DS.checkerB.setFill()
        if firstX < lastX, firstY < lastY {
            for y in firstY..<lastY {
                for x in firstX..<lastX where (x + y) % 2 == 0 {
                    NSRect(x: CGFloat(x) * side, y: CGFloat(y) * side, width: side, height: side).fill()
                }
            }
        }
        guard let context = NSGraphicsContext.current?.cgContext else { return }
        // CoreGraphics draws an image and a clipping mask bottom-up; the view
        // is flipped, so the pictures are drawn in an upright copy of its space.
        context.saveGState()
        context.translateBy(x: 0, y: bounds.height)
        context.scaleBy(x: 1, y: -1)
        context.interpolationQuality = .none
        let frame = CGRect(origin: .zero, size: bounds.size)
        if let clipMask, let image {
            // base·(1 − coverage) + image·coverage, as the commit writes it, in a
            // layer of their own so a transparent result stays transparent over
            // the checkerboard: the base, erased through the mask, then the
            // image added through the mask.
            context.beginTransparencyLayer(auxiliaryInfo: nil)
            if let baseImage { context.draw(baseImage, in: frame) }
            context.saveGState()
            context.clip(to: frame, mask: clipMask)
            context.setBlendMode(.destinationOut)
            context.setFillColor(NSColor.black.cgColor)
            context.fill(frame)
            context.setBlendMode(.plusLighter)
            context.draw(image, in: frame)
            context.restoreGState()
            context.endTransparencyLayer()
        } else if let image {
            context.draw(image, in: frame)
        }
        if let meshImage { context.draw(meshImage, in: frame) }
        if let freezeImage {
            context.saveGState()
            context.clip(to: frame, mask: freezeImage)
            context.setFillColor(DS.coral.withAlphaComponent(0.4).cgColor)
            context.fill(frame)
            context.restoreGState()
        }
        context.restoreGState()
        drawBrush()
    }

    /// The brush as a circle one screen pixel wide at any magnification, dark
    /// under light so it reads on any picture.
    private func drawBrush() {
        guard let brush, brush.radius > 0 else { return }
        let hairline = 1 / max(enclosingScrollView?.magnification ?? 1, 0.01)
        let outline = NSBezierPath(ovalIn: NSRect(x: brush.center.x - brush.radius, y: brush.center.y - brush.radius,
                                                  width: brush.radius * 2, height: brush.radius * 2))
        outline.lineWidth = hairline * 3
        NSColor.black.withAlphaComponent(0.5).setStroke()
        outline.stroke()
        outline.lineWidth = hairline
        NSColor.white.setStroke()
        outline.stroke()
    }
}
