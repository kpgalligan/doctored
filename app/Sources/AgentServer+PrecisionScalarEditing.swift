import AppKit

extension AgentServer {
    /// Mirrors brush/eraser and Fill while a Channels row or layer mask is targeted.
    func precisionScalarPaint(_ tool: String, _ a: [String: Any], document: ImageDocument,
                              target requested: PaintTarget) throws -> String {
        let capture = try PrecisionEditorCapture(document)
        let requestedLayer = try structuralLayerIndex(a, document)
        let erase = tool == "eraser_stroke" || tool == "clear_selection"
        let resolved = try resolvePaintTarget(a, document, requestedLayer: requestedLayer,
            erase: erase, blend: try blendModeArg(a))
        guard ["brush_stroke", "eraser_stroke", "fill", "clear_selection", "dodge_burn"].contains(tool) else {
            throw ToolError(message: "Retouch requires layer RGB; choose brush or eraser for scalar coverage.")
        }
        if !capture.quickMask, let reason = scopeRefusal(.filter, target: resolved.target,
            doc: capture.facade, layer: resolved.layer) { throw ToolError(message: reason) }
        let edit: PrecisionScalarWork
        if tool == "fill" {
            guard let x = intArg(a, "x"), let y = intArg(a, "y") else { throw ToolError(message: "Fill requires x and y.") }
            let tolerance = doubleArg(a, "tolerance") ?? 32
            guard tolerance.isFinite, (0...255).contains(tolerance) else { throw ToolError(message: "Tolerance must be between 0 and 255.") }
            let authored = try precisionInk(a, document: document)
            let color: PrecisionInk
            if try precisionInkSpace(a) == .document { color = authored }
            else {
                let srgb = NSColor(srgbRed: authored.rgba[0], green: authored.rgba[1],
                    blue: authored.rgba[2], alpha: authored.rgba[3])
                color = try PrecisionInk(color: srgb,
                    space: ColorProfile.coverageSpace(document.nsColorSpace, for: srgb))
            }
            let gray = try scalarInk(color)
            let mode = stringArg(a, "fill_mode")
            guard mode == nil || mode == "color_match" || mode == "selection" else { throw ToolError(message: "fill_mode must be color_match or selection.") }
            let fillOpacity = mode == nil ? 1 : doubleArg(a, "opacity") ?? 1
            guard fillOpacity.isFinite, (0...1).contains(fillOpacity) else { throw ToolError(message: "Native fill opacity must be in 0…1.") }
            if mode == "color_match", let selected = capture.selection?.maskSamples16(),
               x >= 0, y >= 0, x < capture.facade.width, y < capture.facade.height,
               selected[y * capture.facade.width + x] == 0 {
                return try jsonResult(["ok": true, "changed": false, "target": resolved.target.agentName(in: capture.facade)])
            }
            let operation: [String: Any] = mode == "selection"
                ? ["kind": "fill", "value": gray, "opacity": color.rgba[3] * fillOpacity]
                : ["kind": "bucket_fill", "x": x, "y": y, "tolerance": Int((tolerance * 257).rounded()),
                   "contiguous": boolArg(a, "contiguous") ?? true, "value": gray, "opacity": color.rgba[3] * fillOpacity]
            edit = try PrecisionScalarOperations.edit(capture, target: resolved.target, layer: resolved.layer, operation: operation)
        } else if tool == "dodge_burn" {
            // R4m: Dodge / Burn on a mask or alpha channel at every
            // native depth — the stroke's coverage lerps each sample toward
            // `65535·shaped(s/65535)`; nothing is encoded.
            let exposure = doubleArg(a, "exposure") ?? 50, size = doubleArg(a, "size") ?? 24
            let ranges = ["shadows", "midtones", "highlights"]
            guard exposure.isFinite, size.isFinite,
                  let range = ranges.firstIndex(of: stringArg(a, "range") ?? "midtones") else {
                throw ToolError(message: "Invalid Dodge / Burn size, exposure or tonal range.")
            }
            let native = try capturedNativeStroke(a) ?? NativeBrushStrokeSession.generated(points: parsePoints(a),
                size: CGFloat(min(max(size, 1), EditorTool.dodge.maximumBrushSize)), tip: strokeTip(a))
            let coverage = try native.coverage(width: capture.facade.width, height: capture.facade.height, selection: nil)
            edit = try PrecisionScalarOperations.edit(capture, target: resolved.target, layer: resolved.layer,
                operation: ["kind": "dodge_burn", "exposure": min(max(exposure, 0), 100) / 100,
                    "range": range, "burn": boolArg(a, "burn") ?? false], input: coverage.samples)
        } else if tool == "clear_selection" {
            guard capture.selection != nil else { throw ToolError(message: "Make a selection before clearing it.") }
            edit = try PrecisionScalarOperations.edit(capture, target: resolved.target, layer: resolved.layer,
                operation: ["kind": "fill", "value": 0])
        } else {
            let points = try parsePoints(a)
            let size = doubleArg(a, "size") ?? 16
            let opacity = doubleArg(a, "opacity") ?? 1
            guard size.isFinite, opacity.isFinite else { throw ToolError(message: "Size and opacity must be finite.") }
            let native = try capturedNativeStroke(a) ?? NativeBrushStrokeSession.generated(points: points,
                size: CGFloat(min(max(size, 1), EditorTool.brush.maximumBrushSize)), tip: strokeTip(a))
            let coverage = try native.coverage(width: capture.facade.width, height: capture.facade.height, selection: nil)
            edit = try PrecisionScalarOperations.edit(capture, target: resolved.target, layer: resolved.layer,
                operation: ["kind": "paint", "value": erase ? 0 : 65535,
                    "opacity": min(max(opacity, 0), 1)], input: coverage.samples)
        }
        let changed = try capture.publish(edit.run(title: resolved.actionName), to: document, name: resolved.actionName)
        // The core plan the edit was admitted at (`scripts/cmyk-admission`'s plate cases).
        return try jsonResult(["ok": true, "changed": changed, "layer": resolved.layer,
            "target": resolved.target.agentName(in: capture.facade), "storage": "u16",
            "working_memory_plan_bytes": edit.scratchEstimate,
            "coverage": a["native_stroke"] == nil ? "Native version 1 dab coverage generated from points; the scalar target stays u16." : "Native version 1 dab coverage with native selection."])
    }

    func precisionScalarFilter(_ document: ImageDocument, filter: String, params: [String: Any],
                               layer: Int, target: PaintTarget) throws -> String {
        let capture = try PrecisionEditorCapture(document)
        if !capture.quickMask, let reason = scopeRefusal(.filter, target: target,
            doc: capture.facade, layer: layer) { throw ToolError(message: reason) }
        let edit = try PrecisionScalarOperations.filter(capture, target: target, layer: layer,
            filter: filter, params: SelectedAdjustment.numericParameters(params))
        let changed = try capture.publish(edit.run(title: "Apply \(filter)"), to: document, name: "Apply \(filter)")
        // A float layer's Red/Green/Blue/Alpha plane is written as f32; a mask
        // or channel keeps its u16 storage at every depth.
        let storage = capture.facade.isFloat && target.isPlane ? "rgba32_float" : "u16"
        return try jsonResult(["ok": true, "changed": changed, "layer": layer,
            "target": target.agentName(in: capture.facade), "filter": filter, "storage": storage,
            "working_memory_plan_bytes": edit.scratchEstimate])
    }

    private func scalarInk(_ ink: PrecisionInk) throws -> UInt16 {
        try PrecisionScalarOperations.ink(ink)
    }
}

extension EditorViewController {
    func beginPrecisionScalarStroke(_ capture: PrecisionEditorCapture) throws -> Bool {
        // A one-plane target has no saturation for the Sponge to change.
        if currentTool == .sponge, !capture.quickMask, !capture.target.isPlane {
            throw DoctoredCoreError(message: FloatRetouch.spongeCoverageRefusal)
        }
        guard let document, !capture.quickMask, [.brush, .eraser, .dodge].contains(currentTool) else {
            throw DoctoredCoreError(message: "Native scalar strokes support Brush, Eraser, Dodge and Burn outside Quick Mask.")
        }
        // R4m covers a mask or an alpha channel only, as `precisionDodgeBurn` refuses; one CMYK
        // ink plate follows the colour plane's refusal (#1).
        if currentTool == .dodge, capture.target.isPlane || capture.target.isInk {
            throw DoctoredCoreError(message: "Dodge / Burn's target is the layer, its mask or an alpha channel.")
        }
        if let reason = OperationScope.refusalReason(.filter, target: capture.target,
            doc: capture.facade, layerIndex: capture.layer) { throw DoctoredCoreError(message: reason) }
        if capture.target.isPlane, !document.confirmRasterize(layer: capture.layer) { return false }
        try capture.requireCurrent(document)
        var stroke = PrecisionCanvasStroke(capture: capture, tool: currentTool,
            ink: try PrecisionInk(red: 1, green: 1, blue: 1), size: canvas.brushSize,
            opacity: Double(canvas.brushOpacity), retouchOptions: nil, sourcePlate: nil)
        // R4m: Dodge / Burn takes the bar's range and mode; `opacity`
        // carries its exposure, as on a layer.
        if currentTool == .dodge {
            stroke.dodgeRange = ToolOptionsStore.shared.dodge.rangeIndex
            stroke.burn = ToolOptionsStore.shared.dodge.burn
        }
        precisionStroke = stroke
        strokeTool = currentTool; strokeTarget = capture.target; strokeBlendMode = DR_BLEND_NORMAL
        canvas.precisionStrokePreview = true
        return true
    }

    func finishPrecisionScalarStroke(_ stroke: PrecisionCanvasStroke, name: String) throws {
        guard let document else { return }
        try stroke.capture.requireCurrent(document)
        let source = stroke.capture.facade
        guard let session = canvas.nativeStrokeSession else { throw DoctoredCoreError(message: "Native stroke capture is missing.") }
        let native = try session.resolved()
        let operation: [String: Any] = stroke.tool == .dodge
            ? ["kind": "dodge_burn", "exposure": stroke.opacity, "range": stroke.dodgeRange,
               "burn": stroke.burn]
            : ["kind": "paint", "value": stroke.tool == .eraser ? 0 : 65535, "opacity": stroke.opacity]
        let built = try PrecisionScalarOperations.command(stroke.capture, operation: operation)
        let work = try PrecisionStrokeWork(source: stroke.capture.source, stroke: native, command: built.command,
            selection: built.selection, width: source.width, height: source.height)
        var record = ActionRecorder.shared.gestureRecord(strokeRecord(name))
        let canonical = try native.canonical
        for index in record.indices {
            record[index].arguments["native_stroke"] = canonical
            record[index].arguments["size"] = Double(stroke.size)
            if stroke.tool == .dodge {
                // `dodge_burn`'s own keys, as the layer stroke records them.
                record[index].arguments["exposure"] = stroke.opacity * 100
                record[index].arguments["range"] = ["shadows", "midtones", "highlights"][stroke.dodgeRange]
                record[index].arguments["burn"] = stroke.burn
                record[index].arguments["target"] = stroke.capture.target.agentName(in: source)
            } else {
                record[index].arguments["opacity"] = stroke.opacity
            }
        }
        try startPrecisionStrokeJob(work, stroke: stroke, name: name, record: record,
            rasterizesTarget: stroke.capture.target.isPlane)
    }
}
