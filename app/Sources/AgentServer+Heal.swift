import AppKit

/// heal_stroke and spot_heal_stroke: the agent mirrors of the Healing Brush
/// and the Spot Healing Brush. Both rasterize their footprint through the
/// shared `retouchPixels` rasterizer — the same canvas-sized premultiplied
/// overlay the interactive stroke pipeline builds, clipped to the same
/// selection — and hand it to the same core ops, so a stroke driven from
/// here and the same stroke drawn by hand land identical pixels.
///
/// The one difference between the two is what the overlay carries. The
/// Healing Brush's holds the ALIGNED SOURCE premultiplied by the footprint's
/// coverage (the Clone Stamp's overlay, literally); the Spot Healing Brush's
/// holds pure white coverage, because there is no sampled source — the core
/// inpaints one — and the op ignores the overlay's RGB by contract.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let healHandlers: [String: ToolHandler] = [
        "heal_stroke": { $0.healStroke },
        "spot_heal_stroke": { $0.spotHealStroke },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let healToolFacts: [String: ToolFacts] = [
        // Both heal a float layer's pixels (`FloatRetouch`).
        "heal_stroke": [.typedTool, .rasterTool, .floatTool, .cmykTool],
        "spot_heal_stroke": [.typedTool, .rasterTool, .floatTool, .cmykTool],
    ]

    // MARK: - Shared arguments

    /// The two healing brushes' shared stroke arguments: the stroke width in
    /// px, the heal's strength, and the tip.
    ///
    /// Two arguments the other stroke tools take are REFUSED rather than
    /// quietly ignored, because honoring them would break the op:
    ///
    /// - `flow` — the solve's domain is the overlay's coverage (alpha at or
    ///   above half), so a flowed-down dab would deposit less than that
    ///   everywhere and leave nothing to solve; the stroke always deposits
    ///   full coverage and `opacity` carries the strength instead. That is
    ///   the same rule the canvas applies by latching flow to 1 for these
    ///   two tools.
    /// - `blend_mode` — the heal replaces the covered pixels with a solved
    ///   result; there is no compositing step for a mode to change.
    private func healStrokeArgs(_ a: [String: Any], _ tool: String) throws
        -> (size: CGFloat, strength: Double, tip: BrushTip)
    {
        guard a["flow"] == nil else {
            throw ToolError(
                message: "\(tool) has no flow: the heal solves over the stroke's COVERAGE, so "
                    + "a flowed-down dab would leave nothing to solve. The stroke always "
                    + "deposits full coverage and opacity (1-100) is the heal's strength.")
        }
        guard stringArg(a, "blend_mode") == nil else {
            throw ToolError(
                message: "\(tool) has no blend_mode — it replaces the covered pixels with the "
                    + "blended result; blend modes apply to brush_stroke and clone_stamp.")
        }
        let rawSize = doubleArg(a, "size") ?? 24
        let rawOpacity = doubleArg(a, "opacity") ?? 100
        guard rawSize.isFinite, rawOpacity.isFinite else {
            throw ToolError(message: "size and opacity must be finite numbers")
        }
        var tip = try strokeTip(a)
        // The canvas's own latch, restated: BrushTip carries flow through to
        // every dab's alpha, and the covered set is what the solve runs over.
        tip.flow = 1
        return (
            // The floor is the tool's, not the paint family's: a healing
            // footprint under three pixels is entirely boundary and the op
            // answers "changed nothing" (EditorTool.minimumBrushSize says
            // why). Both healing brushes share it.
            size: CGFloat(min(max(rawSize, EditorTool.heal.minimumBrushSize), 200)),
            strength: min(max(rawOpacity, 1), 100) / 100,
            tip: tip
        )
    }

    /// The refusal both handlers report when the op itself answers "nothing
    /// would change" — a no-op, not a failure: no edit landed and no undo
    /// step opened, which is exactly what the tools do when they beep and
    /// roll a stroke back.
    private func healNoOpResult(_ tool: String, layer: Int, extra: String) throws -> String {
        try jsonResult([
            "ok": true, "changed": false, "layer": layer,
            "note": "\(tool) changed nothing. Either the stroke never covered layer "
                + "\(layer)'s pixels — the layer's extent may not reach the points, the "
                + "active selection may have clipped the whole stroke away, or a very soft "
                + "tip left the footprint below the half coverage the solve needs — or "
                + "\(extra) No undo step was added.",
        ])
    }

    /// `heal_stroke`'s no-op reply, shared by the byte and typed jobs.
    func healStrokeNoOpResult(layer: Int) throws -> String {
        try healNoOpResult("heal_stroke", layer: layer,
            extra: "the sampled texture is already what sits under the stroke.")
    }

    /// `spot_heal_stroke`'s no-op reply, shared by the byte and typed jobs.
    func spotHealNoOpResult(layer: Int) throws -> String {
        try healNoOpResult("spot_heal_stroke", layer: layer,
            extra: "the content-aware fill reproduced the pixels already under the stroke.")
    }

    // MARK: - Healing brush

    /// heal_stroke — mirrors the Healing Brush tool: a snapshot displaced by
    /// (first point − source) is painted into the stroke's coverage, and the
    /// pixels under the WHOLE footprint are Poisson-blended into the layer
    /// once, so the healed patch keeps the source's texture and the
    /// destination's illumination. The interactive tool commits the identical
    /// overlay through the identical op at mouse-up
    /// (EditorViewController+Heal.commitHealOverlay).
    func healStroke(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard a["source_layer"] == nil else {
            throw ToolError(message: "source_layer selects a separate raster only in native 16-bit or 32-bit float retouch. Choose sample for an 8-bit source.")
        }
        let index = try paintLayerIndex(a, document)
        // The UI refuses a healing stroke on an adjustment layer outright
        // (onStrokeBegin): there are no pixels to rewrite.
        try rejectAdjustmentPixelEdit(document, index)
        guard let sourceX = doubleArg(a, "source_x"), let sourceY = doubleArg(a, "source_y"),
            sourceX.isFinite, sourceY.isFinite
        else {
            throw ToolError(
                message: "heal_stroke requires source_x and source_y — the canvas point the "
                    + "texture is healed FROM (what sits there lands under the stroke's "
                    + "first point). Use spot_heal_stroke when there is no clean source to "
                    + "name.")
        }
        let points = try byteStrokePoints(a)
        let stroke = try healStrokeArgs(a, "heal_stroke")
        let usesDabs = try retouchUsesDabs(a, tip: stroke.tip, size: stroke.size)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        // The sampling mode picks the PLATE and nothing else, exactly as the
        // tool does (EditorViewController+Heal.strokeSourceImage): the core
        // never learns which was chosen, because the plate's pixels are
        // already in the overlay by the time it sees one.
        //
        // Latched before the edit, like the interactive stroke's mouse-down
        // latch: nothing can change what is being healed from mid-call.
        let choice = try retouchSampling(a, default: .all)
        let output = try retouchOutput(a)
        let plate = try retouchPlate(document, layer: index, choice: choice, output: output)
        // Tagged with the OVERLAY's space, not the pixels' own tag: the two
        // differ on a LUT/no-B2A profile, and a sampled colour must convert
        // nowhere on its way into the overlay (`RetouchPlate.cgImage(in:)`).
        let geometry = try retouchGeometry(a, source: CGPoint(x: sourceX, y: sourceY), destination: points[0])
        let transformed = try plate.image.retouchTransformed(matrix: geometry.values)
        guard let snapshot = transformed.makeCGImage(in: doc.drawingSpace) else {
            throw ToolError(message: "Could not read the picture to heal from.")
        }
        let expand = try retouchExpansion(a)

        // `performPixelEdit`'s retained-source refusal, restated for the job
        // path: the dispatch gate skips it under a mask or channel edit
        // target, but an in-place heal rewrites the layer's own pixels
        // whatever the target is.
        if output == .layer, doc.hasRetainedSource(index) {
            throw ToolError(message: RetainedImagePolicy.rasterizeFirst)
        }
        try rejectLockedEdit(document, index, DR_EDIT_PIXELS)
        let pixels: [UInt8]
        let details: [String: Any]
        if let canonical = try capturedNativeStroke(a) {
            pixels = try byteCanonicalRetouchPixels(document, stroke: canonical,
                size: stroke.size, ink: .source(snapshot))
            var applied: [String: Any] = ["size": Double(stroke.size),
                "opacity": stroke.strength * 100, "source_x": sourceX,
                "source_y": sourceY, "output": output.rawValue]
            applied.merge(choice.report) { _, new in new }
            details = ["dabs": canonical.dabs.count / 4, "applied": applied]
        } else {
        let centers = try usesDabs ? SoftBrush.stampCenters(along: points, spacing: SoftBrush.spacing(
            for: stroke.size, percent: stroke.tip.spacingPercent)) : []
        pixels = try retouchPixels(document) { context in
            // The un-flip inside: the overlay context is flipped
            // (row 0 = top), so the snapshot flips back locally to
            // land right side up at the displaced position — the
            // canvas's stampCloneDab rule, which the Healing Brush
            // shares with the Clone Stamp (the same closure body as
            // AgentServer+Retouch.cloneStamp's, which owns it: the
            // healing brush IS the clone stamp with a different op
            // at commit).
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
            // A non-default tip stamps the snapshot dab by dab: the
            // tip's footprint (gray falloff mask when soft, hard
            // ellipse otherwise, squashed and rotated) clips each
            // dab. Flow is 1 here by construction, so no dab is
            // deposited at reduced alpha — hardness SHRINKS the
            // healed footprint inside the brush circle rather than
            // fading the heal, which the join makes seamless anyway.
            if usesDabs {
                let mask = SoftBrush.isSoft(
                    hardness: stroke.tip.hardness, size: stroke.size)
                    ? SoftBrush.dabMask(
                        diameter: stroke.size, hardness: stroke.tip.hardness)
                    : nil
                for center in centers {
                    context.saveGState()
                    SoftBrush.clipDab(
                        in: context, at: center, diameter: stroke.size,
                        tip: stroke.tip, mask: mask)
                    drawSnapshot(context)
                    context.restoreGState()
                }
                return
            }
            context.addPath(
                Self.strokeCoverage(points: points, size: stroke.size))
            context.clip()
            drawSnapshot(context)
        }
        var applied: [String: Any] = [
            "size": Self.transformNumber(Double(stroke.size)),
            "opacity": Self.transformNumber(stroke.strength * 100),
            "source_x": Self.transformNumber(sourceX),
            "source_y": Self.transformNumber(sourceY),
            "output": output.rawValue,
        ]
        applied.merge(choice.report) { _, new in new }
        details = ["points": points.count, "applied": applied]
        }
        // The plate a new-layer heal sits on: the retained HANDLE, read in
        // place on the worker (EditorViewController+Heal.startHealJob).
        let backdrop = plate.backdrop
        let job = try DocumentJobs.start(document: document, layer: index, title: "Healing Brush",
            scratch: UInt64(pixels.count), record: .notACommand, showProgress: false,
            rasterizesTarget: output == .layer, compute: { source, control in
                let source = expand && output == .layer ? try source.expandingRetouchDestination(index) : source
                if let backdrop {
                    return try source.healToNewLayerJob(
                        index, overlay: pixels, strength: stroke.strength, plate: backdrop,
                        name: Self.retouchLayerName, control: control)
                }
                return try source.healJob(index, overlay: pixels, strength: stroke.strength,
                    control: control)
            })
        job.details = details
        return try finishJobCall(job, tool: "heal_stroke", arguments: a,
            unchanged: { try self.healStrokeNoOpResult(layer: index) })
    }

    // MARK: - Spot healing brush

    /// spot_heal_stroke — mirrors the Spot Healing Brush tool: the same
    /// stroke with no source point. The overlay carries pure white coverage
    /// (the dodge_burn shape — the core reads only its alpha), the core
    /// inpaints the footprint content-aware from a ring of valid pixels
    /// around it, and blends the result in exactly as heal_stroke's sampled
    /// patch is blended. The interactive tool commits the identical overlay
    /// through the identical op at mouse-up
    /// (EditorViewController+Heal.commitHealOverlay).
    func spotHealStroke(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let index = try paintLayerIndex(a, document)
        // The UI refuses a healing stroke on an adjustment layer outright
        // (onStrokeBegin): there are no pixels to rewrite.
        try rejectAdjustmentPixelEdit(document, index)
        let points = try byteStrokePoints(a)
        let stroke = try healStrokeArgs(a, "spot_heal_stroke")
        let usesDabs = try retouchUsesDabs(a, tip: stroke.tip, size: stroke.size)
        let rawRing = intArg(a, "ring") ?? 0
        guard rawRing >= 0, rawRing <= 512 else {
            throw ToolError(
                message: "ring must be between 0 and 512 px (0 = automatic, which is the "
                    + "default). Anything from 1 to 20 is raised to 21 — three patch widths, "
                    + "the narrowest band a source patch has room to move in.")
        }
        let rawSeed = intArg(a, "seed") ?? 0
        guard rawSeed >= 0 else {
            throw ToolError(message: "seed must be a non-negative integer (default 0)")
        }
        // The options bar's Type; absent is Content-Aware.
        let type = try SpotHealType.argument(a)
        // Unlike the Healing Brush's, this choice is the CORE's: the core
        // generates the source, so it is the side that has to know where to
        // sample from — the overlay carries coverage and nothing else.
        // Default `current`, which is what this tool's `sample_all_layers`
        // default of false always meant: spot healing is used on the layer it
        // is used on, and sampling the composite would inpaint from pixels
        // that layer does not own.
        let choice = try retouchSampling(a, default: .current)
        let output = try retouchOutput(a)
        let expand = try retouchExpansion(a)
        // The plate a new-layer repair will sit on, read before the worker
        // starts: the job runs off the main thread and cannot reach back for
        // a document handle.
        let backdrop: [UInt8]? = try output == .newLayer
            ? retouchPlate(document, layer: index, choice: choice, output: output)
                .backdrop.flatMap { image in
                    image.withPlateBytes {
                        Array(UnsafeBufferPointer(start: $0, count: image.width * image.height * 4))
                    }
                }
            : nil

        try rejectLockedEdit(document, index, DR_EDIT_PIXELS)
        let pixels: [UInt8]
        if let canonical = try capturedNativeStroke(a) {
            pixels = try byteCanonicalRetouchPixels(document, stroke: canonical,
                size: stroke.size,
                ink: .color(ColorProfile.coverageInk(.white, in: document.drawingSpace)))
        } else {
        let centers = try usesDabs ? SoftBrush.stampCenters(along: points, spacing: SoftBrush.spacing(
            for: stroke.size, percent: stroke.tip.spacingPercent)) : []
        pixels = try retouchPixels(document) { context in
            // Pure coverage: white at full alpha, since the op reads
            // only the overlay's alpha and there is no sampled
            // source to place. Flow is 1 by construction, so a
            // stamped tip deposits full-alpha dabs and hardness
            // shrinks the footprint rather than fading it.
            let white = ColorProfile.coverageInk(
                NSColor(srgbRed: 1, green: 1, blue: 1, alpha: 1), in: document.drawingSpace)
            if usesDabs,
                let dab = SoftBrush.dab(
                    color: white, diameter: stroke.size,
                    hardness: stroke.tip.hardness, space: document.drawingSpace) {
                for center in centers {
                    SoftBrush.stamp(
                        dab, in: context, at: center, diameter: stroke.size,
                        tip: stroke.tip)
                }
                return
            }
            context.setFillColor(white.cgColor)
            context.addPath(
                Self.strokeCoverage(points: points, size: stroke.size))
            context.fillPath()

        }
        }
        let job = try DocumentJobs.start(document: document, layer: index, title: "Spot Healing",
            scratch: UInt64(pixels.count), record: .notACommand, showProgress: false,
            rasterizesTarget: output == .layer, compute: { source, control in
                let source = expand && output == .layer ? try source.expandingRetouchDestination(index) : source
                if let backdrop {
                    return try source.spotHealToNewLayerJob(
                        index, overlay: pixels, strength: stroke.strength, ring: rawRing,
                        seed: UInt64(rawSeed), type: type, plate: backdrop,
                        name: Self.retouchLayerName, control: control)
                }
                return try source.spotHealJob(index, overlay: pixels, strength: stroke.strength,
                    ring: rawRing, seed: UInt64(rawSeed), type: type, sample: choice.mode,
                    ignoreAdjustments: choice.ignoreAdjustments, control: control)
            })
        var applied: [String: Any] = [
            "size": Self.transformNumber(Double(stroke.size)),
            "opacity": Self.transformNumber(stroke.strength * 100),
            "ring": rawRing, "seed": rawSeed, "output": output.rawValue, "type": type.rawValue,
        ]
        applied.merge(choice.report) { _, new in new }
        job.details = ["points": points.count, "applied": applied]
        return try finishJobCall(job, tool: "spot_heal_stroke", arguments: a,
            unchanged: { try self.spotHealNoOpResult(layer: index) })
    }
}
