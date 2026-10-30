import AppKit

extension AgentServer {
    /// Mirrors the typed canvas brush/eraser/clone/heal paths. Retouch takes
    /// captured authoritative native-depth sampling plates and continuous source geometry.
    func precisionPaint(_ tool: String, _ a: [String: Any], document: ImageDocument) throws -> String {
        let requested = try paintTarget(a, document)
        // `rgba32` colours a float layer's pixels. A mask, a channel or
        // an adjustment layer (whose strokes paint its mask) is 0…1 coverage
        // that reads no colour, so the argument refuses here, ahead of every
        // route that would ignore it — for the Brush and, since the float
        // Paint Bucket, for `fill`.
        if tool == "brush_stroke" || tool == "fill", a["rgba32"] != nil {
            let adjustment = try document.doc?.layerIsAdjustment(structuralLayerIndex(a, document)) == true
            if requested.isCoverage || adjustment {
                throw ToolError(message: FloatPaint.coverageInkRefusal)
            }
        }
        // `fill`'s blend_mode / sample_all_layers, refused by name on a
        // mask, plane or channel target (+BucketFill).
        var fillStyle = tool == "fill" ? try bucketFillStyle(a, target: requested) : BucketFillStyle()
        // #1 Indexed phase 2c: Sample all layers is off on an Indexed document, as the Fill
        // tool's (`fillToolStyle`): its one layer's region is the composite's. A Bitmap document
        // (#1 Bitmap phase 2a-i) holds one layer too.
        if document.doc?.isIndexed == true || document.doc?.isBitmap == true {
            fillStyle.sampleAllLayers = false
        }
        // Only a layer stroke commits as a job; the scalar strokes still wait.
        if boolArg(a, "background") == true, Self.precisionStrokeJobTools.contains(tool) {
            let adjustment = try ["brush_stroke", "eraser_stroke"].contains(tool)
                && document.doc?.layerIsAdjustment(structuralLayerIndex(a, document)) == true
            // Quick Mask needs no clause here: the pre-dispatch gate refuses a layer
            // stroke in Quick Mask before this handler runs.
            if requested.isCoverage || adjustment {
                throw ToolError(message: Self.backgroundStrokeRefusal)
            }
        }
        if requested.isCoverage { return try precisionScalarPaint(tool, a, document: document, target: requested) }
        if ["brush_stroke", "eraser_stroke"].contains(tool),
           document.doc?.layerIsAdjustment(try structuralLayerIndex(a, document)) == true {
            return try precisionScalarPaint(tool, a, document: document, target: requested)
        }
        let capture = try PrecisionEditorCapture(document)
        let layer = try paintLayerIndex(a, document)
        // Locks, in the one place every typed LAYER-pixel gesture passes
        // through. The byte twin is `AgentServer.performPixelEdit`'s
        // `rejectLockedEdit`, which this path does not reach; the wording is
        // that one's (`LockFlags.refusal`), so the sentence a caller sees is
        // the same at both depths. The core refuses regardless
        // (`precision_lock::under_locks`) — this is purely about the MESSAGE.
        // The two branches above have already returned: a coverage target is
        // a mask or a channel, which the Pixels lock deliberately does not
        // block, and they carry their own gate in the core.
        try rejectLockedEdit(document, layer, DR_EDIT_PIXELS)
        let width = capture.facade.width, height = capture.facade.height
        // What a reply says of its storage, by depth and mode (#1).
        let cmyk = capture.source.isCmyk
        let replyStorage = cmyk ? "cmyka16_unorm" : capture.facade.isFloat ? "rgba32_float" : "rgba16_unorm"
        let command: PrecisionPixelCommand
        var coverage: PrecisionScalarBuffer?
        if tool == "clear_selection" {
            guard capture.selection != nil else { throw ToolError(message: "Make a selection before clearing it.") }
            coverage = try PrecisionStrokeCoverage.selection(width: width, height: height, selection: capture.selection)
            command = try .erase(layer: layer)
        } else if tool == "fill", cmyk {
            // #1: the Paint Bucket in ink on a CMYK layer (`cmyk_paint`),
            // mirroring EditorViewController.fillPrecisionColor's CMYK branch
            // (All Layers: `cmykBucketFilledSamplingComposite`).
            coverage = try PrecisionStrokeCoverage.selection(width: width, height: height, selection: capture.selection)
            if let reason = CmykPaint.modeRefusal(fillStyle.mode) { throw ToolError(message: reason) }
            let mode = stringArg(a, "fill_mode")
            guard mode == nil || mode == "selection" || mode == "color_match" else { throw ToolError(message: "fill_mode must be selection or color_match.") }
            let fillOpacity = mode == nil ? 1 : doubleArg(a, "opacity") ?? 1
            guard fillOpacity.isFinite, (0...1).contains(fillOpacity) else { throw ToolError(message: "Native fill opacity must be in 0…1.") }
            let ink = try cmykPaintInk(a, "ink", colorKey: "color", fallback: .black, precision: capture.source)
            if fillStyle.sampleAllLayers, mode == "selection" {
                throw ToolError(message: "sample_all_layers grows a region from x, y; fill_mode selection fills the whole selection.")
            }
            if !fillStyle.sampleAllLayers, mode == "selection" || (mode == nil && boolArg(a, "contiguous") == false) {
                command = try CmykPaint.command(["op": "paint_blend", "layer": layer, "ink": ink,
                                                 "opacity": fillOpacity,
                                                 "blend_mode": Int(fillStyle.mode.rawValue)])
            } else {
                guard let x = intArg(a, "x"), let y = intArg(a, "y") else { throw ToolError(message: "Fill requires x and y.") }
                let tolerance = doubleArg(a, "tolerance") ?? 32
                guard tolerance.isFinite, (0...255).contains(tolerance) else { throw ToolError(message: "Tolerance must be in 0…255.") }
                if fillStyle.sampleAllLayers {
                    let result = try capture.source.cmykBucketFilledSamplingComposite(
                        layer: layer, x: x, y: y, tolerance: Int((tolerance * 257).rounded()),
                        contiguous: boolArg(a, "contiguous") ?? true, ink: ink, opacity: fillOpacity,
                        mode: fillStyle.mode, selection: capture.selection, width: width, height: height)
                    let changed = try capture.publish(result, to: document, name: "fill")
                    return try jsonResult(["ok": true, "changed": changed, "layer": layer,
                                           "storage": replyStorage, "sample_all_layers": true,
                                           "blend_mode": DrBlendMode.displayName(for: fillStyle.mode)])
                }
                command = try CmykPaint.command(["op": "bucket_fill", "layer": layer, "ink": ink,
                    "opacity": fillOpacity, "x": x, "y": y, "tolerance": Int((tolerance * 257).rounded()),
                    "contiguous": boolArg(a, "contiguous") ?? true,
                    "blend_mode": Int(fillStyle.mode.rawValue)])
            }
        } else if tool == "fill" {
            coverage = try PrecisionStrokeCoverage.selection(width: width, height: height, selection: capture.selection)
            let mode = stringArg(a, "fill_mode")
            guard mode == nil || mode == "selection" || mode == "color_match" else { throw ToolError(message: "fill_mode must be selection or color_match.") }
            let fillOpacity = mode == nil ? 1 : doubleArg(a, "opacity") ?? 1
            guard fillOpacity.isFinite, (0...1).contains(fillOpacity) else { throw ToolError(message: "Native fill opacity must be in 0…1.") }
            // `rgba32` is a float layer's linear ink, read once and sent
            // as `color_space` `linear` on every layer branch below.
            let linear = try linearInkArgument(a, document)
            if linear != nil, a["rgba"] != nil {
                throw ToolError(message: "Pass rgba32 or rgba, not both.")
            }
            let fillColor = try linear ?? precisionInk(a, document: document).rgba
            let fillSpace = try linear == nil ? precisionInkSpace(a).rawValue : "linear"
            if mode == nil && boolArg(a, "contiguous") == false && fillStyle.isDefault {
                // Historical typed contiguous=false meant selection-wide fill;
                // linear ink takes the `paint_blend` fill_mode selection builds.
                command = try linear == nil
                    ? .fill(layer: layer, ink: precisionInk(a, document: document), space: precisionInkSpace(a))
                    : PrecisionPixelCommand(["op": "paint_blend", "layer": layer, "color": fillColor,
                        "color_space": fillSpace, "opacity": fillOpacity,
                        "blend_mode": fillStyle.mode.rawValue])
            } else if mode == "selection" {
                guard !fillStyle.sampleAllLayers else {
                    throw ToolError(message: "sample_all_layers grows a region from x, y; fill_mode selection fills the whole selection.")
                }
                command = try PrecisionPixelCommand(["op": "paint_blend", "layer": layer,
                    "color": fillColor, "color_space": fillSpace,
                    "opacity": fillOpacity, "blend_mode": fillStyle.mode.rawValue])
            } else {
                guard let x = intArg(a, "x"), let y = intArg(a, "y") else { throw ToolError(message: "Fill requires x and y.") }
                let tolerance = doubleArg(a, "tolerance") ?? 32
                guard tolerance.isFinite, (0...255).contains(tolerance) else { throw ToolError(message: "Tolerance must be in 0…255.") }
                if fillStyle.sampleAllLayers {
                    // Mirrors the 16-bit Fill tool with Sample all layers
                    // (EditorViewController.fillPrecisionColor): the composite's
                    // wand region as `paint_blend` coverage.
                    let result = try capture.source.bucketFilledSamplingComposite(
                        layer: layer, x: x, y: y, tolerance: Int((tolerance * 257).rounded()),
                        contiguous: boolArg(a, "contiguous") ?? true,
                        color: fillColor, colorSpace: fillSpace, opacity: fillOpacity,
                        mode: fillStyle.mode, selection: capture.selection, width: width, height: height)
                    let changed = try capture.publish(result, to: document, name: "fill")
                    return try jsonResult(["ok": true, "changed": changed, "layer": layer,
                                           "storage": replyStorage, "sample_all_layers": true,
                                           "blend_mode": DrBlendMode.displayName(for: fillStyle.mode)])
                }
                command = try PrecisionPixelCommand(["op": "bucket_fill", "layer": layer,
                    "color": fillColor, "color_space": fillSpace, "opacity": fillOpacity,
                    "x": x, "y": y, "tolerance": Int((tolerance * 257).rounded()),
                    "contiguous": boolArg(a, "contiguous") ?? true,
                    "blend_mode": fillStyle.mode.rawValue])
            }
        } else {
            let points = try parsePoints(a)
            let spot = tool == "spot_heal_stroke"
            let heal = tool == "heal_stroke" || spot
            let clone = tool == "clone_stamp"
            let rawSize = doubleArg(a, "size") ?? (heal || clone ? 24 : 16)
            let rawOpacity = doubleArg(a, "opacity") ?? (heal ? 100 : 1)
            guard rawSize.isFinite, rawOpacity.isFinite else { throw ToolError(message: "Size and opacity must be finite.") }
            let mirrored = Self.strokeEditorTool(tool)
            let size = CGFloat(min(max(rawSize, mirrored.minimumBrushSize), mirrored.maximumBrushSize))
            let opacity = heal ? min(max(rawOpacity, 1), 100) / 100 : min(max(rawOpacity, 0), 1)
            var tip = try strokeTip(a)
            if heal {
                guard a["flow"] == nil, a["blend_mode"] == nil else {
                    throw ToolError(message: "Healing takes strength through opacity (1–100), with no flow or blend mode.")
                }
                tip.flow = 1
            }
            if tool == "eraser_stroke", a["blend_mode"] != nil {
                throw ToolError(message: "Eraser has no blend mode.")
            }
            if heal || clone, a["native_stroke"] == nil { _ = try retouchUsesDabs(a, tip: tip, size: size) }
            let native = try capturedNativeStroke(a) ?? NativeBrushStrokeSession.generated(points: points, size: size, tip: tip)
            let selected = capture.selection != nil
            // What every stroke job's reply says of its storage, at 16 bits
            // and at 32-bit float.
            let float = capture.facade.isFloat
            let storage = replyStorage
            let note = (a["native_stroke"] == nil
                ? "Native version 1 dab coverage generated from points; "
                : "Native version 1 dab coverage; ")
                + (capture.source.isCmyk ? "source ink stays CMYKA16."
                    : float ? "source RGB stays linear f32." : "source RGB stays u16.")
            var rasterizes = true
            // The Spot Healing Brush's Type, echoed in the reply.
            var spotType = SpotHealType.contentAware
            if heal || clone {
                let choice = try retouchSampling(a, default: .current)
                let output = try retouchOutput(a)
                rasterizes = output == .layer
                let expand = try retouchExpansion(a)
                if spot {
                    let ring = intArg(a, "ring") ?? 0, seed = intArg(a, "seed") ?? 0
                    guard (0...512).contains(ring), seed >= 0 else {
                        throw ToolError(message: "ring must be 0…512 and seed must be nonnegative.")
                    }
                    spotType = try SpotHealType.argument(a)
                    let bounds = native.healBounds(width: width, height: height)
                    command = try .spotHeal(layer: layer, choice: choice, output: output, expand: expand,
                        opacity: opacity, ring: ring, seed: UInt64(seed), type: spotType,
                        selection: selected, bounds: bounds)
                } else {
                    guard let sx = doubleArg(a, "source_x"), let sy = doubleArg(a, "source_y"), sx.isFinite, sy.isFinite else {
                        throw ToolError(message: "Retouch requires finite source_x and source_y.")
                    }
                    let geometry = try retouchGeometry(a, source: CGPoint(x: sx, y: sy), destination: points[0])
                    // The clone's `blend_mode` at 16 bits, with the 8-bit
                    // handler's refusal for a new layer (AgentServer+Retouch.swift).
                    let blend = clone ? try blendModeArg(a) ?? DR_BLEND_NORMAL : DR_BLEND_NORMAL
                    if output == .newLayer, blend != DR_BLEND_NORMAL {
                        throw ToolError(
                            message: "output: \"new_layer\" starts from a transparent layer, so there is "
                                + "nothing for blend_mode "
                                + "\(DrBlendMode.displayName(for: blend)) to blend with. Clone into the "
                                + "layer itself to use a blend mode, or drop blend_mode.")
                    }
                    command = try .retouch(layer: layer, sourceLayer: intArg(a, "source_layer"), heal: heal,
                        geometry: geometry, choice: choice, output: output, expand: expand, opacity: opacity,
                        blendMode: blend, selection: selected,
                        bounds: heal ? native.healBounds(width: width, height: height) : nil)
                }
                if heal {
                    // Mirrors the 16-bit Healing Brush / Spot Healing canvas
                    // strokes (EditorViewController.finishPrecisionStroke).
                    let work = try PrecisionStrokeWork(source: capture.source, stroke: native, command: command,
                        selection: selected ? capture.selection?.maskSamples16() : nil, width: width, height: height)
                    let job = try DocumentJobs.startPrecisionStroke(work, document: document, capture: capture,
                        layer: layer, title: spot ? "Spot Healing" : "Healing Brush", record: .notACommand,
                        showProgress: false, rasterizesTarget: output == .layer)
                    job.details = ["storage": storage, "coverage": note]
                    if spot { job.details["applied"] = ["type": spotType.rawValue] }
                    return try finishJobCall(job, tool: tool, arguments: a, unchanged: {
                        try spot ? self.spotHealNoOpResult(layer: layer) : self.healStrokeNoOpResult(layer: layer)
                    })
                }
            } else if tool == "eraser_stroke" {
                command = try PrecisionPixelCommand(["op": "erase_native", "layer": layer, "opacity": opacity, "selection": selected])
            } else if cmyk {
                // #1: the Brush in ink on a CMYK layer, in the CMYK modes
                // (EditorViewController.finishPrecisionStroke's CMYK branch).
                let blend = try blendModeArg(a) ?? DR_BLEND_NORMAL
                if let reason = CmykPaint.modeRefusal(blend) { throw ToolError(message: reason) }
                command = try CmykPaint.command(["op": "paint_blend", "layer": layer,
                    "ink": cmykPaintInk(a, "ink", colorKey: "color", fallback: .black, precision: capture.source),
                    "opacity": opacity, "blend_mode": Int(blend.rawValue), "selection": selected])
            } else {
                let blend = try blendModeArg(a) ?? DR_BLEND_NORMAL
                // A float layer's `rgba32` is linear ink, used as it is.
                let linear = try linearInkArgument(a, document)
                if linear != nil, a["rgba"] != nil {
                    throw ToolError(message: "Pass rgba32 or rgba, not both.")
                }
                command = try PrecisionPixelCommand(["op": "paint_blend", "layer": layer,
                    "color": linear ?? precisionInk(a, document: document).rgba,
                    "color_space": linear == nil ? precisionInkSpace(a).rawValue : "linear",
                    "opacity": opacity, "blend_mode": blend.rawValue, "selection": selected])
            }
            // Mirrors the 16-bit Brush, Eraser and Clone Stamp canvas strokes
            // (EditorViewController.finishPrecisionStroke): one job whose
            // worker rasterizes the footprint.
            let work = try PrecisionStrokeWork(source: capture.source, stroke: native, command: command,
                selection: selected ? capture.selection?.maskSamples16() : nil, width: width, height: height)
            let job = try DocumentJobs.startPrecisionStroke(work, document: document, capture: capture,
                layer: layer, title: tool.replacingOccurrences(of: "_", with: " "), record: .notACommand,
                showProgress: false, rasterizesTarget: rasterizes, completion: Self.noteStrokeJobChange)
            job.details = ["storage": storage, "coverage": note]
            // #1 Indexed phase 2b: the Brush's admitted plan (the native paint's and the
            // re-index's), which `scripts/indexed-admission/measure.py`'s `brush-indexed` gates on;
            // on a Bitmap document (#1 Bitmap phase 2a-i) the native paint's and the republish's,
            // `scripts/bitmap-admission/measure.py`'s `brush-bitmap`.
            if document.doc?.isIndexed == true || document.doc?.isBitmap == true {
                job.details["working_memory_plan_bytes"] = work.scratchEstimate
            }
            return try finishJobCall(job, tool: tool, arguments: a, unchanged: {
                try self.jsonResult(["ok": true, "changed": false, "layer": layer,
                                     "storage": storage, "coverage": note])
            })
        }
        let work = try PrecisionPixelEdit(capture.source, command: command, input: coverage)
        let changed = try capture.publish(work.run(), to: document, name: tool.replacingOccurrences(of: "_", with: " "))
        let kept = capture.facade.isFloat ? "source RGB stays linear f32." : "source RGB stays u16."
        return try jsonResult(["ok": true, "changed": changed, "layer": layer, "storage": replyStorage,
                               "coverage": (a["native_stroke"] == nil ? "Native version 1 dab coverage generated from points; " : "Native version 1 dab coverage; ") + kept])
    }

    static let backgroundStrokeRefusal = "background applies only to 16-bit or 32-bit float layer strokes; a mask, channel, "
        + "Quick Mask or adjustment-layer stroke always waits. Call it again without background."

    /// A 16-bit stroke job's `changed` reply key, set once it ends.
    static func noteStrokeJobChange(_ job: DocumentJob) {
        job.details["changed"] = job.state == "completed"
    }

    /// The continuous ink a typed paint/fill takes. `display_p3` (Phase 12A)
    /// is converted HERE, once, into the document's own space, so the command
    /// carries document components and `precisionInkSpace` answers `.document`
    /// for it — the wide-gamut spelling a caller holding P3 numbers wants,
    /// with no second conversion anywhere. A `document` hex is the
    /// document's own bytes (`documentHexInk`), exact ×257.
    func precisionInk(_ a: [String: Any], document: ImageDocument) throws -> PrecisionInk {
        let space = try colorSpaceArgument(a)
        let p3 = space == .displayP3
        if let values = a["rgba"] as? [NSNumber], values.count == 4 {
            let ink = try PrecisionInk(red: values[0].doubleValue, green: values[1].doubleValue,
                                       blue: values[2].doubleValue, alpha: values[3].doubleValue)
            guard p3 else { return ink }
            return try PrecisionInk(color: NSColor(colorSpace: .displayP3,
                components: ink.rgba.map { CGFloat($0) }, count: 4), space: document.nsColorSpace)
        }
        if a["rgba"] != nil { throw ToolError(message: "rgba requires four normalized sRGB components.") }
        if space == .document { return try documentHexInk(a, "color", fallback: .black) }
        let authored = try parseColor(a, "color", fallback: .black)
        guard p3 else { return try PrecisionInk(color: authored) }
        guard let bytes = ColorProfile.bytes(authored, in: .sRGB) else {
            throw ToolError(message: "Could not convert the color")
        }
        return try PrecisionInk(color: NSColor(colorSpace: .displayP3,
            components: bytes.map { CGFloat($0) / 255 }, count: 4), space: document.nsColorSpace)
    }

    func precisionInkSpace(_ a: [String: Any]) throws -> PrecisionColorSpace {
        // `display_p3` ink is already converted into the document's space by
        // `precisionInk`, and a `document` hex already IS document components,
        // so both are interpreted as document components.
        switch try colorSpaceArgument(a) {
        case .srgb: return .srgb
        case .document, .displayP3: return .document
        }
    }
}
