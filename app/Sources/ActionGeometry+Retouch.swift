import Foundation

/// The retouch steps' geometry kinds (`GeometryField.length`, `.discs`, `.sourceMask`,
/// `.strokes`) and a stroke's canonical dabs, beside `ActionGeometry`'s positions and extents.
///
/// **A length has no axis.** A brush diameter or a disc radius on a picture resized by `sx × sy`
/// becomes `√(sx · sy)` times itself under `.relative`: the exact scale under a uniform resize, and
/// the area-preserving one otherwise. It is clamped into the range the tool itself accepts, so a
/// relative replay that would exceed a tool's ceiling runs at the ceiling, exactly as the tool's
/// own clamp would have run it. `.anchor` keeps every length, as it keeps every extent.
///
/// **Canonical dabs move with their points.** A recorded stroke carrying `native_stroke` replays
/// those dabs, so each dab's centre is remapped as a position and its diameter as a length
/// (clamped to 1 px and the tool's ceiling), and the payload is re-encoded with any per-dab tip
/// shapes (`shapes`) and any texture (its tile, scale and canvas-origin anchor) unchanged. The
/// grid-snapped kinds
/// (`pixel`, `aliased`) keep their refusal: a snapped dab has no honest scaled or half-pixel-shifted
/// form.
extension ActionGeometry {
    /// `√(sx · sy)` for a relative remap from `recorded` into `live`.
    static func lengthFactor(
        recorded: (width: Int, height: Int), live: (width: Int, height: Int)
    ) -> Double {
        let sx = Double(live.width) / Double(recorded.width)
        let sy = Double(live.height) / Double(recorded.height)
        return (sx * sy).squareRoot()
    }

    /// One length, clamped into `range` when it is scaled.
    static func mappedLength(
        _ number: NSNumber, range: ClosedRange<Double>?, mode: GeometryMode,
        recorded: (width: Int, height: Int), live: (width: Int, height: Int)
    ) -> Any {
        guard mode == .relative else { return number }
        var value = number.doubleValue * lengthFactor(recorded: recorded, live: live)
        if let range { value = min(max(value, range.lowerBound), range.upperBound) }
        return quantized(value, like: number)
    }

    /// The nested retouch kinds.
    static func mappedRetouch(
        _ value: Any, key: String, field: GeometryField, mode: GeometryMode,
        recorded: (width: Int, height: Int), live: (width: Int, height: Int)
    ) -> Result<Any, ActionError> {
        if value is NSNull { return .success(value) }
        func number(_ any: Any?) -> NSNumber? { ActionJSON.number(any) }
        switch field {
        case .discs:
            guard let discs = value as? [Any] else {
                return .failure(ActionError("a remapped \"\(key)\" needs [{x, y, radius}, …]"))
            }
            var out: [Any] = []
            for disc in discs {
                guard var fields = disc as? [String: Any], let x = number(fields["x"]),
                      let y = number(fields["y"]), let radius = number(fields["radius"])
                else {
                    return .failure(
                        ActionError("a remapped \"\(key)\" needs {x, y, radius} numbers"))
                }
                fields["x"] = mapped(x, field: .x, mode: mode, recorded: recorded, live: live)
                fields["y"] = mapped(y, field: .y, mode: mode, recorded: recorded, live: live)
                fields["radius"] = mappedLength(
                    radius, range: nil, mode: mode, recorded: recorded, live: live)
                out.append(fields)
            }
            return .success(out)
        case .strokes:
            guard let strokes = value as? [Any] else {
                return .failure(ActionError("a remapped \"\(key)\" needs [{points, size}, …]"))
            }
            let range = EditorTool.remove.minimumBrushSize...EditorTool.remove.maximumBrushSize
            var out: [Any] = []
            for stroke in strokes {
                guard var fields = stroke as? [String: Any], let points = fields["points"] else {
                    return .failure(
                        ActionError("a remapped \"\(key)\" needs objects carrying \"points\""))
                }
                switch mappedPoints(points, mode: mode, recorded: recorded, live: live) {
                case .success(let mapped): fields["points"] = mapped
                case .failure(let error): return .failure(error)
                }
                if let size = number(fields["size"]) {
                    fields["size"] = mappedLength(
                        size, range: range, mode: mode, recorded: recorded, live: live)
                }
                out.append(fields)
            }
            return .success(out)
        case .sourceMask:
            guard var mask = value as? [String: Any], let operations = mask["operations"] as? [Any]
            else {
                return .failure(
                    ActionError("a remapped \"\(key)\" needs an object with \"operations\""))
            }
            var out: [Any] = []
            for operation in operations {
                guard var fields = operation as? [String: Any] else {
                    return .failure(ActionError("each \"\(key)\" operation must be an object"))
                }
                if fields["x"] != nil || fields["width"] != nil {
                    switch mappedRect(
                        fields, key: key, mode: mode, recorded: recorded, live: live)
                    {
                    case .success(let rect as [String: Any]): fields = rect
                    case .success: break
                    case .failure(let error): return .failure(error)
                    }
                }
                if let points = fields["points"] {
                    switch mappedPoints(points, mode: mode, recorded: recorded, live: live) {
                    case .success(let mapped): fields["points"] = mapped
                    case .failure(let error): return .failure(error)
                    }
                }
                if let radius = number(fields["radius"]) {
                    // The core's own range for a source-mask brush.
                    fields["radius"] = mappedLength(
                        radius, range: 0.5...4096, mode: mode, recorded: recorded, live: live)
                }
                out.append(fields)
            }
            mask["operations"] = out
            return .success(mask)
        case .x, .y, .w, .h, .dx, .dy, .points, .guideAxis, .pair, .rect, .rectArray, .affine,
             .length:
            return .success(value)
        }
    }

    /// Why a stroke's canonical dabs may not be remapped, or nil when they may: the grid-snapped
    /// kinds, and a payload that is not a canonical object at all. Read off the object's flags,
    /// so a decode-time check never inflates the dabs.
    static func nativeStrokeRefusal(tool: String, _ native: Any) -> String? {
        guard let canonical = native as? [String: Any] else {
            return "\"\(tool)\"'s native_stroke is not a canonical stroke object, so its dabs "
                + "cannot be remapped"
        }
        let pixel = NativeBrushStroke.jsonBoolean(canonical["pixel"]) ?? false
        let aliased = NativeBrushStroke.jsonBoolean(canonical["aliased"]) ?? false
        guard pixel || aliased else { return nil }
        return "\"\(tool)\" was captured as a \(aliased ? "Pencil" : "pixel-brush") stroke, whose "
            + "canonical dabs are snapped to the pixel grid and have no scaled or shifted form — "
            + "relative and anchor remapping cannot apply. Re-record it on the target size"
    }

    /// `native` with every dab's centre remapped as a position and its diameter as a length,
    /// clamped to 1 px and the tool's own ceiling (`AgentServer.strokeEditorTool`), re-encoded.
    static func mappedNativeStroke(
        _ native: Any, tool: String, mode: GeometryMode,
        recorded: (width: Int, height: Int), live: (width: Int, height: Int)
    ) -> Result<Any, ActionError> {
        guard let canonical = native as? [String: Any] else {
            return .failure(ActionError(nativeStrokeRefusal(tool: tool, native) ?? "native_stroke"))
        }
        do {
            let stroke = try NativeBrushStroke(canonical: canonical)
            let ceiling = min(
                AgentServer.strokeEditorTool(tool).maximumBrushSize, NativeBrushStroke.maximumDiameter)
            let factor = mode == .relative ? lengthFactor(recorded: recorded, live: live) : 1
            func mapped(_ list: [Double]) -> [Double] {
                var dabs = list
                for index in stride(from: 0, to: dabs.count, by: 4) {
                    if let x = mappedValue(
                        dabs[index], horizontal: true, isPosition: true, mode: mode,
                        recorded: recorded, live: live) { dabs[index] = x }
                    if let y = mappedValue(
                        dabs[index + 1], horizontal: false, isPosition: true, mode: mode,
                        recorded: recorded, live: live) { dabs[index + 1] = y }
                    dabs[index + 2] = min(max(dabs[index + 2] * factor, 1), ceiling)
                }
                return dabs
            }
            // A tip shape's angle and roundness are not lengths: the diameter's one scalar
            // factor leaves them as they are, so `shapes` passes through unchanged. A texture
            // is anchored at the canvas origin at its own scale, so it passes through as
            // recorded too (its tile, scale and anchor). A dual brush's second dabs map as the
            // first's (centres as positions, diameters by the one factor); its tip's hardness,
            // angle and roundness pass through as recorded.
            let dual = try stroke.dual.map { try StrokeDual(tip: $0.tip, dabs: mapped($0.dabs)) }
            let remapped = try NativeBrushStroke(tip: stroke.tip, pixel: stroke.pixel,
                                                 dabs: mapped(stroke.dabs), shapes: stroke.shapes,
                                                 texture: stroke.texture, dual: dual)
            return .success(try remapped.canonical)
        } catch {
            return .failure(
                ActionError("\"\(tool)\"'s native stroke could not be remapped: "
                    + error.localizedDescription))
        }
    }
}
