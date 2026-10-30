import Foundation

extension AgentServer {
    /// The five retouch-brush stroke tools (handlers in AgentServer+RetouchBrushes.swift,
    /// 16-bit and 32-bit float twins in PrecisionRetouchBrushTools.swift), and the Art History
    /// Brush (AgentServer+ArtHistoryBrush.swift).
    func retouchBrushCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        let points: [String: Any] = [
            "type": "array",
            "description": "[[x, y], …] along the stroke, in canvas px.",
            "items": [
                "type": "array", "items": ["type": "number"], "minItems": 2, "maxItems": 2,
            ] as [String: Any],
            "minItems": 1, "maxItems": 10_000,
        ]
        let size: [String: Any] = [
            "type": "number", "description": "Stroke width in px (1-200, default 24).",
        ]
        let hardness: [String: Any] = [
            "type": "number", "minimum": 0, "maximum": 100,
            "description": "Edge hardness in percent (default 100 = crisp). Below 100 the "
                + "effect feathers out from hardness% of the radius to the rim.",
        ]
        let strength: [String: Any] = [
            "type": "number", "minimum": 0, "maximum": 100,
            "description": "Strength in percent (0-100, default 50).",
        ]
        // The shared stroke arguments; each tool adds its own amount.
        var stroke: [String: Any] = [
            "points": points, "size": size, "hardness": hardness,
            "flow": schema.flowProperty, "spacing": schema.spacingProperty,
            "angle": schema.angleProperty, "roundness": schema.roundnessProperty,
            "layer": schema.index, "document_id": schema.docID,
        ]
        let rewrites = " Errors on an adjustment layer. This rewrites the layer's "
            + "pixels, so a re-editable text or shape layer drops its description "
            + "(undo restores it)."
        // All five run on a layer of a 32-bit float document (the Sponge too),
        // in linear light with the range kept; each says what that means for it.
        let float = { (rule: String) -> String in
            " Respects the active selection. 8-, 16- and 32-bit float documents. In a 32-bit "
                + "float document it works on the layer's pixels only, in linear light with "
                + "colour never clamped to 0–1 (the reply's storage is \"rgba32_float\"): "
                + rule + rewrites
        }
        var history = stroke
        history["opacity"] = [
            "type": "number", "minimum": 0, "maximum": 100,
            "description": "How far the stroke paints the state back, in percent (0-100, "
                + "default 100 = exactly the state's pixels under full coverage).",
        ] as [String: Any]
        history["state_id"] = [
            "type": "string",
            "description": "The history state to paint from: an id from list_history. "
                + "Default: list_history's brush_source_id, the state the History panel "
                + "marks as the source, which is the oldest retained state until one is set.",
        ] as [String: Any]
        // The Art History Brush's options, beside the History Brush's source.
        var art = history
        art["opacity"] = [
            "type": "number", "minimum": 0, "maximum": 100,
            "description": "Opacity in percent (0-100, default 100).",
        ] as [String: Any]
        art["blend_mode"] = [
            "type": "string",
            "enum": ArtHistoryOptions.modes.map(ArtHistoryOptions.name(of:)),
            "description": "The tool's Mode (default Normal); any other name is "
                + "refused: \(ArtHistoryOptions.modeRefusal).",
        ] as [String: Any]
        art["style"] = [
            "type": "string", "enum": ArtHistoryStyle.allCases.map(\.rawValue),
            "description": "The strokes' shape (default tight_short): every style but dab heads "
                + "along the source's luminance contours, with the brighter side on its right; "
                + "tight styles hold their heading closely and their length (±20%), loose ones "
                + "jitter a little more and vary their length more (±50%); short, medium and "
                + "long set their length (about 0.5, 1.75 and 3 brush diameters of travel; "
                + "loose_long 2.25); dab lays single dabs; the curl styles turn as they go, "
                + "always the same way.",
        ] as [String: Any]
        art["area"] = [
            "type": "number", "minimum": 0, "maximum": 500,
            "description": "The disc each click's strokes start in, its diameter in px (0-500, "
                + "default 50); a larger area lays more strokes.",
        ] as [String: Any]
        art["tolerance"] = [
            "type": "number", "minimum": 0, "maximum": 100,
            "description": "In percent (0-100, default 0): a stroke starts only where the source "
                + "state differs from the layer by at least this much (the largest channel "
                + "difference, premultiplied), so 100 on an unchanged layer paints nothing.",
        ] as [String: Any]
        art["seed"] = [
            "type": "integer", "minimum": 0, "maximum": 4294967295,
            "description": "The random seed the strokes are drawn from (default 0): the same "
                + "seed and stroke paint the same pixels.",
        ] as [String: Any]
        stroke["strength"] = strength
        var sponge = stroke
        // Sample All Layers on Blur, Sharpen and Smudge (not the Sponge).
        var sampled = stroke
        sampled["sample_all_layers"] = [
            "type": "boolean",
            "description": "Sample All Layers (default false): read the visible picture made "
                + "by every visible layer (this one, those below and those above it; as they "
                + "composite, inside the canvas) instead of the layer alone, and write the "
                + "result into this layer — each covered pixel mixes toward it premultiplied, "
                + "alpha included, so an empty layer fills with the retouched picture and the "
                + "other layers stay untouched. A transparency lock keeps alpha. Refused with a mask or "
                + "channel target: \(AgentServer.sampleAllLayersTargetRefusal).",
        ] as [String: Any]
        sponge["mode"] = [
            "type": "string", "enum": ["desaturate", "saturate"],
            "description": "desaturate (default) moves colour toward its luma; saturate "
                + "moves it away.",
        ] as [String: Any]
        sponge["vibrance"] = [
            "type": "boolean",
            "description": "The Sponge's Vibrance checkbox (default false here; the options "
                + "bar's defaults on): the target becomes Image > Adjustments "
                + "> Vibrance's Vibrance stage at +100 (saturate) or -100 (desaturate), which "
                + "moves muted colours most and spares nearly saturated ones from clipping, "
                + "mixed by coverage × strength. In a 32-bit float document it runs through "
                + "the display encoding (as that adjustment does at float); in a CMYK document "
                + "on the C, M and Y inks with K held.",
        ] as [String: Any]
        return [
            tool(
                "blur_stroke",
                "Softens a layer's pixels where a round-capped stroke along points covers "
                    + "them — the Blur tool. Each covered pixel moves toward the Gaussian "
                    + "blur (sigma 2 px) of the layer's own pixels by coverage × strength; "
                    + "repeat the stroke to build the effect up. Alpha is never changed "
                    + "unless sample_all_layers is on."
                    + float("a pixel far above white spreads into its neighbours and "
                        + "lifts them above 1."),
                sampled, required: ["points"]),
            tool(
                "sharpen_stroke",
                "Sharpens a layer's pixels under a stroke — the Sharpen tool: an unsharp "
                    + "mask (sigma 1 px, amount 100%) applied by coverage × strength. "
                    + "Alpha is never changed unless sample_all_layers is on."
                    + float("an overshoot is kept, so an edge may go below zero or further "
                        + "above white, up to the stored bound of ±65504."),
                sampled, required: ["points"]),
            tool(
                "smudge_stroke",
                "Drags a layer's pixels along a stroke — the Smudge tool. The picture "
                    + "under the tip is carried in the stroke's direction, a fraction "
                    + "strength of the way at every pixel of travel, so the smear is "
                    + "continuous and does not depend on spacing; direction and order of "
                    + "points matter, and a single point moves nothing. Transparent areas "
                    + "smear too (alpha moves with colour) unless the layer's transparency "
                    + "is locked. With sample_all_layers the stroke smears the sampled "
                    + "picture into the layer over its part of the canvas, and a pixel the "
                    + "smear does not move keeps the layer's own. Long strokes with a large "
                    + "tip can exceed the brush work limit and are refused by name: use a "
                    + "shorter stroke or a smaller size."
                    + float("values above white are carried along the stroke as they are."),
                sampled, required: ["points"]),
            tool(
                "sponge_stroke",
                "Changes colour saturation under a stroke — the Sponge tool and its Mode: "
                    + "desaturate moves each covered pixel toward its Rec. 709 luma, "
                    + "saturate doubles its distance from it (clamped), by coverage × "
                    + "strength; vibrance takes the Vibrance target instead. Alpha is never "
                    + "changed. A mask or alpha channel target refuses "
                    + "at every depth: \(FloatRetouch.spongeCoverageRefusal)"
                    + float("the luma and the doubled chroma are taken over linear values, "
                        + "unclamped, and held to ±65504."),
                sponge, required: ["points"]),
            tool(
                "history_brush_stroke",
                "Paints a layer back toward what it held in an earlier history state — the "
                    + "History Brush. Under the stroke each pixel mixes (premultiplied, "
                    + "alpha included) toward the same layer's pixel at the same canvas "
                    + "position in that state, by coverage × opacity; where the state's "
                    + "layer held nothing the stroke clears, and the layer never grows. "
                    + "Refuses by name when the state does not hold the layer, or has a "
                    + "different canvas size or bit depth. Never recorded into an Action: "
                    + "a state id names this session's history."
                    + float("full coverage at opacity 100 restores the state's exact float "
                        + "samples, above white and below zero included."),
                history, required: ["points"]),
            tool(
                "art_history_brush_stroke",
                "Paints a layer from an earlier history state in stylized strokes — the Art "
                    + "History Brush and its options (Mode, Opacity, Style, Area, Tolerance). The "
                    + "first point is a click, then every point at least max(area/2, size) px "
                    + "from the last; each click lays the style's strokes, each starting at a "
                    + "random point of the area and coloured from the state's pixel there, "
                    + "composited through blend_mode at opacity. Seeded: the same seed and "
                    + "stroke give the same pixels. Paints from state_id, by default the "
                    + "History panel's source. Recorded into an Action without state_id, so a "
                    + "replay paints from that document's own source. It runs on "
                    + "8-bit documents only: 8-bit RGB and 8-bit CMYK (in ink) paint, and a "
                    + "16-bit, 32-bit or 16-bit CMYK document refuses it by name. A mask, "
                    + "channel or Quick Mask target refuses: "
                    + "\(AgentServer.artHistoryTargetRefusal). Respects the active selection."
                    + rewrites,
                art, required: ["points"]),
        ]
    }
}
