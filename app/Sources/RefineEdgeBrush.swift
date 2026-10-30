import AppKit

// Select and Mask's Refine Edge brush. A stroke marks where the edge stage must (`add`) or
// must not (`subtract`) re-estimate coverage from the picture. Strokes are plain geometry: the
// sheet, the `refine_selection` MCP tool and a replayed Action all hand the same list to
// `SelectionRefinementParameters.brush`, and the core's own stroke rasterizer turns it into the
// area plane.

/// One Refine Edge brush stroke: a round-capped polyline of `radius` canvas
/// pixels through `points`.
struct RefineEdgeStroke: Equatable {
    enum Mode: String { case add, subtract }
    let mode: Mode
    let radius: Double
    let points: [CGPoint]
}

extension RefineEdgeStroke {
    static let maxStrokes = 256
    private static let limits = "Each brush stroke needs mode add or subtract, a radius from 0.5 to 512 px "
        + "and 1–8192 finite [x, y] points within ±100,000."

    /// The `refine_selection` argument form of one stroke.
    var arguments: [String: Any] {
        ["mode": mode.rawValue, "radius": radius, "points": ActionArgs.points(points)]
    }

    /// The limits one stroke and the whole list must hold.
    static func validate(_ strokes: [RefineEdgeStroke]) throws {
        guard strokes.count <= maxStrokes else {
            throw DoctoredCoreError(message: "The Refine Edge brush holds at most \(maxStrokes) strokes.")
        }
        for stroke in strokes {
            guard stroke.radius.isFinite, (0.5...512).contains(stroke.radius), !stroke.points.isEmpty,
                  stroke.points.count <= RefinementTrimapStroke.maxPoints,
                  stroke.points.allSatisfy(RefinementTrimapStroke.validPoint) else {
                throw DoctoredCoreError(message: limits)
            }
        }
        guard strokes.reduce(0, { $0 + $1.points.count }) <= RefinementTrimap.maxPoints else {
            throw DoctoredCoreError(message: "The Refine Edge brush holds at most \(RefinementTrimap.maxPoints) points in all.")
        }
    }

    /// The `brush` argument of `refine_selection` (an array of `{mode,
    /// radius, points}`), or no strokes when it is absent.
    static func parse(_ raw: Any?) throws -> [RefineEdgeStroke] {
        guard let raw, !(raw is NSNull) else { return [] }
        guard let list = raw as? [Any] else {
            throw DoctoredCoreError(message: "brush must be an array of strokes. " + limits)
        }
        let strokes = try list.map { entry -> RefineEdgeStroke in
            guard let fields = entry as? [String: Any],
                  fields.keys.allSatisfy({ ["mode", "radius", "points"].contains($0) }),
                  let mode = (fields["mode"] as? String).flatMap(Mode.init(rawValue:)),
                  let radius = NativeBrushStroke.jsonNumber(fields["radius"]),
                  let rawPoints = fields["points"] as? [Any] else {
                throw DoctoredCoreError(message: limits)
            }
            let points = try rawPoints.map { value -> CGPoint in
                guard let pair = value as? [Any], pair.count == 2,
                      let x = NativeBrushStroke.jsonNumber(pair[0]),
                      let y = NativeBrushStroke.jsonNumber(pair[1]) else {
                    throw DoctoredCoreError(message: limits)
                }
                return CGPoint(x: x, y: y)
            }
            return RefineEdgeStroke(mode: mode, radius: radius, points: points)
        }
        try validate(strokes)
        return strokes
    }

    /// The strokes as the core's area plane, one byte per canvas pixel: 128
    /// (automatic) everywhere, then each stroke in order writing 255 (`add`:
    /// always refined) or 0 (`subtract`: never refined) under its round-capped
    /// path. Nil with no strokes, which the core reads as all automatic. The
    /// rasterizer is the trimap one (`rz_trimap_strokes`): an `add` stroke is
    /// its foreground label and a `subtract` stroke its background label.
    static func areaPlane(_ strokes: [RefineEdgeStroke], width: Int, height: Int) throws -> [UInt8]? {
        guard !strokes.isEmpty else { return nil }
        try validate(strokes)
        let replay = try RefinementTrimap(width: width, height: height, strokes: strokes.map {
            try RefinementTrimapStroke(label: $0.mode == .add ? .foreground : .background,
                                       radius: $0.radius, points: $0.points)
        })
        let pixels = UInt64(width * height)
        _ = try RasterResources.shared.admit(scratch: try replay.coreScratchBytes() + pixels * 2)
        return try RasterImage.applyingTrimapStrokes([UInt8](repeating: 128, count: width * height),
                                                     replay: replay)
    }
}

/// The sheet's Refine Edge brush: which way it paints, its size, and the
/// strokes made so far. A `RefinementTrimapSession` holds the committed
/// strokes (its undo cursor is Undo Stroke); the area plane is rebuilt only
/// when they change.
struct RefineEdgeBrushSession {
    enum Tool: Int { case off, add, subtract }

    var tool = Tool.off
    /// The brush diameter in canvas pixels.
    var size = 30.0
    private let strokes: RefinementTrimapSession
    private let width: Int, height: Int
    /// The stroke being drawn, for the canvas overlay.
    private(set) var draft: RefineEdgeStroke?
    private var plane: (revision: UInt64, bytes: [UInt8]?)?

    init(width: Int, height: Int) throws {
        strokes = try RefinementTrimapSession(width: width, height: height)
        self.width = width; self.height = height
    }

    var isDrawing: Bool { draft != nil }
    var canUndo: Bool { strokes.canUndo }

    /// The committed strokes, in order: the session's, read back whenever
    /// they change (a snapshot is refused while a stroke is being drawn).
    private(set) var committed: [RefineEdgeStroke] = []

    private mutating func readCommitted() {
        guard let replay = try? strokes.snapshot().trimap.strokes else { return }
        committed = replay.compactMap { stroke in
            switch stroke.label {
            case .foreground: return RefineEdgeStroke(mode: .add, radius: stroke.radius, points: stroke.points)
            case .background: return RefineEdgeStroke(mode: .subtract, radius: stroke.radius, points: stroke.points)
            case .unknown: return nil
            }
        }
    }

    /// Starts a stroke at `point`; false when the brush is off or a stroke is
    /// already in progress.
    mutating func begin(at point: CGPoint) -> Bool {
        guard tool != .off, draft == nil else { return false }
        let mode: RefineEdgeStroke.Mode = tool == .add ? .add : .subtract
        let radius = min(512, max(0.5, size / 2))
        do {
            try strokes.begin(label: mode == .add ? .foreground : .background, radius: radius, at: point)
        } catch { return false }
        draft = RefineEdgeStroke(mode: mode, radius: radius, points: [point])
        return true
    }

    /// Extends the stroke; points closer than a quarter of the radius to the
    /// last kept one add nothing a round cap does not already cover.
    mutating func append(_ point: CGPoint) {
        guard let current = draft, let last = current.points.last,
              hypot(point.x - last.x, point.y - last.y) >= max(0.5, current.radius / 4),
              (try? strokes.append(point)) != nil else { return }
        draft = RefineEdgeStroke(mode: current.mode, radius: current.radius, points: current.points + [point])
    }

    /// Commits the stroke; false when none was in progress or the list is full.
    mutating func finish() -> Bool {
        guard draft != nil else { return false }
        draft = nil
        do { try strokes.finish() } catch { strokes.cancel(); return false }
        readCommitted()
        return true
    }

    mutating func cancel() {
        draft = nil
        strokes.cancel()
    }

    /// Removes the last committed stroke.
    mutating func undo() -> Bool {
        guard strokes.undo() else { return false }
        readCommitted()
        return true
    }

    /// Removes every committed stroke.
    mutating func clear() -> Bool {
        var removed = false
        while strokes.undo() { removed = true }
        readCommitted()
        return removed
    }

    /// The committed strokes' area plane, cached until they change.
    mutating func areaPlane() throws -> [UInt8]? {
        if let plane, plane.revision == strokes.revision { return plane.bytes }
        let bytes = try RefineEdgeStroke.areaPlane(committed, width: width, height: height)
        plane = (strokes.revision, bytes)
        return bytes
    }
}
