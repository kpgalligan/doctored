import AppKit

/// One image pixel per point at 100%, with nearest display sampling. Scrolling
/// and magnification belong to the surrounding NSScrollView. With the Refine
/// Edge brush on, drags are forwarded to the sheet through the `onBrush*`
/// closures and the strokes are drawn over the picture.
final class RefinementCanvasView: NSView {
    enum Background: Int, CaseIterable {
        case checker, white, black, gray, mask
        var title: String {
            switch self {
            case .checker: return "Checkerboard"
            case .white: return "White"
            case .black: return "Black"
            case .gray: return "50% Gray"
            case .mask: return "Mask"
            }
        }
    }

    var image: CGImage? { didSet { needsDisplay = true } }
    var background = Background.checker { didSet { needsDisplay = true } }
    /// The Refine Edge strokes to draw: the committed ones, then the draft.
    var strokes: [RefineEdgeStroke] = [] { didSet { needsDisplay = true } }
    /// A drag began at this canvas point; false leaves the drag to the view.
    var onBrushBegin: ((CGPoint) -> Bool)?
    var onBrushMove: ((CGPoint) -> Void)?
    var onBrushEnd: (() -> Void)?
    private var brushing = false
    override var isFlipped: Bool { true }

    override init(frame: NSRect) { super.init(frame: frame) }
    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("RefinementCanvasView does not support NSCoder") }

    override func mouseDown(with event: NSEvent) {
        brushing = onBrushBegin?(convert(event.locationInWindow, from: nil)) ?? false
        if !brushing { super.mouseDown(with: event) }
    }

    override func mouseDragged(with event: NSEvent) {
        guard brushing else { super.mouseDragged(with: event); return }
        onBrushMove?(convert(event.locationInWindow, from: nil))
    }

    override func mouseUp(with event: NSEvent) {
        guard brushing else { super.mouseUp(with: event); return }
        brushing = false
        onBrushEnd?()
    }

    override func draw(_ dirtyRect: NSRect) {
        let visible = dirtyRect.intersection(bounds)
        switch background {
        case .white: NSColor.white.setFill(); visible.fill()
        case .black, .mask: NSColor.black.setFill(); visible.fill()
        case .gray: NSColor(srgbRed: 0.5, green: 0.5, blue: 0.5, alpha: 1).setFill(); visible.fill()
        case .checker:
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
        }
        if let image {
            NSImage(cgImage: image, size: bounds.size).draw(in: bounds, from: .zero,
                operation: .sourceOver, fraction: 1, respectFlipped: true,
                hints: [.interpolation: NSImageInterpolation.none])
        }
        drawStrokes()
    }

    /// Each stroke as its round-capped path at brush width: forest where the
    /// edge will be refined, coral where it will not.
    private func drawStrokes() {
        for stroke in strokes {
            guard let first = stroke.points.first else { continue }
            let path = NSBezierPath()
            path.move(to: first)
            // A single point draws as a dot: a zero-length round-capped line.
            for point in stroke.points.count > 1 ? Array(stroke.points.dropFirst()) : [first] {
                path.line(to: point)
            }
            path.lineWidth = CGFloat(stroke.radius * 2)
            path.lineCapStyle = .round
            path.lineJoinStyle = .round
            (stroke.mode == .add ? DS.forest : DS.coral).withAlphaComponent(0.4).setStroke()
            path.stroke()
        }
    }
}
