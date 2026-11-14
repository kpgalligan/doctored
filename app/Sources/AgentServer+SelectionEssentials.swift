import AppKit

/// Mirrors the Freehand Lasso/Magnetic Lasso/Quick Selection tools and Select >
/// Color Range / Transform Selection. All paths share capture validation and
/// selection history.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let selectionEssentialsHandlers: [String: ToolHandler] = [
        "select_freehand": { $0.selectFreehand },
        "select_magnetic": { $0.selectMagnetic },
        "select_color_range": { $0.selectColorRange },
        "transform_selection": { $0.transformSelection },
        "quick_select": { $0.quickSelect },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let selectionEssentialsToolFacts: [String: ToolFacts] = [
        // Parity with the float menu/tool whitelist in
        // `EditorViewController+Precision.swift`: a selection is canvas
        // coverage, not document pixels, so everything the UI lets a user do
        // with one the agent answers too (including Save/Load Selection,
        // whose `channel_set` family is admitted at depth 32).
        "select_freehand": [.commonTool, .floatTool, .cmykTool],
        // `select_magnetic` records the RESOLVED `select_freehand` outline
        // itself (the Magnetic Lasso tool's step), so a replay is independent
        // of the image it traced and the generic recorder must not add a copy.
        "select_magnetic": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        // Color Range and Quick Selection compare the R1 display copy at float.
        "select_color_range": [.commonTool, .floatTool, .cmykTool],
        "transform_selection": [.selectionSensitive, .commonTool, .cmykTool],
        "quick_select": [.commonTool, .floatTool, .cmykTool],
    ]

    var selectionEssentialsCatalog: [[String: Any]] {
        let doc = Self.docIDProperty
        let number: [String: Any] = ["type": "number"]
        let mode: [String: Any] = ["type": "string", "enum": ["replace", "add", "subtract", "intersect"]]
        let point: [String: Any] = ["type": "array", "items": number, "minItems": 2, "maxItems": 2]
        return [
            tool("select_freehand", "Close a freehand path into a selection. Points use canvas coordinates; selection-only, one Undo. anti_alias false gives a hard 0/255 edge. Mirrors the Freehand Lasso tool.",
                ["document_id": doc, "points": ["type": "array", "items": point, "minItems": 3, "maxItems": 10000],
                 "mode": mode, "feather": number,
                 "anti_alias": ["type": "boolean", "description": "Soften the edge (default true); false selects each pixel whose coverage is at least half, fully."]],
                required: ["points"]),
            tool("select_magnetic", "Trace a Magnetic Lasso outline along image edges and close it into a selection. points (2–10,000 canvas [x, y]) replay the pointer: the first is the starting click, every later one a pointer move, except the indices listed in anchors (strictly increasing, 1…n−1), which are clicks that fasten an anchor there. width is the edge-detection radius in px (1–256, default 10), contrast the edge strength in % below which a gradient is not an edge (1–100, default 10), frequency how soon anchors fasten by themselves (0–100, default 57; 0 fastens only when a move's work budget runs out or the points loop back within 2·width of the last anchor after going farther around an enclosed area, not retracing their way; at width ≥ ~205 every move fastens). Moves closer than max(1, width/4) px to the previous one are coalesced (the last point never is). close \"magnetic\" (default) traces back to the start, \"straight\" joins it straight. Time grows with the number of points, and each point costs more at low frequency. Recorded as the resolved select_freehand outline; the reply adds outline_points. One Undo. In a 32-bit float document the edges are traced on the composite's display codes (the profile's curve over values clamped to 0–1, never the tone map or the view exposure), so a value above white traces as white. Mirrors the Magnetic Lasso tool.",
                ["document_id": doc, "points": ["type": "array", "items": point, "minItems": 2, "maxItems": 10000],
                 "anchors": ["type": "array", "items": ["type": "integer", "minimum": 1]],
                 "width": number, "contrast": number, "frequency": number,
                 "close": ["type": "string", "enum": ["magnetic", "straight"]],
                 "mode": mode, "feather": number], required: ["points"]),
            tool("select_color_range", "Select sampled colors, a hue preset, skin tones or a tonal range from the composite. RGB samples are encoded document-profile values. Tolerance is Lab ΔE76 (0–200); softness is 0–100. Tonal presets use L*: shadows0–35, midtones35–70, highlights70–100. The hue presets reds, yellows, greens, cyans, blues and magentas select each pixel by its share of that hue: a primary (R, G, B) is max(0, channel − the larger other), a secondary (Y, C, M) is max(0, the smaller of its two − the third), times (2 − the largest channel) and capped at full, over the encoded values (0–1) (at 32 bits the display codes; on a CMYK document its view); they take no samples, tolerance or softness. skin_tones selects skin colours: ΔE76 in the document-profile Lab from (70, 18, 19), full within 18 and fading to none over a further 0.25 × fuzziness (0–200, default 40; 0 is a hard edge), over the encoded values (at 32 bits the display codes; on a CMYK document its view); it does not match Photoshop's Skin Tones (measured), takes no samples, tolerance or softness, and is the only range fuzziness applies to. out_of_gamut selects the pixels View > Gamut Warning flags: colours the window's proof profile (else the default proof preset, else Working CMYK) cannot hold, ΔE76 above 5 between a colour and its relative colorimetric round trip through that profile, over the same 8-bit codes the warning proofs (the display codes at 16 and 32 bits). It is all or nothing and takes no samples, tolerance or softness; a CMYK document is all in gamut, so it selects nothing (everything with invert). Its reply adds proof_profile. invert (default false) selects the complement. localized (Localized Color Clusters, sampled only; needs points: one canvas [x, y] per sample, in order) weights each sample by 1 − d/R, d the pixel's distance to its point and R cluster_range (0–100, default 100) % of the canvas's longer side. Zero-alpha pixels stay outside. Mirrors Select > Color Range.",
                ["document_id": doc, "range": ["type": "string", "enum": ["sampled"] + ColorRangeParameters.hueRanges + ["shadows", "midtones", "highlights", ColorRangeParameters.skinTones, ColorRangeGamut.range, "tonal"]],
                 "fuzziness": ["type": "number", "minimum": 0, "maximum": 200],
                 "samples": ["type": "array", "items": ["type": "array", "items": ["type": "integer", "minimum": 0, "maximum": 255], "minItems": 3, "maxItems": 3], "maxItems": 32],
                 "samples16": ["type": "array", "items": ["type": "array", "items": ["type": "integer", "minimum": 0, "maximum": 65535], "minItems": 3, "maxItems": 3], "maxItems": 32],
                 "tolerance": number, "softness": number, "lower": number, "upper": number,
                 "invert": ["type": "boolean"], "localized": ["type": "boolean"],
                 "cluster_range": ["type": "number", "minimum": 0, "maximum": 100],
                 "points": ["type": "array", "items": ["type": "array", "items": ["type": "integer", "minimum": 0], "minItems": 2, "maxItems": 2], "maxItems": 32],
                 "mode": mode]),
            tool("quick_select", "Grow edge-aware connected color regions from 1–4096 integer canvas seed pixels. Tolerance and edge are Lab ΔE76 limits0–200; transparent pixels form barriers. brush_size (1–512 px, default 1) paints each seed as a disc of that diameter: everything opaque under it is selected and grows outward, and an enclosed gap smaller than one dab is filled. seeds × brush_size² must stay under 64,000,000, and seeds closer together than a quarter of the brush size add little. smooth (0–4, default 0) averages colour over a (2·smooth+1)² window without crossing an edge, so noise and texture do not stop the growth; the tool uses 1. enhance_edge (default false) then refines the result's edge against the picture and smooths it. Omitting all three grows from the seed pixels alone. Uses the captured composite and retains its alpha. In a 32-bit float document the distances and enhance_edge read the composite's display codes (the profile's curve over values clamped to 0–1), so a value above white compares as white. Mirrors dragging the Quick Selection tool.",
                ["document_id": doc, "seeds": ["type": "array", "items": point, "minItems": 1, "maxItems": 4096],
                 "tolerance": number, "edge": number,
                 "brush_size": ["type": "number", "minimum": 1, "maximum": 512],
                 "smooth": ["type": "integer", "minimum": 0, "maximum": 4],
                 "enhance_edge": ["type": "boolean"],
                 "mode": mode, "feather": number], required: ["seeds"]),
            tool("transform_selection", "Transform current coverage only; layer pixels stay fixed. Translation uses px, scales use factors (negative flips), angle is clockwise degrees, default anchor is selection-bounds center. Outside canvas becomes zero. Mirrors Select > Transform Selection.",
                ["document_id": doc, "translate_x": number, "translate_y": number,
                 "scale_x": number, "scale_y": number, "angle": number, "anchor_x": number, "anchor_y": number,
                 "filter": ["type": "string", "enum": ["nearest", "bilinear", "bicubic", "lanczos3"]]]),
        ]
    }

    func selectionNumber(_ a: [String: Any], _ key: String, default fallback: Double) throws -> Double {
        guard let raw = a[key] else { return fallback }
        guard !Self.isJSONBoolean(raw), let value = doubleArg(a, key), value.isFinite else {
            throw ToolError(message: "\(key) must be a finite number.")
        }
        return value
    }

    private func selectionResult(_ document: ImageDocument) throws -> String {
        try jsonResult(selectionReplyFields(document))
    }

    func selectionReplyFields(_ document: ImageDocument) -> [String: Any] {
        guard let selection = document.canvasSelection else {
            return ["ok": true, "selection_empty": true]
        }
        return ["ok": true, "selection_empty": false,
            "outline_simplified": selection.contourIsSimplified,
            "outline_note": selection.outlineNotice ?? "Selection contour", "bounds": [
            "x": Int(selection.bounds.minX), "y": Int(selection.bounds.minY),
            "width": Int(selection.bounds.width), "height": Int(selection.bounds.height)]]
    }

    func selectFreehand(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let capture = try SelectionCapture(document: document, needsSource: false)
        let points = try parsePoints(a)
        guard points.count >= 3 else { throw ToolError(message: "A freehand selection needs at least 3 points.") }
        try capture.commit(.polygon(points), in: document, name: "Freehand Lasso",
            mode: selectionMode(a), feather: selectionNumber(a, "feather", default: 0),
            antiAlias: boolArg(a, "anti_alias") ?? true)
        return try selectionResult(document)
    }

    /// `anti_alias` (default true) for `select_ellipse` and `select_polygon`:
    /// false hardens the shape to 0/255 coverage, as the options bar's
    /// Anti-alias checkbox does for the marquee and lasso tools.
    func antiAliased(
        _ a: [String: Any], _ shape: CanvasSelection.Shape, _ document: ImageDocument
    ) -> CanvasSelection.Shape {
        guard boolArg(a, "anti_alias") == false, let doc = document.doc else { return shape }
        return shape.aliased(canvasWidth: doc.width, canvasHeight: doc.height)
    }

    /// `select_magic_wand`'s `sample_all_layers` (default true) and `layer`
    /// (default the active layer): the layer the wand reads alone, or nil for
    /// the composite. Mirrors the wand's Sample popup.
    func wandSampleLayer(_ a: [String: Any], _ document: ImageDocument) throws -> Int? {
        guard boolArg(a, "sample_all_layers") == false else {
            guard a["layer"] == nil || a["layer"] is NSNull else {
                throw ToolError(
                    message: "layer applies only with sample_all_layers: false; "
                        + "the wand reads the composite otherwise.")
            }
            return nil
        }
        return try structuralLayerIndex(a, document)
    }

    /// `select_magic_wand`'s `sample_size` (default 1, Point Sample):
    /// one of the wand's Sample Size popup values, at every depth.
    func wandSampleSize(_ a: [String: Any]) throws -> Int {
        guard let raw = a["sample_size"], !(raw is NSNull) else { return 1 }
        let value = try selectionNumber(a, "sample_size", default: 1)
        guard let size = SelectToolOptions.wandSampleSizes.first(where: { Double($0) == value }) else {
            throw ToolError(message: "sample_size must be 1, 3, 5, 11, 31, 51 or 101.")
        }
        return size
    }

    /// Mirrors the Magnetic Lasso tool: the very session the canvas drives
    /// (`MagneticLassoSession`), fed the points as its clicks and pointer
    /// moves, closed and committed as one "Magnetic Lasso" step recorded as the
    /// resolved `select_freehand` outline (`select_magnetic` itself is
    /// `nonRecordable`). There is no cumulative work cap: each trace is bounded
    /// by the session's own fasten at the work budget, as in the UI, so the
    /// total work grows linearly with the number of points.
    func selectMagnetic(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let capture = try SelectionCapture(document: document, edgeTrace: true)
        guard capture.source != nil else { throw ToolError(message: "The image has no projection to trace.") }
        let points = try parsePoints(a)
        guard points.count >= 2 else {
            throw ToolError(message: "select_magnetic needs at least 2 points: the starting click and one more.")
        }
        let width = CGFloat(capture.doc.width), height = CGFloat(capture.doc.height)
        guard points.allSatisfy({ (0...width).contains($0.x) && (0...height).contains($0.y) }) else {
            throw ToolError(message: "select_magnetic points must lie inside the \(capture.doc.width)×\(capture.doc.height) canvas.")
        }
        var anchors: [Int] = []
        if let raw = a["anchors"], !(raw is NSNull) {
            guard let list = raw as? [Any] else { throw ToolError(message: "anchors must be an array of point indices.") }
            anchors = try list.map { value in
                let index = try selectionNumber(["anchor": value], "anchor", default: -1)
                guard index.rounded(.towardZero) == index, index >= 1, index < Double(points.count) else {
                    throw ToolError(message: "anchors must be integer point indices from 1 to \(points.count - 1).")
                }
                return Int(index)
            }
            guard zip(anchors, anchors.dropFirst()).allSatisfy({ $0 < $1 }) else {
                throw ToolError(message: "anchors must be strictly increasing.")
            }
        }
        let defaults = MagneticLassoOptions.defaults
        let options = MagneticLassoOptions(
            width: try selectionNumber(a, "width", default: defaults.width),
            contrast: try selectionNumber(a, "contrast", default: defaults.contrast),
            frequency: try selectionNumber(a, "frequency", default: defaults.frequency))
        guard options.isValid else {
            throw ToolError(message: "width must be 1–256 px, contrast 1–100 % and frequency 0–100.")
        }
        let close = stringArg(a, "close") ?? "magnetic"
        guard close == "magnetic" || close == "straight" else {
            throw ToolError(message: "close must be \"magnetic\" or \"straight\".")
        }
        let mode = try selectionMode(a)
        let feather = try selectionNumber(a, "feather", default: 0)
        guard (0...250).contains(feather) else { throw ToolError(message: "feather must be 0–250 px.") }
        var session = try MagneticLassoSession(capture: capture, start: points[0], mode: mode,
                                               feather: feather, options: options)
        let clicks = Set(anchors)
        let coalesce = CGFloat(max(1, options.width / 4))
        for (index, point) in points.enumerated().dropFirst() {
            if clicks.contains(index) {
                try session.click(at: point, straight: false)
            } else {
                try session.move(to: point, minimumDistance: index == points.count - 1 ? 0 : coalesce)
            }
        }
        let outline = try session.closedOutline(straight: close == "straight")
        try capture.commit(.polygon(outline), in: document, name: "Magnetic Lasso", mode: mode,
            feather: feather,
            record: .step("select_freehand",
                          ["points": ActionArgs.points(outline), "mode": mode.agentName, "feather": feather],
                          note: "MCP: select_magnetic"))
        var reply = selectionReplyFields(document)
        reply["outline_points"] = outline.count
        return try jsonResult(reply)
    }

    func selectColorRange(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let capture = try SelectionCapture(document: document)
        guard capture.source != nil || capture.doc.isHighDepth else { throw ToolError(message: "The image has no projection.") }
        var parameters = ColorRangeParameters()
        parameters.range = stringArg(a, "range") ?? "sampled"
        parameters.tolerance = try selectionNumber(a, "tolerance", default: 20)
        parameters.softness = try selectionNumber(a, "softness", default: 10)
        parameters.lower = try selectionNumber(a, "lower", default: 0)
        parameters.upper = try selectionNumber(a, "upper", default: 35)
        if a["samples"] != nil && a["samples16"] != nil { throw ToolError(message: "Choose samples or samples16, not both.") }
        if let raw = a["samples16"] {
            guard capture.doc.isHighDepth, let colors = raw as? [[NSNumber]], (1...32).contains(colors.count),
                  colors.allSatisfy({ $0.count == 3 && $0.allSatisfy { (0...65535).contains($0.doubleValue) && $0.doubleValue.rounded(.towardZero) == $0.doubleValue } }) else {
                throw ToolError(message: "samples16 requires 1–32 native integer RGB triples on a 16-bit document.")
            }
            parameters.samples16 = colors.map { $0.map(\.intValue) }
        }
        if let raw = a["samples"] {
            guard let colors = raw as? [[Any]], colors.count <= 32 else {
                throw ToolError(message: "samples must contain at most 32 RGB triples.")
            }
            parameters.samples = try colors.map { color in
                guard color.count == 3 else { throw ToolError(message: "Each sample requires three RGB components.") }
                return try color.map { value in
                    let number = try selectionNumber(["component": value], "component", default: -1)
                    guard (0...255).contains(number), number.rounded(.towardZero) == number else {
                        throw ToolError(message: "RGB sample components must be integers from 0 to 255.")
                    }
                    return Int(number)
                }
            }
        }
        if a["invert"] != nil, boolArg(a, "invert") == nil {
            throw ToolError(message: "invert must be true or false.")
        }
        if a["localized"] != nil, boolArg(a, "localized") == nil {
            throw ToolError(message: "localized must be true or false.")
        }
        parameters.invert = boolArg(a, "invert") ?? false
        parameters.localized = boolArg(a, "localized") ?? false
        parameters.clusterRange = try selectionNumber(a, "cluster_range", default: 100)
        if !parameters.localized, a["points"] != nil || a["cluster_range"] != nil {
            throw ToolError(message: "points and cluster_range apply only with localized: true.")
        }
        if parameters.localized {
            guard let raw = a["points"] as? [[Any]], (1...32).contains(raw.count) else {
                throw ToolError(message: "localized needs points: one canvas [x, y] per sample, in order.")
            }
            parameters.points = try raw.map { pair -> [Int] in
                guard pair.count == 2 else { throw ToolError(message: "Each point requires [x, y].") }
                let x = try selectionNumber(["x": pair[0]], "x", default: -1)
                let y = try selectionNumber(["y": pair[1]], "y", default: -1)
                guard x >= 0, x < Double(capture.doc.width), y >= 0, y < Double(capture.doc.height),
                      x.rounded(.towardZero) == x, y.rounded(.towardZero) == y else {
                    throw ToolError(message: "Points must name integer pixels inside the canvas.")
                }
                return [Int(x), Int(y)]
            }
        }
        if a["fuzziness"] != nil, !(a["fuzziness"] is NSNull) {
            guard parameters.isSkinTones else {
                throw ToolError(message: "fuzziness applies only to range skin_tones.")
            }
            parameters.fuzziness = try selectionNumber(a, "fuzziness", default: 40)
        }
        parameters = try parameters.validated()
        let mode = try selectionMode(a)
        // Out Of Gamut reads the window's proof setup (view state, never written), resolved
        // once: the shape and the reply's proof_profile name the same setup.
        let gamutSetup = parameters.isOutOfGamut
            ? try ColorRangeGamut.setup(window: editor(document)?.proofDisplay.setup) : nil
        let shape = try capture.colorRangeShape(parameters, gamutSetup: gamutSetup, in: document)
        try capture.commit(shape, in: document, name: "Color Range", mode: mode)
        var reply = selectionReplyFields(document)
        if let gamutSetup {
            reply["proof_profile"] = gamutSetup.profileName
        }
        return try jsonResult(reply)
    }

    func quickSelect(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let capture = try SelectionCapture(document: document)
        guard capture.source != nil || capture.doc.isHighDepth else { throw ToolError(message: "The image has no projection.") }
        guard let raw = a["seeds"] as? [[Any]], (1...4096).contains(raw.count) else {
            throw ToolError(message: "seeds requires 1–4096 integer [x, y] canvas pixels.")
        }
        let seeds = try raw.map { pair -> [Int] in
            guard pair.count == 2 else { throw ToolError(message: "Each seed requires [x, y].") }
            let x = try selectionNumber(["x": pair[0]], "x", default: -1)
            let y = try selectionNumber(["y": pair[1]], "y", default: -1)
            guard x >= 0, x < Double(capture.doc.width), y >= 0, y < Double(capture.doc.height),
                  x.rounded(.towardZero) == x, y.rounded(.towardZero) == y else {
                throw ToolError(message: "Seeds must name integer pixels inside the canvas.")
            }
            return [Int(x), Int(y)]
        }
        let tolerance = try selectionNumber(a, "tolerance", default: 25)
        let edge = try selectionNumber(a, "edge", default: 12)
        let feather = try selectionNumber(a, "feather", default: 0)
        let mode = try selectionMode(a)
        guard (0...200).contains(tolerance), (0...200).contains(edge), (0...250).contains(feather) else {
            throw ToolError(message: "Tolerance and edge must be 0–200; feather must be 0–250 px.")
        }
        let size = try selectionNumber(a, "brush_size", default: 1)
        let smooth = try selectionNumber(a, "smooth", default: 0)
        guard (1...512).contains(size), (0...4).contains(smooth), smooth.rounded(.towardZero) == smooth else {
            throw ToolError(message: "brush_size must be 1–512 px and smooth an integer from 0 to 4.")
        }
        if a["enhance_edge"] != nil, boolArg(a, "enhance_edge") == nil {
            throw ToolError(message: "enhance_edge must be true or false.")
        }
        // The core's radius is the brush's reach beyond its centre pixel.
        let core: [String: Any] = ["seeds": seeds, "tolerance": tolerance, "edge": edge,
                                   "radius": (size - 1) / 2, "smooth": Int(smooth)]
        let shape = try capture.quickSelectionShape(core, enhanceEdge: boolArg(a, "enhance_edge") ?? false,
                                                    in: document)
        try capture.commit(shape, in: document, name: "Quick Selection", mode: mode, feather: feather)
        return try selectionResult(document)
    }

    func transformSelection(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let capture = try SelectionCapture(document: document, needsSource: false)
        guard let selection = capture.selection.selection else {
            throw ToolError(message: "Make a selection before transforming it.")
        }
        let parameters = try SelectionTransformParameters(
            translateX: selectionNumber(a, "translate_x", default: 0),
            translateY: selectionNumber(a, "translate_y", default: 0),
            scaleX: selectionNumber(a, "scale_x", default: 1),
            scaleY: selectionNumber(a, "scale_y", default: 1),
            angle: selectionNumber(a, "angle", default: 0),
            anchor: CGPoint(x: selectionNumber(a, "anchor_x", default: selection.bounds.midX),
                            y: selectionNumber(a, "anchor_y", default: selection.bounds.midY)),
            filter: stringArg(a, "filter") ?? "bilinear").validated()
        try capture.commit(capture.transformedShape(parameters, in: document), in: document, name: "Transform Selection")
        return try selectionResult(document)
    }
}
