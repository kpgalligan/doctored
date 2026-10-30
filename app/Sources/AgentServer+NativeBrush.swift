import AppKit

extension AgentServer {
    /// The UI tool an MCP stroke tool mirrors, whose size range it takes
    /// (`EditorTool.minimumBrushSize`…`maximumBrushSize`); `quick_mask_stroke`
    /// follows Brush. The five retouch brushes mirror their own tools,
    /// so they keep their 200 px ceiling.
    static func strokeEditorTool(_ tool: String) -> EditorTool {
        switch tool {
        case "eraser_stroke": return .eraser
        case "clone_stamp": return .clone
        case "heal_stroke": return .heal
        case "spot_heal_stroke": return .spotHeal
        case "dodge_burn": return .dodge
        case "blur_stroke": return .blurBrush
        case "sharpen_stroke": return .sharpenBrush
        case "smudge_stroke": return .smudge
        case "sponge_stroke": return .sponge
        case "history_brush_stroke": return .historyBrush
        case "art_history_brush_stroke": return .artHistoryBrush
        default: return .brush
        }
    }

    /// Resolve the public pressure/timing form before dispatch and recording.
    /// Unversioned point calls retain their historical byte-footprint meaning.
    func nativeStrokeArguments(tool: String, arguments: [String: Any], document: ImageDocument?) throws -> [String: Any] {
        let tools: Set<String> = ["brush_stroke", "eraser_stroke", "clone_stamp", "heal_stroke", "spot_heal_stroke", "dodge_burn", "quick_mask_stroke",
            "blur_stroke", "sharpen_stroke", "smudge_stroke", "sponge_stroke", "history_brush_stroke",
            "art_history_brush_stroke"]
        let keys: Set<String> = ["stroke_version", "native_stroke", "pressures", "times", "duration",
                                 "pressure_size", "pressure_opacity", "airbrush"]
        guard !keys.isDisjoint(with: arguments.keys) else { return arguments }
        // Phase 12A: a BYTE document admits canonical dabs for the two LAYER
        // paint tools, so a recorded pressure/airbrush byte stroke replays as
        // the dabs it actually deposited. The pixel brush (size <= 1, canonical
        // `pixel: true`) keeps the historical point path, and every other
        // stroke tool still refuses by name at byte depth.
        guard tools.contains(tool), document?.doc != nil else {
            throw ToolError(message: "Versioned strokes require a byte or 16-bit document and a stroke tool.")
        }
        if let canonical = arguments["native_stroke"] as? [String: Any] {
            guard keys.subtracting(["native_stroke"]).isDisjoint(with: arguments.keys) else {
                throw ToolError(message: "Canonical native_stroke cannot also contain stroke generation options.")
            }
            let stroke = try NativeBrushStroke(canonical: canonical)
            // A canonical stroke takes the mirrored UI tool's ceiling too (5,000 px, 200 for
            // the healing and retouch brushes). Upper bound only: a pressure-sized canvas dab
            // goes below the tool's minimum.
            let ceiling = Self.strokeEditorTool(tool).maximumBrushSize
            guard stride(from: 2, to: stroke.dabs.count, by: 4).allSatisfy({ stroke.dabs[$0] <= ceiling }) else {
                throw ToolError(message: "Native stroke dab diameter is above the supported maximum "
                    + "(\(Int(ceiling)) px for \(tool)); no partial stroke was applied.")
            }
            return arguments
        }
        let version = NativeBrushStroke.jsonNumber(arguments["stroke_version"])
        guard version == 1, arguments["native_stroke"] == nil else {
            throw ToolError(message: "Generated native strokes require stroke_version=1; unknown versions refuse.")
        }
        let points = try parsePoints(arguments)
        let heal = ["heal_stroke", "spot_heal_stroke"].contains(tool)
        let size = doubleArg(arguments, "size") ?? (tool == "brush_stroke" || tool == "eraser_stroke" ? 16 : 24)
        // The range is the mirrored UI tool's own: 1–5,000 px, 3–200 for the healing
        // brushes, 1–200 for the retouch brushes.
        let mirrored = Self.strokeEditorTool(tool)
        let range = mirrored.minimumBrushSize...mirrored.maximumBrushSize
        guard size.isFinite, range.contains(size) else {
            throw ToolError(message: "Native stroke size is outside the supported range "
                + "(\(Int(range.lowerBound))–\(Int(range.upperBound)) px for \(tool)).")
        }
        var tip = try strokeTip(arguments)
        if heal { tip.flow = 1 }
        func samples(_ key: String, fallback: [Double]) throws -> [Double] {
            guard let raw = arguments[key] else { return fallback }
            guard let array = raw as? [Any], array.count == points.count,
                  array.allSatisfy({ NativeBrushStroke.jsonNumber($0)?.isFinite == true }) else {
                throw ToolError(message: "\(key) must contain one finite number per point.")
            }
            return array.compactMap(NativeBrushStroke.jsonNumber)
        }
        let pressures = try samples("pressures", fallback: Array(repeating: 1, count: points.count))
        let times = try samples("times", fallback: Array(repeating: 0, count: points.count))
        guard pressures.allSatisfy({ (0...1).contains($0) }), times.first == 0,
              zip(times, times.dropFirst()).allSatisfy({ $0 <= $1 }) else {
            throw ToolError(message: "Pressure must be in 0…1; times must start at zero and never decrease.")
        }
        let duration = doubleArg(arguments, "duration") ?? times.last ?? 0
        if let raw = arguments["duration"], NativeBrushStroke.jsonNumber(raw) == nil, !(raw is String) {
            throw ToolError(message: "duration must be numeric elapsed seconds.")
        }
        guard duration.isFinite, duration >= (times.last ?? 0), duration <= 2_499 else {
            throw ToolError(message: "Native stroke duration must include all samples and be at most 2,499 seconds.")
        }
        func flag(_ key: String) throws -> Bool {
            guard let raw = arguments[key] else { return false }
            if let value = NativeBrushStroke.jsonBoolean(raw) { return value }
            if let text = raw as? String, ["true", "false"].contains(text.lowercased()) { return text.lowercased() == "true" }
            throw ToolError(message: "\(key) must be boolean.")
        }
        let pressureSize = try flag("pressure_size")
        let pressureOpacity = try flag("pressure_opacity")
        let airbrush = try flag("airbrush")
        let stroke = try NativeBrushStrokeSession.generated(points: points, size: CGFloat(size), tip: tip,
            pressures: pressures, times: times, duration: duration, pressureSize: pressureSize,
            pressureOpacity: pressureOpacity, airbrush: airbrush)
        var resolved = arguments
        for key in keys { resolved.removeValue(forKey: key) }
        resolved["native_stroke"] = try stroke.canonical
        return resolved
    }

    /// The BYTE replay of a canonical dab stroke — the mirror of the canvas's
    /// own byte commit, which rasterizes the same dabs through the same
    /// routine inside the same selection clip
    /// (`ImageCanvasView.commitByteDabs`). That shared routine is what makes a
    /// recorded byte pressure/airbrush stroke replay byte-identically.
    ///
    /// Refuses BY NAME anything outside the Phase 12A decision: a coverage
    /// target (mask, plane, channel) keeps the overlay/points path, and a
    /// `pixel: true` stroke has no byte dab form.
    func byteDabPaint(
        _ document: ImageDocument, layer: Int, target: PaintTarget, actionName: String,
        color: NSColor, size: CGFloat, opacity: Double, blend: DrBlendMode?,
        stroke: NativeBrushStroke, erase: Bool
    ) throws -> String {
        try byteCanonicalPaint(document, layer: layer, target: target,
            actionName: actionName, color: color, size: size, opacity: opacity,
            blend: blend, stroke: stroke, erase: erase)
    }
    func capturedNativeStroke(_ arguments: [String: Any]) throws -> NativeBrushStroke? {
        guard let value = arguments["native_stroke"] as? [String: Any] else { return nil }
        return try NativeBrushStroke(canonical: value)
    }
}
