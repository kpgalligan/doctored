import AppKit

// Filters ▸ Liquify…. A stroke is plain geometry: the workspace, the `liquify` MCP tool and a
// replayed Action all hand the same list to the core (`liquify.rs`), which turns it into dabs on a
// displacement and freeze field. Points are CANVAS pixels and the amounts are percentages here; the
// core takes plane pixels and fractions (`kernelParameters`).

/// The Liquify tools, in the workspace's order. The raw value is the name the
/// core, the MCP tool and a recorded step all use.
enum LiquifyTool: String, CaseIterable {
    case forwardWarp = "forward_warp"
    case reconstruct
    case twirlClockwise = "twirl_clockwise"
    case twirlCounterclockwise = "twirl_counterclockwise"
    case pucker
    case bloat
    case freeze
    case thaw

    var title: String {
        switch self {
        case .forwardWarp: return "Forward Warp"
        case .reconstruct: return "Reconstruct"
        case .twirlClockwise: return "Twirl Clockwise"
        case .twirlCounterclockwise: return "Twirl Counterclockwise"
        case .pucker: return "Pucker"
        case .bloat: return "Bloat"
        case .freeze: return "Freeze Mask"
        case .thaw: return "Thaw Mask"
        }
    }

    /// The tool ⌥ at mouse-down swaps to: the opposite twirl, Pucker for
    /// Bloat, Thaw for Freeze, and Reconstruct for Forward Warp (taking a
    /// push back). Reconstruct has no opposite.
    var reversed: LiquifyTool {
        switch self {
        case .forwardWarp, .reconstruct: return .reconstruct
        case .twirlClockwise: return .twirlCounterclockwise
        case .twirlCounterclockwise: return .twirlClockwise
        case .pucker: return .bloat
        case .bloat: return .pucker
        case .freeze: return .thaw
        case .thaw: return .freeze
        }
    }

    /// True for a tool that keeps working while the pointer rests: all but
    /// Forward Warp, whose effect is the pointer's own motion.
    var dwells: Bool { self != .forwardWarp }
}

/// One Liquify stroke: `tool` dragged through `points` (canvas pixels) with a
/// brush `size` pixels across. `strength` (the workspace's Pressure) and
/// `density` are percentages, 0…100.
struct LiquifyStroke: Equatable {
    let tool: LiquifyTool
    let size: Double
    let strength: Double
    let density: Double
    var points: [CGPoint]
}

extension LiquifyStroke {
    // The core's caps (`liquify::MAX_STROKES`, `MAX_STROKE_POINTS`, `MAX_POINTS`).
    static let maxStrokes = 2048
    static let maxStrokePoints = 8192
    static let maxPoints = 131_072

    static let listRefusal = "liquify needs strokes: an array of 1 to \(maxStrokes) strokes."
    private static let limits = "Each liquify stroke needs a tool (forward_warp, reconstruct, twirl_clockwise, "
        + "twirl_counterclockwise, pucker, bloat, freeze or thaw), a size from 1 to 5000 px, strength and "
        + "density from 0 to 100, and 1–8192 finite [x, y] points within ±1,000,000."
    private static let keys: Set<String> = ["tool", "points", "size", "strength", "density"]

    /// The `liquify` argument form of one stroke. Nothing is rounded here: the
    /// workspace quantizes its points and amounts as it takes them
    /// (`LiquifySession`), and a parsed stroke keeps what the caller sent.
    var arguments: [String: Any] {
        ["tool": tool.rawValue, "size": size, "strength": strength, "density": density,
         "points": points.map { [Double($0.x), Double($0.y)] }]
    }

    /// The `strokes` argument of `liquify`: an array of `{tool, points, size,
    /// strength, density}`. Strict: an unknown key refuses, and a JSON boolean
    /// is not a number. `strength` and `density` default to 50.
    static func parse(_ raw: Any?) throws -> [LiquifyStroke] {
        guard let list = raw as? [Any] else { throw DoctoredCoreError(message: listRefusal) }
        let strokes = try list.map { entry -> LiquifyStroke in
            guard let fields = entry as? [String: Any] else { throw DoctoredCoreError(message: limits) }
            if let unknown = fields.keys.sorted().first(where: { !keys.contains($0) }) {
                throw DoctoredCoreError(message: limits + " Unknown stroke key: \(unknown).")
            }
            guard let tool = (fields["tool"] as? String).flatMap(LiquifyTool.init(rawValue:)),
                  let size = NativeBrushStroke.jsonNumber(fields["size"]),
                  let rawPoints = fields["points"] as? [Any] else {
                throw DoctoredCoreError(message: limits)
            }
            let amounts = try ["strength", "density"].map { key -> Double in
                guard let value = fields[key] else { return 50 }
                guard let number = NativeBrushStroke.jsonNumber(value) else { throw DoctoredCoreError(message: limits) }
                return number
            }
            let points = try rawPoints.map { value -> CGPoint in
                guard let pair = value as? [Any], pair.count == 2,
                      let x = NativeBrushStroke.jsonNumber(pair[0]),
                      let y = NativeBrushStroke.jsonNumber(pair[1]) else {
                    throw DoctoredCoreError(message: limits)
                }
                return CGPoint(x: x, y: y)
            }
            return LiquifyStroke(tool: tool, size: size, strength: amounts[0], density: amounts[1], points: points)
        }
        try validate(strokes)
        return strokes
    }

    /// The limits one stroke and the whole list must hold.
    static func validate(_ strokes: [LiquifyStroke]) throws {
        guard (1...maxStrokes).contains(strokes.count) else { throw DoctoredCoreError(message: listRefusal) }
        for stroke in strokes {
            guard stroke.size.isFinite, (1...5000).contains(stroke.size),
                  stroke.strength.isFinite, (0...100).contains(stroke.strength),
                  stroke.density.isFinite, (0...100).contains(stroke.density),
                  (1...maxStrokePoints).contains(stroke.points.count),
                  stroke.points.allSatisfy({ $0.x.isFinite && $0.y.isFinite }) else {
                throw DoctoredCoreError(message: limits)
            }
        }
        guard strokes.reduce(0, { $0 + $1.points.count }) <= maxPoints else {
            throw DoctoredCoreError(message: "liquify strokes hold at most \(maxPoints) points in all.")
        }
    }

    /// The core's ±1,000,000 point bound, checked where the core checks it: in
    /// the plane's own pixels, after the layer's canvas offset `origin` is
    /// subtracted (`kernelParameters`), so a layer placed far out takes the
    /// points that land on it.
    static func validatePoints(_ strokes: [LiquifyStroke], origin: (x: Int, y: Int)) throws {
        let (dx, dy) = (Double(origin.x), Double(origin.y))
        for stroke in strokes {
            guard stroke.points.allSatisfy({
                abs(Double($0.x) - dx) <= 1_000_000 && abs(Double($0.y) - dy) <= 1_000_000
            }) else {
                throw DoctoredCoreError(message: limits)
            }
        }
    }

    /// The core's `liquify` params for `strokes` on a plane whose top-left
    /// sits at `origin` on the canvas: plane pixels, amounts as fractions.
    /// `continued` marks every stroke as carrying on from an earlier call (no
    /// dab at its first point), which is how the workspace applies one segment
    /// of a drag.
    static func kernelParameters(_ strokes: [LiquifyStroke], origin: (x: Int, y: Int),
                                 continued: Bool = false) -> [String: Any] {
        let (dx, dy) = (Double(origin.x), Double(origin.y))
        return ["strokes": strokes.map { stroke -> [String: Any] in
            ["tool": stroke.tool.rawValue, "size": stroke.size, "strength": stroke.strength / 100,
             "density": stroke.density / 100, "continued": continued,
             "points": stroke.points.map { [Double($0.x) - dx, Double($0.y) - dy] }]
        }]
    }
}

extension Array where Element == ActionStep {
    /// Filters ▸ Liquify…'s Apply: the workspace's strokes on the active
    /// layer, as the `liquify` tool takes them.
    static func liquify(_ strokes: [LiquifyStroke]) -> [ActionStep] {
        .step("liquify", ["layer": ActionArgs.activeLayer, "strokes": strokes.map(\.arguments)],
              note: "Filters ▸ Liquify…")
    }
}
