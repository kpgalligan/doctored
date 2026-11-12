import Foundation

/// The catalog entries for `fill_selection` and `stroke_selection`, in
/// their own file behind an explicit non-inlining boundary for the reason
/// `AgentCatalog.swift`'s header gives.
extension AgentServer {
    /// `rgba32`, shared by both tools.
    static let linearInkProperty: [String: Any] = [
        "type": "array", "items": ["type": "number"], "minItems": 4, "maxItems": 4,
        "description": "32-bit float documents only, instead of color: [red, green, blue, "
            + "alpha] as linear light in the document's primaries (sample_color's "
            + "center_rgba32 / mean_rgba32), red, green and blue 0–65504 (above 1 is above "
            + "white) and alpha 0–1, used as they are. Takes no color_space; an 8- or 16-bit "
            + "document and Quick Mask refuse it.",
    ]

    @inline(never) func fillSelectionCatalog() -> [[String: Any]] {
        let description = [
            "Fills the active selection — or the whole target when nothing is selected — ",
            "with ONE colour through a blend mode at an opacity: Edit ▸ Fill…, and Edit ▸ ",
            "Fill with Foreground/Background Color (⌥⌫ / ⌘⌫; ⇧ keeps transparency). A ",
            "feathered selection fills in proportion to its coverage. At 8 bits the layer ",
            "never grows; at 16 and 32 bits a trimmed layer grows to the changed pixels, as the 16-bit ",
            "brush does — at neither depth under preserve_transparency or Lock Transparency. ",
            "On a mask or alpha channel the colour becomes its coverage gray (Rec. 709 luma). ",
            "A red, green, blue or alpha target takes ONE value in that channel alone, ",
            "0.30 r + 0.59 g + 0.11 b of the colour's document components (measured ",
            "against Photoshop, not the Rec. 709 gray: rounded to a code at 8 bits, unrounded at ",
            "16 and 32), at opacity × coverage whatever the pixel's alpha. On all of these ",
            "Hue/Saturation/Color/Luminosity refuse and ",
            "preserve_transparency refuses. ONE undo step, \"Fill\". A text or shape ",
            "layer is rasterized and the result says so. Groups, adjustment layers' ",
            "pixels, Lock Pixels / Lock All and a retained source refuse by name; ",
            "Lock Transparency keeps the layer's alpha exactly as preserve_transparency does. ",
            "Under either, every pixel keeps its alpha exactly: a transparent pixel stays ",
            "empty, and on a semi-transparent one the colour mixes in at the fill's own ",
            "opacity × coverage, not boosted by the kept alpha. ",
            "A fill that changes nothing answers changed:false with a note and adds no undo ",
            "step. 8-, 16-bit and 32-bit float documents (the 16-bit reply adds storage: ",
            "\"rgba16_unorm\" for a layer, \"u16\" for a mask, colour plane or channel). At ",
            "32-bit float the reply's storage is \"rgba32_float\" for a layer or a colour ",
            "plane and \"u16\" for a mask or alpha channel; the colour fills as linear light and ",
            "only ", NativeLayerBlending.floatModeNames.joined(separator: ", "),
            " are available on a layer. There a red, green, blue or alpha target takes its ",
            "0.30 r + 0.59 g + 0.11 b value unquantized (of rgba32's linear numbers when ",
            "rgba32 is passed), ",
            "in Normal, Dissolve, Darken, Multiply, Lighten, Linear Dodge (Add), Difference or ",
            "Subtract; Darker Color and Lighter Color refuse there by name. In a 32-bit float ",
            "document rgba32 replaces color with linear-light numbers, above white included ",
            "(a mask or alpha channel clips them to 0–1). In QUICK MASK (or with target ",
            "\"quick_mask\", which refuses by name outside Quick Mask) it fills the whole Quick ",
            "Mask coverage with the colour's coverage gray at every depth, 32-bit float included ",
            "(Edit ▸ Fill…, ⌥⌫ / ",
            "⌘⌫ in Quick Mask), ONE undo step \"Fill\" that leaves the file clean, and answers ",
            "get_selection's fields plus changed and target \"quick_mask\"; layer, any other ",
            "target and preserve_transparency are refused there. With pattern in place of a ",
            "colour it is Edit ▸ Fill…'s Pattern contents: the tile on the canvas grid at 100 %, ",
            "unturned — canvas pixel (x, y) takes texel (x mod width, y mod height) whatever the ",
            "layer's offset, its alpha multiplying the opacity and the coverage — read as the ",
            "document's Pattern Overlay reads a tile: RGB numbers as they are (at 32 bits code / ",
            "255 is linear light), Grayscale greyed, Lab and CMYK through sRGB, Indexed to each ",
            "pixel's nearest entry, Bitmap thresholded. Layer target only: a mask, colour plane, ",
            "alpha channel and the Quick Mask refuse it by name. The reply echoes pattern.",
        ].joined()
        let blendNames = DrBlendMode.allBlendModes.map { $0.1 }
        return [
            tool(
                "fill_selection", description,
                [
                    "color": [
                        "type": "string",
                        "description": "Fill colour, #RRGGBB or #RRGGBBAA (its alpha scales "
                            + "the fill). Required unless rgba32 or pattern is passed — the "
                            + "swatches are not read.",
                    ],
                    "rgba32": AgentServer.linearInkProperty,
                    "pattern": [
                        "type": "object",
                        "description": "Fill with a pattern instead of a colour (not with color, "
                            + "rgba32, color_space or ink): {\"preset\": "
                            + RasterDocument.patternPresets().map(\.id).joined(separator: "|")
                            + "}, {\"name\": <a list_patterns library pattern>} or a tile "
                            + "{name?, width, height, rgba} (sides 1–512, rgba standard base64 "
                            + "of width × height × 4 straight RGBA8 bytes). A recorded step "
                            + "carries the tile itself, so it replays without the library.",
                    ],
                    "color_space": [
                        "type": "string", "enum": ["srgb", "document", "display_p3"],
                        "description": "How color is read. srgb (default): an authored sRGB "
                            + "hex, converted once into the document. document: the hex IS "
                            + "the document's own bytes (sample_color's hex). display_p3: a "
                            + "Display P3 hex, converted once into the document.",
                    ],
                    "blend_mode": [
                        "type": "string", "enum": blendNames,
                        "description": "Blend mode (default Normal). The four HSL modes "
                            + "refuse on a mask, colour plane or alpha channel.",
                    ],
                    "opacity": [
                        "type": "number", "minimum": 0, "maximum": 1,
                        "description": "Fill opacity 0–1 (default 1), separate from the "
                            + "colour's alpha.",
                    ],
                    "preserve_transparency": [
                        "type": "boolean",
                        "description": "Keep the layer's alpha exactly as it is (default "
                            + "false); on a semi-transparent pixel the colour mixes in at the "
                            + "fill's own opacity × coverage, not boosted "
                            + "by the kept alpha. Lock Transparency does the same. Layer "
                            + "target only.",
                    ],
                    "layer": [
                        "type": "integer",
                        "description": "Layer index; default the active layer.",
                    ],
                    "target": [
                        "type": "string",
                        "description": "layer (default), mask, red, green, blue, alpha, "
                            + "channel:<name> or quick_mask (the Quick Mask coverage; the "
                            + "default while in Quick Mask, and what a recording made there "
                            + "saves).",
                    ],
                    "document_id": AgentServer.docIDProperty,
                ]),
        ]
    }

    @inline(never) func strokeSelectionCatalog() -> [[String: Any]] {
        let description = [
            "Strokes the active selection's edge with ONE colour through a blend mode at an ",
            "opacity: Edit ▸ Stroke…. The band is width px wide, inside, centred on (default) ",
            "or outside the selection's 50% contour — Layer Style ▸ Stroke's bands, so the two ",
            "agree pixel for pixel. For inside (and center's inner half) the canvas edge is an ",
            "edge too, so select_all + inside strokes a frame; select_all + outside has nothing ",
            "to stroke and answers changed:false with a note. With nothing selected it strokes ",
            "the TRUE edge of the layer's own pixels (its alpha; the mask is excluded), in ",
            "layer space, including any part of the layer beyond the canvas — layer target ",
            "only: a mask, colour plane or alpha channel needs a selection. The reply's outline ",
            "is \"selection\" or \"layer_edge\"; an empty layer answers changed:false with a ",
            "note. A layer-edge band grows the layer to cover it in the same undo step, past ",
            "the canvas if the band reaches there (a stroke wholly off the canvas still answers ",
            "changed:true); a selection's band is bounded by the canvas, and one that reaches ",
            "past a trimmed or offset layer's pixels grows that layer to cover the canvas, so ",
            "Outside and Center strokes land at full width. Neither grows with ",
            "preserve_transparency or Lock Transparency, which keep the layer's alpha exactly; ",
            "on a semi-transparent pixel the colour then mixes in at the stroke's own ",
            "opacity × coverage, not boosted by the kept alpha. On a ",
            "mask, colour plane or alpha channel the colour becomes fill_selection's gray for ",
            "that target, Hue/Saturation/Color/Luminosity refuse and preserve_transparency ",
            "refuses. ONE undo step, \"Stroke\". Refusals, rasterizing and Lock Transparency ",
            "are fill_selection's, and Quick Mask refuses by name. A stroke that changes ",
            "nothing answers changed:false with a note and adds no undo step. 8-, 16-bit and ",
            "32-bit float documents (the 16-bit reply adds storage: \"rgba16_unorm\" for a ",
            "layer, \"u16\" for a mask, colour plane or channel; at 16 and 32 bits a selection's ",
            "band grows a trimmed layer only to the pixels it changes). At 32-bit float the ",
            "reply's storage is \"rgba32_float\" for a layer or a colour plane and \"u16\" for a ",
            "mask or alpha channel, only ",
            NativeLayerBlending.floatModeNames.joined(separator: ", "),
            " are available on a layer, a red, green, blue or alpha target strokes that one ",
            "channel with fill_selection's value, modes and refusals, and rgba32 replaces ",
            "color with linear-light numbers, above white included.",
        ].joined()
        let blendNames = DrBlendMode.allBlendModes.map { $0.1 }
        return [
            tool(
                "stroke_selection", description,
                [
                    "color": [
                        "type": "string",
                        "description": "Stroke colour, #RRGGBB or #RRGGBBAA (its alpha scales "
                            + "the stroke). Required unless rgba32 is passed — the swatches "
                            + "are not read.",
                    ],
                    "rgba32": AgentServer.linearInkProperty,
                    "color_space": [
                        "type": "string", "enum": ["srgb", "document", "display_p3"],
                        "description": "How color is read. srgb (default): an authored sRGB "
                            + "hex, converted once into the document. document: the hex IS "
                            + "the document's own bytes (sample_color's hex). display_p3: a "
                            + "Display P3 hex, converted once into the document.",
                    ],
                    "width": [
                        "type": "number", "minimum": 1, "maximum": 250,
                        "description": "Stroke width in pixels, 1–250. Required.",
                    ],
                    "location": [
                        "type": "string", "enum": StrokeLocation.allCases.map(\.agentName),
                        "description": "Where the band sits relative to the selection's "
                            + "edge (default center).",
                    ],
                    "blend_mode": [
                        "type": "string", "enum": blendNames,
                        "description": "Blend mode (default Normal). The four HSL modes "
                            + "refuse on a mask, colour plane or alpha channel.",
                    ],
                    "opacity": [
                        "type": "number", "minimum": 0, "maximum": 1,
                        "description": "Stroke opacity 0–1 (default 1), separate from the "
                            + "colour's alpha.",
                    ],
                    "preserve_transparency": [
                        "type": "boolean",
                        "description": "Keep the layer's alpha exactly as it is (default "
                            + "false); on a semi-transparent pixel the colour mixes in at the "
                            + "stroke's own opacity × coverage, not boosted "
                            + "by the kept alpha. Lock Transparency does the same. Layer "
                            + "target only.",
                    ],
                    "layer": [
                        "type": "integer",
                        "description": "Layer index; default the active layer.",
                    ],
                    "target": [
                        "type": "string",
                        "description": "layer (default), mask, red, green, blue, alpha or "
                            + "channel:<name>.",
                    ],
                    "document_id": AgentServer.docIDProperty,
                ], required: ["width"]),
        ]
    }
}
