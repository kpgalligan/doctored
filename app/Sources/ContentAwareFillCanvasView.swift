import AppKit

// Core owns recipe rasterization and synthesis; the view draws geometry/display only.

/// Source-coordinate geometry overlay only. The core remains the sole owner of
/// committed eligibility and image synthesis. 100% is one source per backing
/// pixel; the same viewport is retained across Before/After switches.
final class ContentAwareFillCanvasView: NSView {
    enum Mode: Int { case pan, includeBrush, excludeBrush, includeRect, excludeRect }
    var image: CGImage? { didSet { needsDisplay = true } }
    var sourceMask = ContentAwareSourceMask() { didSet { needsDisplay = true } }
    var mode = Mode.pan
    var radius: CGFloat = 20
    var showSource = true { didSet { needsDisplay = true } }
    var nativeDetail = false { didSet { needsDisplay = true } }
    var onOperation: (([String: Any]) -> Void)?
    var selectionPath: CGPath?
    private var center = CGPoint(x: 0.5, y: 0.5)
    private var points: [CGPoint] = []
    private var dragStart: CGPoint?
    private var originalCenter = CGPoint.zero
    override var isFlipped: Bool { true }

    override init(frame: NSRect) { super.init(frame: frame); wantsLayer = true }
    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("ContentAwareFillCanvasView does not support NSCoder") }

    private var scale: CGFloat {
        guard let image else { return 1 }
        return nativeDetail ? 1 / (window?.backingScaleFactor ?? 1)
            : min(bounds.width / CGFloat(image.width), bounds.height / CGFloat(image.height))
    }
    private var imageRect: CGRect {
        guard let image else { return .zero }
        let size = CGSize(width: CGFloat(image.width) * scale, height: CGFloat(image.height) * scale)
        return CGRect(x: bounds.midX - size.width * (nativeDetail ? center.x : 0.5),
            y: bounds.midY - size.height * (nativeDetail ? center.y : 0.5), width: size.width, height: size.height)
    }
    private func point(_ event: NSEvent) -> CGPoint {
        let p = convert(event.locationInWindow, from: nil), rect = imageRect
        return CGPoint(x: (p.x - rect.minX) / scale, y: (p.y - rect.minY) / scale)
    }
    override func draw(_ dirtyRect: NSRect) {
        NSColor.black.withAlphaComponent(0.8).setFill(); bounds.fill()
        guard let image, let context = NSGraphicsContext.current?.cgContext else { return }
        let rect = imageRect
        context.saveGState()
        context.translateBy(x: rect.minX, y: rect.minY)
        context.scaleBy(x: scale, y: scale)
        let extent = CGRect(x: 0, y: 0, width: image.width, height: image.height)
        context.clip(to: extent)
        context.saveGState(); context.translateBy(x: 0, y: CGFloat(image.height)); context.scaleBy(x: 1, y: -1)
        context.interpolationQuality = nativeDetail ? .none : .high
        context.draw(image, in: extent); context.restoreGState()
        if showSource {
            // An isolated transparency group makes last-write-wins marks
            // replace the overlay, matching the recipe's editing semantics.
            context.setAlpha(0.24); context.beginTransparencyLayer(auxiliaryInfo: nil)
            context.setBlendMode(.copy)
            context.setFillColor((sourceMask.initial == "all" ? NSColor.systemGreen : .systemRed).cgColor)
            context.fill(extent)
            for operation in sourceMask.operations { draw(operation, in: context) }
            if let operation = gestureOperation { draw(operation, in: context) }
            context.endTransparencyLayer()
        }
        if let selectionPath {
            context.setAlpha(1); context.setBlendMode(.normal)
            context.addPath(selectionPath); context.setLineWidth(3 / scale)
            context.setStrokeColor(NSColor.black.cgColor); context.strokePath()
            context.addPath(selectionPath); context.setLineWidth(1 / scale)
            context.setStrokeColor(NSColor.white.cgColor); context.strokePath()
        }
        context.restoreGState()
    }
    private func draw(_ operation: [String: Any], in context: CGContext) {
        guard let op = operation["op"] as? String else { return }
        let color = op.hasPrefix("include") ? NSColor.systemGreen : .systemRed
        context.setFillColor(color.cgColor); context.setStrokeColor(color.cgColor)
        if op.hasSuffix("rect"), let x = operation["x"] as? Double,
           let y = operation["y"] as? Double, let width = operation["width"] as? Double,
           let height = operation["height"] as? Double {
            context.fill(CGRect(x: x, y: y, width: width, height: height))
        } else if let points = operation["points"] as? [[Double]],
                  let radius = operation["radius"] as? Double, let first = points.first, first.count == 2 {
            if points.count == 1 {
                context.fillEllipse(in: CGRect(x: first[0] - radius, y: first[1] - radius,
                    width: radius * 2, height: radius * 2))
            } else {
                context.beginPath(); context.move(to: CGPoint(x: first[0], y: first[1]))
                for point in points.dropFirst() where point.count == 2 {
                    context.addLine(to: CGPoint(x: point[0], y: point[1]))
                }
                context.setLineWidth(radius * 2); context.setLineCap(.round); context.setLineJoin(.round)
                context.strokePath()
            }
        }
    }
    private var gestureOperation: [String: Any]? {
        guard let first = points.first, let last = points.last else { return nil }
        switch mode {
        case .pan: return nil
        case .includeBrush, .excludeBrush:
            return ["op": mode == .includeBrush ? "include_brush" : "exclude_brush",
                "radius": Double(radius), "points": points.map { [Double($0.x), Double($0.y)] }]
        case .includeRect, .excludeRect:
            guard abs(last.x - first.x) > 0, abs(last.y - first.y) > 0 else { return nil }
            return ["op": mode == .includeRect ? "include_rect" : "exclude_rect",
                "x": Double(min(first.x, last.x)), "y": Double(min(first.y, last.y)),
                "width": Double(abs(last.x - first.x)), "height": Double(abs(last.y - first.y))]
        }
    }
    override func mouseDown(with event: NSEvent) {
        guard image != nil else { return }
        dragStart = convert(event.locationInWindow, from: nil); originalCenter = center
        if mode != .pan { points = [point(event)]; needsDisplay = true }
    }
    override func mouseDragged(with event: NSEvent) {
        guard let dragStart, let image else { return }
        if mode == .pan {
            let p = convert(event.locationInWindow, from: nil)
            center.x = min(1, max(0, originalCenter.x - (p.x - dragStart.x) / (CGFloat(image.width) * scale)))
            center.y = min(1, max(0, originalCenter.y - (p.y - dragStart.y) / (CGFloat(image.height) * scale)))
        } else if points.count < 4096 {
            let p = point(event)
            if mode == .includeRect || mode == .excludeRect { points = [points[0], p] }
            else { points.append(p) }
        }
        needsDisplay = true
    }
    override func mouseUp(with event: NSEvent) {
        if dragStart != nil { mouseDragged(with: event) }
        let operation = gestureOperation
        points = []; dragStart = nil; needsDisplay = true
        if let operation { onOperation?(operation) }
    }
}
