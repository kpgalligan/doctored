import AppKit

// Byte/typed drag sources are captured core plates; native interpolation precedes display
// reduction. Typed spot healing uses `DocumentJobs` with captured revision, target, selection,
// cancellation and one central history transition. `DoctoredCore` owns plate/transform/expansion
// admission and all FFI calls.

/// Canvas-center destination → source mapping, shared by gestures and Actions.
struct RetouchGeometry {
    let matrix: CGAffineTransform

    init(matrix: CGAffineTransform) throws {
        let values = [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty]
        let determinant = matrix.a * matrix.d - matrix.b * matrix.c
        guard values.allSatisfy({ $0.isFinite && abs($0) <= 1_000_000_000 }),
              determinant.isFinite, abs(determinant) >= 0.000000001 else {
            throw DoctoredCoreError(message: "source_transform requires six finite affine values and an invertible mapping.")
        }
        self.matrix = matrix
    }

    init(source: CGPoint, destination: CGPoint, options: PaintToolOptions? = nil) throws {
        let scale = CGFloat(options?.sourceScale ?? 100) / 100
        let angle = CGFloat(options?.sourceRotation ?? 0) * .pi / 180
        let flipX: CGFloat = options?.sourceFlipX == true ? -1 : 1
        let flipY: CGFloat = options?.sourceFlipY == true ? -1 : 1
        let a = cos(angle) * flipX / scale, b = sin(angle) * flipX / scale
        let c = -sin(angle) * flipY / scale, d = cos(angle) * flipY / scale
        try self.init(matrix: CGAffineTransform(a: a, b: b, c: c, d: d,
            tx: source.x - a * destination.x - c * destination.y,
            ty: source.y - b * destination.x - d * destination.y))
    }

    var values: [Double] { [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty].map(Double.init) }

    /// Input CGContext is in top-left canvas coordinates. CGImage's own
    /// bottom-left convention is undone after applying source placement.
    func draw(_ image: CGImage, in context: CGContext) {
        context.saveGState()
        context.concatenate(matrix.inverted())
        context.translateBy(x: 0, y: CGFloat(image.height))
        context.scaleBy(x: 1, y: -1)
        context.interpolationQuality = .low
        context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        context.restoreGState()
    }
}

extension AgentServer {
    func retouchGeometry(_ a: [String: Any], source: CGPoint, destination: CGPoint) throws -> RetouchGeometry {
        guard let raw = a["source_transform"] else {
            return try RetouchGeometry(source: source, destination: destination)
        }
        guard let values = raw as? [NSNumber], values.count == 6 else {
            throw ToolError(message: "source_transform requires [a,b,c,d,tx,ty], mapping destination canvas centers to source canvas centers.")
        }
        return try RetouchGeometry(matrix: CGAffineTransform(a: values[0].doubleValue,
            b: values[1].doubleValue, c: values[2].doubleValue, d: values[3].doubleValue,
            tx: values[4].doubleValue, ty: values[5].doubleValue))
    }

    /// Legacy calls retain their historical automatic path/dab choice. New
    /// canvas clone/heal recordings name their always-stamped footprint.
    func retouchUsesDabs(_ a: [String: Any], tip: BrushTip, size: CGFloat) throws -> Bool {
        guard let raw = a["footprint"] else { return SoftBrush.isStamped(tip: tip, size: size) }
        guard let value = raw as? String, ["automatic", "dabs"].contains(value) else {
            throw ToolError(message: "footprint must be automatic or dabs.")
        }
        return value == "dabs" || SoftBrush.isStamped(tip: tip, size: size)
    }

    func retouchExpansion(_ a: [String: Any]) throws -> Bool {
        guard a["expand_destination"] != nil else { return false }
        guard let value = NativeBrushStroke.jsonBoolean(a["expand_destination"]) else {
            throw ToolError(message: "expand_destination must be a boolean.")
        }
        return value
    }
}

extension PrecisionPixelCommand {
    static func retouch(layer: Int, sourceLayer: Int? = nil, heal: Bool,
                        geometry: RetouchGeometry, choice: RetouchSamplingChoice,
                        output: RetouchOutput, expand: Bool, opacity: Double,
                        blendMode: DrBlendMode = DR_BLEND_NORMAL,
                        selection: Bool = false,
                        bounds: (x: Int, y: Int, width: Int, height: Int)? = nil) throws -> Self {
        var fields: [String: Any] = ["op": heal ? "retouch_heal" : "retouch_clone", "layer": layer,
            "matrix": geometry.values, "sample": choice.mode.rawValue,
            "ignore_adjustments": choice.ignoreAdjustments, "output": output.rawValue,
            "expand_destination": expand, "opacity": opacity]
        // Only the clone composites a source colour, so only it carries
        // a mode, and Normal is the request it always sent.
        if !heal, blendMode != DR_BLEND_NORMAL { fields["blend_mode"] = Int(blendMode.rawValue) }
        if let sourceLayer { fields["source_layer"] = sourceLayer }
        if selection { fields["selection"] = true }
        if let bounds {
            fields["x"] = bounds.x; fields["y"] = bounds.y
            fields["width"] = bounds.width; fields["height"] = bounds.height
        }
        return try Self(fields)
    }

    /// `type` is written only when it is not Content-Aware, so a
    /// Content-Aware request is the request it always was.
    static func spotHeal(layer: Int, choice: RetouchSamplingChoice, output: RetouchOutput,
                         expand: Bool, opacity: Double, ring: Int = 0, seed: UInt64 = 0,
                         type: SpotHealType = .contentAware, selection: Bool = false,
                         bounds: (x: Int, y: Int, width: Int, height: Int)) throws -> Self {
        var fields: [String: Any] = ["op": "spot_heal", "layer": layer, "sample": choice.mode.rawValue,
            "ignore_adjustments": choice.ignoreAdjustments, "output": output.rawValue,
            "expand_destination": expand, "opacity": opacity, "ring": ring, "seed": seed, "selection": selection,
            "x": bounds.x, "y": bounds.y, "width": bounds.width, "height": bounds.height]
        if type != .contentAware { fields["type"] = type.rawValue }
        return try Self(fields)
    }

    static func redEye(layer: Int, rect: CGRect, pupilSize: Double, darken: Double) throws -> Self {
        try Self(["op": "red_eye", "layer": layer, "x": Int(floor(rect.minX)), "y": Int(floor(rect.minY)),
            "width": Int(ceil(rect.maxX)) - Int(floor(rect.minX)),
            "height": Int(ceil(rect.maxY)) - Int(floor(rect.minY)), "pupil_size": pupilSize, "darken": darken])
    }
}
