import AppKit

/// Mirrors the Object Selection tool (`EditorViewController+ObjectSelection.swift`): the region a
/// drag would describe, as a rectangle or a closed path, becomes the outline of the foreground
/// object inside it through the same `ObjectSelection.shape`.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let objectSelectionHandlers: [String: ToolHandler] = [
        "object_select": { $0.objectSelect },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let objectSelectionToolFacts: [String: ToolFacts] = [
        "object_select": [.commonTool, .floatTool, .cmykTool],
    ]

    var objectSelectionCatalog: [[String: Any]] {
        let number: [String: Any] = ["type": "number"]
        let point: [String: Any] = ["type": "array", "items": number, "minItems": 2, "maxItems": 2]
        return [
            tool("object_select", "Select the foreground object inside a region: give exactly one of region {x, y, width, height} or points (a closed path of 3–10,000 canvas [x, y]). The objects are Vision's foreground instances (inspect_subjects lists them); every instance with a real presence inside the region is selected, clipped to the region, and one that only clips its edge is left out. refine then follows the picture's own edge within a few pixels of the outline: it defaults to true at 8 and 16 bits and to false in a 32-bit float document, where Vision reads the tone-mapped 8-bit display at view exposure 0 and the result is its outline clipped to the region; refine: true runs the edge stage there on the display codes (the profile's curve over values clamped to 0–1), where an above-white subject on an above-white background is flat white. Refuses when no object is inside the region. The reply lists the instances chosen, instance_count and refined, which says whether the edge stage ran. One selection Undo; pixels unchanged. Mirrors dragging the Object Selection tool.",
                 ["document_id": Self.docIDProperty,
                  "region": ["type": "object", "properties": ["x": number, "y": number, "width": number, "height": number], "required": ["x", "y", "width", "height"]],
                  "points": ["type": "array", "items": point, "minItems": 3, "maxItems": 10000],
                  "mode": ["type": "string", "enum": ["replace", "add", "subtract", "intersect"]],
                  "feather": number,
                  "refine": ["type": "boolean", "description": "Refine the outline against the picture's edges (default true at 8 and 16 bits, false at 32-bit float, where true refines on the display codes)."]]),
        ]
    }

    func objectSelect(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseNullOrBooleanNumbers(a, ["feather"], in: "object_select")
        if let region = a["region"] as? [String: Any] {
            try refuseNullOrBooleanNumbers(
                region, ["x", "y", "width", "height"], in: "object_select region")
        }
        let capture = try SelectionCapture(document: document)
        let mode = try selectionMode(a)
        var feather = 0.0
        if a["feather"] != nil {
            guard let value = doubleArg(a, "feather"), value.isFinite, (0...250).contains(value) else {
                throw ToolError(message: "feather must be 0–250 pixels.")
            }
            feather = value
        }
        if a["refine"] != nil, boolArg(a, "refine") == nil {
            throw ToolError(message: "refine must be true or false.")
        }
        // The default follows the depth (off at float, where Vision reads
        // the tone-mapped display and the edge stage reads R1 Clip), and an
        // explicit true at float runs the edge stage.
        let refine = boolArg(a, "refine") ?? ObjectSelection.refines(capture.doc)
        let hasRegion = a["region"] != nil && !(a["region"] is NSNull)
        let hasPoints = a["points"] != nil && !(a["points"] is NSNull)
        guard hasRegion != hasPoints else {
            throw ToolError(message: "Give exactly one of region or points.")
        }
        let shape: CanvasSelection.Shape
        let bounds: CGRect
        if hasRegion {
            guard let region = a["region"] as? [String: Any] else {
                throw ToolError(message: "region requires x, y, width, height.")
            }
            func coordinate(_ key: String) throws -> Double {
                guard let value = doubleArg(region, key), value.isFinite, abs(value) <= 100_000 else {
                    throw ToolError(message: "region \(key) must be a finite coordinate within ±100,000 pixels.")
                }
                return value
            }
            let rect = try CGRect(x: coordinate("x"), y: coordinate("y"),
                                  width: coordinate("width"), height: coordinate("height"))
            // The stored size: `rect.width`/`.height` standardize, so a negative reads positive.
            guard rect.size.width > 0, rect.size.height > 0 else {
                throw ToolError(message: "Region width and height must be positive.")
            }
            shape = .rect(rect)
            bounds = rect
        } else {
            let points = try parsePoints(a)
            guard points.count >= 3 else {
                throw ToolError(message: "points needs at least 3 [x, y] pairs: a closed path.")
            }
            shape = .polygon(points)
            bounds = points.dropFirst().reduce(CGRect(origin: points[0], size: .zero)) {
                $0.union(CGRect(origin: $1, size: .zero))
            }
        }
        let found = try ObjectSelection.shape(capture, in: document, region: shape, bounds: bounds,
                                              refine: refine)
        try capture.commit(found.shape, in: document, name: "Object Selection", mode: mode, feather: feather)
        var reply: [String: Any] = ["ok": true, "instances": found.instances,
                                    "instance_count": found.instanceCount, "refined": refine,
                                    "selection_empty": document.canvasSelection == nil]
        if let selection = document.canvasSelection {
            reply["bounds"] = ["x": Int(selection.bounds.minX), "y": Int(selection.bounds.minY),
                               "width": Int(selection.bounds.width), "height": Int(selection.bounds.height)]
        }
        return try jsonResult(reply)
    }
}
