import AppKit

// `MagneticLassoSession` is NSView-free so the canvas and MCP `select_magnetic` drive one set of
// rules — `fixed` path + `anchors`, the pointer `trail` since the last anchor and the `live` wire,
// each move traced on main by `RasterEdgeMap.trace` (`dr_selection_trace_edge`) over the
// `SelectionCapture`'s byte source (at 16 bits its display derivative, at 32-bit float its R1
// trace image `edgeTraceComposite`, rule M; geometry out only) through the session's one edge
// map, so a move computes no edge sample an earlier move of the outline computed,
// force-fastened when the trail fills or the projected corridor passes `DR_EDGE_TRACE_WORK_BUDGET`
// (at any Frequency, 0 included), and fastened by the core
// where a trail loops back within 2·width of its anchor around an enclosed area — an overshoot
// backed up along itself is not a loop — (two legs joined at the far point, at any Frequency, so a
// Frequency-0 loop keeps its shape); ⌫ re-traces at Frequency 0 so one press removes exactly one
// anchor; a magnetic close traces back to the start in budget-sized pieces at the Width set.
// Input (the phase-6 ADR, "Magnetic Lasso input"): `freehand(to:)` joins ⌥-drag points as given
// until `endFreehand` makes the last one an anchor; pen pressure narrows `traceWidth`.

/// The Magnetic Lasso's three options: Width, the corridor
/// radius in px the tracer looks for an edge in (1–256); Contrast, the edge
/// strength in % below which a gradient is not an edge (1–100); Frequency,
/// how soon the tracer fastens an anchor (0–100, 0 = only at the work budget
/// or where the trail loops back to the anchor).
struct MagneticLassoOptions: Equatable {
    var width: Double
    var contrast: Double
    var frequency: Double

    static let defaults = MagneticLassoOptions(width: 10, contrast: 10, frequency: 57)
    static let widthRange = 1.0...256
    static let contrastRange = 1.0...100
    static let frequencyRange = 0.0...100

    var isValid: Bool {
        Self.widthRange.contains(width) && Self.contrastRange.contains(contrast)
            && Self.frequencyRange.contains(frequency)
    }

    /// The corridor radius a trace uses under pen pressure `pressure` (0–1;
    /// nil for a mouse or a pen with Pen Pressure off): `max(1, width·(1 − p))`,
    /// so bearing down narrows the corridor and the Width set stays the
    /// ceiling (the phase-6 ADR, "Magnetic Lasso input").
    static func traceWidth(_ width: Double, pressure: Double?) -> Double {
        guard let pressure else { return width }
        return max(1, width * (1 - min(max(pressure, 0), 1)))
    }
}

/// The tool's persisted options — separate keys, the `QuickSelectionSettings`
/// pattern. A stored value outside its range reads back clamped.
final class MagneticLassoSettings {
    static let shared = MagneticLassoSettings()

    var options: MagneticLassoOptions {
        get {
            let defaults = MagneticLassoOptions.defaults
            return MagneticLassoOptions(
                width: Self.read("MagneticLassoWidth", defaults.width, MagneticLassoOptions.widthRange),
                contrast: Self.read("MagneticLassoContrast", defaults.contrast,
                                    MagneticLassoOptions.contrastRange),
                frequency: Self.read("MagneticLassoFrequency", defaults.frequency,
                                     MagneticLassoOptions.frequencyRange))
        }
        set {
            UserDefaults.standard.set(newValue.width, forKey: "MagneticLassoWidth")
            UserDefaults.standard.set(newValue.contrast, forKey: "MagneticLassoContrast")
            UserDefaults.standard.set(newValue.frequency, forKey: "MagneticLassoFrequency")
        }
    }

    /// The options bar's Pen Pressure: a pen bearing down narrows the trace
    /// width (`MagneticLassoOptions.traceWidth`). On by default.
    var penPressure: Bool {
        get { UserDefaults.standard.object(forKey: "MagneticLassoPenPressure") as? Bool ?? true }
        set { UserDefaults.standard.set(newValue, forKey: "MagneticLassoPenPressure") }
    }

    private static func read(_ key: String, _ fallback: Double, _ range: ClosedRange<Double>) -> Double {
        guard let value = UserDefaults.standard.object(forKey: key) as? Double, value.isFinite else {
            return fallback
        }
        return min(max(value, range.lowerBound), range.upperBound)
    }
}

/// One Magnetic Lasso outline in progress, NSView-free so the canvas and the
/// `select_magnetic` MCP tool drive the very same rules. `fixed` is the
/// fastened path from the starting click through the last anchor (`anchors`
/// index it, the start is anchor 0); `trail` is the pointer's positions since
/// that anchor; `live` is the current live wire from the anchor (`live[0]` is
/// the anchor). Every trace runs on the main thread through the core's
/// `dr_selection_trace_edge`, over the capture's byte source (at 16 bits the
/// display derivative `SelectionCapture` makes); the output is geometry only.
/// A traced vertex sits on the edge its pixel locates and floors to that
/// pixel, so an anchor passes straight back to the core as the next `from`.
struct MagneticLassoSession {
    let capture: SelectionCapture
    let mode: SelectionCombineMode
    let feather: Double
    var options: MagneticLassoOptions
    private(set) var fixed: [CGPoint]
    private(set) var anchors: [Int]
    private(set) var trail: [CGPoint]
    private(set) var live: [CGPoint]
    /// The latest pointer event's pen pressure (0–1), nil for none: the
    /// unfastened wire is traced at `traceWidth`.
    private(set) var pressure: Double?
    /// A freehand (⌥-drag) segment is open: its points are fixed as given,
    /// and its last point becomes an anchor when it ends (`endFreehand`).
    private(set) var freehandOpen = false
    /// The press that started the outline is still down. Its drag traces
    /// whatever ⌥ says, since ⌥ at that press chose Subtract; the canvas
    /// sets it at that press and clears it at mouse-up.
    var startingPressHeld = false
    /// The capture source's edge map, made once per outline. The session is
    /// copied on every move; every copy shares this one map, which is the
    /// point: its samples outlive the move that computed them.
    private let edgeMap: RasterEdgeMap

    /// The corridor pixels one pointer move may cost before the session
    /// fastens and starts afresh — the core's own figure, so a hover trace
    /// through the session's edge map, once it holds the corridor's samples,
    /// stays inside a frame.
    static let workBudget = Double(DR_EDGE_TRACE_WORK_BUDGET)
    /// The closed outline's vertex limit, `select_freehand`'s.
    static let outlineLimit = 10_000
    /// The core's trail bound per trace.
    private static let trailLimit = 4096

    init(capture: SelectionCapture, start: CGPoint, mode: SelectionCombineMode, feather: Double,
         options: MagneticLassoOptions) throws {
        guard let source = capture.source else {
            throw DoctoredCoreError(message: "The Magnetic Lasso needs an image to trace.")
        }
        guard options.isValid else {
            throw DoctoredCoreError(message:
                "Magnetic Lasso width must be 1–256 px, contrast 1–100 % and frequency 0–100.")
        }
        let map = try RasterEdgeMap(source: source)
        // The starting click becomes its pixel's placed vertex (the phase-6
        // ADR, "The host's points"): a one-pixel trace answers it, and the
        // closing trace back to the start then ends exactly on it.
        let placed = try map.trace(parameters: [
            "from": [Double(start.x), Double(start.y)], "trail": [[Double(start.x), Double(start.y)]],
            "end": [Double(start.x), Double(start.y)], "width": 1, "contrast": options.contrast,
            "frequency": 0,
        ]).points.first ?? start
        edgeMap = map
        self.capture = capture
        self.mode = mode
        self.feather = feather
        self.options = options
        fixed = [placed]
        anchors = [0]
        trail = []
        live = [placed]
    }

    /// The starting click, placed on the edge its pixel locates (or at the
    /// pixel's centre) as every traced vertex is.
    var start: CGPoint { fixed[0] }

    /// The last fastened anchor, where the live wire starts.
    private var anchor: CGPoint { fixed[fixed.count - 1] }

    /// The corridor radius the live wire is traced at: the Width under the
    /// latest event's pen pressure.
    var traceWidth: Double { MagneticLassoOptions.traceWidth(options.width, pressure: pressure) }

    /// The fixed path and the live wire's bounds, in canvas units.
    var previewBounds: CGRect {
        var box = CGRect(origin: start, size: .zero)
        for point in fixed { box = box.union(CGRect(origin: point, size: .zero)) }
        for point in live { box = box.union(CGRect(origin: point, size: .zero)) }
        return box
    }

    /// A pointer move: extends the trail and re-traces the live wire. A move
    /// closer than `minimumDistance` to the last trail point is coalesced.
    /// When the trail is full, or the trace would cover more corridor than
    /// the work budget, the live wire is fastened first — at any Frequency,
    /// 0 included (at Width ≥ ~205 the budget's disc alone is exceeded, so
    /// every move fastens). A fasten the core reports — Frequency's, or, at any
    /// Frequency, the junction of a trail that went more than 2·width from
    /// the anchor and came back within 2·width of it around an enclosed
    /// area (not an overshoot backed up along itself) — fixes the path
    /// through it and reseeds the trail with the rest of the live wire plus
    /// this point, so a loop back to the anchor keeps its shape. A refused
    /// trace throws and leaves the session exactly as it was. `pressure` is
    /// the event's pen pressure (`traceWidth`); a move ends an open freehand
    /// segment first.
    mutating func move(to point: CGPoint, minimumDistance: CGFloat,
                       pressure: CGFloat? = nil) throws {
        if !freehandOpen, let last = trail.last,
           hypot(point.x - last.x, point.y - last.y) < minimumDistance {
            return
        }
        var next = self
        next.endFreehand()
        next.pressure = pressure.map { Double($0) }
        if next.live.count >= 2,
           next.trail.count >= Self.trailLimit || next.projectedWork(to: point) > Self.workBudget {
            next.fastenLive()
        }
        next.trail.append(point)
        let result = try next.trace(frequency: options.frequency)
        if let k = result.fasten, k > 0 {
            next.fixed.append(contentsOf: result.points[1...k])
            next.anchors.append(next.fixed.count - 1)
            next.live = Array(result.points[k...])
            next.trail = Array(result.points[(k + 1)...]) + [point]
        } else {
            next.live = [next.anchor] + result.points.dropFirst()
        }
        self = next
    }

    /// A click: a magnetic click traces to the pointer and fastens there; a
    /// straight (⌥) click drops the live wire and joins the point straight.
    /// A click ends an open freehand segment first.
    mutating func click(at point: CGPoint, straight: Bool, pressure: CGFloat? = nil) throws {
        var next = self
        next.endFreehand()
        if straight {
            next.fixed.append(point)
        } else {
            try next.move(to: point, minimumDistance: 0, pressure: pressure)
            next.fixed.append(contentsOf: next.live.dropFirst())
        }
        // A click on the anchor itself adds no path, so no second anchor.
        if next.anchors.last != next.fixed.count - 1 { next.anchors.append(next.fixed.count - 1) }
        next.trail = []
        next.live = [next.anchor]
        self = next
    }

    /// A freehand (⌥-drag) tick: the point is joined to the path as given,
    /// with no tracing. The first tick of a segment fixes the live wire as it
    /// stands, as an anchor, so the segment continues from its end. A tick
    /// closer than `minimumDistance` to the path's last point is coalesced.
    mutating func freehand(to point: CGPoint, minimumDistance: CGFloat) {
        if !freehandOpen {
            if live.count >= 2 { fastenLive() }
            freehandOpen = true
        }
        let last = anchor
        guard point != last, hypot(point.x - last.x, point.y - last.y) >= minimumDistance else {
            return
        }
        fixed.append(point)
        trail = []
        live = [point]
    }

    /// Ends an open freehand segment: its last point becomes an anchor, and
    /// tracing resumes from it. Nothing when none is open.
    mutating func endFreehand() {
        guard freehandOpen else { return }
        freehandOpen = false
        if anchors.last != fixed.count - 1 { anchors.append(fixed.count - 1) }
        trail = []
        live = [anchor]
    }

    /// ⌫: removes the last anchor and the path fastened since the one before
    /// it, then re-traces from that one along the removed path to the
    /// pointer at Frequency 0, ignoring any fasten — so one ⌫ with no
    /// movement removes exactly one anchor (a whole freehand segment
    /// included), and fastening resumes only on the next move. A re-trace the
    /// core refuses (a removed freehand segment past the corridor cap) leaves
    /// the wire resting on the anchor until the next move. False when only
    /// the start is left.
    mutating func removeLastAnchor() -> Bool {
        var next = self
        next.endFreehand()
        guard next.anchors.count > 1 else { return false }
        let removedAt = next.anchors.removeLast()
        let previous = next.anchors[next.anchors.count - 1]
        let removed = Array(next.fixed[(previous + 1)...removedAt])
        let pointer = next.trail.last ?? removed[removed.count - 1]
        next.fixed.removeSubrange((previous + 1)...)
        var reseeded = removed + [pointer]
        if reseeded.count > Self.trailLimit {
            let stride = Double(reseeded.count - 1) / Double(Self.trailLimit - 1)
            reseeded = (0..<Self.trailLimit).map { reseeded[Int((Double($0) * stride).rounded())] }
        }
        next.trail = reseeded
        if let result = try? next.trace(frequency: 0) {
            next.live = [next.anchor] + result.points.dropFirst()
        } else {
            next.trail = []
            next.live = [next.anchor]
        }
        self = next
        return true
    }

    /// The closed outline. Straight: the fixed path and the live wire, closed
    /// by a straight edge back to the start. Magnetic: the live wire is
    /// fastened, then the way back to the start is traced in pieces along the
    /// straight line to it — each piece short enough to stay inside the work
    /// budget, snapping to the edge, the last ending exactly on the start — and
    /// a piece the core refuses becomes a straight join. Consecutive
    /// duplicates are dropped; more than `outlineLimit` vertices, or fewer
    /// than three, refuse.
    func closedOutline(straight: Bool) throws -> [CGPoint] {
        var path = fixed + live.dropFirst()
        if !straight {
            let width = options.width
            let piece = max(width, (Self.workBudget - Double.pi * width * width) / (2 * width))
            let origin = path[path.count - 1]
            let dx = start.x - origin.x, dy = start.y - origin.y
            let pieces = max(1, Int((hypot(dx, dy) / piece).rounded(.up)))
            for k in 1...pieces {
                let last = k == pieces
                let t = CGFloat(k) / CGFloat(pieces)
                let target = last ? start : CGPoint(x: origin.x + dx * t, y: origin.y + dy * t)
                let end: Any = last ? [Double(start.x), Double(start.y)] : "snap"
                if let result = try? trace(from: path[path.count - 1], trail: [target], end: end,
                                           frequency: 0, width: width) {
                    path.append(contentsOf: result.points.dropFirst())
                } else {
                    path.append(target)
                }
            }
        }
        var outline: [CGPoint] = []
        for point in path where outline.last != point { outline.append(point) }
        if outline.count > 1, outline[outline.count - 1] == outline[0] { outline.removeLast() }
        guard outline.count <= Self.outlineLimit else {
            throw DoctoredCoreError(message:
                "This Magnetic Lasso outline has more than 10,000 points. Lower the Frequency or "
                + "close the outline sooner.")
        }
        guard outline.count >= 3 else {
            throw DoctoredCoreError(message: "A Magnetic Lasso outline needs at least three points.")
        }
        return outline
    }

    // MARK: - Tracing

    /// Fixes the path through the live wire's end and makes it an anchor.
    private mutating func fastenLive() {
        fixed.append(contentsOf: live.dropFirst())
        anchors.append(fixed.count - 1)
        trail = []
        live = [anchor]
    }

    /// The corridor the next trace would cover: a disc of `traceWidth` around
    /// the anchor plus a `2·traceWidth` band along anchor → trail → `point`.
    private func projectedWork(to point: CGPoint) -> Double {
        let width = traceWidth
        var total = Double.pi * width * width
        var previous = anchor
        for next in trail + [point] {
            total += 2 * width * Double(hypot(next.x - previous.x, next.y - previous.y))
            previous = next
        }
        return total
    }

    /// The live wire from the anchor along the trail, at `traceWidth`.
    private func trace(frequency: Double) throws -> EdgeTraceResult {
        try trace(from: anchor, trail: trail, end: "snap", frequency: frequency, width: traceWidth)
    }

    private func trace(from: CGPoint, trail: [CGPoint], end: Any, frequency: Double,
                       width: Double) throws -> EdgeTraceResult {
        try edgeMap.trace(parameters: [
            "from": [Double(from.x), Double(from.y)],
            "trail": trail.map { [Double($0.x), Double($0.y)] },
            "end": end, "width": width, "contrast": options.contrast,
            "frequency": frequency,
        ])
    }
}
