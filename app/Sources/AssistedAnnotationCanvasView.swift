import AppKit

// The canvas view draws and reports gestures only, and implements `undo:`/`redo:` itself because
// `NSWindow` precedes its window controller in the responder chain (the `EditorViewController`
// reason).

/// The assisted annotation canvas: the case source at one view pixel per
/// native pixel, the starting mask as a rubylith overlay, the annotator's own
/// guidance on top, and a brush ring showing the current diameter.
///
/// It draws and reports gestures only. Every decision — whether a stroke is
/// accepted, what it costs, what the plane becomes — belongs to
/// `AssistedAnnotationSession` through the `on*` closures. NOTHING here knows
/// about a reference, a candidate or a score.
///
/// `undo:`/`redo:` are implemented HERE, on the first responder, because
/// `NSWindow` precedes its window controller in the responder chain and would
/// otherwise run Cmd-Z against its own (empty) undo manager — the same reason
/// `EditorViewController` intercepts them.
final class AssistedAnnotationCanvasView: NSView, NSUserInterfaceValidations {
    var sourceImage: CGImage? { didSet { needsDisplay = true } }
    var priorOverlay: CGImage? { didSet { needsDisplay = true } }
    var guidanceOverlay: CGImage? { didSet { needsDisplay = true } }
    var showPrior = false { didSet { needsDisplay = true } }
    var showGuidance = true { didSet { needsDisplay = true } }
    var diameter = 9 { didSet { needsDisplay = true } }

    var onStrokeBegin: (([Int]) -> Bool)?
    var onStrokeMove: (([Int]) -> Void)?
    var onStrokeEnd: (() -> Void)?
    var onUndo: (() -> Void)?
    var onRedo: (() -> Void)?
    var canUndo: (() -> Bool)?
    var canRedo: (() -> Bool)?

    private var isStroking = false
    private var cursorPoint: NSPoint?
    private var tracking: NSTrackingArea?

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    override init(frame: NSRect) { super.init(frame: frame); wantsLayer = true }
    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("AssistedAnnotationCanvasView does not support NSCoder")
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let existing = tracking { removeTrackingArea(existing) }
        let area = NSTrackingArea(
            rect: .zero, options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow,
                                   .inVisibleRect], owner: self, userInfo: nil)
        addTrackingArea(area)
        tracking = area
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let context = NSGraphicsContext.current?.cgContext else { return }
        context.setFillColor(NSColor.black.cgColor)
        context.fill(dirtyRect)
        context.interpolationQuality = .none
        let full = CGRect(origin: .zero, size: bounds.size)
        if let image = sourceImage { context.draw(image, in: full) }
        if showPrior, let image = priorOverlay { context.draw(image, in: full) }
        if showGuidance, let image = guidanceOverlay { context.draw(image, in: full) }
        drawCursorRing(context)
    }

    private func drawCursorRing(_ context: CGContext) {
        guard let point = cursorPoint else { return }
        let side = CGFloat(diameter)
        let ring = CGRect(x: point.x - side / 2, y: point.y - side / 2, width: side, height: side)
        context.setLineWidth(1)
        context.setStrokeColor(NSColor.white.withAlphaComponent(0.9).cgColor)
        context.strokeEllipse(in: ring)
        context.setStrokeColor(NSColor.black.withAlphaComponent(0.6).cgColor)
        context.strokeEllipse(in: ring.insetBy(dx: -1, dy: -1))
    }

    // MARK: - Mouse

    private func canvasPoint(_ event: NSEvent) -> [Int] {
        let local = convert(event.locationInWindow, from: nil)
        return [Int(local.x.rounded(.down)), Int(local.y.rounded(.down))]
    }

    override func mouseMoved(with event: NSEvent) {
        cursorPoint = convert(event.locationInWindow, from: nil)
        needsDisplay = true
    }

    override func mouseExited(with event: NSEvent) {
        cursorPoint = nil
        needsDisplay = true
    }

    override func mouseDown(with event: NSEvent) {
        cursorPoint = convert(event.locationInWindow, from: nil)
        isStroking = onStrokeBegin?(canvasPoint(event)) ?? false
        needsDisplay = true
    }

    override func mouseDragged(with event: NSEvent) {
        cursorPoint = convert(event.locationInWindow, from: nil)
        if isStroking { onStrokeMove?(canvasPoint(event)) }
        needsDisplay = true
    }

    override func mouseUp(with event: NSEvent) {
        cursorPoint = convert(event.locationInWindow, from: nil)
        endStrokeNow()
        needsDisplay = true
    }

    /// Ends an in-progress stroke exactly as mouse-up would, so the one-second
    /// budget tick can close a gesture the annotator is still drawing; a no-op
    /// when nothing is in progress.
    func endStrokeNow() {
        guard isStroking else { return }
        isStroking = false
        onStrokeEnd?()
    }

    // MARK: - Undo plumbing

    @objc func undo(_ sender: Any?) { onUndo?() }

    @objc func redo(_ sender: Any?) { onRedo?() }

    func validateUserInterfaceItem(_ item: NSValidatedUserInterfaceItem) -> Bool {
        switch item.action {
        case #selector(undo(_:)): return canUndo?() ?? false
        case #selector(redo(_:)): return canRedo?() ?? false
        default: return false
        }
    }

    // MARK: - Overlays

    private static func rgbaImage(_ pixels: [UInt8], width: Int, height: Int) -> CGImage? {
        guard width > 0, height > 0, pixels.count == width * height * 4,
              let provider = CGDataProvider(data: Data(pixels) as CFData)
        else { return nil }
        return CGImage(
            width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32,
            bytesPerRow: width * 4, space: ColorProfile.sRGB,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
    }

    /// The starting mask, rubylith style: half-opaque red wherever the prior
    /// is below mid grey. It is context, never an answer.
    static func priorOverlayImage(_ prior: [UInt8], width: Int, height: Int) -> CGImage? {
        guard prior.count == width * height else { return nil }
        var pixels = [UInt8](repeating: 0, count: prior.count * 4)
        for (index, sample) in prior.enumerated() where sample < 128 {
            pixels[index * 4] = 220
            pixels[index * 4 + 3] = 128
        }
        return rgbaImage(pixels, width: width, height: height)
    }

    /// The annotator's own labels: green Foreground, red Background. An
    /// Unknown-stroked pixel is indistinguishable from an untouched one and
    /// deliberately draws nothing, because Unknown RELEASES a constraint.
    static func guidanceOverlayImage(_ plane: [UInt8], width: Int, height: Int) -> CGImage? {
        guard plane.count == width * height else { return nil }
        var pixels = [UInt8](repeating: 0, count: plane.count * 4)
        for (index, sample) in plane.enumerated() {
            switch sample {
            case 255:
                pixels[index * 4 + 1] = 210
                pixels[index * 4 + 3] = 140
            case 0:
                pixels[index * 4] = 230
                pixels[index * 4 + 3] = 140
            default:
                break
            }
        }
        return rgbaImage(pixels, width: width, height: height)
    }
}
