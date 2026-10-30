import AppKit

/// The agent mirrors of the five retouch brushes at 8 bits: `blur_stroke`, `sharpen_stroke`,
/// `sponge_stroke` and `history_brush_stroke` rasterize the same coverage overlay `dodge_burn`
/// builds and hand it to the canvas tools' own core ops (`EditorViewController+RetouchBrushes.swift`
/// drives the same ops per tick); `smudge_stroke` hands the stroke's DABS to the core, exactly as
/// the canvas Smudge's final tick does. Their 16-bit and 32-bit float twins are
/// `PrecisionRetouchBrushTools.swift`. `sponge_stroke`'s `vibrance` mirrors the Sponge's Vibrance
/// checkbox and `sample_all_layers` on Blur, Sharpen and Smudge their Sample All Layers
/// checkbox; both default to false here (the options bar's Vibrance defaults on).
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let retouchBrushHandlers: [String: ToolHandler] = [
        "blur_stroke": { $0.blurStroke },
        "sharpen_stroke": { $0.sharpenStroke },
        "smudge_stroke": { $0.smudgeStroke },
        "sponge_stroke": { $0.spongeStroke },
        "history_brush_stroke": { $0.historyBrushStroke },
        "art_history_brush_stroke": { $0.artHistoryBrushStroke },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in
    /// AgentServer+Registry.swift. All five are `.floatTool` (`FloatRetouch.brushTools`,
    /// linear light with the range kept; the Sponge is `display_encoding`'s R5).
    /// `history_brush_stroke` names a state of this session's history, which no later replay
    /// can name, so it is never recorded.
    static let retouchBrushToolFacts: [String: ToolFacts] = [
        "blur_stroke": [.typedTool, .rasterTool, .floatTool, .cmykTool],
        "sharpen_stroke": [.typedTool, .rasterTool, .floatTool, .cmykTool],
        "smudge_stroke": [.typedTool, .rasterTool, .floatTool, .cmykTool],
        "sponge_stroke": [.typedTool, .rasterTool, .floatTool, .cmykTool],
        "history_brush_stroke": [.typedTool, .rasterTool, .nonRecordable, .floatTool, .cmykTool],
        // Recordable — the step drops `state_id` (`ActionRecorder.recordAgentCall`) and
        // replays from the History panel's source; typed depths reach the core's refusals.
        "art_history_brush_stroke": [.typedTool, .rasterTool, .floatTool, .cmykTool],
    ]

    // MARK: - Shared arguments

    /// The stroke's `size` (1–200, default 24) and its amount in percent
    /// (`strength` or `opacity`, 0–100).
    func retouchBrushAmounts(_ a: [String: Any], key: String, fallback: Double)
        throws -> (size: CGFloat, percent: Double) {
        let rawSize = doubleArg(a, "size") ?? 24
        let rawAmount = doubleArg(a, key) ?? fallback
        guard rawSize.isFinite, rawAmount.isFinite else {
            throw ToolError(message: "size and \(key) must be finite numbers")
        }
        return (CGFloat(min(max(rawSize, 1), 200)), min(max(rawAmount, 0), 100))
    }

    /// `sponge_stroke`'s `mode`: desaturate (the default) or saturate.
    func spongeSaturates(_ a: [String: Any]) throws -> Bool {
        switch stringArg(a, "mode") ?? "desaturate" {
        case "desaturate": return false
        case "saturate": return true
        case let other:
            throw ToolError(message: "mode must be desaturate or saturate (got \"\(other)\")")
        }
    }

    /// The History Brush's source for `layer`, as a `ToolError` sentence when
    /// it cannot be latched (`HistoryBrushSource.latch`).
    func historyBrushSource(_ a: [String: Any], document: ImageDocument,
                            layer: Int) throws -> HistoryBrushSource {
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        do {
            return try HistoryBrushSource.latch(in: document, current: doc, layer: layer,
                                                stateID: stringArg(a, "state_id"))
        } catch { throw ToolError(message: error.localizedDescription) }
    }

    /// The refusal of Sample All Layers aimed at a mask or a channel.
    static let sampleAllLayersTargetRefusal =
        "sample_all_layers samples the picture into a layer's pixels; it does not apply to a "
        + "mask or channel target"

    /// The core's two option refusals (`retouch_brush::VIBRANCE_REFUSAL` and
    /// `SAMPLE_ALL_LAYERS_REFUSAL`, the master copies), asked by name before
    /// any work: `vibrance` on any tool but the Sponge, `sample_all_layers`
    /// on the Sponge.
    static let vibranceRefusal = "vibrance applies to the sponge only"
    static let sampleAllLayersRefusal = "sample_all_layers applies to blur, sharpen and smudge"

    func refuseMisplacedOption(_ a: [String: Any], sponge: Bool) throws {
        if sponge, boolArg(a, "sample_all_layers") == true {
            throw ToolError(message: Self.sampleAllLayersRefusal)
        }
        if !sponge, boolArg(a, "vibrance") == true {
            throw ToolError(message: Self.vibranceRefusal)
        }
    }

    /// `sample_all_layers` (Blur, Sharpen, Smudge; default false), refused
    /// with a `target` of `mask` or `channel:…`.
    func sampleAllLayersArg(_ a: [String: Any]) throws -> Bool {
        if let refusal = Self.sampledTargetRefusal(a) { throw ToolError(message: refusal) }
        return boolArg(a, "sample_all_layers") ?? false
    }

    /// The same refusal as a sentence, for the 32-bit float and CMYK
    /// pre-dispatch gates (`FloatRetouch`, `CmykRetouch`), which answer a
    /// target before the handler runs: one sentence at every depth.
    static func sampledTargetRefusal(_ a: [String: Any]) -> String? {
        let on: Bool
        if let number = a["sample_all_layers"] as? NSNumber { on = number.boolValue }
        else { on = ["true", "1"].contains(a["sample_all_layers"] as? String ?? "") }
        guard on, let raw = a["target"] as? String,
              raw == "mask" || raw.hasPrefix("channel:") else { return nil }
        return sampleAllLayersTargetRefusal
    }

    func refuseBlendMode(_ a: [String: Any], tool: String) throws {
        guard stringArg(a, "blend_mode") == nil else {
            throw ToolError(
                message: "\(tool) has no blend_mode — it reworks the layer's own pixels; "
                    + "blend modes apply to brush_stroke and clone_stamp.")
        }
    }

    // MARK: - The four coverage strokes

    /// One coverage stroke committed through `op`: the canvas-sized overlay
    /// (white ink through the selection clip, exactly as `dodge_burn` builds
    /// it — canonical dabs when the call carries `native_stroke`) handed to a
    /// core op that reads only its alpha. `op` answering nil is a no-op, not a
    /// failure: no edit landed and no undo step opened. A sentence `op`
    /// throws is latched and rethrown as the call's `ToolError`.
    private func retouchCoverageStroke(
        _ a: [String: Any], document: ImageDocument, layer index: Int, actionName: String,
        size: CGFloat, applied: [String: Any],
        op: @escaping (RasterDocument, UnsafePointer<UInt8>, Int, Int) throws -> RasterDocument?
    ) throws -> String {
        let points = try byteStrokePoints(a)
        let tip = try strokeTip(a)
        // Latched when the OP ITSELF answers nil, so only that case reads as
        // a no-op — any other failure stays an error.
        var opRefused = false
        var failure: String?
        let commit: (RasterDocument, UnsafePointer<UInt8>, Int, Int) -> RasterDocument? = {
            do {
                let out = try op($0, $1, $2, $3)
                if out == nil { opRefused = true }
                return out
            } catch {
                failure = error.localizedDescription
                return nil
            }
        }
        let unchanged = {
            try self.jsonResult([
                "ok": true, "changed": false, "layer": index,
                "note": "\(actionName) changed no pixel: the stroke never covered layer "
                    + "\(index)'s pixels, or they are already what the tool would make them. "
                    + "No undo step was added.",
            ])
        }
        let white = ColorProfile.coverageInk(
            NSColor(srgbRed: 1, green: 1, blue: 1, alpha: 1), in: document.drawingSpace)
        if let canonical = try capturedNativeStroke(a) {
            let pixels = try byteCanonicalRetouchPixels(document, stroke: canonical, size: size,
                                                        ink: .color(white))
            let rasterized: DroppedDescription?
            do {
                rasterized = try byteCanonicalRetouchCommit(
                    document, layer: index, actionName: actionName, pixels: pixels, commit: commit)
            } catch let error as ToolError {
                if let failure { throw ToolError(message: failure) }
                guard opRefused else { throw error }
                return try unchanged()
            }
            return try pixelEditResult(
                ["ok": true, "action": actionName, "layer": index,
                 "dabs": canonical.dabs.count / 4, "applied": applied],
                layer: index, rasterized: rasterized)
        }
        let stamped = SoftBrush.isStamped(tip: tip, size: size)
        let centers = try stamped ? SoftBrush.stampCenters(along: points, spacing: SoftBrush.spacing(
            for: size, percent: tip.spacingPercent)) : []
        let rasterized: DroppedDescription?
        do {
            rasterized = try retouchOverlay(
                document, layer: index, actionName: actionName,
                draw: { context in
                    // Pure coverage, like the interactive stroke: a non-default
                    // tip stamps white falloff dabs at the tip's flow.
                    if stamped,
                        let dab = SoftBrush.dab(
                            color: white.withAlphaComponent(tip.flow), diameter: size,
                            hardness: tip.hardness, space: document.drawingSpace) {
                        for center in centers {
                            SoftBrush.stamp(dab, in: context, at: center, diameter: size, tip: tip)
                        }
                        return
                    }
                    context.setFillColor(white.cgColor)
                    context.addPath(Self.strokeCoverage(points: points, size: size))
                    context.fillPath()
                },
                commit: commit)
        } catch let error as ToolError {
            if let failure { throw ToolError(message: failure) }
            guard opRefused else { throw error }
            return try unchanged()
        }
        return try pixelEditResult(
            ["ok": true, "action": actionName, "layer": index, "points": points.count,
             "applied": applied],
            layer: index, rasterized: rasterized)
    }

    /// Blur, Sharpen or Sponge: the shared stroke over `retouchBrushLayer`,
    /// with the Sponge's `vibrance` or Blur and Sharpen's `sample_all_layers`.
    private func retouchKindStroke(_ a: [String: Any], tool: String, actionName: String,
                                   kind: RetouchBrushKind, applied: [String: Any] = [:],
                                   vibrance: Bool = false) throws -> String {
        let sponge = kind == .saturate || kind == .desaturate
        try refuseMisplacedOption(a, sponge: sponge)
        let sampleAllLayers = sponge ? false : try sampleAllLayersArg(a)
        let document = try target(a)
        let index = try paintLayerIndex(a, document)
        // The UI refuses these strokes on an adjustment layer outright
        // (onStrokeBegin): there are no pixels to rework.
        try rejectAdjustmentPixelEdit(document, index)
        let (size, percent) = try retouchBrushAmounts(a, key: "strength", fallback: 50)
        try refuseBlendMode(a, tool: tool)
        var applied = applied
        applied["strength"] = Self.transformNumber(percent)
        if kind == .saturate || kind == .desaturate {
            applied["vibrance"] = vibrance
        } else {
            applied["sample_all_layers"] = sampleAllLayers
        }
        return try retouchCoverageStroke(
            a, document: document, layer: index, actionName: actionName, size: size,
            applied: applied
        ) { current, base, w, h in
            try current.retouchBrushLayer(index, overlay: base, w: w, h: h, kind: kind,
                                          strength: percent / 100, vibrance: vibrance,
                                          sampleAllLayers: sampleAllLayers)
        }
    }

    /// blur_stroke — mirrors the Blur tool and its Sample All Layers checkbox:
    /// softens the layer's pixels under the stroke's coverage toward their own
    /// Gaussian blur, or toward the sampled picture's.
    func blurStroke(_ a: [String: Any]) throws -> String {
        try retouchKindStroke(a, tool: "blur_stroke", actionName: "Blur", kind: .blur)
    }

    /// sharpen_stroke — mirrors the Sharpen tool and its Sample All Layers
    /// checkbox: an unsharp mask under the stroke's coverage.
    func sharpenStroke(_ a: [String: Any]) throws -> String {
        try retouchKindStroke(a, tool: "sharpen_stroke", actionName: "Sharpen", kind: .sharpen)
    }

    /// sponge_stroke — mirrors the Sponge tool, its Mode popup and its Vibrance
    /// checkbox: moves the colour under the stroke toward its luma
    /// (desaturate) or away from it (saturate), or with `vibrance` toward the
    /// Vibrance target.
    func spongeStroke(_ a: [String: Any]) throws -> String {
        // One sentence at every depth (`FloatRetouch.spongeCoverageRefusal`).
        if let raw = stringArg(a, "target"), raw == "mask" || raw.hasPrefix("channel:") {
            throw ToolError(message: FloatRetouch.spongeCoverageRefusal)
        }
        let saturates = try spongeSaturates(a)
        return try retouchKindStroke(
            a, tool: "sponge_stroke", actionName: "Sponge",
            kind: saturates ? .saturate : .desaturate,
            applied: ["mode": saturates ? "saturate" : "desaturate"],
            vibrance: boolArg(a, "vibrance") ?? false)
    }

    /// history_brush_stroke — mirrors the History Brush: paints the layer back
    /// toward what it held in a history state (`state_id` from list_history;
    /// by default the History panel's source, else the oldest retained state).
    func historyBrushStroke(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let index = try paintLayerIndex(a, document)
        try rejectAdjustmentPixelEdit(document, index)
        let (size, percent) = try retouchBrushAmounts(a, key: "opacity", fallback: 100)
        try refuseBlendMode(a, tool: "history_brush_stroke")
        let source = try historyBrushSource(a, document: document, layer: index)
        return try retouchCoverageStroke(
            a, document: document, layer: index, actionName: "History Brush", size: size,
            applied: ["opacity": Self.transformNumber(percent)]
        ) { current, base, w, h in
            current.historyBrushLayer(index, overlay: base, w: w, h: h, source: source.document,
                                      sourceLayer: source.layer, opacity: percent / 100)
        }
    }

    // MARK: - Smudge

    /// The dabs a smudge call walks: canonical `native_stroke` dabs, or the
    /// generated walk of `points` at the tip's spacing — the same dabs the
    /// canvas stroke stamps.
    func smudgeDabs(_ a: [String: Any], size: CGFloat) throws -> NativeBrushStroke {
        do {
            return try capturedNativeStroke(a) ?? NativeBrushStrokeSession.generated(
                points: parsePoints(a), size: size, tip: strokeTip(a))
        } catch let error as ToolError { throw error } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// smudge_stroke — mirrors the Smudge tool and its Sample All Layers
    /// checkbox: drags the layer's pixels (or the sampled picture) along the
    /// stroke. The whole dab list runs once over the layer, which is what the
    /// canvas stroke's final tick computes.
    func smudgeStroke(_ a: [String: Any]) throws -> String {
        try refuseMisplacedOption(a, sponge: false)
        let sampleAllLayers = try sampleAllLayersArg(a)
        let document = try target(a)
        let index = try paintLayerIndex(a, document)
        try rejectAdjustmentPixelEdit(document, index)
        let (size, percent) = try retouchBrushAmounts(a, key: "strength", fallback: 50)
        try refuseBlendMode(a, tool: "smudge_stroke")
        let stroke = try smudgeDabs(a, size: size)
        let request: String
        do { request = try stroke.request } catch {
            throw ToolError(message: error.localizedDescription)
        }
        let selection = selectionMask(document)
        // The core op answers nil for "nothing moved" and throws its own
        // sentence for a stroke it refuses; both are latched here because
        // `performPixelEdit`'s transform cannot throw.
        var opRefused = false
        var failure: String?
        let rasterized: DroppedDescription?
        do {
            rasterized = try performPixelEdit(document, "Smudge", pixelLayer: index) { current in
                do {
                    let out = try current.smudgeLayer(index, request: request, dabs: stroke.dabs,
                        strength: percent / 100, selection: selection,
                        sampleAllLayers: sampleAllLayers)
                    if out == nil { opRefused = true }
                    return out
                } catch {
                    failure = error.localizedDescription
                    return nil
                }
            }
        } catch let error as ToolError {
            if let failure { throw ToolError(message: failure) }
            guard opRefused else { throw error }
            return try jsonResult([
                "ok": true, "changed": false, "layer": index,
                "note": "Smudge changed no pixel: the stroke has fewer than two dabs, never "
                    + "reached layer \(index)'s pixels, or dragged a flat area. "
                    + "No undo step was added.",
            ])
        }
        return try pixelEditResult(
            ["ok": true, "action": "Smudge", "layer": index, "dabs": stroke.dabs.count / 4,
             "applied": ["strength": Self.transformNumber(percent),
                         "sample_all_layers": sampleAllLayers]],
            layer: index, rasterized: rasterized)
    }
}
