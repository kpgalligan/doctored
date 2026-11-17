import AppKit

/// Brush dynamics over MCP (`designs/brush-dynamics.md`). `dynamics`, `tilts` and
/// `rotations` are generation arguments of `brush_stroke`, `eraser_stroke` and
/// `quick_mask_stroke`, the three tools whose UI strokes take them (the Brush, the Pencil
/// and the Eraser, each with its Dynamics… sheet). `nativeStrokeArguments` resolves them to
/// `native_stroke` dabs (and, with a tip shape, their `shapes`) before dispatch and
/// recording, so a recorded step holds the dabs, never the seed.
extension AgentServer {
    /// The stroke tools that take `dynamics`, `tilts` and `rotations`.
    static let dynamicsStrokeTools: Set<String> = ["brush_stroke", "eraser_stroke", "quick_mask_stroke"]

    /// Refuses `dynamics`, `tilts` or `rotations` by name on every other tool
    /// (`stroke_path` among them: a path is stroked at one pressure, with no dynamics),
    /// and `tilts` or `rotations` without `dynamics`.
    static func refuseForeignDynamics(tool: String, arguments: [String: Any]) throws {
        guard let key = ["dynamics", "tilts", "rotations"].first(where: { arguments[$0] != nil })
        else { return }
        guard dynamicsStrokeTools.contains(tool) else {
            throw ToolError(message: "\(key) is the Brush's, Pencil's and Eraser's: brush_stroke, "
                + "eraser_stroke and quick_mask_stroke take it; \(tool) has none.")
        }
        guard arguments["dynamics"] != nil else {
            throw ToolError(message: key == "tilts"
                ? "tilts are read only by dynamics' Pen Tilt controls; pass dynamics with them."
                : "rotations are read only by dynamics.angle's rotation control; pass dynamics "
                    + "with them.")
        }
    }

    /// Refuses a canonical `native_stroke` holding per-dab `shapes` on a tool outside the
    /// three: every other stroke reads the stroke's one tip.
    static func refuseForeignShapes(tool: String, stroke: NativeBrushStroke) throws {
        guard stroke.shapes != nil, !dynamicsStrokeTools.contains(tool) else { return }
        throw ToolError(message: "native_stroke shapes are the Brush's and Eraser's tip-shape "
            + "dynamics: brush_stroke, eraser_stroke and quick_mask_stroke take them; \(tool) "
            + "has none.")
    }

    /// Refuses a canonical `native_stroke` holding a `texture` on a tool outside the three:
    /// every other stroke lays its dabs' coverage untextured.
    static func refuseForeignTexture(tool: String, stroke: NativeBrushStroke) throws {
        guard stroke.texture != nil, !dynamicsStrokeTools.contains(tool) else { return }
        throw ToolError(message: "native_stroke texture is the Brush's and Eraser's texture "
            + "dynamics: brush_stroke, eraser_stroke and quick_mask_stroke take it; \(tool) "
            + "has none.")
    }

    /// Refuses a canonical `native_stroke` holding a `dual` on a tool outside the three: every
    /// other stroke lays its one tip's coverage.
    static func refuseForeignDual(tool: String, stroke: NativeBrushStroke) throws {
        guard stroke.dual != nil, !dynamicsStrokeTools.contains(tool) else { return }
        throw ToolError(message: "native_stroke dual is the Brush's and Eraser's dual brush "
            + "dynamics: brush_stroke, eraser_stroke and quick_mask_stroke take it; \(tool) "
            + "has none.")
    }

    /// The parsed `dynamics`, `tilts` and `rotations` of a generated stroke: nil without
    /// `dynamics` (`refuseForeignDynamics` has refused a `tilts` or `rotations` alone).
    /// Refuses by name a size-1 pixel stroke, a control given both here and as
    /// `pressure_size` / `pressure_opacity`, a `seed` outside 0…2³²−1, a `tilts` or
    /// `rotations` list of the wrong length or range, and `angle`, `roundness`, `texture` or
    /// `dual` on the Pencil's tip (`tip.aliased`), which is round and two-valued. `shaped` is
    /// whether the dabs carry a tip shape; `texture` is the stroke's, its `{name}` resolved from
    /// the library (`BrushDynamics.Texture.init(json:library:)`), so the recorded step holds the
    /// tile; `dual` is the second tip's walk (seeded with `seed`) and tip; `colour` the colour
    /// dynamics' amounts beside a handle (`AgentServer+BrushColour.swift` builds its handle on
    /// `seed`).
    static func strokeDynamics(_ arguments: [String: Any], points: Int, size: Double, tip: BrushTip)
        throws -> (handle: RasterBrushDynamics?, tilts: [CGVector]?, rotations: [Double]?, shaped: Bool,
                   texture: StrokeTexture?, dual: (handle: RasterBrushDual, tip: BrushTip)?,
                   colour: BrushDynamics.ColorDynamics?, seed: UInt32)? {
        guard let raw = arguments["dynamics"] else { return nil }
        guard let object = raw as? [String: Any] else {
            throw ToolError(message: "dynamics must be an object.")
        }
        let dynamics: BrushDynamics
        do { dynamics = try BrushDynamics(json: object, extra: ["seed"], library: true) }
        catch { throw ToolError(message: error.localizedDescription) }
        var seed: UInt32 = 0
        if let value = object["seed"] {
            guard let number = NativeBrushStroke.jsonNumber(value), number.isFinite,
                  number == number.rounded(), (0...Double(UInt32.max)).contains(number) else {
                throw ToolError(message: "dynamics.seed must be a whole number from 0 to 4294967295.")
            }
            seed = UInt32(number)
        }
        guard size > 1 else {
            throw ToolError(message: "dynamics need a brush size above 1 px; the pixel brush "
                + "takes none.")
        }
        if tip.aliased, let key = ["angle", "roundness"].first(where: { object[$0] != nil }) {
            throw ToolError(message: "dynamics.\(key) shapes a stamped tip; the Pencil's tip "
                + "(aliased) is round and takes no angle or roundness dynamics.")
        }
        if tip.aliased, object["texture"] != nil {
            throw ToolError(message: "dynamics.texture thins a stamped tip's coverage; the "
                + "Pencil's tip (aliased) is two-valued and takes no texture.")
        }
        if tip.aliased, object["dual"] != nil {
            throw ToolError(message: "dynamics.dual multiplies a stamped tip's coverage; the "
                + "Pencil's tip (aliased) is two-valued and takes no dual brush.")
        }
        for (key, response) in [("pressure_size", dynamics.size), ("pressure_opacity", dynamics.opacity)]
        where response.control != nil && arguments[key] != nil {
            let name = key == "pressure_size" ? "size" : "opacity"
            throw ToolError(message: "Pass either dynamics.\(name).control or \(key), not both.")
        }
        var tilts: [CGVector]?
        if let raw = arguments["tilts"] {
            guard let rows = raw as? [Any], rows.count == points else {
                throw ToolError(message: "tilts must contain one [x, y] per point.")
            }
            tilts = try rows.map { row in
                guard let pair = row as? [Any], pair.count == 2,
                      let x = NativeBrushStroke.jsonNumber(pair[0]),
                      let y = NativeBrushStroke.jsonNumber(pair[1]),
                      x.isFinite, y.isFinite, (-1...1).contains(x), (-1...1).contains(y) else {
                    throw ToolError(message: "Each tilt is [x, y] with each axis from -1 to 1 "
                        + "(0, 0 is upright).")
                }
                return CGVector(dx: x, dy: y)
            }
        }
        var rotations: [Double]?
        if let raw = arguments["rotations"] {
            guard let rows = raw as? [Any], rows.count == points,
                  rows.allSatisfy({ NativeBrushStroke.jsonNumber($0).map { (-360...360).contains($0) } == true })
            else {
                throw ToolError(message: "rotations must contain one number per point, each "
                    + "from -360 to 360 degrees (counter-clockwise).")
            }
            rotations = rows.compactMap(NativeBrushStroke.jsonNumber)
        }
        let pressureSize = NativeBrushStroke.jsonBoolean(arguments["pressure_size"])
            ?? ((arguments["pressure_size"] as? String)?.lowercased() == "true")
        let pressureOpacity = NativeBrushStroke.jsonBoolean(arguments["pressure_opacity"])
            ?? ((arguments["pressure_opacity"] as? String)?.lowercased() == "true")
        do {
            let handle = try BrushDynamics.handle(dynamics, size: CGFloat(size), seed: seed,
                pressureSize: pressureSize, pressureOpacity: pressureOpacity, tip: tip)
            // A texture at depth 0 is dropped by `normalized`, and rides only beside a handle.
            let texture = try handle.flatMap { _ in
                try dynamics.normalized?.texture.map { try StrokeTexture($0) }
            }
            // The dual brush rides only beside a handle too, on the stroke's one seed.
            let dual = try handle.flatMap { _ in
                try BrushDynamics.dualHandle(dynamics, size: CGFloat(size), seed: seed)
            }
            return (handle, tilts, rotations, handle != nil && dynamics.normalized?.shape != nil,
                    texture, dual, handle == nil ? nil : dynamics.normalized?.color, seed)
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }
}
