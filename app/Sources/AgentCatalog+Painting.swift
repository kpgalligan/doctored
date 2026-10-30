import Foundation

extension AgentServer {
    @inline(never)
    func paintingCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        let docID = schema.docID
        let index = schema.index
        let flowProperty = schema.flowProperty
        let spacingProperty = schema.spacingProperty
        let angleProperty = schema.angleProperty
        let roundnessProperty = schema.roundnessProperty
        let strokeBlendProperty = schema.strokeBlendProperty
        let targetVocabulary = schema.targetVocabulary
        let cloneBlendProperty = schema.cloneBlendProperty
        let sampleProperty = schema.sampleProperty
        let ignoreAdjustmentsProperty = schema.ignoreAdjustmentsProperty
        let retouchOutputProperty = schema.retouchOutputProperty
        return [
            tool(
                "brush_stroke",
                "Paints a brush stroke onto a layer's pixels: a smooth polyline through "
                    + "points (canvas coordinates) with round caps and joins. One point "
                    + "paints a dot. Draw shapes with several strokes; use render to check "
                    + "the result. With target: \"mask\" the same stroke paints the layer's "
                    + "mask instead, revealing what it covers.",
                [
                    "points": [
                        "type": "array",
                        "description": "[[x, y], …] along the stroke, in canvas px.",
                        "items": [
                            "type": "array", "items": ["type": "number"],
                            "minItems": 2, "maxItems": 2,
                        ],
                        "minItems": 1, "maxItems": 10_000,
                    ],
                    "size": [
                        "type": "number",
                        "description": "Stroke width in px (1-\(Int(EditorTool.brush.maximumBrushSize)), default 16).",
                    ],
                    "color": [
                        "type": "string",
                        "description": "Hex color, #RRGGBB or #RRGGBBAA (default #000000). "
                            + "Ignored when target is \"mask\".",
                    ],
                    "opacity": ["type": "number", "minimum": 0, "maximum": 1],
                    "hardness": [
                        "type": "number", "minimum": 0, "maximum": 100,
                        "description": "Edge hardness in percent (default 100 = crisp). "
                            + "Below 100 the stroke stamps soft round dabs whose alpha "
                            + "fades from hardness% of the radius out to the rim — an "
                            + "airbrushed edge.",
                    ],
                    "flow": flowProperty,
                    "spacing": spacingProperty,
                    "angle": angleProperty,
                    "roundness": roundnessProperty,
                    "blend_mode": strokeBlendProperty,
                    "layer": index,
                    "target": [
                        "type": "string",
                        "description": "What the stroke paints: " + targetVocabulary
                            + " On a mask the stroke REVEALS what it covers. Coverage is "
                            + "not color, so every target but \"layer\" forces the stroke "
                            + "to WHITE whatever color you pass, opacity becomes partial "
                            + "coverage, and blend_mode is not valid. \"mask\" needs a "
                            + "layer that already has one (add_layer_mask). On an "
                            + "ADJUSTMENT layer a layer or colour-plane stroke paints the "
                            + "mask instead, whatever this says — a channel target is "
                            + "document state and is never rerouted. A GROUP has no pixels "
                            + "of its own, so \"layer\" and the colour planes are refused "
                            + "on one, but its canvas-sized MASK is paintable exactly like "
                            + "any other and so is a channel.",
                    ],
                    "document_id": docID,
                ], required: ["points"]),
            tool(
                "eraser_stroke",
                "Erases along a polyline (same geometry as brush_stroke): pixels under the "
                    + "stroke become transparent. opacity is the eraser strength. With "
                    + "target: \"mask\" it hides through the layer's mask instead, leaving "
                    + "the pixels intact.",
                [
                    "points": [
                        "type": "array",
                        "description": "[[x, y], …] along the stroke, in canvas px.",
                        "items": [
                            "type": "array", "items": ["type": "number"],
                            "minItems": 2, "maxItems": 2,
                        ],
                        "minItems": 1, "maxItems": 10_000,
                    ],
                    "size": [
                        "type": "number",
                        "description": "Stroke width in px (1-\(Int(EditorTool.eraser.maximumBrushSize)), default 16).",
                    ],
                    "opacity": ["type": "number", "minimum": 0, "maximum": 1],
                    "hardness": [
                        "type": "number", "minimum": 0, "maximum": 100,
                        "description": "Edge hardness in percent (default 100 = crisp). "
                            + "Below 100 the erase feathers out from hardness% of the "
                            + "radius to the rim.",
                    ],
                    "flow": flowProperty,
                    "spacing": spacingProperty,
                    "angle": angleProperty,
                    "roundness": roundnessProperty,
                    "layer": index,
                    "target": [
                        "type": "string",
                        "description": "What the stroke erases: " + targetVocabulary
                            + " \"layer\" makes the layer's own pixels transparent; every "
                            + "other target is coverage, painted BLACK, so the mask HIDES "
                            + "what the stroke covers (the pixels stay intact — undo it by "
                            + "brushing with brush_stroke and the same target), a colour "
                            + "plane goes toward 0, and a channel deselects there. opacity "
                            + "becomes partial coverage. \"mask\" needs a layer that "
                            + "already has one (add_layer_mask). On an ADJUSTMENT layer a "
                            + "layer or colour-plane stroke paints the mask instead.",
                    ],
                    "document_id": docID,
                ], required: ["points"]),
            tool(
                "clone_stamp",
                "Clones pixels from one part of the picture onto a layer — the Clone Stamp "
                    + "tool. A snapshot of the sampled picture (sample; the whole composite "
                    + "by default), displaced by "
                    + "(first point − source), is painted through a round-capped stroke "
                    + "along points, so what lands under the stroke comes from a region the "
                    + "same offset away: source_x,source_y is what appears under the FIRST "
                    + "point, and the offset stays fixed along the stroke (aligned cloning). "
                    + "Respects the active selection. Errors on an adjustment layer (no "
                    + "pixels to rewrite). This rewrites the layer's pixels, so a "
                    + "re-editable text or shape layer drops its description "
                    + "(undo restores it).",
                [
                    "source_x": [
                        "type": "number",
                        "description": "Canvas x of the point cloned FROM — the pixels there "
                            + "land under the stroke's first point.",
                    ],
                    "source_y": [
                        "type": "number",
                        "description": "Canvas y of the point cloned FROM.",
                    ],
                    "points": [
                        "type": "array",
                        "description": "[[x, y], …] along the stroke, in canvas px.",
                        "items": [
                            "type": "array", "items": ["type": "number"],
                            "minItems": 2, "maxItems": 2,
                        ],
                        "minItems": 1, "maxItems": 10_000,
                    ],
                    "size": [
                        "type": "number",
                        "description": "Stroke width in px (1-\(Int(EditorTool.clone.maximumBrushSize)), default 24).",
                    ],
                    "opacity": [
                        "type": "number", "minimum": 0, "maximum": 1,
                        "description": "Strength of the cloned paint (default 1).",
                    ],
                    "hardness": [
                        "type": "number", "minimum": 0, "maximum": 100,
                        "description": "Edge hardness in percent (default 100 = crisp). "
                            + "Below 100 the cloned paint feathers out from hardness% of "
                            + "the dab radius to the rim, blending it into the "
                            + "surroundings.",
                    ],
                    "flow": flowProperty,
                    "spacing": spacingProperty,
                    "angle": angleProperty,
                    "roundness": roundnessProperty,
                    "blend_mode": cloneBlendProperty,
                    "sample": sampleProperty,
                    "ignore_adjustments": ignoreAdjustmentsProperty,
                    "sample_all_layers": [
                        "type": "boolean",
                        "description": "Legacy spelling of sample: true is all, false is "
                            + "current (default all — this tool has always sampled the "
                            + "composite). Prefer sample, which also offers "
                            + "current_and_below.",
                    ],
                    "output": retouchOutputProperty,
                    "footprint": ["type": "string", "enum": ["automatic", "dabs"],
                        "description": "Brush footprint (default automatic preserves existing Actions). dabs explicitly stamps the tip at its spacing, including a hard default tip; new canvas clone/heal recordings use it for exact replay."],
                    "expand_destination": ["type": "boolean", "description": "Explicitly expand the destination raster to include the canvas (default false). Canvas size is unchanged; transparency-locked targets refuse expansion."],
                    "layer": index,
                    "document_id": docID,
                    "source_transform": ["type": "array", "items": ["type": "number"], "minItems": 6, "maxItems": 6,
                        "description": "Optional [a,b,c,d,tx,ty] affine mapping destination canvas pixel centers to source canvas pixel centers; overrides the translation from source_x/source_y. Fractional offsets use the core's alpha-aware bilinear sampler."],
                    "source_layer": ["type": "integer", "description": "Optional distinct native 16-bit source raster with sample current. Refused in other sampling modes and in 8-bit mode."],
                ], required: ["source_x", "source_y", "points"]),
            tool(
                "dodge_burn",
                "Lightens (dodge) or, with burn: true, darkens a layer's pixels where a "
                    + "round-capped stroke along points covers them — the Dodge / Burn tool. "
                    + "exposure is the strength and range picks the tones that move most: "
                    + "shadows, midtones (default), or highlights. Respects the active "
                    + "selection; repeat the stroke to build the effect up. Errors on an "
                    + "adjustment layer (edit its parameters instead). This rewrites the "
                    + "layer's pixels, so a re-editable text or shape layer drops its description "
                    + "(undo restores it).",
                [
                    "points": [
                        "type": "array",
                        "description": "[[x, y], …] along the stroke, in canvas px.",
                        "items": [
                            "type": "array", "items": ["type": "number"],
                            "minItems": 2, "maxItems": 2,
                        ],
                        "minItems": 1, "maxItems": 10_000,
                    ],
                    "size": [
                        "type": "number",
                        "description": "Stroke width in px (1-\(Int(EditorTool.dodge.maximumBrushSize)), default 24).",
                    ],
                    "exposure": [
                        "type": "number", "minimum": 0, "maximum": 100,
                        "description": "Strength in percent (0-100, default 50).",
                    ],
                    "range": [
                        "type": "string",
                        "enum": ["shadows", "midtones", "highlights"],
                        "description": "Which tones the stroke moves most (default "
                            + "midtones).",
                    ],
                    "burn": [
                        "type": "boolean",
                        "description": "true darkens (burn) instead of lightening (dodge). "
                            + "Default false.",
                    ],
                    "protect_tones": [
                        "type": "boolean",
                        "description": "Protect Tones (default false): each pixel's channels move "
                            + "together — the largest by the tool's curve, the others keeping "
                            + "their ratios to it — so hue and saturation hold. At every depth; "
                            + "on a CMYK document the C/M/Y inks move together and K by the "
                            + "plain curve. A mask or channel target takes the plain curve.",
                    ],
                    "hardness": [
                        "type": "number", "minimum": 0, "maximum": 100,
                        "description": "Edge hardness in percent (default 100 = crisp). "
                            + "Below 100 the effect feathers out from hardness% of the "
                            + "radius to the rim.",
                    ],
                    "flow": flowProperty,
                    "spacing": spacingProperty,
                    "angle": angleProperty,
                    "roundness": roundnessProperty,
                    "layer": index,
                    "document_id": docID,
                ], required: ["points"]),
        ]
    }
}
