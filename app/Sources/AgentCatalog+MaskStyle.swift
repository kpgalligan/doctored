import Foundation

extension AgentServer {
    @inline(never)
    func maskStyleCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        let docID = schema.docID
        let index = schema.index
        return [
            tool(
                "add_layer_mask",
                "Gives a layer a mask: a grayscale coverage channel that gates the layer's "
                    + "alpha without touching its pixels (white shows, black hides, grays are "
                    + "partial), so hiding is non-destructive and reversible. A reveal_all or "
                    + "hide_all mask is the layer's size; a from_selection mask is CANVAS-sized "
                    + "at (0, 0) at every depth, never cropped to the layer. A linked mask "
                    + "moves, transforms and crops with its layer. Replaces any "
                    + "existing mask and enables it. Paint it afterwards with brush_stroke / "
                    + "eraser_stroke and target: \"mask\". A GROUP takes a mask too: its mask "
                    + "is CANVAS-sized, it gates everything inside the group per pixel — "
                    + "including an adjustment layer's reach over the layers BELOW the group, "
                    + "which is the \"group the adjustment layers and mask the group\" "
                    + "workflow — and it does not make the group composite as a unit.",
                [
                    "layer": index,
                    "kind": [
                        "type": "string",
                        "enum": ["reveal_all", "hide_all", "from_selection"],
                        "description": "reveal_all (default) starts fully white — nothing "
                            + "hidden yet; hide_all starts black — the layer disappears until "
                            + "you paint it back; from_selection builds the mask from the "
                            + "CURRENT selection (selected shows, the rest hides), so it "
                            + "requires an active selection from a select_* tool. Re-applying "
                            + "the identical selection mask replies changed: false.",
                    ],
                    "document_id": docID,
                ]),
            tool(
                "remove_layer_mask",
                "Removes a layer's mask. With apply: true the coverage is first BAKED into "
                    + "the layer's alpha — permanent, pixels lost, and it bakes regardless of "
                    + "whether the mask was enabled — so what the mask hid becomes really "
                    + "erased. With apply: false (the default) the mask is simply discarded "
                    + "and the layer is revealed in full again, pixels untouched. Because "
                    + "apply rewrites the alpha channel, a transparency lock refuses it; "
                    + "every other mask operation is gated only by Lock All. Answers at "
                    + "every depth: in a 32-bit float document apply multiplies each alpha "
                    + "by the coverage and keeps every linear RGB value exactly (the reply "
                    + "adds storage: rgba32_float).",
                [
                    "layer": index,
                    "apply": [
                        "type": "boolean",
                        "description": "Bake the mask into the layer's alpha before dropping "
                            + "it (default false = discard it).",
                    ],
                    "document_id": docID,
                ]),
            tool(
                "set_layer_mask_enabled",
                "Turns a layer's mask on or off. A disabled mask is kept (and saved) but "
                    + "ignored while compositing, so the layer shows in full — useful to "
                    + "compare with and without. Errors when the layer has no mask.",
                [
                    "layer": index,
                    "enabled": ["type": "boolean"],
                    "document_id": docID,
                ], required: ["enabled"]),
            tool(
                "set_layer_clipped",
                "Clips a layer to the one below it (a clipping mask) or releases "
                    + "it. A clipped layer only shows where the first UNCLIPPED layer "
                    + "beneath it AMONG ITS SIBLINGS has content — that base layer's alpha "
                    + "footprint gates the whole group, and the group blends as one unit "
                    + "with the base's blend mode and opacity. Grouping is positional and "
                    + "re-derived WITHIN a level: consecutive clipped layers above a base "
                    + "all clip to it, a clipped layer that is first in its group has no "
                    + "base and composites as if unclipped — a GROUP in that position keeps "
                    + "PASSING THROUGH, since a flag the compositor ignores never forces "
                    + "isolation — and reordering re-derives the groups with no extra "
                    + "bookkeeping. Hiding the base hides its group. "
                    + "CLIPPING A LAYER TO A GROUP makes that group composite as a unit, so "
                    + "an adjustment layer inside it stops reaching the layers below the "
                    + "group — a clip base needs its own alpha footprint, which a "
                    + "pass-through group does not have. "
                    + "Works in 8-bit and 16-bit documents. "
                    + "Pass `layers` instead of `layer` to clip or release several entries "
                    + "as ONE undo step — what the Layer menu does over a multi-selection. "
                    + "The set FOLDS, exactly as set_layer_lock does: an entry the document "
                    + "will not change (at 16 bits, one already in that state) is skipped "
                    + "and listed in `skipped`; the call is refused only when NO entry "
                    + "changed. "
                    + "Non-destructive and reversible: pixels are untouched either way.",
                [
                    "layer": index,
                    "layers": [
                        "type": "array", "items": ["type": "integer"],
                        "description": "Several layers, exclusive with `layer`/`index`. In "
                            + "a recorded Action this is {\"$layers\": \"selected\"}.",
                    ],
                    "clipped": [
                        "type": "boolean",
                        "description": "true clips the layer to the one below it in the "
                            + "same group; false releases it.",
                    ],
                    "document_id": docID,
                ], required: ["clipped"]),
            tool(
                "set_layer_style",
                "Replaces a layer's WHOLE layer style — the effect stack plus "
                    + "blending options — in one undoable step; the same object get_document "
                    + "reports as style. Shape: {\"fill_opacity\": 0..1 (default 1; scales "
                    + "the pixels, never the effects), \"blend_if\": null or {channel: "
                    + "gray|red|green|blue (on a CMYK document gray|cyan|magenta|yellow|black), "
                    + "this_layer: [lo0, lo1, hi0, hi1], underlying: "
                    + "[lo0, lo1, hi0, hi1], extra: optional [{channel, this_layer, "
                    + "underlying}, ...] — one more range per other channel, "
                    + "several Blend If channels at once, every range's weight multiplied; a "
                    + "channel named twice is refused}, with 0..255 ramps in order (weight "
                    + "fades in over lo0..lo1 and out over hi0..hi1; [0, 0, 255, 255] is full "
                    + "weight; full ranges are dropped and the rest reported in channel order), "
                    + "\"effects\": [ ... ]}. Effects: up to 10 each of drop_shadow, "
                    + "inner_shadow, stroke, color_overlay and gradient_overlay (stacked "
                    + "instances; within a type a LATER one draws on top; "
                    + "an 11th is refused by name), at most one of any other type, each with "
                    + "\"type\", \"enabled\" (default true), its own \"blend\" and "
                    + "\"opacity\" 0..1 — defaults in parentheses: drop_shadow (multiply, "
                    + "color #000000, opacity 0.75, angle 120, use_global_light true, distance "
                    + "5 px, spread 0..1 (0), size 0..250 px (5), layer_knocks_out true); "
                    + "inner_shadow (multiply, #000000, 0.75, angle 120, use_global_light "
                    + "true, distance 5, choke 0, size 5); outer_glow (screen, #ffffbe, 0.75, "
                    + "spread 0, size 5); inner_glow (screen, #ffffbe, 0.75, choke 0, size 5, "
                    + "source edge|center); stroke (normal, opacity 1, size 3, position "
                    + "outside|inside|center, fill_type color|gradient|pattern, color "
                    + "#000000, gradient, pattern_fill: an object of the pattern_overlay keys "
                    + "pattern/angle/scale/link_with_layer/phase, the same defaults, with "
                    + "its own echoed \"anchor\", read when fill_type is pattern); "
                    + "color_overlay (normal, #ff0000, 1); gradient_overlay "
                    + "(normal, 1, gradient); pattern_overlay (normal, 1, pattern "
                    + "{\"preset\": checkerboard|stripes|dots|grid} (checkerboard) or a custom "
                    + "tile {\"name\", \"width\", \"height\" 1..512, \"rgba\": standard base64 "
                    + "of width x height x 4 straight RGBA8 bytes}, angle: the Angle "
                    + "in degrees, any number normalized into -180..180 (0); rotates the tile "
                    + "about its origin, scale 0.01..10 (1 = 100 %), "
                    + "link_with_layer true, phase [x, y] px ([0, 0]); a linked tile starts "
                    + "at the canvas origin + phase + the layer's moves since it was made "
                    + "(Photoshop's fxrp) and then moves with the layer, and the reply echoes "
                    + "where the layer was made as \"anchor\": echo anchor to keep the tile, "
                    + "omit it to anchor where the layer was made; at 32 bits tile codes / "
                    + "255 are linear values; in CMYK each texel converts to ink as a colour "
                    + "overlay's colour does); "
                    + "bevel_emboss (style outer_bevel|inner_bevel|"
                    + "emboss|pillow_emboss, technique smooth|chisel_hard|chisel_soft "
                    + "(smooth), depth 1 = 100 %, direction up|down, size 5, "
                    + "soften 0..16 (0), angle 120, use_global_light true, altitude 0..90 "
                    + "(30), highlight_blend screen, highlight_color #ffffff, "
                    + "highlight_opacity 0.75, shadow_blend multiply, shadow_color #000000, "
                    + "shadow_opacity 0.75); satin (multiply, #000000, 0.5, angle 19, "
                    + "distance 11, size 14, invert true). Wide colours: beside \"color\" on "
                    + "drop_shadow, inner_shadow, outer_glow, inner_glow, stroke, "
                    + "color_overlay and satin, \"linear\" [r, g, b] (bevel_emboss: "
                    + "\"highlight_linear\" / \"shadow_linear\") gives the colour as "
                    + "linear-light sRGB components, extended range (negative outside "
                    + "sRGB's gamut, each clamped into -4..4): a Display P3 or 16-bit pick "
                    + "a hex cannot hold. One that is an sRGB byte is dropped for its hex; "
                    + "otherwise \"color\" becomes its nearest sRGB byte. Contours (Layer Style ▸ the "
                    + "Contour picker): drop_shadow, inner_shadow, outer_glow, inner_glow "
                    + "and satin take \"contour\" (linear) and \"anti_aliased\" (false); "
                    + "outer_glow and inner_glow take \"range\" 0.01..1 (0.5 = 50 %); "
                    + "bevel_emboss takes \"gloss_contour\" and "
                    + "\"gloss_anti_aliased\", and its Contour sub-panel \"contour_enabled\" "
                    + "(false), \"contour\", \"contour_anti_aliased\" and \"contour_range\" "
                    + "0.01..1 (0.5). A contour is {\"preset\": linear|cone|cone_inverted|"
                    + "gaussian|half_round|ring|ring_double|rolling_slope_descending|"
                    + "rounded_steps|sawtooth_1} or a custom curve {\"points\": [{\"x\", "
                    + "\"y\" 0..255, \"corner\": true ends a smooth run}, ...]} (2..16 "
                    + "points, sorted by x, a duplicate x keeping the first; the natural "
                    + "spline through them). Each contour key is written back only when it "
                    + "is not at its default; anti_aliased is kept but draws the same "
                    + "pixels. Noise (Layer Style ▸ the Noise slider): drop_shadow, "
                    + "inner_shadow, outer_glow and inner_glow take \"noise\" 0..1 (0 = "
                    + "0 %), a fixed grain, added to the effect's coverage, that moves with "
                    + "the layer, written back only when above 0. Texture (Bevel & "
                    + "Emboss ▸ Texture): bevel_emboss takes \"texture_enabled\" (false) "
                    + "and \"texture\": an object of the pattern_overlay keys pattern/"
                    + "scale/link_with_layer/phase (the same defaults; no angle) plus "
                    + "\"depth\" -10..10 (1 = +100 %) and \"invert\" (false), "
                    + "with its own echoed \"anchor\"; the tile's Gray is read as height "
                    + "and shades the bevel, painting no colour. The texture is written "
                    + "back when texture_enabled is true or it is not the default. "
                    + "Glow fill (Layer Style ▸ Outer Glow / Inner Glow, the gradient "
                    + "button and Jitter): outer_glow and inner_glow take \"fill_type\" "
                    + "color|gradient (color), \"gradient\" (its stops only: every other "
                    + "gradient key is reset) and \"jitter\" 0..1 (0 = 0 %). "
                    + "A gradient glow places location 0 where the glow is strongest and "
                    + "takes its coverage from the stops' opacity (Foreground to "
                    + "Transparent draws the colour glow); Jitter scatters each pixel's "
                    + "place on the gradient with a fixed field that moves with the layer. "
                    + "fill_type is written back only when gradient, gradient when the fill "
                    + "is gradient or its stops are not the default, jitter when above 0. "
                    + "A gradient is "
                    + "{stops: [{position 0..1, color, opacity, midpoint, opacity_midpoint}, ...] "
                    + "(2..32; midpoint and opacity_midpoint 0.05..0.95, default 0.5: the "
                    + "colour and opacity midpoints toward the next stop, rounded to 0.01, written "
                    + "back only when not 0.5, ignored on a glow; outside the range refused), "
                    + "style linear|radial|angle|"
                    + "reflected|diamond (and shape_burst on a stroke only: the ramp runs "
                    + "across the band, outer edge to inner), angle 90, scale 0.1..1.5, "
                    + "reverse false, align_with_layer true, dither false, smoothness 1 "
                    + "(Smoothness as a fraction: 1 the Classic ramp, 0 linear, "
                    + "between them the two blended; written back only when below 1)}. Blend names are "
                    + "the snake_case forms of the layer blend modes (normal, multiply, screen, linear_dodge, ...); "
                    + "colors are \"#rrggbb\"; angles are degrees, 0 = light from the right, "
                    + "90 = from the top (120 casts down-right); sizes are canvas px; "
                    + "spread/choke are fractions of size. Effects with use_global_light "
                    + "read the document's global light (set_global_light). Numbers are "
                    + "stored to 4 decimals; an unknown key is refused by name (an effect's "
                    + "mode key is `blend`), ranges are clamped, and "
                    + "an unknown type or enum value is an error whose message names the "
                    + "key. null (or an omitted style) clears; so does a style with no "
                    + "enabled effect, fill_opacity 1 and no blend_if. Idempotent: an "
                    + "unchanged style reports unchanged: true. The whole style replaces "
                    + "the previous one, so read it back from get_document first to change "
                    + "one knob. Effects render from the layer's shape (alpha × mask) at "
                    + "composite time — render shows them, layer renders and thumbnails do "
                    + "not — and follow moves, mask edits and text re-renders; "
                    + "transform_layer, distort_layer and image_size scale the px sizes "
                    + "(Scale Effects); merge_down and flatten_image bake them in. "
                    + "Interior effects use their own blend mode; the layer's blend mode "
                    + "applies to its pixels. An adjustment layer takes one too: its shape "
                    + "is its mask over the canvas (the whole canvas when unmasked), the "
                    + "adjustment applies after its own Drop Shadow and other below effects "
                    + "and keeps the backdrop's alpha, Blend If's This Layer reads the "
                    + "adjusted colour, and Fill scales the adjustment — on a CMYK "
                    + "document's adjustment layer too, on ink. Two more "
                    + "blending options sit beside fill_opacity and blend_if, and both are "
                    + "written back only when they are NOT at their default, so a style "
                    + "without them reads exactly as it always did: \"knockout\": "
                    + "\"none\"|\"shallow\"|\"deep\" (default \"none\") and "
                    + "\"blend_interior_as_group\": bool (default false). Because this "
                    + "tool takes the WHOLE style, adding one means reading the layer's "
                    + "current style from get_document and sending it back with the key "
                    + "merged in — sending the key alone clears everything else. Knockout "
                    + "makes the layer's shape punch a hole through a backdrop instead of "
                    + "compositing onto it: an isolated group's buffer stops BOTH shallow "
                    + "and deep (the hole then shows what lies beneath the group), a "
                    + "clipped layer punches to its clip base alone, shallow inside a "
                    + "pass-through group punches to what that group's contents started "
                    + "from, and deep punches to the Background layer — the bottom layer "
                    + "when it is a plain, visible, unmasked, unstyled, Normal, opacity-1, "
                    + "canvas-sized layer at (0,0) — or to transparency when there is "
                    + "none. blend_interior_as_group makes the LAYER's blend mode apply to "
                    + "inner_glow, satin and the two overlays — on a "
                    + "clipping base with visible clipped layers too, where those effects "
                    + "recolour the base and its clipped layers together. merge_down (Merge "
                    + "Down and Merge Layers) merges a knockout against its REAL backdrop; where none "
                    + "can be captured the hole bakes as transparency and the reply carries a "
                    + "note. flatten_image, stamp_visible and merge_visible bake it exactly, "
                    + "because all three bake the projection. Both keys are written to a PSD "
                    + "as Photoshop's knko and infx records and are SUPPORTED in 16-bit and "
                    + "32-bit float documents. On a "
                    + "16-bit or 32-bit float document the same style is carried on raster layers, "
                    + "adjustment layers and groups (a styled Pass Through group composites isolated) "
                    + "and rendered under the document's global light. In a 32-bit float document an effect's colour "
                    + "composites as its linear light and every effect blend mode must be one the "
                    + "float document admits on a layer ("
                    + NativeLayerBlending.floatModeNames.joined(separator: ", ")
                    + ") — any other, Screen (the 8/16-bit default of outer_glow, inner_glow and "
                    + "the bevel's highlight) "
                    + "included, is refused by name.",
                [
                    "layer": index,
                    "style": [
                        "type": ["object", "null"],
                        "description": "The whole style object described above, or null "
                            + "to clear.",
                    ],
                    "document_id": docID,
                ]),
            tool(
                "set_global_light",
                "Sets the document's global light — the direction every effect with "
                    + "use_global_light on (drop shadow, inner shadow, bevel & emboss by "
                    + "default) reads instead of its own angle, so one edit re-lights every "
                    + "styled layer. Either argument may be omitted; the light defaults to "
                    + "angle 120 (down-right shadows), altitude 30. Altitude is clamped to "
                    + "0..90, the angle normalized to -180..180, both stored to 4 decimals "
                    + "(so the value get_document reports echoes back as unchanged); an "
                    + "unchanged light reports unchanged: true. get_document reports "
                    + "global_light. Works on 8-bit, 16-bit and 32-bit float documents (one undo "
                    + "step at every depth).",
                [
                    "angle": [
                        "type": "number",
                        "description": "Degrees: 0 = light from the right, 90 = from the top.",
                    ],
                    "altitude": [
                        "type": "number", "minimum": 0, "maximum": 90,
                        "description": "Degrees above the canvas plane (bevel shading only).",
                    ],
                    "document_id": docID,
                ]),
        ]
    }
}
