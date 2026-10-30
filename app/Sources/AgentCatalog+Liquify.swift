import Foundation

extension AgentServer {
    /// `liquify` (handler in AgentServer+Liquify.swift, the stroke model
    /// and its limits in LiquifyStroke.swift).
    func liquifyCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        let point: [String: Any] = [
            "type": "array", "items": ["type": "number"], "minItems": 2, "maxItems": 2,
        ]
        let percent: (String) -> [String: Any] = {
            ["type": "number", "minimum": 0, "maximum": 100, "description": $0]
        }
        let strokeProperties: [String: Any] = [
            "tool": [
                "type": "string", "enum": LiquifyTool.allCases.map(\.rawValue),
            ] as [String: Any],
            "points": [
                "type": "array", "items": point, "minItems": 1,
                "maxItems": LiquifyStroke.maxStrokePoints,
                "description": "[[x, y], …] along the stroke, in canvas px, each within "
                    + "±1,000,000. Two equal consecutive points are a dwell: one more dab in place.",
            ] as [String: Any],
            "size": [
                "type": "number", "minimum": 1, "maximum": 5000,
                "description": "Brush diameter in px (1-5000).",
            ] as [String: Any],
            "strength": percent("How far each dab acts, in percent (0-100, default 50): the "
                + "workspace's Pressure."),
            "density": percent("How much of the brush radius acts at full strength before the "
                + "falloff to its rim, in percent (0-100, default 50)."),
        ]
        let stroke: [String: Any] = [
            "type": "object", "properties": strokeProperties, "required": ["tool", "points", "size"],
        ]
        // An array of literals joined, never a `+` chain (see `adjustmentOps`).
        let description = [
            "Reshapes a layer's pixels with brush strokes \u{2014} Filters \u{25B8} Liquify\u{2026}. ",
            "The strokes run in order on one displacement field and the layer is resampled once ",
            "(bicubic), as one undo step. Each stroke is {tool, points, size, strength, density} ",
            "with points in CANVAS px. Tools: forward_warp pushes the picture along the stroke's ",
            "direction; pucker pulls it toward the brush centre and bloat pushes it away; ",
            "twirl_clockwise and twirl_counterclockwise turn it about the centre; reconstruct ",
            "moves the field back toward the unwarped picture; freeze protects what it paints ",
            "from every later stroke of this call and thaw removes that protection. Every tool ",
            "but forward_warp works per dab, so a single point acts once and repeated points ",
            "build the effect; forward_warp needs at least two different points. ",
            "Limits: 1-2048 strokes, 1-8192 points a stroke, 131,072 points in all, and 1 ",
            "billion field updates in all, past which the call is refused by name (use fewer or ",
            "shorter strokes or a smaller brush). Unknown keys are refused, at the top level and ",
            "in a stroke. Only the layer's own raster is rewritten: it does not grow, a sample ",
            "taken past its edge repeats the edge pixel, and points outside it are allowed. ",
            "Writes only inside the active selection. Strokes that move nothing answer changed: ",
            "false and add no undo step. The reply carries strokes, the count that ran, and at ",
            "16 and 32 bits storage. 8-, 16- and 32-bit float documents: a float layer is ",
            "resampled in linear light with each colour clamped to the range its visible pixels ",
            "hold (not to 0 to 1), on the layer's own pixels only. Errors on a group or an ",
            "adjustment layer. This rewrites the layer's pixels, so a re-editable text or shape ",
            "layer drops its description (undo restores it).",
        ].joined()
        return [
            tool(
                "liquify", description,
                [
                    "strokes": [
                        "type": "array", "items": stroke, "minItems": 1,
                        "maxItems": LiquifyStroke.maxStrokes,
                        "description": "The strokes, applied in order.",
                    ] as [String: Any],
                    "layer": schema.index, "document_id": schema.docID,
                ], required: ["strokes"]),
        ]
    }
}
