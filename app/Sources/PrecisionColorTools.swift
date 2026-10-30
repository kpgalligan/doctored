import AppKit

/// Shared captured native Gradient model. Continuous endpoint alpha is kept
/// separate from scalar ink; a transparent endpoint fades its contribution.
struct PrecisionGradient {
    let from: CGPoint
    let to: CGPoint
    let start: PrecisionInk
    let end: PrecisionInk
    let radial: Bool
    let space: PrecisionColorSpace
    let opacity: Double
    /// The stops gradient: each stop's position and ink, first to
    /// last, and the shape's name (`GradientRamp.shapeNames`). nil is the
    /// two-colour gradient, whose requests are unchanged; with stops,
    /// `start` / `end` still carry the first and last stop.
    var stops: [(position: Double, ink: PrecisionInk)]? = nil
    var shape: String? = nil
    /// Each stop's midpoints, parallel to `stops`; nil is every one at
    /// 0.5. Every stop a request or a scalar operation carries merges its
    /// `keys`.
    var midpoints: [GradientRamp.StopMidpoint]? = nil
    /// Smoothness as a fraction: sent with stops, and
    /// recorded, only below 1, so a default gradient's request is unchanged.
    var smoothness: Double = 1

    /// Stop `index`'s midpoint keys (`GradientRamp.StopMidpoint.keys`).
    private func midpointKeys(_ index: Int) -> [String: Any] {
        guard let midpoints, index < midpoints.count else { return [:] }
        return midpoints[index].keys
    }
    /// How each colour meets the layer's pixels; Normal sends the
    /// request, and records the step, the gradient always has. A coverage
    /// target never carries one (`GradientRamp.coverageTargetRefusal`).
    var blendMode: DrBlendMode = DR_BLEND_NORMAL
    /// A float layer's two endpoints as linear ink: linear light in
    /// the document's primaries, RGB 0…65504 and alpha 0…1, sent with
    /// `color_space: "linear"` and recorded as `start_rgba32` / `end_rgba32`.
    /// nil is every other gradient, whose request and record are unchanged;
    /// a coverage target never carries one.
    var linear: (start: [Double], end: [Double])? = nil

    func command(layer: Int) throws -> PrecisionPixelCommand {
        var request: [String: Any] = ["op": "gradient", "layer": layer, "from": [from.x, from.y],
            "to": [to.x, to.y], "start": start.rgba, "end": end.rgba,
            "color_space": space.rawValue, "opacity": opacity, "radial": radial]
        if let stops {
            request["stops"] = stops.enumerated().map { index, stop -> [String: Any] in
                (["position": stop.position, "color": stop.ink.rgba] as [String: Any])
                    .merging(midpointKeys(index)) { _, new in new }
            }
            request["style"] = shape ?? (radial ? "radial" : "linear")
            if smoothness < 1 { request["smoothness"] = smoothness }
        }
        if blendMode != DR_BLEND_NORMAL { request["blend_mode"] = Int(blendMode.rawValue) }
        if let linear {
            request["start"] = linear.start
            request["end"] = linear.end
            request["color_space"] = "linear"
            // Off the two-colour path (a shape past radial) the ramp is the
            // same two colours as its stops.
            if stops != nil {
                request["stops"] = [["position": 0.0, "color": linear.start],
                                    ["position": 1.0, "color": linear.end]]
            }
        }
        return try PrecisionPixelCommand(request)
    }

    func scalarOperation(document: ImageDocument) throws -> [String: Any] {
        func gray(_ color: PrecisionInk) throws -> UInt16 {
            if space == .document { return try PrecisionScalarOperations.ink(color) }
            // Read in `coverageSpace`: white stays 65535 on a LUT-mode document.
            let srgb = NSColor(srgbRed: color.rgba[0], green: color.rgba[1], blue: color.rgba[2],
                alpha: color.rgba[3])
            return try PrecisionScalarOperations.ink(PrecisionInk(color: srgb,
                space: ColorProfile.coverageSpace(document.nsColorSpace, for: srgb)))
        }
        var operation: [String: Any] = try ["kind": "gradient", "from": [from.x, from.y], "to": [to.x, to.y], "radial": radial,
            "start": gray(start), "end": gray(end), "start_alpha": start.rgba[3], "end_alpha": end.rgba[3], "opacity": opacity]
        if let stops {
            // A scalar stop is `[g, g, g, alpha]`, g the coverage gray over 65535.
            operation["stops"] = try stops.enumerated().map { index, stop -> [String: Any] in
                let g = Double(try gray(stop.ink)) / 65535
                return (["position": stop.position, "color": [g, g, g, stop.ink.rgba[3]]]
                    as [String: Any]).merging(midpointKeys(index)) { _, new in new }
            }
            operation["style"] = shape ?? (radial ? "radial" : "linear")
            if smoothness < 1 { operation["smoothness"] = smoothness }
        }
        return operation
    }

    func record(target: String) -> [ActionStep] {
        if let linear {
            // `gradient`'s linear endpoints replace every other colour
            // key, and the shape is the drag's own.
            var arguments: [String: Any] = ["layer": ActionArgs.activeLayer, "target": target,
                "x0": Double(from.x), "y0": Double(from.y), "x1": Double(to.x), "y1": Double(to.y),
                "shape": shape ?? (radial ? "radial" : "linear"),
                "start_rgba32": linear.start, "end_rgba32": linear.end, "opacity": opacity]
            if blendMode != DR_BLEND_NORMAL {
                arguments["blend_mode"] = DrBlendMode.displayName(for: blendMode)
            }
            if smoothness < 1 { arguments["smoothness"] = ActionArgs.number(smoothness) }
            return [ActionStep(tool: "gradient", arguments: arguments, note: "Gradient tool: drag")]
        }
        var arguments: [String: Any] = ["layer": ActionArgs.activeLayer, "target": target,
            "x0": Double(from.x), "y0": Double(from.y), "x1": Double(to.x), "y1": Double(to.y),
            "shape": radial ? "radial" : "linear", "start_rgba": start.rgba, "end_rgba": end.rgba,
            "color_space": space.rawValue, "opacity": opacity]
        if blendMode != DR_BLEND_NORMAL {
            arguments["blend_mode"] = DrBlendMode.displayName(for: blendMode)
        }
        return [ActionStep(tool: "gradient", arguments: arguments, note: "Gradient tool: drag")]
    }
}

extension EditorViewController {
    func fillPrecisionColor(at point: CGPoint) {
        guard let document, !refuseOperationScope(.fill) else { return }
        // One colour channel of a float layer is no bucket target
        // (`FloatPaint.planeRefusal`); a mask or channel keeps the scalar route.
        if document.doc?.isFloat == true, paintTarget.isPlane {
            document.presentError(DoctoredCoreError(message: FloatPaint.planeRefusal))
            return
        }
        if paintTarget.isCoverage { fillNativePlane(at: point); return }
        do {
            let capture = try PrecisionEditorCapture(document), options = ToolOptionsStore.shared.fill
            // Contents picks the colour; Mode and Sample all layers the style. #1 Lab
            // phase 2e: on a Lab layer the Contents swatch's Lab ink when it has one.
            let shared = ToolOptionsStore.shared.sharedState
            let color = LabPaint.color(fillToolColor,
                stored: options.contentsIndex == 1 ? shared.backgroundLab : shared.foregroundLab,
                in: document.doc)
            let style = fillToolStyle
            // A float layer takes the float modes only, and a colour
            // above white is linear ink, used as it is and recorded as `rgba32`.
            if capture.facade.isFloat, let reason = FloatPaint.modeRefusal(style.mode) {
                throw DoctoredCoreError(message: reason)
            }
            let linear = capture.facade.isFloat
                ? ExtendedRangeColor.aboveWhite(color, in: capture.facade) : nil
            let ink: PrecisionInk? = try linear == nil
                ? PrecisionInk(color: color, space: document.nsColorSpace) : nil
            let inkColor = linear ?? ink?.rgba ?? []
            let inkSpace = linear == nil ? "document" : "linear"
            let opacity = min(max(options.opacity, 0), 100) / 100
            let x = Int(floor(point.x)), y = Int(floor(point.y))
            let tolerance = Int((options.tolerance * 257).rounded())
            // #1: a CMYK layer fills in ink, from its own pixels or (All Layers) the
            // composite's ink, in the CMYK modes.
            if capture.source.isCmyk {
                if let reason = CmykPaint.modeRefusal(style.mode) { throw DoctoredCoreError(message: reason) }
                let ink = try CmykPaint.ink(color, stored: fillToolInk, in: capture.source)
                var record = style.recording(fillRecord(at: point, color: color, opacity: opacity, options: options))
                for i in record.indices {
                    for key in ["color", "rgba", "color_space"] { record[i].arguments.removeValue(forKey: key) }
                    record[i].arguments["ink"] = ink.prefix(4).map { $0 * 100 }
                    record[i].arguments["fill_mode"] = "color_match"
                    record[i].arguments["opacity"] = opacity
                }
                if style.sampleAllLayers {
                    let result = try capture.source.cmykBucketFilledSamplingComposite(
                        layer: capture.layer, x: x, y: y, tolerance: tolerance,
                        contiguous: options.contiguous, ink: ink, opacity: opacity,
                        mode: style.mode, selection: capture.selection, width: capture.facade.width,
                        height: capture.facade.height)
                    _ = try capture.publish(result, to: document, name: "Fill", record: record)
                    return
                }
                let command = try CmykPaint.command(["op": "bucket_fill", "layer": capture.layer,
                    "x": x, "y": y, "tolerance": tolerance, "contiguous": options.contiguous,
                    "ink": ink, "opacity": opacity, "blend_mode": Int(style.mode.rawValue)])
                let coverage = try PrecisionStrokeCoverage.selection(width: capture.facade.width,
                    height: capture.facade.height, selection: capture.selection)
                let edit = try PrecisionPixelEdit(capture.source, command: command, input: coverage)
                _ = try capture.publish(edit.run(title: "Fill"), to: document, name: "Fill", record: record)
                return
            }
            var record = style.recording(
                fillRecord(at: point, color: color, opacity: opacity, options: options))
            for i in record.indices {
                if let linear {
                    record[i].arguments["rgba32"] = linear
                    for key in ["color", "rgba", "color_space"] {
                        record[i].arguments.removeValue(forKey: key)
                    }
                } else {
                    record[i].arguments["rgba"] = inkColor
                    record[i].arguments["color_space"] = "document"
                }
                record[i].arguments["fill_mode"] = "color_match"
                record[i].arguments["opacity"] = opacity
            }
            if style.sampleAllLayers {
                guard document.confirmRasterize(layer: capture.layer) else { return }
                let result = try capture.source.bucketFilledSamplingComposite(
                    layer: capture.layer, x: x, y: y, tolerance: tolerance,
                    contiguous: options.contiguous, color: inkColor, colorSpace: inkSpace,
                    opacity: opacity, mode: style.mode, selection: capture.selection,
                    width: capture.facade.width, height: capture.facade.height)
                _ = try capture.publish(result, to: document, name: "Fill", record: record)
                return
            }
            let command = try PrecisionPixelCommand(["op": "bucket_fill", "layer": capture.layer,
                "x": x, "y": y, "tolerance": tolerance,
                "contiguous": options.contiguous, "color": inkColor, "color_space": inkSpace, "opacity": opacity,
                "blend_mode": style.mode.rawValue])
            let coverage = try PrecisionStrokeCoverage.selection(width: capture.facade.width,
                height: capture.facade.height, selection: capture.selection)
            let edit = try PrecisionPixelEdit(capture.source, command: command, input: coverage)
            guard document.confirmRasterize(layer: capture.layer) else { return }
            _ = try capture.publish(edit.run(title: "Fill"), to: document, name: "Fill", record: record)
        } catch { document.presentError(error) }
    }

    func gradientPrecisionColor(from: CGPoint, to: CGPoint) {
        guard let document, !refuseOperationScope(.gradient) else { return }
        do {
            let capture = try PrecisionEditorCapture(document), options = ToolOptionsStore.shared.gradient
            // At float one colour channel is no gradient target, and a
            // layer takes the float blend modes only (`FloatPaint`).
            if capture.facade.isFloat {
                if capture.target.isPlane { throw DoctoredCoreError(message: FloatPaint.planeRefusal) }
                if !capture.target.isCoverage, let reason = FloatPaint.modeRefusal(gradientToolMode) {
                    throw DoctoredCoreError(message: reason)
                }
            }
            // A coverage target reads the colours in `coverageSpace`.
            let ink = { (color: NSColor) in
                try PrecisionInk(color: color, space: capture.target.isCoverage
                    ? ColorProfile.coverageSpace(document.nsColorSpace, for: color)
                    : document.nsColorSpace)
            }
            // The bar's ramp with the opacity left out of the stops: the
            // request carries it once, as the two-colour request always has.
            var unfaded = options
            unfaded.opacity = 100
            // #1 Lab phase 2e: a Lab layer's ends are the swatches' Lab inks when they have
            // them; a coverage target keeps the sRGB swatches.
            let shared = ToolOptionsStore.shared.sharedState
            let labDoc = capture.target.isCoverage ? nil : document.doc
            let ramp = GradientRamp(options: unfaded,
                foreground: LabPaint.color(paintColor, stored: shared.foregroundLab, in: labDoc),
                background: LabPaint.color(backgroundColor, stored: shared.backgroundLab, in: labDoc))
            // #1: a CMYK layer's gradient is the ramp in ink, in the CMYK modes.
            if capture.source.isCmyk, !capture.target.isCoverage {
                if let reason = CmykPaint.modeRefusal(gradientToolMode) { throw DoctoredCoreError(message: reason) }
                let shared = ToolOptionsStore.shared.sharedState
                let twoColour = (options.stops?.count ?? 0) < 2
                let stops = try CmykPaint.stops(ramp.stops.map { ($0.position, $0.color) },
                    foreground: twoColour ? shared.foregroundInk : nil,
                    background: twoColour ? shared.backgroundInk : nil, reversed: options.reverse,
                    midpoints: ramp.midpoints, in: capture.source)
                var request: [String: Any] = ["op": "gradient", "layer": capture.layer,
                    "from": [from.x, from.y], "to": [to.x, to.y], "stops": stops,
                    "style": ramp.shapeName, "opacity": min(max(options.opacity, 0), 100) / 100,
                    "blend_mode": Int(gradientToolMode.rawValue)]
                if ramp.smoothness < 1 { request["smoothness"] = ramp.smoothness }
                let command = try CmykPaint.command(request)
                let coverage = try PrecisionStrokeCoverage.selection(width: capture.facade.width,
                    height: capture.facade.height, selection: capture.selection)
                let result = try PrecisionPixelEdit(capture.source, command: command, input: coverage)
                    .run(title: "Gradient")
                let percent = { (stop: [String: Any]) in ((stop["ink"] as? [Double]) ?? []).prefix(4).map { $0 * 100 } }
                var record: [ActionStep] = twoColour
                    ? [ActionStep(tool: "gradient", arguments: ["layer": ActionArgs.activeLayer, "target": "layer",
                        "x0": Double(from.x), "y0": Double(from.y), "x1": Double(to.x), "y1": Double(to.y),
                        "shape": ramp.shapeName, "start_ink": percent(stops[0]),
                        "end_ink": percent(stops[stops.count - 1]),
                        "opacity": min(max(options.opacity, 0), 100) / 100], note: "Gradient tool: drag")]
                    : .gradientStops(from: from, to: to, ramp: gradientToolRamp, target: "layer",
                                     space: document.nsColorSpace, mode: gradientToolMode)
                if twoColour, gradientToolMode != DR_BLEND_NORMAL {
                    for i in record.indices {
                        record[i].arguments["blend_mode"] = DrBlendMode.displayName(for: gradientToolMode)
                    }
                }
                if twoColour, ramp.smoothness < 1 {
                    for i in record.indices {
                        record[i].arguments["smoothness"] = ActionArgs.number(ramp.smoothness)
                    }
                }
                _ = try capture.publish(result, to: document, name: "Gradient", record: record)
                return
            }
            let inks = try ramp.stops.map { (position: $0.position, ink: try ink($0.color)) }
            var gradient = PrecisionGradient(from: from, to: to,
                start: inks[0].ink, end: inks[inks.count - 1].ink,
                radial: ramp.style == DR_GRADIENT_RADIAL, space: .document, opacity: min(max(options.opacity, 0), 100) / 100)
            if !ramp.isLegacyTwoStop {
                gradient.stops = inks
                gradient.shape = ramp.shapeName
                gradient.midpoints = ramp.midpoints
                gradient.smoothness = ramp.smoothness
            }
            // The bar's Mode. `gradientCommitted` has already refused
            // a non-Normal one on a coverage target, which never carries one.
            if !capture.target.isCoverage { gradient.blendMode = gradientToolMode }
            // A float layer's Foreground → Background ramp with a colour
            // above white is linear ink (stored stops are sRGB hex).
            if !capture.target.isCoverage, (options.stops?.count ?? 0) < 2 {
                gradient.linear = FloatPaint.linearEndpoints(start: ramp.stops[0].color,
                    end: ramp.stops[ramp.stops.count - 1].color, facade: capture.facade)
            }
            if !capture.target.isCoverage || capture.target.isPlane {
                guard document.confirmRasterize(layer: capture.layer) else { return }
            }
            let result: RasterPrecisionDocument?
            if capture.target.isCoverage {
                result = try PrecisionScalarOperations.edit(capture,
                    operation: gradient.scalarOperation(document: document)).run(title: "Gradient")
            } else {
                let coverage = try PrecisionStrokeCoverage.selection(width: capture.facade.width,
                    height: capture.facade.height, selection: capture.selection)
                result = try PrecisionPixelEdit(capture.source, command: gradient.command(layer: capture.layer), input: coverage).run(title: "Gradient")
            }
            let target = capture.target.agentName(in: capture.facade)
            _ = try capture.publish(result, to: document, name: "Gradient",
                record: ramp.isLegacyTwoStop || gradient.linear != nil ? gradient.record(target: target)
                    : .gradientStops(from: from, to: to, ramp: gradientToolRamp, target: target,
                                     space: document.nsColorSpace, mode: gradient.blendMode))
        } catch { document.presentError(error) }
    }
}

extension AgentServer {
    /// Mirrors the native Gradient drag for RGB and every scalar target.
    func precisionGradient(_ a: [String: Any], document: ImageDocument) throws -> String {
        // A float layer's linear endpoints name the ramp themselves.
        let linear = try linearGradientEndpoints(a, document)
        let capture = try PrecisionEditorCapture(document), layer = try structuralLayerIndex(a, document)
        // #1: a CMYK layer's gradient in ink (`start_ink`/`end_ink`, or colours converted).
        if capture.source.isCmyk, try !paintTarget(a, document).isCoverage {
            return try cmykGradient(a, document: document, capture: capture, layer: layer)
        }
        if linear == nil { try requireGradientRamp(a, numericEndpoints: true) }
        let target = try paintTarget(a, document)
        guard let x0 = doubleArg(a, "x0"), let y0 = doubleArg(a, "y0"),
              let x1 = doubleArg(a, "x1"), let y1 = doubleArg(a, "y1"), [x0,y0,x1,y1].allSatisfy(\.isFinite) else {
            throw ToolError(message: "Gradient requires finite x0, y0, x1 and y1.")
        }
        let shape = stringArg(a, "shape") ?? "linear", opacity = doubleArg(a, "opacity") ?? 1
        // `blend_mode`, the shared vocabulary; refused by name on a
        // coverage target, as the 8-bit handler refuses it (`gradientPlane`).
        let blend = try blendModeArg(a) ?? DR_BLEND_NORMAL
        if target.isCoverage, blend != DR_BLEND_NORMAL {
            throw ToolError(message: GradientRamp.coverageTargetRefusal)
        }
        guard GradientRamp.shapeNames.contains(shape), opacity.isFinite, (0...1).contains(opacity) else {
            throw ToolError(message: "Gradient shape must be linear, radial, angle, reflected or diamond; opacity must be in 0…1.")
        }
        // Phase 12A: `display_p3` endpoints are converted HERE, once, into the
        // document's own space — exactly what `precisionInkSpace` does for the
        // typed paint/fill — so the command carries document components and the
        // catalog's "at either depth" claim is true of this tool too.
        // A `document` hex endpoint is the document's own bytes at
        // either depth (`documentHexInk`, exact ×257 here).
        let named = try colorSpaceArgument(a)
        let p3 = named == .displayP3
        let space: PrecisionColorSpace = named == .srgb ? .srgb : .document
        func ink(_ a: [String: Any], _ key: String, colorKey: String, fallback: NSColor) throws -> PrecisionInk {
            if let raw = a[key] as? [Any], raw.count == 4,
               raw.allSatisfy({ NativeBrushStroke.jsonNumber($0) != nil }) {
                let values = raw.compactMap(NativeBrushStroke.jsonNumber)
                let ink = try PrecisionInk(red: values[0], green: values[1], blue: values[2], alpha: values[3])
                guard p3 else { return ink }
                return try PrecisionInk(color: NSColor(colorSpace: .displayP3,
                    components: ink.rgba.map { CGFloat($0) }, count: 4), space: document.nsColorSpace)
            }
            guard a[key] == nil else { throw ToolError(message: "\(key) requires four normalized components.") }
            if named == .document { return try documentHexInk(a, colorKey, fallback: fallback) }
            let authored = try parseColor(a, colorKey, fallback: fallback)
            guard p3 else { return try PrecisionInk(color: authored) }
            guard let bytes = ColorProfile.bytes(authored, in: .sRGB) else {
                throw ToolError(message: "Could not convert the color")
            }
            return try PrecisionInk(color: NSColor(colorSpace: .displayP3,
                components: bytes.map { CGFloat($0) / 255 }, count: 4), space: document.nsColorSpace)
        }
        var gradient = try PrecisionGradient(from: CGPoint(x: x0, y: y0), to: CGPoint(x: x1, y: y1),
            start: ink(a, "start_rgba", colorKey: "start_color", fallback: .black),
            end: ink(a, "end_rgba", colorKey: "end_color", fallback: .clear), radial: shape == "radial", space: space, opacity: opacity)
        gradient.blendMode = blend
        gradient.linear = linear
        gradient.smoothness = try gradientSmoothnessArgument(a)
        // `stops`, or a shape past radial, is the stops request; a
        // stop's `opacity` scales its colour's alpha.
        if let stops = try gradientStopArguments(a) {
            gradient.stops = try stops.map { stop in
                let color = try ink(stop.color, "rgba", colorKey: "color", fallback: .black).rgba
                return (stop.position, try PrecisionInk(red: color[0], green: color[1], blue: color[2],
                                                        alpha: color[3] * stop.opacity))
            }
            gradient.midpoints = stops.map(\.midpoint)
            gradient.shape = shape
        } else if !["linear", "radial"].contains(shape) || gradient.smoothness < 1 {
            // A Smoothness below 1 needs the stops request too.
            gradient.stops = [(0, gradient.start), (1, gradient.end)]
            gradient.shape = shape
        }
        let result: RasterPrecisionDocument?
        if target.isCoverage {
            if !capture.quickMask, let reason = scopeRefusal(.filter, target: target,
                doc: capture.facade, layer: layer) { throw ToolError(message: reason) }
            result = try PrecisionScalarOperations.edit(capture, target: target, layer: layer,
                operation: gradient.scalarOperation(document: document)).run(title: "Gradient")
        } else {
            _ = try paintLayerIndex(a, document)
            let coverage = try PrecisionStrokeCoverage.selection(width: capture.facade.width,
                height: capture.facade.height, selection: capture.selection)
            result = try PrecisionPixelEdit(capture.source, command: gradient.command(layer: layer), input: coverage).run(title: "Gradient")
        }
        let changed = try capture.publish(result, to: document, name: "Gradient")
        // A float layer's pixels are f32; a mask or channel is u16.
        let storage = capture.facade.isFloat && !target.isCoverage ? "rgba32_float" : "u16"
        return try jsonResult(["ok": true, "changed": changed, "storage": storage])
    }

    /// `gradient` on a CMYK layer (#1 phase 2) — mirrors the Gradient tool's CMYK drag
    /// (`gradientPrecisionColor`): `start_ink`/`end_ink` (C, M, Y, K percent) or `start_color`/
    /// `end_color` converted through the document's profile, or `stops` whose colours convert;
    /// in the CMYK modes (`CmykPaint.modeRefusal`).
    private func cmykGradient(_ a: [String: Any], document: ImageDocument,
                              capture: PrecisionEditorCapture, layer: Int) throws -> String {
        guard let x0 = doubleArg(a, "x0"), let y0 = doubleArg(a, "y0"),
              let x1 = doubleArg(a, "x1"), let y1 = doubleArg(a, "y1"), [x0, y0, x1, y1].allSatisfy(\.isFinite) else {
            throw ToolError(message: "Gradient requires finite x0, y0, x1 and y1.")
        }
        let shape = stringArg(a, "shape") ?? "linear", opacity = doubleArg(a, "opacity") ?? 1
        guard GradientRamp.shapeNames.contains(shape), opacity.isFinite, (0...1).contains(opacity) else {
            throw ToolError(message: "Gradient shape must be linear, radial, angle, reflected or diamond; opacity must be in 0…1.")
        }
        let mode = try blendModeArg(a) ?? DR_BLEND_NORMAL
        if let reason = CmykPaint.modeRefusal(mode) { throw ToolError(message: reason) }
        let smoothness = try gradientSmoothnessArgument(a)
        _ = try paintLayerIndex(a, document)
        let stops: [[String: Any]]
        if let given = try gradientStopArguments(a) {
            stops = try given.map { stop in
                var ink = try cmykPaintInk(stop.color, "ink", colorKey: "color", fallback: .black,
                                           precision: capture.source)
                ink[4] *= stop.opacity
                return (["position": stop.position, "ink": ink] as [String: Any])
                    .merging(stop.midpoint.keys) { _, new in new }
            }
        } else {
            stops = [
                ["position": 0.0, "ink": try cmykPaintInk(a, "start_ink", colorKey: "start_color",
                                                         fallback: .black, precision: capture.source)],
                ["position": 1.0, "ink": try cmykPaintInk(a, "end_ink", colorKey: "end_color",
                                                         fallback: .clear, precision: capture.source)],
            ]
        }
        var request: [String: Any] = ["op": "gradient", "layer": layer, "from": [x0, y0],
            "to": [x1, y1], "stops": stops, "style": shape, "opacity": opacity,
            "blend_mode": Int(mode.rawValue)]
        if smoothness < 1 { request["smoothness"] = smoothness }
        let command = try CmykPaint.command(request)
        let coverage = try PrecisionStrokeCoverage.selection(width: capture.facade.width,
            height: capture.facade.height, selection: capture.selection)
        let result = try PrecisionPixelEdit(capture.source, command: command, input: coverage).run(title: "Gradient")
        let changed = try capture.publish(result, to: document, name: "Gradient")
        return try jsonResult(["ok": true, "changed": changed, "storage": "cmyka16_unorm"])
    }

    /// `gradient`'s `start_rgba32` + `end_rgba32`: both ends of a
    /// float layer's ramp as linear light in the document's primaries, each
    /// parsed as `linearInkArgument` parses `rgba32` (four numbers, RGB
    /// 0…65504 and alpha 0…1). nil when neither is passed. One alone, either
    /// beside `stops`, a hex or numeric endpoint or `color_space`, and both on
    /// a mask or channel target (`FloatPaint.coverageInkRefusal`) refuse by
    /// name; an 8- or 16-bit document and Quick Mask refuse them before
    /// dispatch (`FloatPaint.refusal`).
    func linearGradientEndpoints(
        _ a: [String: Any], _ document: ImageDocument
    ) throws -> (start: [Double], end: [Double])? {
        let keys = ["start_rgba32", "end_rgba32"]
        guard keys.contains(where: { a[$0] != nil }) else { return nil }
        guard keys.allSatisfy({ a[$0] != nil }) else {
            throw ToolError(message: "Pass start_rgba32 and end_rgba32 together: they are the "
                + "two ends of one linear-light ramp.")
        }
        let replaced = ["stops", "start_color", "end_color", "start_rgba", "end_rgba", "color_space"]
        guard !replaced.contains(where: { a[$0] != nil }) else {
            throw ToolError(message: "start_rgba32 and end_rgba32 replace start_color, end_color, "
                + "start_rgba, end_rgba and stops, and take no color_space. Pass them alone.")
        }
        if try paintTarget(a, document).isCoverage {
            throw ToolError(message: FloatPaint.coverageInkRefusal)
        }
        func endpoint(_ key: String) throws -> [Double] {
            let parsed: [Double]?
            do {
                parsed = try a[key].flatMap { try linearInkArgument(["rgba32": $0], document) }
            } catch let error as ToolError {
                // `linearInkArgument` names `rgba32`; this call's key is `key`.
                throw ToolError(message: error.message.replacingOccurrences(of: "rgba32", with: key))
            }
            guard let rgba = parsed else { throw ToolError(message: "\(key) requires four numbers.") }
            return rgba
        }
        return (try endpoint("start_rgba32"), try endpoint("end_rgba32"))
    }

    /// Mirrors the native Dodge / Burn stroke and its exposure/range controls.
    /// A 16-bit or float layer stroke commits as a `DocumentJobs` job;
    /// a mask or alpha channel `target` is the scalar `dodge_burn` (R4m).
    func precisionDodgeBurn(_ a: [String: Any], document: ImageDocument) throws -> String {
        guard a["blend_mode"] == nil else { throw ToolError(message: "Dodge / Burn has no blend mode.") }
        let requested = try paintTarget(a, document)
        if case .plane = requested {
            throw ToolError(message: "Dodge / Burn's target is the layer, its mask or an alpha channel.")
        }
        if requested.isCoverage {
            return try precisionScalarPaint("dodge_burn", a, document: document, target: requested)
        }
        let capture = try PrecisionEditorCapture(document), layer = try paintLayerIndex(a, document)
        let exposure = doubleArg(a, "exposure") ?? 50, size = doubleArg(a, "size") ?? 24
        let ranges = ["shadows", "midtones", "highlights"]
        guard exposure.isFinite, size.isFinite, let range = ranges.firstIndex(of: stringArg(a, "range") ?? "midtones") else {
            throw ToolError(message: "Invalid Dodge / Burn size, exposure or tonal range.")
        }
        let native = try capturedNativeStroke(a) ?? NativeBrushStrokeSession.generated(points: parsePoints(a),
            size: CGFloat(min(max(size, 1), EditorTool.dodge.maximumBrushSize)), tip: strokeTip(a))
        let selected = capture.selection != nil
        let command = try PrecisionPixelCommand(["op": "dodge_burn", "layer": layer, "exposure": min(max(exposure, 0), 100) / 100,
            "range": range, "burn": boolArg(a, "burn") ?? false,
            "protect_tones": boolArg(a, "protect_tones") ?? false, "selection": selected])
        // One job whose worker rasterizes the footprint, as the 16-bit
        // canvas Dodge / Burn stroke commits.
        let work = try PrecisionStrokeWork(source: capture.source, stroke: native, command: command,
            selection: selected ? capture.selection?.maskSamples16() : nil,
            width: capture.facade.width, height: capture.facade.height)
        let job = try DocumentJobs.startPrecisionStroke(work, document: document, capture: capture, layer: layer,
            title: "Dodge / Burn", record: .notACommand, showProgress: false, rasterizesTarget: true,
            completion: Self.noteStrokeJobChange)
        // A float layer's pixels are f32, a CMYK layer's ink (#1).
        let storage = capture.source.isCmyk ? "cmyka16_unorm" : capture.facade.isFloat ? "rgba32_float" : "u16"
        job.details = ["storage": storage]
        return try finishJobCall(job, tool: "dodge_burn", arguments: a, unchanged: {
            try self.jsonResult(["ok": true, "changed": false, "storage": storage])
        })
    }
}
