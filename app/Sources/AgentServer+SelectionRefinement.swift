import AppKit

/// Mirrors Select > Select and Mask; inspection backgrounds and zoom are view
/// preferences. The same captured coverage and validation drive UI and MCP.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let selectionRefinementHandlers: [String: ToolHandler] = [
        "refine_selection": { $0.refineSelection },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let selectionRefinementToolFacts: [String: ToolFacts] = [
        "refine_selection": [.selectionSensitive, .typedTool, .floatTool, .cmykTool],
    ]

    var selectionRefinementCatalog: [[String: Any]] {
        let radius: [String: Any] = ["type": "number", "minimum": 0, "maximum": 64]
        let point: [String: Any] = ["type": "array", "items": ["type": "number"], "minItems": 2, "maxItems": 2]
        let stroke: [String: Any] = ["type": "object", "properties": [
            "mode": ["type": "string", "enum": ["add", "subtract"]],
            "radius": ["type": "number", "minimum": 0.5, "maximum": 512],
            "points": ["type": "array", "items": point, "minItems": 1, "maxItems": 8192],
        ], "required": ["mode", "radius", "points"]]
        return [tool("refine_selection",
            "Select and Mask: refine the current selection's coverage and write it to one of four outputs, as one Undo. "
            + "edge_radius (0–64 px) re-estimates coverage from the picture's own colours within that distance of the selection's edge, so a hard selection follows soft or fine edges; smart_radius fades that estimate where the picture has no edge, keeping a hard edge across flat colour. "
            + "brush is the Refine Edge brush: up to 256 strokes {mode, radius, points} in canvas px, applied in order — add always refines under the stroke (with or without edge_radius), subtract never does. "
            + "Then Smooth, Shift Edge, Feather and Contrast (smooth/feather 0–64 px, shift −64–64 px, contrast 0–1). "
            + "output: selection (default), mask (a new or replacement mask on layer), new_layer (a \"Refined Cutout\" layer whose alpha carries the coverage) or new_layer_with_mask (the same layer with the coverage as its editable mask); both layer outputs hide every other top-level layer and keep the composite's own alpha. "
            + "decontaminate replaces fringe colour with the estimated foreground colour by decontaminate_amount (0–1, default 1); it changes pixels, so it needs a layer output and a fully opaque composite. "
            + "Computes from the current coverage once. The reply adds new_layer (the new layer's index) for the layer outputs, and decontaminated. Not learned matting: hair against a busy background and glass stay approximate. "
            + "On a 16-bit document coverage is refined at 65,536 levels, a mask output is a native u16 mask on the unchanged RGBA16 layer and a layer output keeps RGBA16. "
            + "On a 32-bit float document the same holds with RGBA32 float: edge_radius and the brush read the composite's display codes (the profile's curve over values clamped to 0–1, never the tone map), so a value above white refines as white; a layer output keeps the float composite's values, above white included; decontaminate is unavailable.",
            ["document_id": Self.docIDProperty, "smooth": radius, "feather": radius,
             "shift": ["type": "number", "minimum": -64, "maximum": 64],
             "contrast": ["type": "number", "minimum": 0, "maximum": 1],
             "edge_radius": radius,
             "smart_radius": ["type": "boolean"],
             "brush": ["type": "array", "items": stroke, "maxItems": 256],
             "decontaminate": ["type": "boolean"],
             "decontaminate_amount": ["type": "number", "minimum": 0, "maximum": 1],
             "output": ["type": "string", "enum": ["selection", "mask", "new_layer", "new_layer_with_mask"]],
             "layer": ["type": "integer", "minimum": 0]])]
    }

    func refineSelection(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseNullOrBooleanNumbers(a, ["layer"], in: "refine_selection")
        let allowed: Set<String> = ["document_id", "smooth", "feather", "shift", "contrast",
                                    "edge_radius", "smart_radius", "brush", "decontaminate",
                                    "decontaminate_amount", "output", "layer"]
        if let unexpected = a.keys.sorted().first(where: { !allowed.contains($0) }) {
            throw ToolError(message: "Unknown refinement parameter: \(unexpected).")
        }
        func number(_ key: String, default fallback: Double = 0) throws -> Double {
            guard let raw = a[key] else { return fallback }
            guard !Self.isJSONBoolean(raw), let value = doubleArg(a, key), value.isFinite else {
                throw ToolError(message: "\(key) must be a finite number.")
            }
            return value
        }
        func flag(_ key: String) throws -> Bool {
            guard a[key] != nil else { return false }
            guard let value = boolArg(a, key) else {
                throw ToolError(message: "\(key) must be true or false.")
            }
            return value
        }
        guard let output = SelectionRefinementOutput(rawValue: stringArg(a, "output") ?? "selection") else {
            throw ToolError(message: "output must be selection, mask, new_layer or new_layer_with_mask.")
        }
        if a["layer"] != nil && intArg(a, "layer") == nil {
            throw ToolError(message: "layer must name a layer index.")
        }
        let brush: [RefineEdgeStroke]
        do { brush = try RefineEdgeStroke.parse(a["brush"]) }
        catch { throw ToolError(message: error.localizedDescription) }
        let capture = try SelectionRefinementCapture(document, layer: intArg(a, "layer"))
        let parameters = try SelectionRefinementParameters(
            smooth: number("smooth"), feather: number("feather"),
            shift: number("shift"), contrast: number("contrast"),
            edgeRadius: number("edge_radius"), smartRadius: flag("smart_radius"), brush: brush,
            decontaminate: flag("decontaminate"),
            decontaminateAmount: number("decontaminate_amount", default: 1)).validated(for: output)
        let result = try capture.applyReporting(parameters, output: output, in: document,
                                                record: .notACommand)
        var reply: [String: Any] = ["ok": true, "changed": result.changed, "output": output.rawValue,
                                    "layer": capture.layer,
                                    "decontaminated": result.decontaminated,
                                    "coverage_storage": capture.isNative ? "u16" : "u8"]
        if let layer = result.newLayer { reply["new_layer"] = layer }
        return try jsonResult(reply)
    }
}
