import Foundation

extension AgentServer {
    /// `perspective_warp` (handler in AgentServer+PerspectiveWarp.swift,
    /// the model in PerspectiveWarp.swift).
    func perspectiveWarpCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        let corners: [String: Any] = [
            "type": "array", "items": ["type": "number"], "minItems": 8, "maxItems": 8,
        ]
        let quad: [String: Any] = [
            "type": "object",
            "properties": [
                "from": corners.merging(["description": "[x0, y0, x1, y1, x2, y2, x3, y3] in "
                    + "canvas px: the plane's four corners in the picture, any consistent "
                    + "winding, convex with no three in a line."]) { $1 },
                "to": corners.merging(["description": "The same four corners where they go "
                    + "(the pins), in the same order; convex, turning the same way."]) { $1 },
            ] as [String: Any],
            "required": ["from", "to"],
        ]
        let straighten: [String: Any] = [
            "type": "object",
            "properties": [
                "quad": ["type": "integer", "minimum": 0, "description": "The quad's index."],
                "edge": [
                    "type": "integer", "minimum": 0, "maximum": 3,
                    "description": "Edge k runs from corner k to corner k + 1 (mod 4).",
                ] as [String: Any],
                "direction": [
                    "type": ["string", "number"],
                    "description": "auto (default: horizontal within 30\u{00B0} of level, "
                        + "vertical within 30\u{00B0} of plumb, else as drawn), near_horizontal "
                        + "or near_vertical (only when within 30\u{00B0} of that axis), "
                        + "horizontal, vertical, or an angle in degrees, clockwise on the canvas.",
                ] as [String: Any],
            ] as [String: Any],
            "required": ["quad", "edge"],
        ]
        // An array of literals joined, never a `+` chain (see `adjustmentOps`).
        let description = [
            "Warps a layer by planes \u{2014} Edit \u{25B8} Perspective Warp. Each quad names a ",
            "plane of the picture (from: its four corners, such as a building face or a table ",
            "top) and where those corners go (to); the plane is resampled through its own ",
            "homography. Quads whose from corners coincide share that corner (its to must match) ",
            "and, along a common side, an edge: joined planes stay continuous there. The rest of ",
            "the layer follows as rigidly as it can, continuing each plane across its border. ",
            "straighten levels or plumbs edges exactly (the warped corners move; the first entry ",
            "per edge that applies is kept). With a selection active, only the selected pixels ",
            "float and warp; they land source-over and leave a hole where they were, and the ",
            "selection stays. The layer is resampled once (bicubic) as one undo step and grows ",
            "to the warped extent. Unmoved corners answer changed: false and add no undo step. ",
            "The reply carries the resolved quads ({from, to}, straightening applied), the ",
            "shared edges as pairs of from points, storage and write_scope. Refused by name: no ",
            "quads or more than 64, a malformed quad, a degenerate quad (three corners in a line ",
            "or self-crossing), a warped quad that folds or turns the other way, joined corners ",
            "warped apart, a bad straighten entry, a result past the pixel limit, a group or ",
            "adjustment layer, and a locked layer. 8-, 16- and 32-bit float documents; a float ",
            "layer keeps each colour within the range its visible pixels hold. This rewrites the ",
            "layer's pixels, so a text or shape layer drops its description (undo restores it).",
        ].joined()
        return [
            tool(
                "perspective_warp", description,
                [
                    "quads": [
                        "type": "array", "items": quad, "minItems": 1,
                        "maxItems": PerspectiveWarp.maxQuads,
                        "description": "The planes, 1-64.",
                    ] as [String: Any],
                    "straighten": [
                        "type": "array", "items": straighten, "maxItems": 256,
                        "description": "Edges to level or plumb (Shift-click in Warp).",
                    ] as [String: Any],
                    "layer": schema.index, "document_id": schema.docID,
                ], required: ["quads"]),
        ]
    }
}
