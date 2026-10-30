import AppKit

/// The retouch strokes and the straightening crop: the agent mirrors of the
/// Clone Stamp tool, the Dodge / Burn tool, and the Crop tool (straighten
/// included). The stroke handlers rasterize the same canvas-sized
/// premultiplied overlay the interactive stroke pipeline builds and hand it
/// to the same core ops, so identical inputs give identical pixels.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let retouchHandlers: [String: ToolHandler] = [
        "clone_stamp": { $0.cloneStamp },
        "dodge_burn": { $0.dodgeBurn },
        "crop": { $0.crop },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let retouchToolFacts: [String: ToolFacts] = [
        // A float layer's Clone Stamp copies linear light (`FloatRetouch`).
        "clone_stamp": [.typedTool, .rasterTool, .floatTool, .cmykTool],
        // R4 on a float layer, R4m on a mask or channel (`FloatPaint`); #547: on ink
        // (`CmykRetouch`), the Clone Stamp likewise.
        "dodge_burn": [.typedTool, .rasterTool, .floatTool, .cmykTool],
        // Phase 12F-1b: the EXACT canvas geometry. A zero-angle `crop` keeps
        // every raster and mask allocation; a quarter turn and a flip are a
        // Nearest PERMUTATION, so an extended-range component is only ever
        // copied. The FILTERED geometry is admitted too — a `crop`
        // with an angle (straighten), an arbitrary `rotate`, `image_size`,
        // the layer transforms and `transform_mask` — through the core's
        // premultiplied extended-range resampler, which clamps each RGB
        // component to the layer's own visible range (D3). `transform_again`
        // repeats a layer transform; with a selection it would transform the
        // selected PIXELS, which float does not admit, so
        // `DocumentPrecision.refusal` refuses that case by name.
        "crop": [.selectionSensitive, .typedTool, .floatTool, .cmykTool],
    ]

    // MARK: - Shared stroke rasterization

    /// Builds a canvas-sized premultiplied RGBA8 overlay (row 0 = top,
    /// top-left-origin drawing — paintOverlay's exact context), clips it to
    /// the live selection the way every interactive stroke does, lets `draw`
    /// fill it, and hands the bytes to `commit` inside performPixelEdit —
    /// the pixel-rewrite chokepoint that silently drops a described layer's
    /// meta and reports it. The Clone Stamp and Dodge / Burn commit through
    /// it; the healing and Patch handlers run as `DocumentJobs` jobs over
    /// `retouchPixels` instead. Internal: the retouch brushes'
    /// stroke tools (`AgentServer+RetouchBrushes.swift`) commit through it too.
    func retouchOverlay(
        _ document: ImageDocument, layer: Int, actionName: String, rasterizesTarget: Bool = true,
        draw: (CGContext) -> Void,
        commit: (RasterDocument, UnsafePointer<UInt8>, Int, Int) -> RasterDocument?
    ) throws -> DroppedDescription? {
        let data = try retouchPixels(document, draw: draw)
        return try performPixelEdit(document, actionName, pixelLayer: layer, rasterizesTarget: rasterizesTarget) { current in
            data.withUnsafeBufferPointer { buffer in
                guard let base = buffer.baseAddress else { return nil }
                return commit(current, base, current.width, current.height)
            }
        }
    }

    /// Shared main-thread stroke rasterization, also used to capture worker input.
    func retouchPixels(_ document: ImageDocument, draw: (CGContext) -> Void) throws -> [UInt8] {
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let width = doc.width
        let height = doc.height
        var data = [UInt8](repeating: 0, count: width * height * 4)
        try data.withUnsafeMutableBufferPointer { buffer in
            guard let base = buffer.baseAddress,
                  let context = CGContext(data: base, width: width, height: height, bitsPerComponent: 8,
                    bytesPerRow: width * 4, space: doc.drawingSpace,
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
                throw ToolError(message: "Could not rasterize the stroke.")
            }
            context.translateBy(x: 0, y: CGFloat(height))
            context.scaleBy(x: 1, y: -1)
            editor(document)?.agentSelection?.clip(context)
            draw(context)
        }
        return data
    }

    /// The stroke tools' optional `hardness` argument, 0–100, returned as
    /// the 0–1 fraction SoftBrush speaks; default 1 (the classic hard
    /// round). Internal: paint_stroke shares it.
    func strokeHardness(_ a: [String: Any]) throws -> CGFloat {
        let raw = doubleArg(a, "hardness") ?? 100
        guard raw.isFinite, raw >= 0, raw <= 100 else {
            throw ToolError(message: "hardness must be between 0 and 100 (percent)")
        }
        return CGFloat(raw / 100)
    }

    /// The stroke tools' shared tip arguments — hardness plus `flow`
    /// (1–100%, per-dab deposit), `spacing` (1–200% of the diameter),
    /// `angle` (degrees, counter-clockwise) and `roundness` (1–100%) —
    /// resolved to the fractions SoftBrush speaks. The defaults are the
    /// classic tip, whose strokes render exactly as they did before the
    /// tip options existed. Internal: all four stroke tools share it.
    func strokeTip(_ a: [String: Any]) throws -> BrushTip {
        let flow = doubleArg(a, "flow") ?? 100
        guard flow.isFinite, flow >= 1, flow <= 100 else {
            throw ToolError(message: "flow must be between 1 and 100 (percent)")
        }
        let spacing = doubleArg(a, "spacing") ?? Double(BrushTip.defaultSpacingPercent)
        guard spacing.isFinite, spacing >= 1, spacing <= 200 else {
            throw ToolError(
                message: "spacing must be between 1 and 200 (percent of the brush diameter)")
        }
        let angle = doubleArg(a, "angle") ?? 0
        guard angle.isFinite, angle >= -180, angle <= 180 else {
            throw ToolError(message: "angle must be between -180 and 180 (degrees)")
        }
        let roundness = doubleArg(a, "roundness") ?? 100
        guard roundness.isFinite, roundness >= 1, roundness <= 100 else {
            throw ToolError(message: "roundness must be between 1 and 100 (percent)")
        }
        return BrushTip(
            hardness: try strokeHardness(a), flow: CGFloat(flow / 100),
            spacingPercent: CGFloat(spacing), angleDegrees: CGFloat(angle),
            roundness: CGFloat(roundness / 100))
    }

    /// The optional `blend_mode` argument: a blend-mode display name,
    /// matched case-insensitively, nil when absent. Internal: brush_stroke,
    /// clone_stamp, apply_image, calculations and set_layer_properties all
    /// share it.
    ///
    /// `allowPassThrough` picks the vocabulary, and it is off by default
    /// because Pass Through is a GROUP's declaration, not a mode: it says
    /// "do not composite this level as a unit". Painting with it would be
    /// meaningless — the core maps it to Normal for any leaf that somehow
    /// carries it — so a brush or a clone stamp naming it must get a refusal
    /// that lists the vocabulary it really speaks, exactly as before this
    /// phase. Only `set_layer_properties` passes true, since only it can
    /// address a group entry.
    ///
    /// Naming Pass Through for a RASTER entry is not caught here: the check
    /// belongs to the core's `with_layer_blend_mode`, which refuses it, and
    /// the sentence below tells a model which half of the pair was wrong
    /// before it retries.
    func blendModeArg(_ a: [String: Any], allowPassThrough: Bool = false) throws -> DrBlendMode? {
        guard let name = stringArg(a, "blend_mode") else { return nil }
        let vocabulary = allowPassThrough ? DrBlendMode.groupBlendModes : DrBlendMode.allBlendModes
        guard let mode = vocabulary.first(where: {
            $0.1.caseInsensitiveCompare(name) == .orderedSame
        })?.0
        else {
            let names = vocabulary.map { $0.1 }.joined(separator: ", ")
            var message = "Unknown blend mode \"\(name)\". One of: \(names)"
            // The one near-miss worth naming: a model that has read
            // get_document's "Pass Through" on a group row and reached for it
            // on a paint tool. Without this it reads the long list and
            // guesses again.
            if !allowPassThrough,
               "Pass Through".caseInsensitiveCompare(name) == .orderedSame {
                message += ". Pass Through applies to GROUPS only — set it with "
                    + "set_layer_properties on a group entry."
            }
            throw ToolError(message: message)
        }
        return mode
    }

    /// The stroke's coverage: the round-capped, round-joined outline of the
    /// polyline through `points`, `size` px wide — the same geometry a
    /// brush stroke paints. A single point gets an epsilon segment so it
    /// strokes as a round dot instead of an empty path.
    /// Internal: the healing handlers stroke the same coverage rather than
    /// writing a second outline builder.
    static func strokeCoverage(points: [CGPoint], size: CGFloat) -> CGPath {
        let path = CGMutablePath()
        path.move(to: points[0])
        if points.count == 1 {
            path.addLine(to: CGPoint(x: points[0].x + 0.01, y: points[0].y))
        } else {
            for point in points.dropFirst() {
                path.addLine(to: point)
            }
        }
        return path.copy(
            strokingWithWidth: size, lineCap: .round, lineJoin: .round, miterLimit: 10)
    }

    // MARK: - Clone stamp

    /// clone_stamp — mirrors the Clone Stamp tool: a snapshot of the CURRENT
    /// flattened composite, displaced by (first point − source), is painted
    /// onto the layer through the stroke's round coverage, so the pixel
    /// landing at p is the composite's p − offset — the classic aligned-clone
    /// rule the canvas's stampCloneDab stamps dab by dab.
    func cloneStamp(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard a["source_layer"] == nil else {
            throw ToolError(message: "source_layer selects a separate raster only in native 16-bit or 32-bit float retouch. Choose sample for an 8-bit source.")
        }
        let index = try paintLayerIndex(a, document)
        // The UI refuses clone strokes on an adjustment layer outright
        // (onStrokeBegin): there are no pixels to rewrite.
        try rejectAdjustmentPixelEdit(document, index)
        guard let sourceX = doubleArg(a, "source_x"), let sourceY = doubleArg(a, "source_y"),
            sourceX.isFinite, sourceY.isFinite
        else {
            throw ToolError(
                message: "clone_stamp requires source_x and source_y — the canvas point the "
                    + "pixels are cloned FROM (the stroke's first point paints what sits "
                    + "there).")
        }
        let points = try byteStrokePoints(a)
        let rawSize = doubleArg(a, "size") ?? 24
        let rawOpacity = doubleArg(a, "opacity") ?? 1
        guard rawSize.isFinite, rawOpacity.isFinite else {
            throw ToolError(message: "size and opacity must be finite numbers")
        }
        let size = CGFloat(min(max(rawSize, 1), EditorTool.clone.maximumBrushSize))
        let opacity = min(max(rawOpacity, 0), 1)
        let tip = try strokeTip(a)
        let blend = try blendModeArg(a)
        let usesDabs = try retouchUsesDabs(a, tip: tip, size: size)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        // The plate is latched before the edit, like the interactive stroke's
        // mouse-down latch: nothing can change what is being cloned
        // mid-stroke. It is the core's answer from the document's own layers,
        // never a projection this side happens to be holding — the phase's
        // rule that no display image may become a sampling source
        // (`RetouchPlate`).
        //
        // The default is `all`, which is exactly what this tool always did:
        // it sampled the flattened composite unconditionally and published no
        // sampling argument at all. A recorded call that names none still
        // clones the composite.
        let choice = try retouchSampling(a, default: .all)
        let output = try retouchOutput(a)
        // A blend mode describes how the cloned paint meets the pixels it
        // lands on, and on a brand-new transparent layer there are none —
        // every mode would composite as Normal. Refuse rather than accept an
        // argument that cannot mean what it says.
        if output == .newLayer, let blend, blend != DR_BLEND_NORMAL {
            throw ToolError(
                message: "output: \"new_layer\" starts from a transparent layer, so there is "
                    + "nothing for blend_mode "
                    + "\(DrBlendMode.displayName(for: blend)) to blend with. Clone into the "
                    + "layer itself to use a blend mode, or drop blend_mode.")
        }
        let plate = try retouchPlate(document, layer: index, choice: choice, output: output)
        // Tagged with the OVERLAY's space so the displaced draw converts
        // nothing, which is what makes a zero-offset clone an identity copy
        // on every document, LUT profiles included.
        let geometry = try retouchGeometry(a, source: CGPoint(x: sourceX, y: sourceY), destination: points[0])
        let transformed = try plate.image.retouchTransformed(matrix: geometry.values)
        guard let snapshot = transformed.makeCGImage(in: doc.drawingSpace) else {
            throw ToolError(message: "Could not read the picture to clone from.")
        }
        let expand = try retouchExpansion(a)

        if let canonical = try capturedNativeStroke(a) {
            let pixels = try byteCanonicalRetouchPixels(document, stroke: canonical,
                size: size, ink: .source(snapshot))
            var opRefused = false
            let rasterized: DroppedDescription?
            do {
                rasterized = try byteCanonicalRetouchCommit(document, layer: index,
                    actionName: "Clone Stamp", rasterizesTarget: output == .layer,
                    pixels: pixels) { current, base, w, h in
                    guard let destination = expand && output == .layer
                        ? try? current.expandingRetouchDestination(index) : current
                    else { return nil }
                    let result: RasterDocument?
                    if output == .newLayer {
                        result = try? destination.cloneToNewLayer(index, overlay: base,
                            w: w, h: h, alpha: opacity, name: Self.retouchLayerName)
                    } else if let blend, blend != DR_BLEND_NORMAL {
                        result = destination.paintingLayerBlend(index, overlay: base,
                            w: w, h: h, mode: blend, alpha: opacity)
                    } else {
                        result = destination.paintingLayer(index, overlay: base,
                            w: w, h: h, mode: DR_COMPOSITE_OVER, alpha: opacity)
                    }
                    if result == nil { opRefused = true }
                    return result
                }
            } catch let error as ToolError {
                guard opRefused else { throw error }
                return try jsonResult(["ok": true, "changed": false, "layer": index,
                    "note": "Clone Stamp changed no pixel; no undo step was added."])
            }
            var applied: [String: Any] = ["output": output.rawValue]
            applied.merge(choice.report) { _, new in new }
            return try pixelEditResult(["ok": true, "action": "Clone Stamp",
                "layer": index, "dabs": canonical.dabs.count / 4,
                "applied": applied], layer: index, rasterized: rasterized)
        }

        let rasterized: DroppedDescription?
        // Latched when the PAINT OP ITSELF answers nil, so only that case
        // gets the friendlier message — any other failure stays an error.
        var opRefused = false
        let centers = try usesDabs ? SoftBrush.stampCenters(along: points, spacing: SoftBrush.spacing(
            for: size, percent: tip.spacingPercent)) : []
        if usesDabs, SoftBrush.isSoft(hardness: tip.hardness, size: size) { try SoftBrush.admitDabImage(diameter: size) }
        do {
            rasterized = try retouchOverlay(
                document, layer: index, actionName: "Clone Stamp", rasterizesTarget: output == .layer,
                draw: { context in
                    // The un-flip inside: the overlay context is flipped
                    // (row 0 = top), so the snapshot flips back locally to
                    // land right side up at the displaced position —
                    // stampCloneDab's rule.
                    let drawSnapshot: (CGContext) -> Void = { context in
                        context.translateBy(x: 0, y: CGFloat(snapshot.height))
                        context.scaleBy(x: 1, y: -1)
                        context.draw(
                            snapshot,
                            in: CGRect(
                                x: 0, y: 0,
                                width: CGFloat(snapshot.width),
                                height: CGFloat(snapshot.height)))
                    }
                    // A non-default tip stamps the snapshot dab by dab —
                    // the canvas's stampCloneDab: the tip's footprint (gray
                    // falloff mask when soft, hard ellipse otherwise,
                    // squashed and rotated) clips, and each dab deposits at
                    // the tip's flow.
                    if usesDabs {
                        let mask = SoftBrush.isSoft(hardness: tip.hardness, size: size)
                            ? SoftBrush.dabMask(diameter: size, hardness: tip.hardness)
                            : nil
                        for center in centers {
                            context.saveGState()
                            SoftBrush.clipDab(
                                in: context, at: center, diameter: size,
                                tip: tip, mask: mask)
                            if tip.flow < 0.995 { context.setAlpha(tip.flow) }
                            drawSnapshot(context)
                            context.restoreGState()
                        }
                        return
                    }
                    context.addPath(Self.strokeCoverage(points: points, size: size))
                    context.clip()
                    drawSnapshot(context)
                },
                commit: { current, base, w, h in
                    guard let current = expand && output == .layer
                        ? try? current.expandingRetouchDestination(index) : current else { return nil }
                    let out: RasterDocument?
                    if output == .newLayer {
                        // A new-layer clone needs no plate at the commit: the
                        // sampled pixels are already in the overlay, and an
                        // OVER composite onto emptiness is exactly the
                        // overlay. A blend mode has nothing to blend WITH
                        // there, which `retouchOutput`'s own refusal covers.
                        out = try? current.cloneToNewLayer(
                            index, overlay: base, w: w, h: h, alpha: opacity,
                            name: Self.retouchLayerName)
                    } else if let blend = blend, blend != DR_BLEND_NORMAL {
                        out = current.paintingLayerBlend(
                            index, overlay: base, w: w, h: h,
                            mode: blend, alpha: opacity)
                    } else {
                        out = current.paintingLayer(
                            index, overlay: base, w: w, h: h,
                            mode: DR_COMPOSITE_OVER, alpha: opacity)
                    }
                    if out == nil { opRefused = true }
                    return out
                })
        } catch let error as ToolError {
            // The paint op answers nil when the stroke never touches the
            // layer's extent; name that instead of the generic edit failure.
            guard opRefused else { throw error }
            let blendCause = blend == nil
                ? ""
                : ", or blend mode \(DrBlendMode.displayName(for: blend ?? DR_BLEND_NORMAL)) "
                    + "left every covered pixel exactly as it was (an identity blend, like "
                    + "multiplying white by white)"
            throw ToolError(
                message: "Clone Stamp changed nothing: the stroke never landed on layer "
                    + "\(index)'s pixels — its extent may not reach the points, the "
                    + "active selection clipped the whole stroke away\(blendCause).")
        }
        var applied: [String: Any] = ["output": output.rawValue]
        applied.merge(choice.report) { _, new in new }
        return try pixelEditResult(
            ["ok": true, "action": "Clone Stamp", "layer": index, "points": points.count,
             "applied": applied],
            layer: index, rasterized: rasterized)
    }

    // MARK: - Dodge / burn

    /// dodge_burn — mirrors the Dodge / Burn tool: lightens (dodge) or
    /// darkens (burn) the layer's pixels where the stroke's coverage lands,
    /// weighted toward shadows, midtones or highlights — the same retouch op
    /// the interactive stroke drives, which reads only the overlay's alpha.
    func dodgeBurn(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        // `target` mask / channel is the 16-bit and float scalar route
        // (`precisionDodgeBurn`); an 8-bit document refuses it, never ignores it.
        if let plane = stringArg(a, "target"), plane != "layer" {
            throw ToolError(message: "dodge_burn target \"\(plane)\" is a 16-bit or 32-bit float "
                + "document's mask or alpha channel; in an 8-bit document Dodge and Burn work on the layer.")
        }
        let index = try paintLayerIndex(a, document)
        // The UI refuses dodge/burn strokes on an adjustment layer outright
        // (onStrokeBegin): there are no pixels to rewrite.
        try rejectAdjustmentPixelEdit(document, index)
        let points = try byteStrokePoints(a)
        let rawSize = doubleArg(a, "size") ?? 24
        let rawExposure = doubleArg(a, "exposure") ?? 50
        guard rawSize.isFinite, rawExposure.isFinite else {
            throw ToolError(message: "size and exposure must be finite numbers")
        }
        let size = CGFloat(min(max(rawSize, 1), EditorTool.dodge.maximumBrushSize))
        let exposurePercent = min(max(rawExposure, 0), 100)
        let tip = try strokeTip(a)
        guard stringArg(a, "blend_mode") == nil else {
            throw ToolError(
                message: "dodge_burn has no blend_mode — it reshapes tones in place; "
                    + "blend modes apply to brush_stroke and clone_stamp.")
        }
        let burn = boolArg(a, "burn") ?? false
        // #547: Protect Tones, off unless asked (the tool's checkbox).
        let protectTones = boolArg(a, "protect_tones") ?? false
        let rangeName = stringArg(a, "range") ?? "midtones"
        let range: Int
        switch rangeName {
        case "shadows": range = 0
        case "midtones": range = 1
        case "highlights": range = 2
        case let other:
            throw ToolError(
                message: "range must be shadows, midtones, or highlights (got \"\(other)\")")
        }

        if let canonical = try capturedNativeStroke(a) {
            if a["burn"] != nil, NativeBrushStroke.jsonBoolean(a["burn"]) == nil {
                throw ToolError(message: "burn must be boolean.")
            }
            let pixels = try byteCanonicalRetouchPixels(document, stroke: canonical,
                size: size,
                ink: .color(ColorProfile.coverageInk(.white, in: document.drawingSpace)))
            var opRefused = false
            let rasterized: DroppedDescription?
            do {
                rasterized = try byteCanonicalRetouchCommit(document, layer: index,
                    actionName: "Dodge / Burn", pixels: pixels) { current, base, w, h in
                    let result = current.dodgeBurnLayer(index, overlay: base, w: w, h: h,
                        exposure: exposurePercent / 100, range: range, burn: burn,
                        protectTones: protectTones)
                    if result == nil { opRefused = true }
                    return result
                }
            } catch let error as ToolError {
                guard opRefused else { throw error }
                return try jsonResult(["ok": true, "changed": false, "layer": index,
                    "note": "Dodge / Burn changed no pixel; no undo step was added."])
            }
            return try pixelEditResult(["ok": true, "action": "Dodge / Burn",
                "layer": index, "dabs": canonical.dabs.count / 4,
                "applied": ["exposure": Self.transformNumber(exposurePercent),
                    "range": rangeName, "burn": burn, "protect_tones": protectTones]],
                layer: index, rasterized: rasterized)
        }

        let rasterized: DroppedDescription?
        // Latched when the RETOUCH OP ITSELF answers nil, so only that case
        // reads as a no-op — any other failure stays an error.
        var opRefused = false
        let stamped = SoftBrush.isStamped(tip: tip, size: size)
        let centers = try stamped ? SoftBrush.stampCenters(along: points, spacing: SoftBrush.spacing(
            for: size, percent: tip.spacingPercent)) : []
        if stamped { try SoftBrush.admitDabImage(diameter: size) }
        do {
            rasterized = try retouchOverlay(
                document, layer: index, actionName: "Dodge / Burn",
                draw: { context in
                    // Pure coverage, like the interactive stroke: white —
                    // the exposure lives in the op, not the stroke. A
                    // non-default tip stamps white falloff dabs through the
                    // tip's footprint, each deposited at the tip's flow, so
                    // the coverage itself feathers and builds up.
                    let white = ColorProfile.coverageInk(
                        NSColor(srgbRed: 1, green: 1, blue: 1, alpha: 1), in: document.drawingSpace)
                    if stamped,
                        let dab = SoftBrush.dab(
                            color: white.withAlphaComponent(tip.flow),
                            diameter: size, hardness: tip.hardness,
                            space: document.drawingSpace) {
                        for center in centers {
                            SoftBrush.stamp(
                                dab, in: context, at: center, diameter: size, tip: tip)
                        }
                        return
                    }
                    context.setFillColor(white.cgColor)
                    context.addPath(Self.strokeCoverage(points: points, size: size))
                    context.fillPath()
                },
                commit: { current, base, w, h in
                    let out = current.dodgeBurnLayer(
                        index, overlay: base, w: w, h: h,
                        exposure: exposurePercent / 100, range: range, burn: burn,
                        protectTones: protectTones)
                    if out == nil { opRefused = true }
                    return out
                })
        } catch let error as ToolError {
            // The op answers nil when nothing would change — the stroke
            // missed the layer, the selection clipped it away, or the
            // covered pixels are already at the end of their range. That is
            // a no-op, not a failure: no edit landed, no undo step opened.
            guard opRefused else { throw error }
            return try jsonResult([
                "ok": true, "changed": false, "layer": index,
                "note": "Nothing changed: the stroke never covered layer \(index)'s pixels, "
                    + "or the pixels under it are already fully "
                    + "\(burn ? "darkened" : "lightened"). No undo step was added.",
            ])
        }
        return try pixelEditResult(
            [
                "ok": true, "action": "Dodge / Burn", "layer": index,
                "points": points.count,
                "applied": [
                    "exposure": Self.transformNumber(exposurePercent),
                    "range": rangeName, "burn": burn, "protect_tones": protectTones,
                ],
            ], layer: index, rasterized: rasterized)
    }

    // MARK: - Crop (rect + straighten)

    /// crop — mirrors the Crop tool, straighten included: a nonzero `angle`
    /// first rotates EVERY layer, and every alpha channel, by −angle about
    /// the crop rect's center (the identical matrix
    /// EditorViewController+Crop.commitCropSession commits), then the canvas
    /// window moves to the rect — one undo step.
    /// Straightening resamples every layer's pixels, so described
    /// (text/shape) layers silently rasterize and the result
    /// reports which — the agent-side convention, since a modal prompt would
    /// block the MCP connection. A plain rect crop keeps every description
    /// valid.
    ///
    /// `sampler` is the straighten's resampling filter, in `transform_layer`'s
    /// vocabulary, because the Crop tool straightens with whatever the Free
    /// Transform options bar is set to. It is ignored at angle 0, where
    /// nothing is resampled.
    ///
    /// Phase 12F-1b2: a non-zero angle answers at 16 bits too, through the ONE
    /// host composition `RasterDocument.straightenedCrop`, whose typed branch
    /// is the core's single `straighten` op. Angle 0 keeps its own early route
    /// to `cropped` below at every depth and never enters that composition —
    /// the typed op would bypass `cropped`'s description capture/restore.
    ///
    /// `delete_cropped` mirrors the Crop tool's Delete Cropped Pixels:
    /// each plain raster is trimmed to the new canvas at every depth.
    ///
    /// Mirrors a Crop tool box dragged past the canvas: a rect reaching
    /// past any edge extends the canvas (`RasterDocument.extendedCrop` at
    /// angle 0, `straightenedCrop` turned) with the Background (Default) fill
    /// in `background_color` (default white, parsed before any edit): an
    /// opaque Background is padded with it under Delete Cropped Pixels, every
    /// other layer's new area is transparent. The reply adds `extended` and
    /// the job's `planned_bytes`.
    ///
    /// Mirrors the Crop tool's Fill popup: `fill` (`background`
    /// by default, `generative_expand`, `content_aware`) is parsed and refused
    /// before any edit, and a window past the canvas with a fill other than
    /// `background` runs `cropFill` (AgentServer+CropFill.swift). Every other
    /// route is unchanged.
    func crop(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard let x = intArg(a, "x"), let y = intArg(a, "y"),
            let w = intArg(a, "width"), let h = intArg(a, "height")
        else {
            throw ToolError(message: "crop requires x, y, width, height")
        }
        // `DocumentPrecision.cropAngle` is the ONE angle parser: 0 when the
        // caller named none, `.nan` when a value is present but unreadable —
        // the same NSNumber/String parse `doubleArg` performs. Both depth
        // gates read it, and since Phase 12F-1b2 so does this handler, so a
        // malformed `angle` is REFUSED by the guard below at 8 AND 16 bits
        // instead of being silently read as "no straighten".
        let angle = DocumentPrecision.cropAngle(a)
        guard angle.isFinite, abs(angle) <= 45 else {
            throw ToolError(message: "angle must be between -45 and 45 degrees")
        }
        let deleteCropped = boolArg(a, "delete_cropped") ?? false
        let samplerName = (stringArg(a, "sampler") ?? "bicubic").lowercased()
        guard let sampler = Self.transformSamplers[samplerName] else {
            throw ToolError(
                message: "sampler must be nearest, bilinear, bicubic or lanczos "
                    + "(got \"\(samplerName)\")")
        }
        // #547 Lab phase 4a: a Lab document takes the authored sRGB colour unquantized (the
        // Brush's `precisionInk` rule), so the pad converts by Mode ▸ Lab Color's rule in the
        // core at 16 bits too, not through the document's 8-bit codes.
        let backgroundColor = doc.isLab
            ? try parseColor(a, "background_color", fallback: .white)
            : try qualifiedColor(a, "background_color", in: document, fallback: .white)
        var fill = CropFill.background
        if a["fill"] != nil {
            guard let name = stringArg(a, "fill"), let parsed = CropFill(rawValue: name) else {
                throw ToolError(message: "fill must be background, generative_expand or content_aware")
            }
            fill = parsed
        }
        // Compared by subtraction so a huge integer cannot overflow `x + w`.
        let extended = w > 0 && h > 0
            && (x < 0 || y < 0 || x > doc.width - w || y > doc.height - h)
        if !extended, fill != .background { throw ToolError(message: CropFillRun.insideWindow) }
        var background: CropBackground?
        var planned: Int?
        if extended {
            if let refusal = doc.extendedCropRefusal(width: w, height: h) {
                throw ToolError(message: refusal)
            }
            let made = try CropBackground.make(backgroundColor, cmykInk: nil, in: doc)
            background = made
            let matrix = Self.cropMatrix(x: x, y: y, w: w, h: h, angle: angle)
            let command = try PrecisionPixelCommand.straighten(
                matrix: matrix,
                filter: angle == 0 ? .nearest : RasterDocument.precisionFilter(sampler.filter),
                x: x, y: y, width: w, height: h, deleteCropped: deleteCropped,
                background: made.fields)
            planned = Int(try PrecisionDocumentDescriptor.integer(
                doc.geometryEditPlan(request: command.request), "scratch_bytes"))
            if fill != .background {
                return try cropFill(a, document: document, request: CropFillRequest(
                    rect: CGRect(x: x, y: y, width: w, height: h), angle: angle,
                    sampler: sampler.filter,
                    deleteCropped: deleteCropped || fill == .contentAware, background: made,
                    described: angle == 0
                        ? [] : (0..<doc.layerCount).filter { document.layerDescribesSource($0) }),
                    fill: fill)
            }
        }
        // Phase 12F-1b3 (O1's argument-decidable half): a zero-angle crop to
        // the WHOLE canvas would change nothing, and that is the house
        // successful no-op rather than an error — `canvas_size`'s answer, and
        // the Crop tool's own silent return (`commitCropSession`). It is
        // decidable from the ARGUMENTS alone, which is what separates it from
        // the content-dependent no-ops (a symmetric flip, a `rotate 180` of a
        // symmetric picture) that stay errors and are owned by 12F-1b4.
        if angle == 0, x == 0, y == 0, w == doc.width, h == doc.height {
            return try structureNoOp(
                ["document": summary(document)],
                why: "the crop rectangle is the whole \(doc.width) × \(doc.height) canvas.")
        }
        if extended, angle == 0, let background {
            try performGroupedEdit(
                document, "Crop", changesCanvasGeometry: true,
                resultingSelection: document.selectionCarried(croppedToX: x, y: y, width: w, height: h)
            ) {
                try $0.extendedCrop(x: x, y: y, w: w, h: h, deleteCropped: deleteCropped,
                                    background: background)
            }
            return try jsonResult(["ok": true, "document": summary(document), "extended": true,
                                   "planned_bytes": planned ?? 0])
        }
        guard angle != 0 else {
            // The plain rectangle crop: a canvas-window move only.
            try performGroupedEdit(
                document, "Crop", changesCanvasGeometry: true,
                resultingSelection: document.selectionCarried(croppedToX: x, y: y, width: w, height: h)
            ) { try $0.cropped(x: x, y: y, w: w, h: h, deleteCropped: deleteCropped) }
            return try jsonResult(["ok": true, "document": summary(document)])
        }
        // The layers whose descriptions the rotation's resample invalidates:
        // exactly the UI's layerDescribesSource set. Adjustment layers keep
        // their meta — it describes an op, not pixels, so rotation cannot
        // contradict it.
        let described = (0..<doc.layerCount).filter { document.layerDescribesSource($0) }
        // The identical matrix commitCropSession builds: rotate by −angle
        // about the crop rect's center.
        let matrix = Self.cropMatrix(x: x, y: y, w: w, h: h, angle: angle)
        // ONE host composition, shared with the Crop tool's own commit:
        // `RasterDocument.straightenedCrop` runs the `straighten` request as
        // one job at either depth (CropTool.swift). A core
        // refusal arrives as a thrown sentence, which is reported verbatim the
        // way `AgentServer+Artwork.performArtworkEdit` reports one. The job
        // prepares the display too, published through `preparedEdit`.
        // nil without a throw means the WHOLE canvas is unchanged (the core
        // refuses a window outside the canvas by name), which is the house
        // `changed:false` no-op.
        var failure: Error?
        var unchanged = false
        defer { document.preparedEdit = nil }
        do {
            // The selection goes through the straighten's own map.
            let carried = document.selectionCarried(
                by: CropSession.canvasMap(
                    window: CGRect(x: x, y: y, width: w, height: h), angle: angle),
                width: w, height: h)
            try performGroupedEdit(document, "Crop", changesCanvasGeometry: true,
                                   resultingSelection: carried) { base in
                do {
                    guard let prepared = try base.straightenedCrop(
                        matrix, sampler: sampler.filter, x: x, y: y, w: w, h: h,
                        described: described, deleteCropped: deleteCropped,
                        background: background) else {
                        unchanged = true
                        return nil
                    }
                    document.preparedEdit = prepared
                    return prepared.document
                } catch {
                    failure = error
                    return nil
                }
            }
        } catch {
            if let failure { throw ToolError(message: failure.localizedDescription) }
            if unchanged {
                return try structureNoOp(
                    ["document": summary(document)],
                    why: CropSession.unchangedStraighten(
                        width: doc.width, height: doc.height, angle: angle))
            }
            throw error
        }
        var result: [String: Any] = [
            "ok": true,
            "angle": Self.transformNumber(angle),
            "sampler": sampler.name,
            "document": summary(document),
        ]
        if extended {
            result["extended"] = true
            result["planned_bytes"] = planned ?? 0
        }
        return try jsonResult(result)
    }

    /// The identical matrix `commitCropSession` builds: rotate by −angle
    /// about the crop rect's center; the identity at angle 0.
    static func cropMatrix(x: Int, y: Int, w: Int, h: Int, angle: Double) -> CGAffineTransform {
        guard angle != 0 else { return .identity }
        let center = CGPoint(x: Double(x) + Double(w) / 2, y: Double(y) + Double(h) / 2)
        return CGAffineTransform(translationX: center.x, y: center.y)
            .rotated(by: CGFloat(-angle) * .pi / 180)
            .translatedBy(x: -center.x, y: -center.y)
    }
}

// MARK: - Retouch sampling arguments

extension AgentServer {
    /// The `sample` / `sample_all_layers` / `ignore_adjustments` arguments,
    /// resolved once for every retouch tool.
    ///
    /// # Compatibility, which is the whole of this function
    ///
    /// `sample_all_layers` keeps its name, its units and its PER-TOOL
    /// default, because a step recorded before this release simply omits the
    /// new argument and must replay to the same pixels. The defaults differ
    /// by tool on purpose and are passed in by the caller: heal_stroke,
    /// clone_stamp and patch_region sampled the composite, spot_heal_stroke
    /// and content_aware_fill sampled the layer.
    ///
    /// The two spellings are NOT mutually exclusive. The app's own recorder
    /// writes both whenever the mode has a truthful boolean — that is what
    /// lets a step recorded today replay on a build that predates `sample`,
    /// which ignores the unknown argument and would otherwise fall back to
    /// its own default rather than to what the user did. So co-presence is
    /// accepted and only a CONTRADICTION is refused, with a sentence that
    /// names both values.
    ///
    /// `current_and_below` has no boolean spelling at all, so sending it
    /// alongside any `sample_all_layers` is a contradiction by definition.
    func retouchSampling(
        _ a: [String: Any], default fallback: RetouchSampling
    ) throws -> RetouchSamplingChoice {
        let legacy = boolArg(a, "sample_all_layers")
        var mode = legacy.map(RetouchSampling.legacy) ?? fallback
        if let raw = a["sample"] {
            guard let name = raw as? String, let explicit = RetouchSampling(rawValue: name) else {
                throw ToolError(
                    message: RetouchSamplingArguments.unknownMode(String(describing: raw)))
            }
            if let legacy, explicit.legacyAllLayers != legacy {
                throw ToolError(
                    message: RetouchSamplingArguments.contradiction(explicit, legacy))
            }
            mode = explicit
        }
        return RetouchSamplingChoice(
            mode: mode, ignoreAdjustments: boolArg(a, "ignore_adjustments") ?? false)
    }

    /// Refuses an `output` argument on a tool that has no new-layer form,
    /// rather than ignoring it. A silently dropped argument is the failure
    /// mode this whole family is written against: the call reports `ok` and
    /// writes into the photograph the caller asked to leave alone.
    func rejectRetouchOutput(_ a: [String: Any], _ tool: String) throws {
        guard let raw = a["output"] else { return }
        guard (raw as? String) == RetouchOutput.layer.rawValue else {
            throw ToolError(
                message: "\(tool) writes into the target layer; it has no new-layer form. "
                    + "Use clone_stamp, heal_stroke or spot_heal_stroke with "
                    + "output: \"new_layer\" to keep the original pixels.")
        }
    }

    /// The `output` argument: where the repair lands.
    func retouchOutput(_ a: [String: Any]) throws -> RetouchOutput {
        guard let raw = a["output"] else { return .layer }
        guard let name = raw as? String, let output = RetouchOutput(rawValue: name) else {
            throw ToolError(
                message: "output must be one of \(RetouchOutput.vocabulary.joined(separator: ", "))"
                    + " — got \"\(String(describing: raw))\". new_layer puts the repair on a new "
                    + "layer directly above the target, leaving the original pixels unchanged.")
        }
        return output
    }

    /// This call's sampling plate, or a thrown sentence explaining why there
    /// is none. The `RetouchPlate` type's own comment says why every retouch
    /// path goes through the core for this instead of reading a projection.
    func retouchPlate(
        _ document: ImageDocument, layer: Int, choice: RetouchSamplingChoice,
        output: RetouchOutput = .layer
    ) throws -> RetouchPlate {
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard let plate = try RetouchSource.plate(
            doc, layer: layer, choice: choice, output: output)
        else {
            throw ToolError(message: RetouchSource.missingPlate(choice.mode))
        }
        return plate
    }

    /// The name a created retouch layer carries, matching the interactive
    /// tools' so a recorded gesture and its replay name the same layer.
    static let retouchLayerName = EditorViewController.retouchLayerName
}
