import AppKit

/// The shared channel pieces the FROZEN AgentServer.swift needs, so each of
/// its call sites stays one line: the widened paint `target` vocabulary and
/// its routing, the coverage commit, apply_filter on a plane, a plane
/// loaded as the selection, `render {channel:…}` and `get_document`'s
/// channel list.
///
/// Mirrors the Channels panel's row selection
/// (ChannelsPanelViewController) and the editor's paint target
/// (EditorViewController+Channels). The eleven channel TOOLS themselves
/// live in AgentServer+Channels.swift.
extension AgentServer {
    // MARK: - The target vocabulary

    /// The paint tools' `target` argument, widened from layer|mask to
    /// layer|mask|red|green|blue|alpha|channel:<name>. `allowMask` is false
    /// only for commands whose own support table excludes masks.
    func paintTarget(
        _ a: [String: Any], _ document: ImageDocument, allowMask: Bool = true
    ) throws -> PaintTarget {
        let raw = stringArg(a, "target") ?? "layer"
        if raw == "layer" { return .layer }
        if raw == "mask" {
            guard allowMask else {
                throw Self.badTargetError(raw, allowMask: false, cmyk: document.doc?.isCmyk == true)
            }
            return .mask
        }
        // #547 Lab phase 2e: a Lab document's Lightness, a and b are the red, green and blue
        // planes of its codes, and alpha its own; red, green and blue are not its names.
        if document.doc?.isLab == true {
            if let plane = LabDocument.plane(named: raw) ?? (raw == "alpha" ? .alpha : nil) {
                return .plane(plane)
            }
            if ["red", "green", "blue"].contains(raw) {
                throw ToolError(message: LabDocument.planeTargetRefusal)
            }
        }
        if let plane = RasterPlane.named(raw), PaintTarget.paintablePlanes.contains(plane) {
            return .plane(plane)
        }
        // #547: one ink plate of a CMYK layer: the Move tool and ⌘T on its selected pixels, and
        // painting, filling, filtering and adjusting its stored `1 − ink` (`cmyk_plate_scalar`).
        if let plate = CmykPlane(key: raw) {
            guard document.doc?.isCmyk == true else {
                throw ToolError(message: "The \(raw) channel requires a CMYK document")
            }
            return .ink(plate)
        }
        if raw.hasPrefix("channel:") {
            let name = String(raw.dropFirst("channel:".count))
            guard let doc = document.doc, let index = doc.channelIndex(named: name) else {
                throw ToolError(
                    message: "No channel named \"\(name)\" (list_channels shows them)")
            }
            return .channel(index)
        }
        throw Self.badTargetError(raw, allowMask: allowMask, cmyk: document.doc?.isCmyk == true)
    }

    private static func badTargetError(_ got: String, allowMask: Bool, cmyk: Bool = false) -> ToolError {
        let mask = allowMask ? "\"mask\", " : ""
        // #547: a CMYK document's colour targets are its four ink plates.
        let planes = cmyk ? "\"cyan\", \"magenta\", \"yellow\", \"black\""
            : "\"red\", \"green\", \"blue\", \"alpha\""
        return ToolError(
            message: "target must be \"layer\", \(mask)\(planes) or \"channel:<name>\" "
                + "(got \"\(got)\")")
    }

    /// paintStroke's whole routing decision, lifted out of the frozen file:
    /// parses `target`, applies the ADJUSTMENT-LAYER forcing to PIXEL
    /// targets only (a `channel:` target is document state and is never
    /// retargeted — and it must not go through maskedLayerIndex, which
    /// validates a mask a channel has nothing to do with), picks the
    /// coverage colour (white brush / black eraser, opacity in the alpha),
    /// refuses `blend_mode` for every coverage target, and names the undo
    /// action. `note` is the in-band explanation when the adjustment
    /// routing moved a stroke the caller aimed elsewhere.
    func resolvePaintTarget(
        _ a: [String: Any], _ document: ImageDocument, requestedLayer: Int,
        erase: Bool, blend: DrBlendMode?
    ) throws -> (
        target: PaintTarget, layer: Int, actionName: String, color: NSColor, note: String?
    ) {
        let requested = try paintTarget(a, document)
        let isAdjustment = document.doc?.layerIsAdjustment(requestedLayer) == true
        // A CHANNEL edits document state, so an adjustment layer neither
        // forces it nor blocks it — the layer is not being painted at all.
        let forced = isAdjustment && !requested.isChannel
        if forced, document.doc?.layerHasMask(requestedLayer) != true {
            throw ToolError(
                message: "Layer \(requestedLayer) is an adjustment layer, so strokes paint "
                    + "its MASK — but its mask was deleted. Add one with add_layer_mask.")
        }
        let target: PaintTarget = forced ? .mask : requested
        let layer: Int
        if target == .mask {
            layer = try maskedLayerIndex(a, document, erase ? "erase" : "paint")
        } else {
            layer = requestedLayer
        }
        if target.isPlane || target.isInk {
            try rejectAdjustmentPixelEdit(document, layer)
        }
        if blend != nil, target.isCoverage {
            if forced, requested != .mask {
                // The caller DID target the layer; the adjustment routing
                // moved the stroke, so "use the layer target" would loop.
                throw ToolError(
                    message: "Layer \(layer) is an adjustment layer, so strokes always paint "
                        + "its MASK — blend_mode cannot apply there. Drop blend_mode, or "
                        + "stroke a pixel layer.")
            }
            throw ToolError(
                message: "blend_mode does not apply to a \(Self.coverageName(target)) stroke "
                    + "— coverage is not color. Stroke the layer target instead.")
        }
        // A coverage stroke carries its opacity in the stroke's own alpha:
        // the coverage ops take no separate alpha. Same clamp paintStroke
        // uses for the composite alpha, read from the same argument.
        let opacity = min(max(doubleArg(a, "opacity") ?? 1, 0), 1)
        let color: NSColor
        if target.isCoverage {
            let level: CGFloat = erase ? 0 : 1
            color = ColorProfile.coverageInk(
                NSColor(srgbRed: level, green: level, blue: level, alpha: CGFloat(opacity)),
                in: document.drawingSpace)
        } else if erase {
            color = .black
        } else {
            // Profile-qualified (AgentServer+QualifiedColor.swift): an
            // unqualified hex keeps today's authored-sRGB meaning, and the
            // result is always the document's own quantized numbers.
            color = try qualifiedColor(a, "color", in: document, fallback: .black)
        }
        let actionName: String
        if target.isCoverage {
            actionName = target.strokeActionName(erasing: erase, in: document.doc)
        } else {
            actionName = erase ? "Eraser Stroke" : "Brush Stroke"
        }
        let note: String? =
            forced && requested != .mask
            ? "Layer \(layer) is an adjustment layer (no pixels to paint), so the stroke was "
                + "routed to its mask."
            : nil
        return (target, layer, actionName, color, note)
    }

    private static func coverageName(_ target: PaintTarget) -> String {
        switch target {
        case .mask: return "mask"
        case .plane(let plane): return plane.displayName.lowercased() + " plane"
        case .channel: return "channel"
        case .layer: return "layer"
        case .ink(let plate): return plate.key + " plate"
        }
    }

    // MARK: - The commit

    /// Which layer's PIXELS an overlay edit rewrites — nil for a mask or a
    /// channel, where a text/shape description stays valid.
    func pixelLayer(for target: PaintTarget, layer: Int) -> Int? {
        switch target {
        case .layer, .plane, .ink: return layer
        case .mask, .channel: return nil
        }
    }

    /// The commit for a COVERAGE target — the branch paintOverlay used to
    /// spell as `if toMask`. All three take the same canvas-sized
    /// premultiplied overlay and the same lerp (white paints toward 255,
    /// black toward 0).
    ///
    /// A PLANE and a CHANNEL carry the core's `changed` latch: nil means no
    /// byte would move (white over white, or a selection that clipped the
    /// whole stroke away), which is a no-op to REPORT — `onRefusal` latches
    /// it so paintStroke answers `changed: false` instead of a parameter
    /// error. The MASK op has no such latch, so nil there is a real failure
    /// and is deliberately left to throw exactly as it does today.
    func commitCoverage(
        _ current: RasterDocument, target: PaintTarget, layer: Int,
        overlay: UnsafePointer<UInt8>, width: Int, height: Int,
        onRefusal: (() -> Void)? = nil
    ) -> RasterDocument? {
        switch target {
        case .layer, .ink:
            return nil
        case .mask:
            return current.paintingLayerMask(layer, overlay: overlay, w: width, h: height)
        case .plane(let plane):
            let out = current.paintingLayerPlane(
                layer, plane.dr, overlay: overlay, w: width, h: height)
            if out == nil { onRefusal?() }
            return out
        case .channel(let index):
            let out = current.paintingChannel(index, overlay: overlay, w: width, h: height)
            if out == nil { onRefusal?() }
            return out
        }
    }

    /// The in-band "nothing changed" result for a stroke the core refused
    /// because no pixel would move: an identity blend (Multiply by white), a
    /// coverage stroke that painted the coverage that was already there, or
    /// a stroke the active selection clipped away entirely. Never an error —
    /// `changed: false` says plainly that no undo step was added, which is
    /// what lets a model carry on instead of retrying the same call.
    func noOpStrokeResult(
        _ document: ImageDocument, target: PaintTarget, layer: Int, blend: DrBlendMode?
    ) throws -> String {
        let name = target.agentName(in: document.doc)
        let why: String
        if let blend = blend {
            why =
                "blend mode \(DrBlendMode.displayName(for: blend)) left every covered pixel "
                + "exactly as it was (an identity blend, like Multiply by white), or the "
                + "stroke never reached layer \(layer)'s pixels"
        } else {
            why =
                "the stroke painted \(name) exactly the coverage it already had, or the "
                + "active selection clipped it away entirely"
        }
        return try jsonResult([
            "ok": true, "changed": false, "layer": layer, "target": name,
            "note": "Nothing changed: \(why). No undo step was added.",
        ])
    }

    // MARK: - Fill and gradient on a plane

    /// `fill`'s arguments, parsed once: the seed, the colour as straight
    /// RGBA bytes IN THE DOCUMENT'S SPACE (`colorRGBA`, which is where that
    /// rule lives), and the region-grow options. Shared by the layer path in
    /// the frozen AgentServer and the plane path below, so the two spellings
    /// of `fill` can never drift — and the document is a parameter for
    /// exactly that reason: the colour cannot be read without it.
    struct FillArguments {
        let x: Int
        let y: Int
        let rgba: [UInt8]
        /// The colour's bytes for a coverage target (`coverageRGBA`).
        let coverage: [UInt8]
        let tolerance: Int
        let contiguous: Bool
    }

    /// A colour's straight RGBA bytes for a COVERAGE target to reduce to
    /// gray: read in `ColorProfile.coverageSpace`, so an authored white
    /// (`coverageColor`) stays 255 and black 0 on a LUT-mode document
    /// (`colorRGBA` elsewhere).
    func coverageRGBA(_ color: NSColor, in document: ImageDocument) throws -> [UInt8] {
        guard let bytes = ColorProfile.bytes(
            color, in: ColorProfile.coverageSpace(document.nsColorSpace, for: color))
        else { throw ToolError(message: "Could not convert the color") }
        return bytes
    }

    func fillArguments(_ a: [String: Any], _ document: ImageDocument) throws -> FillArguments {
        guard let x = intArg(a, "x"), let y = intArg(a, "y") else {
            throw ToolError(message: "fill requires x and y (the seed point)")
        }
        let color = try qualifiedColor(a, "color", in: document, fallback: .black)
        return FillArguments(
            x: x, y: y, rgba: try colorRGBA(color, in: document),
            coverage: try coverageRGBA(
                coverageColor(a, "color", in: document, fallback: .black), in: document),
            tolerance: intArg(a, "tolerance") ?? 32, contiguous: boolArg(a, "contiguous") ?? true)
    }

    /// `gradient`'s arguments, parsed once — same reason as `FillArguments`.
    struct GradientArguments {
        let from: CGPoint
        let to: CGPoint
        let start: [UInt8]
        let end: [UInt8]
        /// `start` and `end` for a coverage target (`coverageRGBA`).
        let startCoverage: [UInt8]
        let endCoverage: [UInt8]
        let kind: DrGradientKind
        /// The stops path: each stop's position with its document
        /// bytes, its coverage bytes and its midpoints, when the call
        /// names `stops` or a shape past radial. nil is the two-colour linear
        /// or radial op.
        let ramp: [(position: Double, rgba: [UInt8], coverage: [UInt8],
                    midpoint: GradientRamp.StopMidpoint)]?
        /// `blend_mode`: how each colour meets the layer's pixels.
        /// Normal is the gradient the tool has always painted.
        let mode: DrBlendMode
        /// `smoothness`, a fraction; below 1 the call has a `ramp`.
        let smoothness: Double

        /// The gradient on layer `layer`'s own pixels: the stops op with a
        /// ramp, the two-colour op without one. Under a mode other than
        /// Normal a two-colour ramp becomes its two stops, because only the
        /// stops op takes a mode.
        func painting(_ doc: RasterDocument, layer: Int, mask: [UInt8]?) -> RasterDocument? {
            let half = GradientRamp.StopMidpoint()
            guard let ramp = ramp ?? (mode == DR_BLEND_NORMAL
                ? nil : [(0, start, startCoverage, half), (1, end, endCoverage, half)])
            else {
                return doc.gradiented(
                    layer, from: from, to: to, start: start, end: end, kind: kind, mask: mask)
            }
            let stops = ramp.flatMap {
                [Float($0.position)] + $0.rgba.map { Float($0) / 255 }
                    + [Float($0.midpoint.color), Float($0.midpoint.opacity)]
            }
            return doc.gradientedStops(
                layer, from: from, to: to, stops: stops, kind: kind, mode: mode,
                smoothness: smoothness, mask: mask)
        }
    }

    /// One entry of `gradient`'s `stops`: its position, its opacity,
    /// `color` (the `["color": …, "color_space": …]` pair every colour helper
    /// here reads) and its midpoints.
    struct GradientStopArgument {
        let position: Double
        let opacity: Double
        let color: [String: Any]
        let midpoint: GradientRamp.StopMidpoint
    }

    /// A stop's `midpoint` and `opacity_midpoint`: absent or null is
    /// 0.5; present, a number 0.05…0.95 rounded to 0.01 (a whole
    /// percent), else refused as `<path>.<key> must be 0.05…0.95`. `path` is
    /// the stop's own (`stops[2]`, `gradient.stops[2]`). Shared by the
    /// `gradient` tool, the fill layers and `set_layer_style`.
    func stopMidpoint(_ entry: [String: Any], at path: String) throws -> GradientRamp.StopMidpoint {
        func read(_ key: String) throws -> Double {
            guard let given = entry[key], !(given is NSNull) else { return 0.5 }
            guard !Self.isJSONBoolean(given), let value = doubleArg(entry, key), value.isFinite,
                  GradientStop.midpointRange.contains(value)
            else { throw ToolError(message: "\(path).\(key) must be 0.05…0.95") }
            return (value * 100).rounded() / 100
        }
        return GradientRamp.StopMidpoint(
            color: try read("midpoint"), opacity: try read("opacity_midpoint"))
    }

    /// `gradient`'s `smoothness`: Smoothness as a fraction,
    /// 1 (100 %) the Classic ramp and 0 the linear one. Absent or JSON null
    /// is 1; a boolean, a non-number, a non-finite value or one outside 0…1
    /// is refused by name. Shared by the layer, plane, Quick Mask, typed and
    /// CMYK `gradient` handlers.
    func gradientSmoothnessArgument(_ a: [String: Any]) throws -> Double {
        guard let given = a["smoothness"], !(given is NSNull) else { return 1 }
        guard !Self.isJSONBoolean(given), let value = doubleArg(a, "smoothness"), value.isFinite,
              (0...1).contains(value)
        else { throw ToolError(message: "smoothness must be between 0 and 1") }
        return value
    }

    /// `gradient`'s `stops`: 2…32 `{position, color, opacity?, midpoint?,
    /// opacity_midpoint?}` objects, positions 0…1 and never decreasing,
    /// opacity 0…1 (default 1), midpoints 0.05…0.95 (default 0.5,
    /// `stopMidpoint`). nil when the call has none. The same parse at both
    /// depths.
    func gradientStopArguments(_ a: [String: Any]) throws -> [GradientStopArgument]? {
        guard let raw = a["stops"] else { return nil }
        guard let list = raw as? [[String: Any]], (2...32).contains(list.count) else {
            throw ToolError(message:
                "stops must be an array of 2 to 32 {position, color, opacity?} objects")
        }
        var previous = 0.0
        return try list.enumerated().map { index, stop in
            guard let position = doubleArg(stop, "position"), position.isFinite,
                  (0...1).contains(position), position >= previous
            else {
                throw ToolError(message: "stops[\(index)].position must be 0…1 and not below "
                    + "the stop before it")
            }
            previous = position
            // An `opacity` that is present but not a number is refused, never
            // read as the default 1.
            guard let opacity = stop["opacity"] == nil ? 1 : doubleArg(stop, "opacity"),
                  opacity.isFinite, (0...1).contains(opacity) else {
                throw ToolError(message: "stops[\(index)].opacity must be 0…1")
            }
            guard let hex = stringArg(stop, "color") else {
                throw ToolError(message: "stops[\(index)] requires color, a hex like #RRGGBB")
            }
            var color: [String: Any] = ["color": hex]
            if let space = a["color_space"] { color["color_space"] = space }
            return GradientStopArgument(
                position: position, opacity: opacity, color: color,
                midpoint: try stopMidpoint(stop, at: "stops[\(index)]"))
        }
    }

    /// `gradient` names its ramp: `start_color` or `stops`, and with
    /// `numericEndpoints` (16 and 32 bits) also `start_rgba`. A call with none
    /// of them is refused, where it used to paint the black-to-transparent
    /// default nobody asked for; the catalog already says "Give start_color
    /// or stops". Asked first by the layer, Quick Mask and typed handlers.
    /// A key whose value is JSON null names nothing, so it counts as absent:
    /// `start_color: null` alone is refused like a call with no ramp.
    func requireGradientRamp(_ a: [String: Any], numericEndpoints: Bool) throws {
        let keys = numericEndpoints ? ["start_color", "start_rgba", "stops"] : ["start_color", "stops"]
        guard keys.contains(where: { a[$0] != nil && !(a[$0] is NSNull) }) else {
            throw ToolError(message: numericEndpoints
                ? "gradient requires start_color, start_rgba or stops"
                : "gradient requires start_color or stops")
        }
    }

    func gradientArguments(
        _ a: [String: Any], _ document: ImageDocument
    ) throws -> GradientArguments {
        try requireGradientRamp(a, numericEndpoints: false)
        guard let x0 = doubleArg(a, "x0"), let y0 = doubleArg(a, "y0"),
            let x1 = doubleArg(a, "x1"), let y1 = doubleArg(a, "y1")
        else {
            throw ToolError(message: "gradient requires x0, y0, x1, y1")
        }
        let start = try colorRGBA(
            qualifiedColor(a, "start_color", in: document, fallback: .black), in: document)
        let startCoverage = try coverageRGBA(
            coverageColor(a, "start_color", in: document, fallback: .black), in: document)
        let end: [UInt8]
        let endCoverage: [UInt8]
        if stringArg(a, "end_color") != nil {
            end = try colorRGBA(
                qualifiedColor(a, "end_color", in: document, fallback: .clear), in: document)
            endCoverage = try coverageRGBA(
                coverageColor(a, "end_color", in: document, fallback: .clear), in: document)
        } else {
            end = [0, 0, 0, 0] // fade to transparent
            endCoverage = end
        }
        guard let kind = GradientRamp.kind(named: stringArg(a, "shape") ?? "linear") else {
            throw ToolError(message:
                "shape must be \"linear\", \"radial\", \"angle\", \"reflected\" or \"diamond\"")
        }
        let smoothness = try gradientSmoothnessArgument(a)
        var ramp: [(position: Double, rgba: [UInt8], coverage: [UInt8],
                    midpoint: GradientRamp.StopMidpoint)]?
        if let stops = try gradientStopArguments(a) {
            ramp = try stops.map { stop in
                // A stop's `opacity` scales its colour's alpha.
                let faded: ([UInt8]) -> [UInt8] = { bytes in
                    var out = bytes
                    out[3] = UInt8((Double(bytes[3]) * stop.opacity).rounded())
                    return out
                }
                return (
                    stop.position,
                    faded(try colorRGBA(
                        qualifiedColor(stop.color, "color", in: document, fallback: .black),
                        in: document)),
                    faded(try coverageRGBA(
                        coverageColor(stop.color, "color", in: document, fallback: .black),
                        in: document)),
                    stop.midpoint
                )
            }
        } else if (kind != DR_GRADIENT_LINEAR && kind != DR_GRADIENT_RADIAL) || smoothness < 1 {
            // The two-colour op has two shapes and no Smoothness; the other
            // three, and a Smoothness below 1, take the stops op with
            // the two colours as its ends.
            let half = GradientRamp.StopMidpoint()
            ramp = [(0, start, startCoverage, half), (1, end, endCoverage, half)]
        }
        return GradientArguments(
            from: CGPoint(x: x0, y: y0), to: CGPoint(x: x1, y: y1),
            start: start, end: end, startCoverage: startCoverage, endCoverage: endCoverage,
            kind: kind, ramp: ramp, mode: try blendModeArg(a) ?? DR_BLEND_NORMAL,
            smoothness: smoothness)
    }

    /// fill on a colour plane or an alpha channel — the MCP mirror of the
    /// Fill tool's redirect in EditorViewController+PlanePaint. The colour
    /// enters as coverage gray (`PlaneAlgebra.coverageColor`) and the region
    /// grow is the EXISTING `bucketFilled`, run on the scratch document the
    /// plane becomes.
    func fillPlane(
        _ document: ImageDocument, target: PaintTarget, layer: Int, _ f: FillArguments,
        mask: [UInt8]?
    ) throws -> String {
        guard let rgba = PlaneAlgebra.coverageColor(f.coverage) else {
            throw ToolError(message: "Could not read that color as coverage")
        }
        return try planeScratchEdit(
            document, target: target, layer: layer, actionName: "Fill",
            why: "the plane already held exactly that value everywhere the fill reached",
            mask: mask,
            .fill(
                x: f.x, y: f.y, rgba: rgba, tolerance: f.tolerance,
                contiguous: f.contiguous))
    }

    /// gradient on a colour plane or an alpha channel — the same mirror, for
    /// the Gradient tool's redirect.
    func gradientPlane(
        _ document: ImageDocument, target: PaintTarget, layer: Int, _ g: GradientArguments,
        mask: [UInt8]?
    ) throws -> String {
        // A blend mode composites into a layer's pixels; a mask, plane
        // or channel holds coverage. Mirrors the Gradient tool's own refusal
        // (EditorViewController.refuseGradientModeOnCoverage).
        guard g.mode == DR_BLEND_NORMAL else {
            throw ToolError(message: GradientRamp.coverageTargetRefusal)
        }
        guard let start = PlaneAlgebra.coverageColor(g.startCoverage),
              let end = PlaneAlgebra.coverageColor(g.endCoverage)
        else {
            throw ToolError(message: "Could not read those colors as coverage")
        }
        var stops: [Float]?
        if let ramp = g.ramp {
            var flat: [Float] = []
            for stop in ramp {
                guard let gray = PlaneAlgebra.coverageColor(stop.coverage) else {
                    throw ToolError(message: "Could not read those colors as coverage")
                }
                flat.append(Float(stop.position))
                flat.append(contentsOf: gray.map { Float($0) / 255 })
                flat.append(Float(stop.midpoint.color))
                flat.append(Float(stop.midpoint.opacity))
            }
            stops = flat
        }
        return try planeScratchEdit(
            document, target: target, layer: layer, actionName: "Gradient",
            why: "the plane already held exactly that ramp", mask: mask,
            .gradient(
                from: g.from, to: g.to, start: start, end: end, kind: g.kind, stops: stops,
                smoothness: g.smoothness))
    }

    /// The scratch-document round trip as an MCP result: read the target
    /// plane in ITS OWN space (a colour plane at the layer's size, a channel
    /// canvas-sized), run an existing document op on its one layer with the
    /// canvas geometry and the selection mask mapped into that space
    /// (`PlaneScratchOp`), write the result's luma back the same way — one
    /// undo step, with a core refusal reported in-band as `changed: false`
    /// exactly as the strokes and apply_filter report theirs.
    private func planeScratchEdit(
        _ document: ImageDocument, target: PaintTarget, layer: Int, actionName: String,
        why: String, mask: [UInt8]?, _ op: PlaneScratchOp
    ) throws -> String {
        let operation: OperationScope = actionName == "Fill" ? .fill : .gradient
        let capture = try operationCapture(document, operation: operation, target: target, layer: layer)
        return try commitScopedResult(
            document, capture: capture, updated: capture.applyingScratch(op), actionName: actionName)
    }

    // MARK: - Selections from a plane

    /// A canvas-sized plane loaded as the selection. An ALL-ZERO plane is
    /// not an error: `CanvasSelection.init?` returns nil for it and
    /// applySelection would throw "The selection would be empty." — so this
    /// builds the selection itself and routes a nil straight to
    /// setCombined's `selection_empty` deselect, exactly as the UI's Load
    /// Selection does. Selection history is undoable without file-dirty changes.
    func applyPlaneSelection(
        _ document: ImageDocument, _ plane: [UInt8], mode: SelectionCombineMode,
        extra: [String: Any] = [:]
    ) throws -> String {
        guard let editorVC = editor(document) else {
            throw ToolError(message: "The document has no editor window to hold a selection.")
        }
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard
            let selection = CanvasSelection(
                shape: .mask(plane), canvasWidth: doc.width, canvasHeight: doc.height)
        else {
            // Nothing to combine with: replace and intersect empty the
            // selection, add and subtract leave it exactly as it was.
            switch mode {
            case .replace, .intersect:
                return try setCombined(editorVC, nil, extra: extra)
            case .add, .subtract:
                return try setCombined(editorVC, editorVC.agentSelection, extra: extra)
            }
        }
        return try setCombined(
            editorVC,
            CanvasSelection.combine(editorVC.agentSelection, with: selection, mode: mode),
            extra: extra)
    }

    // MARK: - Reporting

    /// `render`'s optional `channel` argument → that plane as an opaque
    /// grayscale image, plus the label the result is described by. With
    /// `layer` it is that layer's plane (canvas-sized, 0 outside the layer's
    /// rect); otherwise the flattened composite's.
    ///
    /// The label comes back from HERE because only this function knows what
    /// the string resolved to. `layer` selects a COLOUR plane's source layer
    /// and means nothing for an alpha channel — a channel is document state,
    /// the same plane whatever layer is named — so calling one "Sky plane of
    /// layer 0" and the next "Sky plane of layer 1" handed a model two
    /// identical PNGs described as two different layers' planes, from which
    /// "the layers are identical" is a reasonable conclusion.
    static func planeRenderImage(
        _ doc: RasterDocument, channel: String, layer: Int?
    ) throws -> (image: RasterImage?, what: String) {
        // #547 Indexed phase 3c: an alpha channel by name with no layer; the composite and the
        // Index channel never reach here (`render` draws `IndexedDocument.compositeChannels` as the
        // composite), and anything else refuses with `IndexedDocument.renderRefusal`.
        if doc.isIndexed {
            if layer == nil, RasterPlane.named(channel) == nil, doc.channelIndex(named: channel) != nil {
                return try nativePlaneRenderImage(doc, channel: channel, layer: nil)
            }
            throw ToolError(message: IndexedDocument.renderRefusal)
        }
        // #547: a CMYK document's ink planes, the composite on paper, ink dark (Channels panel's view).
        if let precision = doc.precision, precision.isCmyk {
            guard layer == nil, let plane = CmykPlane(key: channel) else {
                throw ToolError(message: "A CMYK document renders one ink plane of its composite: "
                    + "channel is cyan, magenta, yellow or black, with no layer.")
            }
            let context = try PrecisionWorkContext(source: precision)
            return (try precision.cmykPlaneView(plane, maxSide: 0, workingLimit: context.limit,
                                                otherRetained: context.otherRetained), "\(channel) ink plane")
        }
        // #547 Lab phase 1c: a Lab Color document's lightness, a or b channel of the composite's
        // codes (`labPlaneDisplay`, never the P3 view), a layer's mask, or an alpha channel by name
        // with no layer; anything else refuses with `LabDocument.renderRefusal`.
        if doc.isLab {
            if layer == nil, let plane = LabDocument.plane(named: channel) {
                return (try doc.labPlaneDisplay(plane, maxSide: 0),
                        "8-bit display of native \(channel) of the Lab composite")
            }
            if channel == "mask", layer != nil {
                return try nativePlaneRenderImage(doc, channel: channel, layer: layer)
            }
            if layer == nil, RasterPlane.named(channel) == nil, doc.channelIndex(named: channel) != nil {
                return try nativePlaneRenderImage(doc, channel: channel, layer: nil)
            }
            throw ToolError(message: LabDocument.renderRefusal)
        }
        // #547 phase 1c: a Grayscale document's one gray plane (its R = G = B), a layer's mask, or
        // (phase 2d) an alpha channel; `GrayDocument.refusal` has refused every other channel. The plane is the red
        // plane of the native display (the twin's codes are the gray codes; at 32 bits the tone
        // map's), as `nativeSourcePlaneDisplay` reads it at float: the native plane read is a
        // pixel-derived selection operation, which the gray gate holds until phase 4.
        if doc.isGray {
            if channel == "gray", layer == nil {
                let what = doc.isFloat ? "8-bit tone-mapped display of gray composite"
                    : "8-bit display of native gray composite"
                guard let plane = try doc.nativeDisplay(layer: nil).planeImage(DR_PLANE_RED, maxSide: 0)
                else { throw ToolError(message: "Could not extract the gray plane.") }
                return (plane, what)
            }
            // Phase 2d: an alpha channel by name, document state with no layer (the core's
            // `read_plane` of target channel).
            if layer == nil, channel != "mask", RasterPlane.named(channel) == nil,
               doc.channelIndex(named: channel) != nil {
                return try nativePlaneRenderImage(doc, channel: channel, layer: nil)
            }
            guard channel == "mask", layer != nil else {
                throw ToolError(message: GrayDocument.renderRefusal)
            }
            return try nativePlaneRenderImage(doc, channel: channel, layer: layer)
        }
        if doc.isHighDepth { return try nativePlaneRenderImage(doc, channel: channel, layer: layer) }
        if let layer = layer, layer < 0 || layer >= doc.layerCount {
            throw ToolError(
                message: "Layer \(layer) is out of range (0..\(doc.layerCount - 1))")
        }
        if channel == "mask" {
            guard let layer = layer, doc.layerHasMask(layer) else {
                throw ToolError(message: "Rendering a mask requires a layer with a mask")
            }
            return (doc.layerPlaneImage(layer, DR_PLANE_MASK, maxSide: 0), "mask of layer \(layer)")
        }
        if let plane = RasterPlane.named(channel), plane != .mask {
            if let layer = layer {
                return (
                    doc.layerPlaneImage(layer, plane.dr, maxSide: 0),
                    "\(channel) plane of layer \(layer)"
                )
            }
            return (doc.compositePlaneImage(plane.dr, maxSide: 0), "\(channel) plane")
        }
        guard let index = doc.channelIndex(named: channel) else {
            throw ToolError(
                message: "channel must be \"red\", \"green\", \"blue\", \"alpha\", \"luma\" or "
                    + "an alpha channel's name (list_channels shows them) — got "
                    + "\"\(channel)\"")
        }
        return (doc.channelImage(index, maxSide: 0), "channel \"\(channel)\"")
    }

    /// get_document's (and list_channels') channel list.
    static func channelFields(_ doc: RasterDocument) -> [[String: Any]] {
        // At 16/32 bits each row also keeps the core row's id, dimensions
        // and provenance, merged by index, so get_document and list_channels
        // answer the same rows at every depth.
        let coreChannels = doc.precisionInfo?["channels"] as? [[String: Any]] ?? []
        return (0..<doc.channelCount).compactMap { index -> [String: Any]? in
            guard let info = doc.channelInfo(index) else { return nil }
            var row: [String: Any] = [
                "index": index,
                "name": info.name,
                "overlay_color": RasterImage.hexString(
                    (r: info.red, g: info.green, b: info.blue, a: 255)),
                "overlay_opacity": (info.opacity * 100).rounded() / 100,
                "color_indicates": info.colorIndicatesSelected ? "selected" : "masked",
                // #547: a spot channel's ink, {lab, solidity}; null for an alpha channel.
                "spot": info.spot.map { $0.fields as Any } ?? NSNull(),
            ]
            if coreChannels.indices.contains(index) {
                for key in ["id", "dimensions", "provenance"] {
                    row[key] = coreChannels[index][key] ?? NSNull()
                }
            }
            return row
        }
    }

    // MARK: - Shared argument parsing

    /// A `channel` argument: a channel NAME or an index, either spelling.
    ///
    /// The NAME is tried first. Channel names are free-form, so "7" is a
    /// legal one (`rename_channel` and `add_channel` both accept it), while
    /// `intArg` accepts the string form "7" as an index — parsing the index
    /// first made a channel named after a number reachable only by its
    /// position, and reported the failure as a range error that named the
    /// wrong problem. A real JSON number still means the index, and so does a
    /// string that names no channel, so `channel: "3"` keeps its meaning
    /// wherever nothing is called "3".
    func channelArgument(
        _ a: [String: Any], _ doc: RasterDocument, key: String = "channel"
    ) throws -> Int {
        if let name = stringArg(a, key), let index = doc.channelIndex(named: name) {
            return index
        }
        if let index = intArg(a, key) {
            guard index >= 0, index < doc.channelCount else {
                // An EMPTY list is the common case here and needs its own
                // sentence: the range form would read "(0..-1)", which sends a
                // model looking for the index that fits instead of telling it
                // there is no channel to address at all. Otherwise both
                // spellings are named, so the ambiguity is visible when
                // neither resolves.
                guard doc.channelCount > 0 else {
                    throw ToolError(
                        message: "This document has no alpha channels, so \(key) \(index) "
                            + "names nothing. Create one with add_channel or save_selection.")
                }
                throw ToolError(
                    message: "Channel \(index) is out of range (0..\(doc.channelCount - 1)), "
                        + "and no channel is named \"\(index)\" (list_channels shows them)")
            }
            return index
        }
        guard let name = stringArg(a, key) else {
            throw ToolError(message: "Missing required argument: \(key)")
        }
        throw ToolError(message: "No channel named \"\(name)\" (list_channels shows them)")
    }

    /// A source layer argument: an index, or nil for "merged" (the
    /// flattened composite) — the default when the key is absent.
    ///
    /// A GROUP index is legal here and means its PROJECTION: the layers
    /// inside it composited together, canvas-sized, without the group's own
    /// opacity, blend mode, mask or style. Reading a group is not editing
    /// one, so there is no kind guard — the core answers every plane of a
    /// group from that projection (`layer_plane`), and a group is therefore
    /// the cheapest way to say "everything in this part of the stack" without
    /// first merging it.
    func sourceLayer(
        _ a: [String: Any], _ doc: RasterDocument, key: String
    ) throws -> Int? {
        if let index = intArg(a, key) {
            guard index >= 0, index < doc.layerCount else {
                throw ToolError(
                    message: "Layer \(index) is out of range (0..\(doc.layerCount - 1))")
            }
            return index
        }
        guard let name = stringArg(a, key) else { return nil }
        guard name == "merged" else {
            throw ToolError(
                message: "\(key) must be \"merged\" or a layer index (got \"\(name)\")")
        }
        return nil
    }

    /// A source-plane argument for apply_image / calculations: "rgb",
    /// a colour plane, or an alpha channel's name. On a CMYK document (#547):
    /// "cmyk" (the composite), an ink plate, "luma", "alpha" or a channel's name; on a Grayscale
    /// document (phase 6b) "gray", "alpha" or a channel's name; on a Lab Color document (Lab
    /// phase 6b-i) "lab", "lightness", "a", "b", "alpha" or a channel's name.
    func planeChoice(
        _ a: [String: Any], _ doc: RasterDocument, key: String
    ) throws -> PlaneChoice {
        guard let raw = stringArg(a, key) else { return .rgb }
        if doc.isCmyk {
            if raw == "cmyk" { return .rgb }
            if let plate = CmykPlane(key: raw) { return .ink(plate) }
            if let plane = RasterPlane.named(raw), [.luma, .alpha].contains(plane) {
                return .plane(plane)
            }
            if !["rgb", "red", "green", "blue"].contains(raw), let index = doc.channelIndex(named: raw) {
                return .channel(index)
            }
            throw ToolError(
                message: "\(key) must be \"cmyk\", \"cyan\", \"magenta\", \"yellow\", \"black\", "
                    + "\"luma\", \"alpha\" or an alpha channel's name on a CMYK document "
                    + "(got \"\(raw)\")")
        }
        // #547 phase 6b: a Grayscale document's sources are its Gray composite or layer (R = G =
        // B, so the RGB choice), its alpha and its alpha channels; a colour plane is not its name.
        if doc.isGray {
            if raw == "gray" { return .rgb }
            if raw == "alpha" { return .plane(.alpha) }
            if !["rgb", "red", "green", "blue", "luma"].contains(raw),
               let index = doc.channelIndex(named: raw) {
                return .channel(index)
            }
            throw ToolError(
                message: "\(key) must be \"gray\", \"alpha\" or an alpha channel's name on a "
                    + "Grayscale document (got \"\(raw)\")")
        }
        // #547 Lab phase 6b-i: the Lab composite or layer, Lightness, a, b, alpha and the alpha
        // channels (`LabDocument.planeChoice`).
        if doc.isLab { return try LabDocument.planeChoice(raw, in: doc, key: key) }
        if raw == "rgb" { return .rgb }
        if let plane = RasterPlane.named(raw), plane != .mask { return .plane(plane) }
        guard let index = doc.channelIndex(named: raw) else {
            throw ToolError(
                message: "\(key) must be \"rgb\", \"red\", \"green\", \"blue\", \"luma\", "
                    + "\"alpha\" or an alpha channel's name (got \"\(raw)\")")
        }
        return .channel(index)
    }

    /// Where load_selection reads its plane from. `prefix` lets a tool with
    /// two source blocks spell them apart; load_selection passes "".
    ///
    /// Both layer forms accept a GROUP index. `layer_alpha` on a group is the
    /// alpha of its projection — the combined silhouette of everything inside
    /// it, which is exactly what "select this group's shape" means — and
    /// `layer_mask` is the group's own mask, which unlike a raster layer's is
    /// CANVAS-sized to begin with and so needs no placement.
    func selectionSource(
        _ a: [String: Any], _ document: ImageDocument, prefix: String = ""
    ) throws -> SelectionSource {
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let from = stringArg(a, prefix + "from") ?? "channel"
        switch from {
        case "channel":
            return .channel(try channelArgument(a, doc, key: prefix + "channel"))
        case "layer_alpha", "layer_mask":
            let index = intArg(a, prefix + "layer") ?? document.activeLayerIndex
            guard index >= 0, index < doc.layerCount else {
                throw ToolError(
                    message: "Layer \(index) is out of range (0..\(doc.layerCount - 1))")
            }
            if from == "layer_alpha" { return .layerAlpha(index) }
            guard doc.layerHasMask(index) else {
                throw ToolError(
                    message: "Layer \(index) has no mask to load. Add one with add_layer_mask.")
            }
            return .layerMask(index)
        case "plane":
            let name = stringArg(a, prefix + "plane") ?? "luma"
            // #547: a CMYK document's planes are its four inks (each its stored `1 − ink`), its
            // luminosity and its alpha; an RGB document's are its colour planes.
            if let plate = CmykPlane(key: name) {
                guard doc.isCmyk else {
                    throw ToolError(
                        message: "plane \"\(name)\" is a CMYK plane; an RGB document's planes are "
                            + "red, green, blue, alpha and luma")
                }
                return .inkPlate(plate)
            }
            // #547 phase 4: a Grayscale document's one colour plane, Gray, is its composite's own
            // gray (R = G = B), read as the luminosity.
            if doc.isGray, name == "gray" { return .compositePlane(.luma) }
            // #547 Lab phase 4b-i: a Lab document's planes are Lightness, a and b (the stored red,
            // green and blue codes), luma (its Lightness: `canvasPlane`) and alpha.
            if doc.isLab, name != "luma", name != "alpha" {
                guard let plane = LabDocument.plane(named: name) else {
                    throw ToolError(message: LabDocument.loadPlaneRefusal)
                }
                return .compositePlane(plane)
            }
            if doc.isCmyk, let plane = RasterPlane.named(name), [.red, .green, .blue].contains(plane) {
                throw ToolError(
                    message: "plane \"\(name)\" is an RGB plane; a CMYK document's planes are cyan, "
                        + "magenta, yellow, black, luma and alpha")
            }
            guard let plane = RasterPlane.named(name), plane != .mask else {
                throw ToolError(
                    message: "plane must be \"red\", \"green\", \"blue\", \"alpha\" or "
                        + "\"luma\" (got \"\(name)\")")
            }
            return .compositePlane(plane)
        case let other:
            throw ToolError(
                message: "from must be \"channel\", \"layer_alpha\", \"layer_mask\" or "
                    + "\"plane\" (got \"\(other)\")")
        }
    }
}
