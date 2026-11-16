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
                    + "\" High\", the detail, clipped to the low layer, no mask or style). At 8 and "
                    + "16 bits the detail is the low layer Subtracted at scale 2 and offset 127.5, so "
                    + "(source − low) / 2 + ½ (offset 128 in an 8-bit RGB document, whose Linear Light "
                    + "blends on codes; apply_image's Subtract in RGB, Grayscale and Duotone; "
                    + "in Lab Color the same on each of the L*, a* and b* codes, a Subtract "
                    + "apply_image does not offer there), in Linear Light, and the pair "
                    + "recomposes the original within one code: in RGB, in Grayscale and Duotone (one "
                    + "Gray plane, R = G = B) and in Lab Color (each of L*, a* and b*). In a 32-bit "
                    + "float document (RGB or Grayscale) the detail is source − low, unclamped (the "
                    + "High layer alone shows its negative half as black), in Linear Dodge (Add), "
                    + "within float rounding; a difference outside ±65504 refuses. In a 16-bit "
                    + "document evaluated in linear light the detail is computed on decoded "
                    + "components, in Linear Light, within 4 codes of 65535 in linear light (up to "
                    + "about 24 stored codes in sRGB's shadows, about two 8-bit levels at black under "
                    + "a pure power profile); an 8-bit document in linear light refuses (convert to "
                    + "16 bits or set_evaluation \"encoded\" first). Where the blur changes alpha the "
                    + "pair is close, not exact. Retouch tone on the low layer and texture on the high "
                    + "one; Merge Down on the high layer bakes the pair. The selection is ignored. One "
                    + "undo step. CMYK, Indexed and Bitmap documents, a group, an adjustment layer, a "
                    + "clipped layer, a text, shape, fill or placed layer and a pixel-locked layer "
                    + "refuse by name. Replies low_layer, high_layer and high_blend_mode.",
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
