import AppKit

/// A drag owns its inputs and options from mouse-down through release. The
/// temporary outline never changes authoritative selection/history state.
struct SelectionStrokeSession {
    let capture: SelectionCapture
    let tool: EditorTool
    let mode: SelectionCombineMode
    let feather: Double
    let tolerance: Double
    let edge: Double
    /// Quick Selection's brush radius in canvas pixels, `(size − 1) / 2`; 0
    /// for the lasso.
    let radius: Double
    /// Quick Selection's Lab pre-smoothing reach (the tool always passes 1).
    let smooth: Int
    /// Quick Selection's Enhance Edge.
    let enhanceEdge: Bool
    private(set) var points: [CGPoint]
    private(set) var overflowed = false

    init(capture: SelectionCapture, tool: EditorTool, mode: SelectionCombineMode,
         feather: Double, point: CGPoint) {
        self.capture = capture
        self.tool = tool
        self.mode = mode
        self.feather = feather
        tolerance = QuickSelectionSettings.shared.tolerance
        edge = QuickSelectionSettings.shared.edge
        let quick = tool == .quickSelect
        radius = quick ? (QuickSelectionSettings.shared.size - 1) / 2 : 0
        smooth = quick ? 1 : 0
        enhanceEdge = quick && QuickSelectionSettings.shared.enhanceEdge
        points = [point]
    }

    /// Keeps `point` when it is far enough from the last kept one. A Quick
    /// Selection stroke also thins to half its brush radius: every dropped
    /// point lies inside a kept point's disc, so it would seed nothing new,
    /// and a long drag with a large brush stays far below the core's
    /// brush-pixel cap.
    mutating func append(_ point: CGPoint, minimumDistance: CGFloat) {
        let spacing = tool == .quickSelect ? max(minimumDistance, CGFloat(radius) / 2) : minimumDistance
        guard !overflowed, let last = points.last,
              hypot(point.x - last.x, point.y - last.y) >= spacing else { return }
        guard points.count < (tool == .quickSelect ? 4096 : 10_000) else {
            overflowed = true
            return
        }
        points.append(point)
    }

    /// The kept points as integer canvas seeds.
    var seeds: [[Int]] {
        points.map { [min(capture.doc.width - 1, max(0, Int($0.x))),
                      min(capture.doc.height - 1, max(0, Int($0.y)))] }
    }

    /// The core's Quick Selection parameters for this stroke.
    var quickParameters: [String: Any] {
        ["seeds": seeds, "tolerance": tolerance, "edge": edge, "radius": radius, "smooth": smooth]
    }

    /// The `quick_select` arguments that reproduce this stroke.
    var quickArguments: [String: Any] {
        ["seeds": seeds, "tolerance": tolerance, "edge": edge, "brush_size": radius * 2 + 1,
         "smooth": smooth, "enhance_edge": enhanceEdge]
    }
}

extension ImageCanvasView {
    func beginSelectionStroke(at point: CGPoint, event: NSEvent) {
        guard let capture = onSelectionGestureCapture?() else { NSSound.beep(); return }
        var mode = SelectionCombineMode.from(event.modifierFlags, base: selectionCombineBase)
        // Quick Selection builds a selection up stroke by stroke: with no
        // modifier and the bar at New, a stroke adds to what is selected
        // (⌥ still subtracts; with nothing selected, add is a new selection).
        if tool == .quickSelect, mode == .replace { mode = .add }
        selectionStroke = SelectionStrokeSession(capture: capture, tool: tool, mode: mode,
            feather: selectionFeather, point: point)
        needsDisplay = true
    }

    func continueSelectionStroke(at point: CGPoint) {
        let clamped = CGPoint(x: min(max(0, point.x), bounds.width),
                              y: min(max(0, point.y), bounds.height))
        selectionStroke?.append(clamped, minimumDistance: max(0.5, 0.75 / magnification))
        needsDisplay = true
    }

    func endSelectionStroke(at point: CGPoint) {
        continueSelectionStroke(at: point)
        guard let completed = selectionStroke else { return }
        selectionStroke = nil
        needsDisplay = true
        onSelectionGestureCommit?(completed)
    }

    @discardableResult
    func cancelSelectionStroke() -> Bool {
        guard selectionStroke != nil else { return false }
        selectionStroke = nil
        needsDisplay = true
        return true
    }

    func drawSelectionStrokePreview() {
        guard let session = selectionStroke, let first = session.points.first else { return }
        let path = NSBezierPath()
        path.move(to: first)
        if session.tool == .quickSelect {
            // The brush's own footprint: the kept points joined at brush width.
            for point in session.points.count > 1 ? Array(session.points.dropFirst()) : [first] {
                path.line(to: point)
            }
            path.lineWidth = max(2 / magnification, CGFloat(session.radius * 2 + 1))
            path.lineCapStyle = .round
            path.lineJoinStyle = .round
            DS.marquee.withAlphaComponent(0.3).setStroke()
            path.stroke()
            return
        }
        for point in session.points.dropFirst() { path.line(to: point) }
        if session.tool == .freehandLasso, session.points.count > 2 { path.close() }
        path.lineWidth = 2 / magnification
        path.setLineDash([5 / magnification, 4 / magnification], count: 2, phase: 0)
        DS.marquee.setStroke()
        path.stroke()
    }
}
