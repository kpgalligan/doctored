import AppKit

/// Mirrors Layer > Mask > Properties, Link with Content and Transform Mask.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let independentMasksHandlers: [String: ToolHandler] = [
        "get_mask_properties": { $0.getMaskProperties },
        "set_mask_properties": { $0.setMaskProperties },
        "transform_mask": { $0.transformMask },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let independentMasksToolFacts: [String: ToolFacts] = [
        "get_mask_properties": [.nonRecordable, .typedTool, .floatTool, .cmykTool],
        "set_mask_properties": [.typedTool, .floatTool, .cmykTool],
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
        "transform_mask": [.typedTool, .floatTool, .cmykTool],
    ]

    var independentMaskCatalog: [[String: Any]] {
        let doc = Self.docIDProperty
        let layer: [String: Any] = ["type": "integer", "description": "Layer index; defaults to the active layer."]
        return [
            tool("get_mask_properties", "Original mask placement, content linking, density and feather. Rendering properties never modify original coverage.", ["document_id": doc, "layer": layer]),
            tool("set_mask_properties", "Set persistent mask linking, density (0..1) or feather radius (0..1000 canvas pixels); one undoable edit. Geometry preserves the numeric feather radius.",
                ["document_id": doc, "layer": layer, "linked": ["type": "boolean"],
                 "density": ["type": "number", "minimum": 0, "maximum": 1],
                 "feather": ["type": "number", "minimum": 0, "maximum": 1000]]),
            tool("transform_mask", "Transform only original mask coverage in canvas coordinates; layer pixels stay unchanged. Density, feather and linking are retained. An optional pivot [x, y] records the point the matrix was built around; it does not change the result here, and a relative Action replay keeps the angle, scale and skew about it.",
                ["document_id": doc, "layer": layer, "matrix": ["type": "array", "items": ["type": "number"],
                  "minItems": 6, "maxItems": 6, "description": "Affine [a,b,c,d,tx,ty], matching CGAffineTransform."],
                 "pivot": ["type": "array", "items": ["type": "number"], "minItems": 2, "maxItems": 2],
                 "filter": ["type": "string", "enum": ["nearest", "bilinear", "bicubic", "lanczos3"]]], required: ["matrix"]),
        ]
    }

    func getMaskProperties(_ arguments: [String: Any]) throws -> String {
        let document = try target(arguments)
        let layer = intArg(arguments, "layer") ?? document.activeLayerIndex
        // Beside a vector mask these are the RASTER mask's, which is what
        // set_mask_properties edits (`RasterDocument.maskEditBaseline`).
        guard let properties = document.doc?.maskEditBaseline(layer)?.properties else { throw ToolError(message: "The target layer has no mask.") }
        return try jsonResult(properties.report.merging(["layer": layer]) { first, _ in first })
    }

    func setMaskProperties(_ arguments: [String: Any]) throws -> String {
        let document = try target(arguments)
        if arguments["linked"] != nil && boolArg(arguments, "linked") == nil {
            throw ToolError(message: "linked must be a boolean.")
        }
        for key in ["density", "feather"] where arguments[key] != nil && doubleArg(arguments, key) == nil {
            throw ToolError(message: "\(key) must be a finite number.")
        }
        let capture = try MaskEditCapture(document, layer: intArg(arguments, "layer"))
        let current = capture.properties
        let linked = boolArg(arguments, "linked") ?? current.linked
        let density = doubleArg(arguments, "density") ?? current.density
        let feather = doubleArg(arguments, "feather") ?? current.feather
        guard density.isFinite, feather.isFinite, (0...1).contains(density), (0...1000).contains(feather) else {
            throw ToolError(message: "density must be 0..1 and feather 0..1000 pixels.")
        }
        let changed = try document.applyMaskEdit(.properties(linked: linked, density: density, feather: feather),
            capture: capture, name: "Mask Properties", record: .notACommand)
        var report = document.doc?.maskEditBaseline(capture.layer)?.properties.report ?? [:]
        report["changed"] = changed; report["layer"] = capture.layer
        return try jsonResult(report)
    }

    func transformMask(_ arguments: [String: Any]) throws -> String {
        let document = try target(arguments)
        let capture = try MaskEditCapture(document, layer: intArg(arguments, "layer"))
        guard let supplied = arguments["matrix"] as? [Any], supplied.count == 6 else {
            throw ToolError(message: "matrix must contain [a,b,c,d,tx,ty].")
        }
        let values = supplied.compactMap { value -> Double? in
            if let n = value as? NSNumber { return n.doubleValue }
            if let text = value as? String { return Double(text) }
            return nil
        }
        guard values.count == 6, values.allSatisfy(\.isFinite) else { throw ToolError(message: "matrix must contain six finite numbers.") }
        if let pivot = arguments["pivot"] {
            guard let pair = pivot as? [Any], pair.count == 2,
                  pair.allSatisfy({ ActionJSON.number($0)?.doubleValue.isFinite == true }) else {
                throw ToolError(message: "pivot must contain [x, y], two finite numbers.")
            }
        }
        let matrix = CGAffineTransform(a: values[0], b: values[1], c: values[2], d: values[3], tx: values[4], ty: values[5])
        let filter: DrResizeFilter
        switch stringArg(arguments, "filter") ?? "bilinear" {
        case "nearest": filter = DR_FILTER_NEAREST
        case "bilinear": filter = DR_FILTER_BILINEAR
        case "bicubic": filter = DR_FILTER_CATMULL_ROM
        case "lanczos3": filter = DR_FILTER_LANCZOS3
        default: throw ToolError(message: "Unknown mask resampling filter.")
        }
        let changed = try document.applyMaskEdit(.transform(matrix, filter), capture: capture,
            name: "Transform Mask", record: .notACommand)
        var report = document.doc?.maskEditBaseline(capture.layer)?.properties.report ?? [:]
        report["changed"] = changed; report["layer"] = capture.layer
        return try jsonResult(report)
    }
}
