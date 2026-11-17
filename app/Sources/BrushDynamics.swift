import AppKit

/// The Brush's, the Pencil's and the Eraser's dynamics (`designs/brush-dynamics.md`):
/// how pen pressure or tilt shapes each stamp's size and opacity, how much each dab's
/// size and opacity jitter, and how stamps scatter. The core resolves each nominal stamp
/// into the dabs actually laid (`brush_dynamics.rs`, `RasterBrushDynamics`); a stroke
/// records those dabs, never the seed, so a replay draws what the canvas drew.
///
/// Stored in `PaintToolOptions.dynamics` (nil whenever every value is its default, so a
/// stroke without dynamics never reaches the kernel and keeps its bytes) and in a brush
/// preset's file (`BrushPreset.dynamics`, schema 2). The options bar's two pressure
/// checkboxes stay the pressure on/off: a response's `control` nil follows its checkbox,
/// and only Pen Tilt is stored here (`PaintToolOptions.sizeControl`).
struct BrushDynamics: Codable, Equatable {
    enum Control: String, Codable, CaseIterable {
        case off, pressure, tilt
    }

    /// One of the two responses, `size` or `opacity`.
    struct Response: Codable, Equatable {
        /// nil follows the options bar's checkbox: pressure when it is on, else off.
        var control: Control?
        /// 2…16 points `[x, y]` in 0…1, x strictly increasing from 0 to 1; nil is the
        /// identity.
        var curve: [[Double]]?
        var minimum: Double = 0
        var jitter: Double = 0
    }

    var size = Response()
    var opacity = Response()
    /// Scatter amount, in nominal diameters (0…10).
    var scatter: Double = 0
    var bothAxes = false
    var count = 1
    var countJitter: Double = 0
    /// The tip shape's dynamics (slice 2: angle and roundness), nil for none. Optional, so
    /// a stored options blob or preset without it decodes; the Brush and the Eraser only
    /// (`EditorTool.takesTipShapeDynamics`): the Pencil's tip is round.
    var shape: TipShape?
    /// The texture (slice 3, `BrushTexture.swift`), nil for none; the Brush and the Eraser only
    /// (`EditorTool.takesTextureDynamics`). Optional, as `shape` is. It never reaches the
    /// dynamics kernel: the stroke carries it to the rasterizer (`NativeBrushStroke.texture`).
    var texture: Texture?
    /// The dual brush (slice 4, `BrushDual.swift`), nil for none; the Brush and the Eraser only
    /// (`EditorTool.takesDualBrushDynamics`). Optional, as `texture` is, and its presence is the
    /// switch (`normalized` keeps it). It never reaches the dynamics kernel: the stroke walks it
    /// (`NativeBrushStrokeSession.walkDual`) and carries its dabs (`NativeBrushStroke.dual`).
    var dual: Dual?

    static let maximumCurvePoints = 16
    static let maximumScatter = 10.0
    static let maximumCount = 16

    /// nil when every value is its default (the stored and passed form), the identity
    /// curve, a default tip shape and a texture at depth 0 (which thins nothing) dropped first.
    var normalized: BrushDynamics? {
        var out = self
        for keyPath in [\BrushDynamics.size, \BrushDynamics.opacity]
        where out[keyPath: keyPath].curve == [[0, 0], [1, 1]] {
            out[keyPath: keyPath].curve = nil
        }
        if out.shape == TipShape() { out.shape = nil }
        if out.texture?.depth == 0 { out.texture = nil }
        return out == BrushDynamics() ? nil : out
    }
}

extension BrushDynamics {
    /// The tip shape's dynamics: each dab's angle follows the pen's rotation, its tilt's
    /// direction or the direction of travel, plus a seeded jitter of up to ±180° × jitter;
    /// its roundness follows pressure or tilt down to a minimum, shrunk by a seeded jitter
    /// (`brush_dynamics.rs`, THE TIP SHAPE).
    struct TipShape: Codable, Equatable {
        enum AngleControl: String, Codable, CaseIterable {
            case off, rotation, tilt, direction
        }

        var angleControl: AngleControl = .off
        var angleJitter: Double = 0
        var roundnessControl: Control = .off
        var roundnessMinimum: Double = 0
        var roundnessJitter: Double = 0
    }
}

/// An extension, so the struct keeps its memberwise and default initializers.
extension BrushDynamics {
    // MARK: - The JSON form (MCP arguments, preset files, list rows)

    /// `{"size": R, "opacity": R, "scatter": {"amount", "both_axes", "count",
    /// "count_jitter"}}`, each response `{"control"?, "curve"?, "minimum", "jitter"}`;
    /// `control` and `curve` are written only when set. With a tip shape, also
    /// `"angle": {"control", "jitter"}` and `"roundness": {"control", "minimum", "jitter"}`;
    /// with a texture, `"texture": {"pattern", "scale", "depth"}` (`Texture.json`); with a dual
    /// brush, `"dual": {"size", "hardness", "roundness", "angle_degrees", "spacing", "scatter"}`
    /// (`Dual.json`).
    var json: [String: Any] {
        func response(_ r: Response) -> [String: Any] {
            var out: [String: Any] = ["minimum": r.minimum, "jitter": r.jitter]
            if let control = r.control { out["control"] = control.rawValue }
            if let curve = r.curve { out["curve"] = curve }
            return out
        }
        var out: [String: Any] = [
            "size": response(size), "opacity": response(opacity),
            "scatter": ["amount": scatter, "both_axes": bothAxes, "count": count,
                        "count_jitter": countJitter] as [String: Any]]
        if let shape {
            out["angle"] = ["control": shape.angleControl.rawValue, "jitter": shape.angleJitter]
            out["roundness"] = ["control": shape.roundnessControl.rawValue,
                                "minimum": shape.roundnessMinimum, "jitter": shape.roundnessJitter]
        }
        if let texture { out["texture"] = texture.json }
        if let dual { out["dual"] = dual.json }
        return out
    }

    /// The sections `json` writes, in its order.
    private static let sections = ["size", "opacity", "scatter", "angle", "roundness", "texture",
                                   "dual"]

    /// Strict parse of `json`'s form, every section and key optional, each refusal
    /// naming its key. `extra` names keys the caller handles itself (`seed`). `library` is the
    /// texture's (`Texture.init(json:library:)`): true resolves a library pattern's name.
    init(json object: [String: Any], extra: Set<String> = [], library: Bool = false) throws {
        if let key = Set(object.keys).subtracting(Self.sections).subtracting(extra).sorted().first {
            throw DoctoredCoreError(message: "dynamics takes "
                + (Self.sections + extra.sorted()).joined(separator: ", ")
                + "; it has no \(key).")
        }
        self.init()
        size = try Self.response(object["size"], "size")
        opacity = try Self.response(object["opacity"], "opacity")
        shape = try Self.tipShape(angle: object["angle"], roundness: object["roundness"])
        if let raw = object["texture"] { texture = try Texture(json: raw, library: library) }
        if let raw = object["dual"] { dual = try Dual(json: raw) }
        guard let raw = object["scatter"] else { return }
        guard let scatter = raw as? [String: Any] else {
            throw DoctoredCoreError(message: "dynamics.scatter must be an object.")
        }
        let keys: Set<String> = ["amount", "both_axes", "count", "count_jitter"]
        if let key = Set(scatter.keys).subtracting(keys).sorted().first {
            throw DoctoredCoreError(message: "dynamics.scatter takes amount, both_axes, count and "
                + "count_jitter; it has no \(key).")
        }
        self.scatter = try Self.number(scatter["amount"], "scatter.amount", 0...Self.maximumScatter) ?? 0
        if let raw = scatter["both_axes"] {
            guard let flag = NativeBrushStroke.jsonBoolean(raw) else {
                throw DoctoredCoreError(message: "dynamics.scatter.both_axes must be true or false.")
            }
            bothAxes = flag
        }
        if let count = try Self.number(scatter["count"], "scatter.count",
                                       1...Double(Self.maximumCount)) {
            guard count == count.rounded() else {
                throw DoctoredCoreError(message: "dynamics.scatter.count must be a whole number "
                    + "from 1 to \(Self.maximumCount).")
            }
            self.count = Int(count)
        }
        countJitter = try Self.number(scatter["count_jitter"], "scatter.count_jitter", 0...1) ?? 0
    }

    private static func response(_ raw: Any?, _ name: String) throws -> Response {
        guard let raw else { return Response() }
        guard let object = raw as? [String: Any] else {
            throw DoctoredCoreError(message: "dynamics.\(name) must be an object.")
        }
        let keys: Set<String> = ["control", "curve", "minimum", "jitter"]
        if let key = Set(object.keys).subtracting(keys).sorted().first {
            throw DoctoredCoreError(message: "dynamics.\(name) takes control, curve, minimum and "
                + "jitter; it has no \(key).")
        }
        var out = Response()
        if let control = object["control"] {
            guard let text = control as? String, let value = Control(rawValue: text) else {
                throw DoctoredCoreError(message: "dynamics.\(name).control must be \"off\", "
                    + "\"pressure\" or \"tilt\".")
            }
            out.control = value
        }
        out.minimum = try number(object["minimum"], "\(name).minimum", 0...1) ?? 0
        out.jitter = try number(object["jitter"], "\(name).jitter", 0...1) ?? 0
        if let curve = object["curve"] {
            let refusal = DoctoredCoreError(message: "dynamics.\(name).curve holds 2 to "
                + "\(maximumCurvePoints) points [x, y] in 0…1, x strictly increasing from 0 to 1.")
            guard let rows = curve as? [Any], (2...maximumCurvePoints).contains(rows.count) else {
                throw refusal
            }
            let points: [[Double]] = try rows.map { row in
                guard let pair = row as? [Any], pair.count == 2,
                      let x = NativeBrushStroke.jsonNumber(pair[0]),
                      let y = NativeBrushStroke.jsonNumber(pair[1]),
                      x.isFinite, y.isFinite, (0...1).contains(x), (0...1).contains(y) else {
                    throw refusal
                }
                return [x, y]
            }
            guard points.first?[0] == 0, points.last?[0] == 1,
                  zip(points, points.dropFirst()).allSatisfy({ $0[0] < $1[0] }) else {
                throw refusal
            }
            out.curve = points
        }
        return out
    }

    /// The `angle` and `roundness` sections as one tip shape, nil when neither is given.
    private static func tipShape(angle: Any?, roundness: Any?) throws -> TipShape? {
        guard angle != nil || roundness != nil else { return nil }
        func section(_ raw: Any?, _ name: String, _ keys: [String]) throws -> [String: Any] {
            guard let raw else { return [:] }
            guard let object = raw as? [String: Any] else {
                throw DoctoredCoreError(message: "dynamics.\(name) must be an object.")
            }
            if let key = Set(object.keys).subtracting(keys).sorted().first {
                throw DoctoredCoreError(message: "dynamics.\(name) takes "
                    + keys.joined(separator: ", ") + "; it has no \(key).")
            }
            return object
        }
        let a = try section(angle, "angle", ["control", "jitter"])
        let r = try section(roundness, "roundness", ["control", "minimum", "jitter"])
        var out = TipShape()
        if let raw = a["control"] {
            guard let text = raw as? String, let value = TipShape.AngleControl(rawValue: text) else {
                throw DoctoredCoreError(message: "dynamics.angle.control must be \"off\", "
                    + "\"rotation\", \"tilt\" or \"direction\".")
            }
            out.angleControl = value
        }
        out.angleJitter = try number(a["jitter"], "angle.jitter", 0...1) ?? 0
        if let raw = r["control"] {
            guard let text = raw as? String, let value = Control(rawValue: text) else {
                throw DoctoredCoreError(message: "dynamics.roundness.control must be \"off\", "
                    + "\"pressure\" or \"tilt\".")
            }
            out.roundnessControl = value
        }
        out.roundnessMinimum = try number(r["minimum"], "roundness.minimum", 0...1) ?? 0
        out.roundnessJitter = try number(r["jitter"], "roundness.jitter", 0...1) ?? 0
        return out
    }

    private static func number(_ raw: Any?, _ name: String,
                               _ range: ClosedRange<Double>) throws -> Double? {
        guard let raw else { return nil }
        guard let value = NativeBrushStroke.jsonNumber(raw), value.isFinite,
              range.contains(value) else {
            throw DoctoredCoreError(message: "dynamics.\(name) must be a number from "
                + String(format: "%g to %g.", range.lowerBound, range.upperBound))
        }
        return value
    }

    // MARK: - The core request

    /// The kernel's request (`dr_brush_dynamics_create`): each response's control
    /// resolved against the pressure checkboxes. With a tip shape, the stroke's `tip`
    /// angle and roundness and the `angle` and `roundness` sections too; without one the
    /// request keeps slice 1's bytes.
    func coreRequest(seed: UInt32, pressureSize: Bool, pressureOpacity: Bool,
                     tip: BrushTip) throws -> String {
        func response(_ r: Response, checkbox: Bool) -> [String: Any] {
            var out: [String: Any] = ["control": (r.control ?? (checkbox ? .pressure : .off)).rawValue,
                                      "minimum": r.minimum, "jitter": r.jitter]
            if let curve = r.curve { out["curve"] = curve }
            return out
        }
        var object: [String: Any] = [
            "version": 1, "seed": Int(seed),
            "size": response(size, checkbox: pressureSize),
            "opacity": response(opacity, checkbox: pressureOpacity),
            "scatter": ["amount": scatter, "both_axes": bothAxes, "count": count,
                        "count_jitter": countJitter] as [String: Any],
        ]
        if let shape {
            object["tip"] = ["angle_degrees": Double(tip.angleDegrees),
                             "roundness": Double(tip.roundness)]
            object["angle"] = ["control": shape.angleControl.rawValue, "jitter": shape.angleJitter]
            object["roundness"] = ["control": shape.roundnessControl.rawValue,
                                   "minimum": shape.roundnessMinimum, "jitter": shape.roundnessJitter]
        }
        return try NativeSelectionWork.encoding(object)
    }

    /// A stroke's dynamics handle, or nil when none apply: `dynamics` nil, or the size-1
    /// pixel brush, which takes none. `tip` is the stroke's, whose angle and roundness a
    /// tip shape starts from.
    static func handle(_ dynamics: BrushDynamics?, size: CGFloat, seed: UInt32,
                       pressureSize: Bool, pressureOpacity: Bool,
                       tip: BrushTip) throws -> RasterBrushDynamics? {
        guard let dynamics = dynamics?.normalized, size > 1 else { return nil }
        return try RasterBrushDynamics(request: dynamics.coreRequest(
            seed: seed, pressureSize: pressureSize, pressureOpacity: pressureOpacity, tip: tip))
    }

    /// A fresh stroke seed (the canvas draws one at mouse-down).
    static func strokeSeed() -> UInt32 { UInt32.random(in: 0...UInt32.max) }
}

/// One stamp's pen sample: NaN where the stroke has none (a mouse has none; an MCP
/// call without `pressures`, `tilts` or `rotations` lacks that one), which the kernel
/// reads as off.
struct PenSample: Equatable {
    var pressure: Double = .nan
    /// The pen's tilt, each axis −1…1 (`NSEvent.tilt`, y up); 0, 0 is upright.
    var tiltX: Double = .nan
    var tiltY: Double = .nan
    /// The pen's barrel rotation in degrees, counter-clockwise in the canvas frame.
    var rotation: Double = .nan

    static func == (a: PenSample, b: PenSample) -> Bool {
        a.pressure.bitPattern == b.pressure.bitPattern && a.tiltX.bitPattern == b.tiltX.bitPattern
            && a.tiltY.bitPattern == b.tiltY.bitPattern
            && a.rotation.bitPattern == b.rotation.bitPattern
    }

    /// The tablet tilt of a stroke event, or NaN for a plain mouse.
    static func tilt(of event: NSEvent) -> CGVector {
        guard event.subtype == .tabletPoint else {
            return CGVector(dx: CGFloat.nan, dy: CGFloat.nan)
        }
        return CGVector(dx: min(max(event.tilt.x, -1), 1), dy: min(max(event.tilt.y, -1), 1))
    }

    /// The tablet barrel rotation of a stroke event in degrees (`NSEvent.rotation`, read
    /// as counter-clockwise on screen), or NaN for a plain mouse.
    static func rotation(of event: NSEvent) -> CGFloat {
        guard event.subtype == .tabletPoint else { return .nan }
        return CGFloat(event.rotation)
    }
}

extension EditorTool {
    /// The tools whose strokes take brush dynamics: the Brush, the Pencil and the
    /// Eraser. Every other paint tool keeps `PaintToolOptions.dynamics` nil
    /// (`ToolOptionsStore.paintOptions`) and shows no Dynamics… button.
    var takesBrushDynamics: Bool { [.brush, .pencil, .eraser].contains(self) }

    /// The tools whose dynamics take a tip shape (angle and roundness): the Brush and the
    /// Eraser. The Pencil's tip is round, so its `dynamics.shape` is pinned nil
    /// (`ToolOptionsStore.pinnedDynamics`) and its sheet shows no Angle or Roundness rows.
    var takesTipShapeDynamics: Bool { [.brush, .eraser].contains(self) }
}

extension PaintToolOptions {
    /// The size response's control: the stored Pen Tilt, else the Pressure size checkbox.
    /// Setting Off or Pen Pressure sets the checkbox and clears the stored control.
    var sizeControl: BrushDynamics.Control {
        get { dynamics?.size.control ?? (pressureSize ? .pressure : .off) }
        set {
            if newValue == .tilt {
                var d = dynamics ?? BrushDynamics(); d.size.control = .tilt; dynamics = d
            } else {
                pressureSize = newValue == .pressure
                dynamics?.size.control = nil
            }
            dynamics = dynamics?.normalized
        }
    }

    /// The opacity response's control, as `sizeControl` is the size's.
    var opacityControl: BrushDynamics.Control {
        get { dynamics?.opacity.control ?? ((pressureOpacity ?? false) ? .pressure : .off) }
        set {
            if newValue == .tilt {
                var d = dynamics ?? BrushDynamics(); d.opacity.control = .tilt; dynamics = d
            } else {
                pressureOpacity = newValue == .pressure
                dynamics?.opacity.control = nil
            }
            dynamics = dynamics?.normalized
        }
    }
}
