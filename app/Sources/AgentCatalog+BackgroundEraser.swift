import Foundation

/// The catalog entry for the Background Eraser (`AgentServer+BackgroundEraser.swift`), in its own
/// file behind an explicit non-inlining boundary for the reason `AgentCatalog.swift`'s header
/// gives.
extension AgentServer {
    @inline(never) func backgroundEraserCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        let points: [String: Any] = [
            "type": "array",
            "description": "[[x, y], …] along the stroke, in canvas px.",
            "items": [
                "type": "array", "items": ["type": "number"], "minItems": 2, "maxItems": 2,
            ] as [String: Any],
            "minItems": 1, "maxItems": 10_000,
        ]
        let hex = "#RRGGBB or #RRGGBBAA, sRGB"
        return [
            tool("background_eraser_stroke",
                 "The Background Eraser tool's stroke: under each dab, erase on a layer the "
                 + "pixels whose colour is like the dab's reference colour, and keep the rest. "
                 + "A dab's reference is the pixel under its centre (sampling continuous), the "
                 + "first such pixel of the stroke that is not fully transparent (once), or "
                 + "sample_color (background_swatch); a dab whose centre is off the canvas or "
                 + "on a fully transparent pixel has none (continuous) and erases nothing. A "
                 + "pixel is like the reference when the Magic Wand would select it from a seed "
                 + "of that colour — R, G and B within tolerance and the halved differences "
                 + "within half of it (select_magic_wand; a Lab "
                 + "document compares the Lab wand's key, a CMYK one the ink's naive RGB, a "
                 + "32-bit one the display copy) — read with alpha at full, so a half-erased "
                 + "pixel stays like its neighbours. contiguous reaches only the like pixels "
                 + "connected to the dab's centre under the tip (and a dab whose centre is not "
                 + "itself like the reference erases nothing); discontiguous reaches every like "
                 + "pixel under the tip. With protect_color, a pixel like it (same tolerance) is "
                 + "never erased and stops the contiguous walk. Each reached pixel keeps its "
                 + "colour and takes alpha × (1 − coverage), the tip's coverage accumulated over "
                 + "the dabs and gated by the selection. One undo step. 8-, 16- and 32-bit RGB "
                 + "and Grayscale, Duotone, Lab, CMYK and Indexed documents (tolerance in 8-bit "
                 + "codes at every depth); an Indexed document's erased pixels take its "
                 + "transparent index, else the palette entry nearest white; a Bitmap document "
                 + "refuses (it has no transparency to erase into). Only a layer's pixels: a "
                 + "target other than layer, the Quick Mask, a group, an adjustment layer, a "
                 + "pixels lock and a transparency lock refuse by name, and so do blend_mode, "
                 + "flow, opacity, airbrush, pressure_opacity and aliased; a text, shape or fill "
                 + "layer is rasterized first. More than 256 million work units (the footprint, "
                 + "and per dab its tip work and its box) refuses by name. A stroke that erases "
                 + "nothing answers changed false and adds no step.",
                 ["points": points,
                  "size": ["type": "number", "minimum": 1, "maximum": 1000,
                           "default": BackgroundEraser.defaultSize,
                           "description": "Brush diameter in px (1–1000)."],
                  "hardness": ["type": "number", "minimum": 0, "maximum": 100,
                               "description": "Edge hardness in percent (default 100)."],
                  "spacing": schema.spacingProperty, "angle": schema.angleProperty,
                  "roundness": schema.roundnessProperty,
                  "tolerance": ["type": "number", "minimum": 0, "maximum": 255, "default": 32,
                                "description": "0–255, the Magic Wand's tolerance."],
                  "limits": ["type": "string", "enum": ["contiguous", "discontiguous"],
                             "default": "contiguous",
                             "description": "Which like pixels under the tip a dab reaches."],
                  "sampling": ["type": "string",
                               "enum": ["continuous", "once", "background_swatch"],
                               "default": "continuous",
                               "description": "Where each dab's reference colour comes from."],
                  "sample_color": ["type": "string",
                                   "description": "The colour background_swatch sampling "
                                       + "erases (\(hex)); required with it, refused otherwise."],
                  "sample_ink": ["type": "array", "items": ["type": "number"],
                                 "description": "On a CMYK document, the swatch's ink exactly: "
                                     + "C, M, Y, K in percent 0–100."],
                  "protect_color": ["type": "string",
                                    "description": "Protect Foreground: a colour (\(hex)) whose "
                                        + "like pixels are never erased."],
                  "protect_ink": ["type": "array", "items": ["type": "number"],
                                  "description": "On a CMYK document, the protected ink exactly: "
                                      + "C, M, Y, K in percent 0–100."],
                  "layer": schema.index,
                  "target": ["type": "string", "enum": ["layer"],
                             "description": "Always the layer."],
                  "document_id": schema.docID],
                 required: ["points"]),
        ]
    }
}
