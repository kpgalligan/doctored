import AppKit

/// Fit and native-pixel inspection share the same captured display images. At
/// 100%, one source pixel occupies one backing pixel, including Retina screens.
/// Pan changes only a normalized crop center; every crop comes from the native
/// master: u16, or an f32 master's SDR-clipped view, or its extended image on
/// a screen with headroom (#680).
final class RawDevelopCanvasView: NSView {
    enum Comparison: Int { case after, before, split }
    var comparison: Comparison = .after { didSet { needsDisplay = true } }
    var preview: RawDevelopPreviewFrame? {
        didSet {
            if let preview {
                nativeDetail = preview.viewport.nativeDetail
                nativeSize = CGSize(width: preview.after.image.width, height: preview.after.image.height)
            }
            applyExtendedRange(preview?.extendedPeak != nil)
            needsDisplay = true
        }
    }
    /// The EDR layer and window state (`RawDevelopHDRPreview.swift`).
    var extendedRange = RawDevelopExtendedRangeState()
    var onPan: ((CGPoint) -> Void)?
    var onViewportChange: (() -> Void)?
    private var dragPoint: CGPoint?
    private var nativeDetail = false
    private var nativeSize = CGSize(width: 1, height: 1)

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        setAccessibilityIdentifier("raw-develop-canvas")
        setAccessibilityLabel("RAW preview; drag to pan in 100 percent detail")
    }
    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("RawDevelopCanvasView does not support NSCoder") }

    /// A frame that arrived before the canvas had a window engages EDR now.
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if window != nil { applyExtendedRange(preview?.extendedPeak != nil) }
    }

    var backingSize: CGSize {
        let size = bounds.size
        let scale = window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 1
        return CGSize(width: max(1, size.width * scale), height: max(1, size.height * scale))
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        onViewportChange?()
    }

    override func draw(_ dirtyRect: NSRect) {
        DS.canvasVoid.setFill()
        bounds.fill()
        guard let preview, let context = NSGraphicsContext.current?.cgContext else { return }
        func draw(_ image: CGImage, clippedTo clip: CGRect? = nil) {
            let imageSize = CGSize(width: image.width, height: image.height)
            let scale: CGFloat
            if preview.viewport.nativeDetail {
                scale = 1 / (window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 1)
            } else {
                scale = min(bounds.width / imageSize.width, bounds.height / imageSize.height)
            }
            let rect = CGRect(x: (bounds.width - imageSize.width * scale) / 2,
                y: (bounds.height - imageSize.height * scale) / 2,
                width: imageSize.width * scale, height: imageSize.height * scale)
            context.saveGState()
            if let clip { context.clip(to: clip) }
            context.interpolationQuality = preview.viewport.nativeDetail ? .none : .high
            context.translateBy(x: rect.minX, y: rect.maxY)
            context.scaleBy(x: 1, y: -1)
            context.draw(image, in: CGRect(origin: .zero, size: rect.size))
            context.restoreGState()
        }
        switch comparison {
        case .after: draw(preview.afterDisplay)
        case .before: draw(preview.beforeDisplay)
        case .split:
            draw(preview.beforeDisplay, clippedTo: CGRect(x: 0, y: 0, width: bounds.width / 2, height: bounds.height))
            draw(preview.afterDisplay, clippedTo: CGRect(x: bounds.width / 2, y: 0, width: bounds.width / 2, height: bounds.height))
            DS.textStrong.setStroke()
            let divider = NSBezierPath()
            divider.move(to: CGPoint(x: bounds.midX, y: 0))
            divider.line(to: CGPoint(x: bounds.midX, y: bounds.height))
            divider.lineWidth = 1
            divider.stroke()
        }
        let names: [String]
        switch comparison {
        case .after: names = ["After"]
        case .before: names = ["Before"]
        case .split: names = ["Before", "After"]
        }
        for (index, name) in names.enumerated() {
            let x: CGFloat = index == 0 ? 9 : bounds.midX + 9
            let attrs: [NSAttributedString.Key: Any] = [.font: DS.mono(11), .foregroundColor: NSColor.white,
                .backgroundColor: NSColor.black.withAlphaComponent(0.65)]
            (" \(name) " as NSString).draw(at: CGPoint(x: x, y: 9), withAttributes: attrs)
        }
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        dragPoint = convert(event.locationInWindow, from: nil)
    }
    override func mouseDragged(with event: NSEvent) {
        guard let previous = dragPoint, nativeDetail else { return }
        let point = convert(event.locationInWindow, from: nil)
        let scale = window?.backingScaleFactor ?? 1
        let delta = CGPoint(x: (previous.x - point.x) * scale / nativeSize.width,
            y: (previous.y - point.y) * scale / nativeSize.height)
        dragPoint = point
        onPan?(delta)
    }
    override func mouseUp(with event: NSEvent) { dragPoint = nil }
    override func scrollWheel(with event: NSEvent) {
        guard nativeDetail else { super.scrollWheel(with: event); return }
        let scale = window?.backingScaleFactor ?? 1
        onPan?(CGPoint(x: event.scrollingDeltaX * scale / nativeSize.width,
            y: event.scrollingDeltaY * scale / nativeSize.height))
    }
    override func keyDown(with event: NSEvent) {
        guard let preview, preview.viewport.nativeDetail else { super.keyDown(with: event); return }
        let delta: CGPoint
        switch event.keyCode {
        case 123: delta = CGPoint(x: -64, y: 0)
        case 124: delta = CGPoint(x: 64, y: 0)
        case 125: delta = CGPoint(x: 0, y: 64)
        case 126: delta = CGPoint(x: 0, y: -64)
        default: super.keyDown(with: event); return
        }
        onPan?(CGPoint(x: delta.x / CGFloat(preview.after.image.width),
            y: delta.y / CGFloat(preview.after.image.height)))
    }
}
