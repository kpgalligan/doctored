import AppKit

/// Display-only coverage and source-coordinate gestures. The native mask API
/// authors every coverage sample; this view records bounded numeric geometry.
/// Without a Develop `canvas` it maps through `mapper` (the Develop Filter's
/// mask thumbnail, `DevelopFilterMaskCanvas`), and counts as showing After.
final class RawLocalGestureView: NSView {
    /// `sample` is the range rows' Sample Colour: a click reports its source point and edits
    /// no coverage.
    enum Tool: Int { case pan, radial, linear, add, subtract, sample }
    weak var canvas: RawDevelopCanvasView?
    /// The mapping when `canvas` is nil: the displayed rect and the source it shows.
    var mapper: (() -> Mapping?)?
    var tool: Tool = .pan { didSet { points = []; needsDisplay = true } }
    var overlay: CGImage? { didSet { needsDisplay = true } }
    var showsOverlay = true { didSet { needsDisplay = true } }
    var radius: Double = 24
    var hardness: Double = 0.7
    var opacity: Double = 1
    var enabled = true
    var onCommand: (([String: Any], Int) -> Void)?
    var onSample: ((CGPoint) -> Void)?
    var onError: ((String) -> Void)?
    private var points: [CGPoint] = []
    private var gestureMap: Mapping?
    private var refused = false

    struct Mapping {
        let rect: CGRect
        let source: CGRect
        func sourcePoint(_ point: CGPoint) -> CGPoint {
            CGPoint(x: source.minX + min(1, max(0, (point.x - rect.minX) / rect.width)) * source.width,
                y: source.minY + min(1, max(0, (point.y - rect.minY) / rect.height)) * source.height)
        }
        func displayPoint(_ point: CGPoint) -> CGPoint {
            CGPoint(x: rect.minX + (point.x - source.minX) / source.width * rect.width,
                y: rect.minY + (point.y - source.minY) / source.height * rect.height)
        }
    }

    override var isFlipped: Bool { true }
    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        setAccessibilityIdentifier("raw-local-gesture")
        setAccessibilityLabel("Local RAW mask in original source coordinates")
    }
    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("RawLocalGestureView does not support NSCoder") }

    func mapping() -> Mapping? {
        guard let canvas else { return mapper?() }
        guard let frame = canvas.preview else { return nil }
        let image = frame.afterDisplay
        let size = CGSize(width: image.width, height: image.height)
        let scale = frame.viewport.nativeDetail
            ? 1 / (window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 1)
            : min(bounds.width / size.width, bounds.height / size.height)
        let rect = CGRect(x: (bounds.width - size.width * scale) / 2,
            y: (bounds.height - size.height * scale) / 2,
            width: size.width * scale, height: size.height * scale)
        let region = frame.viewport.region(frame.after.image)
        guard rect.width > 0, rect.height > 0 else { return nil }
        return Mapping(rect: rect, source: CGRect(x: region.x, y: region.y,
            width: region.width, height: region.height))
    }

    override func hitTest(_ point: NSPoint) -> NSView? {
        guard tool != .pan, enabled, mapping() != nil else { return nil }
        return super.hitTest(point)
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let map = mapping(), let context = NSGraphicsContext.current?.cgContext else { return }
        if showsOverlay, let overlay {
            context.saveGState()
            if canvas?.comparison == .before { context.clip(to: .zero) }
            else if canvas?.comparison == .split {
                context.clip(to: CGRect(x: bounds.midX, y: 0, width: bounds.width / 2, height: bounds.height))
            }
            context.setAlpha(0.4)
            context.interpolationQuality = canvas?.preview?.viewport.nativeDetail == true ? .none : .high
            context.translateBy(x: map.rect.minX, y: map.rect.maxY)
            context.scaleBy(x: 1, y: -1)
            context.draw(overlay, in: CGRect(origin: .zero, size: map.rect.size))
            context.restoreGState()
        }
        guard let first = points.first, let last = points.last else { return }
        let start = map.displayPoint(first), end = map.displayPoint(last)
        let path: NSBezierPath
        if tool == .radial {
            path = NSBezierPath(ovalIn: CGRect(x: min(start.x, end.x), y: min(start.y, end.y),
                width: abs(end.x - start.x), height: abs(end.y - start.y)))
        } else {
            path = NSBezierPath()
            path.move(to: start)
            if tool == .linear { path.line(to: end) }
            else { for point in points.dropFirst() { path.line(to: map.displayPoint(point)) } }
        }
        NSColor.white.setStroke()
        path.lineWidth = 1
        path.stroke()
    }

    override func mouseDown(with event: NSEvent) {
        guard enabled, let map = mapping() else { return }
        guard canvas == nil || canvas?.comparison == .after else {
            refuse(tool == .sample ? "Choose After to sample a colour."
                : "Choose After to draw a local mask.")
            return
        }
        let point = convert(event.locationInWindow, from: nil)
        guard map.rect.contains(point) else { return }
        gestureMap = map
        points = [map.sourcePoint(point)]
        refused = false
        needsDisplay = true
    }

    override func mouseDragged(with event: NSEvent) {
        guard let map = gestureMap, !refused else { return }
        guard points.count < 4096 else { refuse("This gesture exceeds 4,096 input points. No partial mask was applied."); return }
        let point = map.sourcePoint(convert(event.locationInWindow, from: nil))
        if tool == .radial || tool == .linear {
            if points.count == 1 { points.append(point) } else { points[1] = point }
        } else if tool == .sample {
            points = [point]
        } else if point != points.last { points.append(point) }
        needsDisplay = true
    }

    override func mouseUp(with event: NSEvent) {
        defer { gestureMap = nil; points = []; needsDisplay = true }
        guard let map = gestureMap, !refused, let first = points.first else { return }
        let last = map.sourcePoint(convert(event.locationInWindow, from: nil))
        switch tool {
        case .pan: return
        case .sample: onSample?(last)
        case .radial:
            let rx = abs(last.x - first.x) / 2, ry = abs(last.y - first.y) / 2
            guard rx >= 0.5, ry >= 0.5 else { refuse("Drag a radial mask at least one source pixel wide and high."); return }
            onCommand?(["op": "radial", "center": [(first.x + last.x) / 2, (first.y + last.y) / 2],
                "radius": [rx, ry], "hardness": hardness], 0)
        case .linear:
            guard hypot(last.x - first.x, last.y - first.y) >= 0.5 else {
                refuse("Drag a linear transition across the source image."); return
            }
            onCommand?(["op": "linear", "from": [first.x, first.y], "to": [last.x, last.y]], 0)
        case .add, .subtract:
            var path = points
            if path.last != last { path.append(last) }
            let spacing = CGFloat(max(0.25, radius * 0.25))
            let length = zip(path, path.dropFirst()).reduce(CGFloat.zero) { total, pair in
                total + hypot(pair.1.x - pair.0.x, pair.1.y - pair.0.y)
            }
            guard path.count <= 4096, radius.isFinite, radius > 0, length.isFinite,
                  floor(length / spacing) + 1 <= 4096 else {
                refuse("This stroke exceeds 4,096 dabs. Use a shorter stroke or larger brush; no prefix was applied."); return
            }
            // Reuse the established geometric walker only. Its byte dab
            // rasterizer is deliberately not part of the local RAW path.
            let centers: [CGPoint]
            do { centers = try SoftBrush.stampCenters(along: path, spacing: spacing) } catch {
                refuse(error.localizedDescription); return
            }
            let dabs = centers.map { ["center": [$0.x, $0.y], "radius": radius,
                "hardness": hardness, "opacity": opacity] as [String: Any] }
            onCommand?(["op": "brush", "mode": tool == .add ? "add" : "subtract", "dabs": dabs], dabs.count)
        }
    }

    override func scrollWheel(with event: NSEvent) {
        guard gestureMap == nil else { return }
        canvas?.scrollWheel(with: event)
    }
    private func refuse(_ message: String) {
        refused = true
        onError?(message)
        NSSound.beep()
    }
    func clear() {
        overlay = nil; points = []; gestureMap = nil
        onCommand = nil; onSample = nil; onError = nil; canvas = nil; mapper = nil
    }
}
