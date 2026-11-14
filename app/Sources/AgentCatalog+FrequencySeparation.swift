import Foundation

extension AgentServer {
    @inline(never)
    func frequencySeparationCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        [
            tool(
                "frequency_separation",
                "Filters > Other > Frequency Separation…: splits one raster layer into a LOW layer "
                    + "(the layer itself, Gaussian Blurred at radius, keeping its name + \" Low\", "
                    + "mask, opacity, blend mode and style) and a HIGH layer above it (name + "
                    + "\" High\": apply_image's Subtract of the low layer at scale 2 and offset "
                    + "127.5, so (source − low) / 2 + ½; Linear Light, clipped to the low layer, "
                    + "no mask or style). The pair recomposes the original within one code where "
                    + "the layer is opaque; where the blur changes alpha it is close, not exact. "
                    + "Retouch tone on the low layer and texture on the high one; Merge Down on "
                    + "the high layer bakes the pair. The selection is ignored. One undo step. "
                    + "Answers at 8 and 16 bits on an RGB document; a 32-bit float document, the "
                    + "colour modes, a document evaluated in linear light (set_evaluation "
                    + "\"encoded\" first: the detail is a difference of stored codes), a group, an "
                    + "adjustment layer, a clipped layer, a text, shape, fill or placed layer and a "
                    + "pixel-locked layer refuse by name. Replies low_layer and high_layer.",
                [
                    "layer": schema.index,
                    "radius": [
                        "type": "number", "minimum": 0.5, "maximum": 64,
                        "description": "The low layer's Gaussian Blur radius (sigma) in pixels "
                            + "(default 4): detail finer than it goes to the high layer.",
                    ],
                    "document_id": schema.docID,
                ]),
        ]
    }
}
