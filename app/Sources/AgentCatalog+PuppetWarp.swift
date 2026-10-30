import Foundation

extension AgentServer {
    /// `puppet_warp` (handler in AgentServer+PuppetWarp.swift, the
    /// model in PuppetWarp.swift).
    func puppetWarpCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        let point: [String: Any] = [
            "type": "array", "items": ["type": "number"], "minItems": 2, "maxItems": 2,
        ]
        let pinProperties: [String: Any] = [
            "from": point.merging(["description": "[x, y] in canvas px, inside the mesh: the "
                + "pin snaps to the nearest vertex of the mesh triangle there."]) { $1 },
            "to": point.merging(["description": "[x, y] in canvas px where that vertex lands, "
                + "moved by to - from when from is not exactly the vertex."]) { $1 },
            "depth": [
                "type": "integer", "minimum": -1000, "maximum": 1000,
                "description": "Pin Depth (default 0): where the mesh overlaps itself, the part "
                    + "nearest the higher-depth pin draws on top.",
            ] as [String: Any],
            "rotate": [
                "type": ["number", "null"],
                "description": "Fixed rotation in degrees, clockwise on the canvas, of the mesh "
                    + "at the pin; null or absent is Auto.",
            ] as [String: Any],
        ]
        let pin: [String: Any] = [
            "type": "object", "properties": pinProperties, "required": ["from", "to"],
        ]
        // An array of literals joined, never a `+` chain (see `adjustmentOps`).
        let description = [
            "Bends a layer by pins on a triangle mesh \u{2014} Edit \u{25B8} Puppet Warp. The ",
            "mesh covers the layer's opaque pixels grown by expansion px; each pin holds the mesh ",
            "vertex nearest its from and moves it to its to, and the rest of the mesh follows as ",
            "rigidly as it can (as-rigid-as-possible). mode rigid resists every stretch, distort ",
            "allows uniform scaling, normal is between. density fewer, normal or more sets how ",
            "fine the mesh is. A pin's depth decides what draws on top where the mesh overlaps ",
            "itself; rotate fixes the mesh's angle at the pin. With a selection active, only the ",
            "selected pixels float and warp; they land source-over and leave a hole where they ",
            "were, and the selection stays. The layer is resampled once (bicubic) as one undo ",
            "step; the layer form grows to the deformed mesh. Unmoved, unrotated pins answer ",
            "changed: false and add no undo step. The reply carries the pins snapped to their ",
            "vertices ({from, to}), the mesh's vertices and triangles, storage and write_scope. ",
            "Refused by name: no pins, more than 256, a pin outside the mesh, two pins on one ",
            "vertex, a layer with no pixels at the expansion, a result past the pixel limit, a ",
            "group or adjustment layer, and a locked layer. 8-, 16- and 32-bit float documents; ",
            "a float layer keeps each colour within the range its visible pixels hold. This ",
            "rewrites the layer's pixels, so a text or shape layer drops its description (undo ",
            "restores it).",
        ].joined()
        return [
            tool(
                "puppet_warp", description,
                [
                    "pins": [
                        "type": "array", "items": pin, "minItems": 1,
                        "maxItems": PuppetWarp.maxPins,
                        "description": "The pins, 1-256.",
                    ] as [String: Any],
                    "mode": [
                        "type": "string", "enum": PuppetMode.allCases.map(\.rawValue),
                        "description": "Default normal.",
                    ] as [String: Any],
                    "density": [
                        "type": "string", "enum": PuppetDensity.allCases.map(\.rawValue),
                        "description": "Default normal.",
                    ] as [String: Any],
                    "expansion": [
                        "type": "integer", "minimum": -100, "maximum": 100,
                        "description": "How far the mesh reaches past the opaque pixels, in px "
                            + "(default 2; negative pulls it in).",
                    ] as [String: Any],
                    "layer": schema.index, "document_id": schema.docID,
                ], required: ["pins"]),
        ]
    }
}
