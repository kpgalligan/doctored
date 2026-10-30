import AppKit

// The Object Selection tool: drag a rectangle or a lasso around something and the selection
// becomes that object's outline. Vision's foreground instances say what the objects are
// (`SubjectTargeting`, the Select Objects picker's analysis); the drag says which one is meant and
// bounds the result; the edge stage (`SelectionCapture.refinedEdge`) then follows the picture's own
// edge. The tool and the `object_select` MCP tool share `ObjectSelection.shape`.
// In a 32-bit float document Vision reads the tone-mapped display at view exposure 0
// (`ImageDocument.analysisComposite(of:)`) and the edge stage is opt-in (`refines` is the default):
// it runs on the R1 display codes (`display_encoding`'s rule E), where an above-white subject on an
// above-white background is flat white, so by default it would rewrite an outline it cannot see.

/// How the tool's drag describes its region.
enum ObjectSelectionMode: String, CaseIterable {
    case rectangle, lasso

    var title: String { self == .rectangle ? "Rectangle" : "Lasso" }
}

/// The options bar's Mode, persisted like Quick Selection's settings.
final class ObjectSelectionSettings {
    static let shared = ObjectSelectionSettings()
    var mode: ObjectSelectionMode {
        get {
            UserDefaults.standard.string(forKey: "ObjectSelectionMode")
                .flatMap(ObjectSelectionMode.init(rawValue:)) ?? .rectangle
        }
        set { UserDefaults.standard.set(newValue.rawValue, forKey: "ObjectSelectionMode") }
    }
    /// Object Finder (`ObjectFinder.swift`): in Rectangle mode a click selects the
    /// object under it and a drag is a box prompt for SAM 2.1; Shift- and ⌥-clicks refine a live
    /// prompt. Off by default; the lasso keeps Vision's path either way.
    var objectFinder: Bool {
        get { UserDefaults.standard.bool(forKey: "ObjectSelectionObjectFinder") }
        set { UserDefaults.standard.set(newValue, forKey: "ObjectSelectionObjectFinder") }
    }
}

enum ObjectSelection {
    /// An instance is "inside" the region when its instance-map cells there
    /// number at least this share of the best-represented instance's: a
    /// neighbour that only clips the region's edge is not what was dragged
    /// around.
    static let minimumShare = 0.05
    static let noObject = "No object was found inside the region."

    /// Whether the edge stage (`SelectionCapture.refinedEdge`) runs for `doc` BY DEFAULT: false in a
    /// 32-bit float document, where Vision found the object on the document's own tone map and the
    /// edge stage reads R1 display codes, which see an above-white subject on an above-white
    /// background as flat white (rule E). `object_select` admits `refine: true` there.
    static func refines(_ doc: RasterDocument) -> Bool { !doc.isFloat }

    /// The coverage of the foreground object(s) inside `region` — a rect or
    /// polygon shape whose bounding box is `bounds` — clipped to the region,
    /// with the instances chosen (1-based, `inspect_subjects`' numbering) and
    /// how many the picture has. `refine` re-estimates the edge from the
    /// picture within a radius that scales with the region (its longer side
    /// over 512, rounded, held to 2–16 px), with Smart Radius so a clean
    /// edge stays as Vision drew it, and clips to the region again. Runs the
    /// analysis, the mask and the refinement as modal jobs on the main thread.
    static func shape(_ capture: SelectionCapture, in document: ImageDocument,
                      region: CanvasSelection.Shape, bounds: CGRect,
                      refine: Bool) throws -> (shape: CanvasSelection.Shape, instances: [Int], instanceCount: Int) {
        let width = capture.doc.width, height = capture.doc.height
        guard let area = CanvasSelection(shape: region, canvasWidth: width, canvasHeight: height) else {
            throw DoctoredCoreError(message: "The region lies outside the canvas.")
        }
        let analysis = try SubjectTargeting.analyze(capture, in: document)
        let instances = analysis.subjects(inside: bounds, minimumShare: minimumShare)
        guard !instances.isEmpty else { throw DoctoredCoreError(message: noObject) }
        let object = try SubjectTargeting.mask(analysis, instances: instances, capture: capture, in: document)
        // The intersection is `CanvasSelection.combine(…, mode: .intersect)`'s
        // own per-sample formula, taken on the planes so no contour is traced
        // for an intermediate that is never shown.
        let clip = area.maskBytes()
        let clipped = PlaneAlgebra.combine(object, clip, mode: .intersect)
        guard clipped.contains(where: { $0 != 0 }) else { throw DoctoredCoreError(message: noObject) }
        guard refine else { return (.mask(clipped), instances, analysis.instanceCount) }
        let radius = min(16, max(2, (Double(max(bounds.width, bounds.height)) / 512).rounded()))
        let refined = try capture.refinedEdge(.mask(clipped), edgeRadius: radius, smartRadius: true,
                                              area: nil, in: document, title: "Object Selection")
        let result: CanvasSelection.Shape
        switch refined {
        case .mask16(let samples):
            result = .mask16(PlaneAlgebra.combine(samples, clip.map { UInt16($0) * 257 }, mode: .intersect))
        case .mask(let bytes):
            result = .mask(PlaneAlgebra.combine(bytes, clip, mode: .intersect))
        case .rect, .ellipse, .polygon:
            result = refined
        }
        return (result, instances, analysis.instanceCount)
    }
}

/// One Object Selection drag: its capture and options from mouse-down, and
/// the points so far. A rectangle keeps its two corners; a lasso its path.
struct ObjectSelectionSession {
    let capture: SelectionCapture
    let mode: ObjectSelectionMode
    let combine: SelectionCombineMode
    let feather: Double
    /// The mouse-down's modifiers: with Object Finder, Shift and ⌥ on a click refine a live prompt.
    let modifiers: NSEvent.ModifierFlags
    private(set) var points: [CGPoint]
    /// Set when the gesture ends: a click (under 4 view pixels of travel) rather than a drag.
    var isClick = false

    init(capture: SelectionCapture, mode: ObjectSelectionMode, combine: SelectionCombineMode,
         feather: Double, point: CGPoint, modifiers: NSEvent.ModifierFlags = []) {
        self.capture = capture
        self.mode = mode
        self.combine = combine
        self.feather = feather
        self.modifiers = modifiers
        points = [point]
    }

    mutating func append(_ point: CGPoint, minimumDistance: CGFloat) {
        guard let first = points.first, let last = points.last else { return }
        switch mode {
        case .rectangle:
            points = [first, point]
        case .lasso:
            guard points.count < 10_000,
                  hypot(point.x - last.x, point.y - last.y) >= minimumDistance else { return }
            points.append(point)
        }
    }

    /// The farthest the drag has gone from where it began, in canvas pixels.
    var reach: CGFloat {
        guard let first = points.first else { return 0 }
        return points.map { hypot($0.x - first.x, $0.y - first.y) }.max() ?? 0
    }

    /// The dragged rectangle, whole pixels (rectangle mode).
    var rect: CGRect {
        guard let first = points.first, let last = points.last else { return .zero }
        return CGRect(x: min(first.x, last.x), y: min(first.y, last.y),
                      width: abs(last.x - first.x), height: abs(last.y - first.y)).integral
    }

    /// The region the drag describes and its bounding box, or nil when it
    /// encloses nothing (a rectangle with no area, a lasso under 3 points).
    var region: (shape: CanvasSelection.Shape, bounds: CGRect)? {
        switch mode {
        case .rectangle:
            let dragged = rect
            guard dragged.width >= 1, dragged.height >= 1 else { return nil }
            return (.rect(dragged), dragged)
        case .lasso:
            guard points.count >= 3, let first = points.first else { return nil }
            let box = points.dropFirst().reduce(CGRect(origin: first, size: .zero)) {
                $0.union(CGRect(origin: $1, size: .zero))
            }
            guard box.width >= 1, box.height >= 1 else { return nil }
            return (.polygon(points), box)
        }
    }
}

extension ImageCanvasView {
    func beginObjectSelection(at point: CGPoint, event: NSEvent) {
        guard let capture = onSelectionGestureCapture?() else { NSSound.beep(); return }
        objectSelection = ObjectSelectionSession(capture: capture,
            mode: ObjectSelectionSettings.shared.mode,
            combine: SelectionCombineMode.from(event.modifierFlags, base: selectionCombineBase),
            feather: selectionFeather, point: point,
            modifiers: event.modifierFlags.intersection([.shift, .option]))
        needsDisplay = true
    }

    func continueObjectSelection(at point: CGPoint) {
        let clamped = CGPoint(x: min(max(0, point.x), bounds.width),
                              y: min(max(0, point.y), bounds.height))
        objectSelection?.append(clamped, minimumDistance: max(0.5, 0.75 / magnification))
        needsDisplay = true
    }

    /// Commits the drag. One shorter than 4 view pixels is a click, which
    /// names no region and is ignored — unless Object Finder is on in
    /// Rectangle mode, where a click is a point prompt.
    func endObjectSelection(at point: CGPoint) {
        continueObjectSelection(at: point)
        guard var completed = objectSelection else { return }
        objectSelection = nil
        needsDisplay = true
        completed.isClick = completed.reach * magnification < 4
        if completed.isClick {
            guard completed.mode == .rectangle, ObjectSelectionSettings.shared.objectFinder else { return }
        }
        onObjectSelectionCommit?(completed)
    }

    @discardableResult
    func cancelObjectSelection() -> Bool {
        guard objectSelection != nil else { return false }
        objectSelection = nil
        needsDisplay = true
        return true
    }

    func drawObjectSelectionPreview() {
        guard let session = objectSelection, let first = session.points.first else { return }
        let path: NSBezierPath
        switch session.mode {
        case .rectangle:
            path = NSBezierPath(rect: session.rect)
        case .lasso:
            path = NSBezierPath()
            path.move(to: first)
            for point in session.points.dropFirst() { path.line(to: point) }
            if session.points.count > 2 { path.close() }
        }
        path.lineWidth = 2 / magnification
        path.setLineDash([5 / magnification, 4 / magnification], count: 2, phase: 0)
        DS.marquee.setStroke()
        path.stroke()
    }
}
