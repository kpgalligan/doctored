import Foundation

/// The catalog entry for the Magic Eraser (`AgentServer+MagicEraser.swift`), in its own file behind
/// an explicit non-inlining boundary for the reason `AgentCatalog.swift`'s header gives.
extension AgentServer {
    @inline(never) func magicEraserCatalog() -> [[String: Any]] {
        [
            tool("magic_eraser",
                 "The Magic Eraser tool's click: erase, on a layer, the region the Magic Wand "
                 + "selects from pixel (x, y). The region is the wand's — every pixel whose R, G, "
                 + "B, A, R−G, G−B and R−B each lie within tolerance of the seed's, four-connected "
                 + "to it when contiguous, over the layer's own pixels or, with "
                 + "sample_all_layers, over the composite — hard-edged, and inside the selection "
                 + "(its coverage scales the erase). Each pixel in it keeps its colour and takes "
                 + "alpha × (1 − opacity × coverage). One undo step. 8-, 16- and 32-bit RGB and "
                 + "Grayscale, Duotone, Lab, CMYK and Indexed documents (at 16 and 32 bits, and in "
                 + "the colour modes, tolerance is in 8-bit codes, as select_magic_wand's; at 32 "
                 + "bits the wand reads the display copy); an Indexed document's erased pixels take "
                 + "its transparent index, else the palette entry nearest white; a Bitmap document "
                 + "refuses (it has no Magic Wand region). Only a layer's pixels: a target other "
                 + "than layer, the Quick Mask, a group, an adjustment layer, a pixels lock and a "
                 + "transparency lock refuse by name, and so does blend_mode; a text, shape or fill "
                 + "layer is rasterized first. A seed off the canvas or outside the selection, or a "
                 + "region with nothing left to erase, answers changed false and adds no step. "
                 + "Answers {ok, changed, layer, storage}.",
                 ["x": ["type": "integer", "description": "Seed pixel, canvas x."],
                  "y": ["type": "integer", "description": "Seed pixel, canvas y."],
                  "layer": ["type": "integer",
                            "description": "Layer index; default the active layer."],
                  "tolerance": ["type": "number", "default": 32,
                                "description": "0–255, the Magic Wand's tolerance."],
                  "contiguous": ["type": "boolean", "default": true,
                                 "description": "Only pixels connected to the seed."],
                  "sample_all_layers": ["type": "boolean", "default": false,
                                        "description": "Grow the region over the composite "
                                            + "instead of the layer alone; the erase still "
                                            + "lands on the layer."],
                  "opacity": ["type": "number", "default": 1,
                              "description": "0–1: how much of each region pixel's alpha is "
                                  + "erased."],
                  "target": ["type": "string", "enum": ["layer"],
                             "description": "Always the layer."],
                  "document_id": AgentServer.docIDProperty],
                 required: ["x", "y"]),
        ]
    }
}
