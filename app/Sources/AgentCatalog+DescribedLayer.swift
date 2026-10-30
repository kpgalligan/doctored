import Foundation

extension AgentServer {
    @inline(never)
    func describedLayerCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        let docID = schema.docID
        let index = schema.index
        let textWeightProperty = schema.textWeightProperty
        let textItalicProperty = schema.textItalicProperty
        let textUnderlineProperty = schema.textUnderlineProperty
        let textStrikethroughProperty = schema.textStrikethroughProperty
        let textTrackingProperty = schema.textTrackingProperty
        let textLeadingProperty = schema.textLeadingProperty
        let textBaselineProperty = schema.textBaselineProperty
        let transformProperty = schema.transformProperty
        // The colour rule every text/shape tool states for 32-bit documents.
        let floatInk = "In a 32-bit float document colours are the document's own linear-light "
            + "numbers (#2E80C8 is 46/255 linear); red, green "
            + "and blue may be above white, 0–65504, and alpha stays 0–1 (8/16-bit "
            + "documents keep 0–1). A layer pasted into or converted out of 32 bits has "
            + "its numbers rewritten so it looks the same; converting to 16 or 8 bits "
            + "clips each channel at 1. get_document reports `ink`."
        return [
            tool(
                "add_text",
                "Rasterizes text onto a layer's pixels — the characters become pixels and "
                    + "cannot be changed afterwards, so prefer add_text_layer when the text "
                    + "may need editing. x,y is the TOP-LEFT corner of the text block; long "
                    + "lines wrap at the canvas edge and \\n starts a new line. Returns the "
                    + "rendered text size so you can position follow-ups.",
                [
                    "text": ["type": "string"],
                    "x": ["type": "number"], "y": ["type": "number"],
                    "size": [
                        "type": "number",
                        "description": "Font size in px (4-1000, default 48).",
                    ],
                    "font": [
                        "type": "string",
                        "description": "Font family or PostScript name (default: system font).",
                    ],
                    "color": [
                        "type": "string",
                        "description": "Hex color, #RRGGBB or #RRGGBBAA (default #000000).",
                    ],
                    "color_space": [
                        "type": "string", "enum": ["srgb", "document", "display_p3"],
                        "description": "8-bit documents: how the color hex is read. Default "
                            + "srgb (authored sRGB); document: the hex IS the document's own "
                            + "bytes (sample_color's hex); display_p3: a Display P3 hex. "
                            + "add_text is unavailable at 16 bits and float.",
                    ],
                    "layer": index,
                    "document_id": docID,
                ], required: ["text", "x", "y"]),
            tool(
                "add_text_layer",
                "Adds a RE-EDITABLE text layer above the active layer and selects it. The "
                    + "layer remembers the string, font, size, color, alignment, typography "
                    + "(weight, italic, tracking, leading, baseline shift, underline, "
                    + "strikethrough), wrap width and transform it was rendered from "
                    + "(get_document reports them, edit_text_layer changes them, "
                    + "transform_layer composes onto them, and they survive saving to .doctored), "
                    + "unlike add_text which just bakes characters into pixels. x,y is the "
                    + "TOP-LEFT corner of the text block in canvas px (its origin in "
                    + "get_document; rounded to whole px for an untransformed block, kept "
                    + "exact under a transform), positioned exactly like add_text; \\n "
                    + "starts a new line and long lines wrap at wrap_width. Returns the new "
                    + "layer's index and bounds. NOTE: painting on the layer afterwards "
                    + "(brush, eraser, fill, gradient, add_text, apply_filter) drops the "
                    + "text and leaves plain pixels. "
                    + floatInk + " "
                    + LayerInsertPosition.catalogSentence,
                [
                    "text": ["type": "string"],
                    "rgba": ["type": "array", "items": ["type": "number"], "minItems": 4, "maxItems": 4,
                        "description": "Continuous normalized sRGB, alternative to color, preserved at both depths. sample_color's srgb_rgba is this spelling of a sampled pixel."],
                    "color_runs": ["type": "array", "maxItems": 64, "items": ["type": "object"],
                        "description": "Sorted nonoverlapping {location,length,rgba} UTF-16 complete-character ranges; [] clears runs. In a CMYK document a run may pass ink (C, M, Y, K percent) for rgba."],
                    "ink": Self.cmykInk("the text colour"),
                    "x": ["type": "number"], "y": ["type": "number"],
                    "size": [
                        "type": "number",
                        "description": "Font size in px (4-1000, default 48).",
                    ],
                    "font": [
                        "type": "string",
                        "description": "Installed font FAMILY name, e.g. \"Helvetica Neue\" "
                            + "or \"Times New Roman\" (not a PostScript face name). "
                            + "Default: the text tool's own default family.",
                    ],
                    "color": [
                        "type": "string",
                        "description": "Hex color, #RRGGBB or #RRGGBBAA (default #000000).",
                    ],
                    "alignment": [
                        "type": "string",
                        "enum": TextLayerPayload.alignments,
                        "description": "How lines align within the text block (default "
                            + "left). The block stays anchored at x,y — only the lines "
                            + "inside it shift, so single-line text looks the same for all "
                            + "three values.",
                    ],
                    "weight": textWeightProperty,
                    "italic": textItalicProperty,
                    "underline": textUnderlineProperty,
                    "strikethrough": textStrikethroughProperty,
                    "tracking": textTrackingProperty,
                    "leading": textLeadingProperty,
                    "baseline_shift": textBaselineProperty,
                    "transform": transformProperty(
                        "applied to the text before it is placed at x,y"),
                    "wrap_width": [
                        "type": "number",
                        "description": "px; the block wraps at this SOURCE-space width and "
                            + "remembers it (box_width); 0 = point text that never wraps "
                            + "(at most 100000 px, the app's own editor cap). Default: "
                            + "from x to the canvas's right edge.",
                    ],
                    "document_id": docID,
                ].merging(LayerInsertPosition.catalogProperties) { current, _ in current },
                required: ["text", "x", "y"]),
            tool(
                "edit_text_layer",
                "Re-renders a text layer made by add_text_layer (or by the app's text tool) "
                    + "from changed parameters: pass any subset of text, font, size, color, "
                    + "alignment, weight, italic, underline, strikethrough, tracking, "
                    + "leading, baseline_shift, wrap_width and transform, and everything you "
                    + "omit keeps the layer's current value. The layer keeps its position "
                    + "(the block is re-laid-out from its origin, keeping its transform "
                    + "unless you pass one, so the bounds follow the new text), its "
                    + "opacity, blend mode, mask, style and stacking. Errors when the target "
                    + "layer is not a text layer — add_text_layer makes one. Returns the "
                    + "resulting bounds and the layer's full text object. "
                    + floatInk,
                [
                    "layer": index,
                    "text": ["type": "string"],
                    "rgba": ["type": "array", "items": ["type": "number"], "minItems": 4, "maxItems": 4,
                        "description": "Continuous normalized sRGB, alternative to color, preserved at both depths. sample_color's srgb_rgba is this spelling of a sampled pixel."],
                    "color_runs": ["type": "array", "maxItems": 64, "items": ["type": "object"],
                        "description": "Sorted nonoverlapping {location,length,rgba} UTF-16 complete-character ranges; [] clears runs. In a CMYK document a run may pass ink (C, M, Y, K percent) for rgba."],
                    "ink": Self.cmykInk("the text colour"),
                    "size": [
                        "type": "number",
                        "description": "Font size in px (4-1000).",
                    ],
                    "font": [
                        "type": "string",
                        "description": "Installed font FAMILY name (not a PostScript face "
                            + "name).",
                    ],
                    "color": [
                        "type": "string",
                        "description": "Hex color, #RRGGBB or #RRGGBBAA.",
                    ],
                    "alignment": [
                        "type": "string",
                        "enum": TextLayerPayload.alignments,
                        "description": "How lines align within the text block; omit to keep "
                            + "the layer's current alignment.",
                    ],
                    "weight": textWeightProperty,
                    "italic": textItalicProperty,
                    "underline": textUnderlineProperty,
                    "strikethrough": textStrikethroughProperty,
                    "tracking": textTrackingProperty,
                    "leading": textLeadingProperty,
                    "baseline_shift": textBaselineProperty,
                    "transform": transformProperty(
                        "replacing the layer's current one (default: keep it)"),
                    "wrap_width": [
                        "type": "number",
                        "description": "px; the block wraps at this SOURCE-space width and "
                            + "remembers it (box_width); 0 = point text that never wraps "
                            + "(at most 100000 px, the app's own editor cap). Default: keep "
                            + "the layer's stored width — a layer saved before "
                            + "widths were stored (box_width null in get_document) re-wraps "
                            + "from its origin to the canvas's right edge, as this tool "
                            + "always did (the app's own editor caps that legacy default at "
                            + "600 px; the two differ only for lines wider than that), and "
                            + "stores that width from then on.",
                    ],
                    "document_id": docID,
                ]),
            tool(
                "add_shape_layer",
                "Adds a RE-EDITABLE shape layer above the active layer and selects it — "
                    + "the same parametric layers the app's shape tools drag out: the kind, "
                    + "box size, fill, stroke and corner radius become the layer's "
                    + "description and the pixels are only their rendering. x,y is the "
                    + "TOP-LEFT corner of the shape box in canvas px (its origin in "
                    + "get_document) and w,h its size; the shape lands exactly there (the "
                    + "layer's raster is a few px larger on every side for stroke overhang "
                    + "and antialiasing), through transform when one is given. rect and ellipse "
                    + "take fill and/or stroke — pass at least one; a line is stroke-only "
                    + "and runs across the box's diagonal, top-left to bottom-right, or "
                    + "bottom-left to top-right with flipped: true. Returns the new layer's "
                    + "index and name. NOTE: painting on the layer afterwards drops the "
                    + "shape description and leaves plain pixels. "
                    + floatInk + " "
                    + LayerInsertPosition.catalogSentence,
                [
                    "kind": [
                        "type": "string",
                        "enum": ShapeLayerPayload.kinds,
                        "description": "What to draw: rect, ellipse, or line.",
                    ],
                    "x": [
                        "type": "number",
                        "description": "Shape box top-left x, canvas px.",
                    ],
                    "y": [
                        "type": "number",
                        "description": "Shape box top-left y, canvas px.",
                    ],
                    "w": [
                        "type": "number",
                        "description": "Shape box width in px (>= 1; a line may have 0 on "
                            + "one axis).",
                    ],
                    "h": [
                        "type": "number",
                        "description": "Shape box height in px (>= 1; a line may have 0 on "
                            + "one axis).",
                    ],
                    "flipped": [
                        "type": "boolean",
                        "description": "Line only (default false): false runs top-left to "
                            + "bottom-right across the box, true bottom-left to top-right.",
                    ],
                    "fill": [
                        "type": "string",
                        "description": "Hex fill color, #RRGGBB or #RRGGBBAA; omit for no "
                            + "fill. Ignored for lines.",
                    ],
                    "stroke": [
                        "type": "string",
                        "description": "Hex stroke color, #RRGGBB or #RRGGBBAA; omit for no "
                            + "stroke (a line requires one).",
                    ],
                    "fill_rgba": ["type": ["array", "null"], "items": ["type": "number"], "minItems": 4, "maxItems": 4,
                        "description": "Continuous normalized sRGB fill, alternative to fill; null removes it. sample_color's srgb_rgba is this spelling of a sampled pixel."],
                    "stroke_rgba": ["type": ["array", "null"], "items": ["type": "number"], "minItems": 4, "maxItems": 4,
                        "description": "Continuous normalized sRGB stroke, alternative to stroke; null removes it. sample_color's srgb_rgba is this spelling of a sampled pixel."],
                    "fill_ink": Self.cmykInk("the fill"),
                    "stroke_ink": Self.cmykInk("the stroke"),
                    "stroke_width": [
                        "type": "number",
                        "description": "Stroke width in px, centered on the path (0-200, "
                            + "default 2; a line needs at least 1).",
                    ],
                    "radius": [
                        "type": "number",
                        "description": "Rect corner radius in px (default 0 = square "
                            + "corners; ignored by ellipse and line).",
                    ],
                    "transform": transformProperty(
                        "applied to the box before it is placed — the box's top-left stays "
                            + "at x,y"),
                    "document_id": docID,
                ].merging(Self.shapeStrokeStyleProperties("default")) { current, _ in current }
                    .merging(LayerInsertPosition.catalogProperties) { current, _ in current },
                required: ["kind", "x", "y", "w", "h"]),
            tool(
                "edit_shape_layer",
                "Changes an existing SHAPE layer's description and re-renders its pixels "
                    + "— the box, fill, stroke, stroke width, corner radius, line "
                    + "direction or transform — as one undo step, the way double-clicking "
                    + "the layer in the app reopens it. Works only on layers get_document "
                    + "reports a \"shape\" object for; the kind (rect / ellipse / line) is "
                    + "fixed at creation. Omitted arguments keep the layer's current values "
                    + "(x,y default to its origin, transform to its current map); the mask "
                    + "and style ride along. Pass fill or stroke as \"\" to remove that "
                    + "paint (at least one visible paint must remain). "
                    + floatInk,
                [
                    "layer": index,
                    "x": [
                        "type": "number",
                        "description": "New shape box top-left x, canvas px (default: keep).",
                    ],
                    "y": [
                        "type": "number",
                        "description": "New shape box top-left y, canvas px (default: keep).",
                    ],
                    "w": [
                        "type": "number",
                        "description": "New box width in px (default: keep).",
                    ],
                    "h": [
                        "type": "number",
                        "description": "New box height in px (default: keep).",
                    ],
                    "flipped": [
                        "type": "boolean",
                        "description": "Line only: false runs top-left to bottom-right "
                            + "across the box, true bottom-left to top-right (default: "
                            + "keep).",
                    ],
                    "fill": [
                        "type": "string",
                        "description": "Hex fill color, #RRGGBB or #RRGGBBAA; \"\" removes "
                            + "the fill (default: keep). Ignored for lines.",
                    ],
                    "stroke": [
                        "type": "string",
                        "description": "Hex stroke color, #RRGGBB or #RRGGBBAA; \"\" removes "
                            + "the stroke (default: keep; a line requires one).",
                    ],
                    "fill_rgba": ["type": ["array", "null"], "items": ["type": "number"], "minItems": 4, "maxItems": 4,
                        "description": "Continuous normalized sRGB fill, alternative to fill; null removes it. sample_color's srgb_rgba is this spelling of a sampled pixel."],
                    "stroke_rgba": ["type": ["array", "null"], "items": ["type": "number"], "minItems": 4, "maxItems": 4,
                        "description": "Continuous normalized sRGB stroke, alternative to stroke; null removes it. sample_color's srgb_rgba is this spelling of a sampled pixel."],
                    "fill_ink": Self.cmykInk("the fill"),
                    "stroke_ink": Self.cmykInk("the stroke"),
                    "stroke_width": [
                        "type": "number",
                        "description": "Stroke width in px, centered on the path (0-200; "
                            + "default: keep).",
                    ],
                    "radius": [
                        "type": "number",
                        "description": "Rect corner radius in px (default: keep; ignored by "
                            + "ellipse and line).",
                    ],
                    "transform": transformProperty(
                        "replacing the layer's current one (default: keep it)"),
                    "document_id": docID,
                ].merging(Self.shapeStrokeStyleProperties("default: keep; new shapes use")) { current, _ in current }),
            // Layer ▸ New Fill Layer and the fill-layer dialog reopened from
            // the Layers panel (AgentServer+FillLayers.swift).
            tool(
                "add_fill_layer",
                "Adds a FILL layer and selects it: one color (kind solid), one gradient "
                    + "(kind gradient) or one tiled pattern (kind pattern) over the whole "
                    + "canvas, re-editable with edit_fill_layer. A fill layer always covers "
                    + "the canvas: it is "
                    + "re-rendered when the canvas is cropped, resized or rotated; a quarter turn or "
                    + "a flip of the canvas also turns a linked pattern fill's tile into a turned "
                    + "custom tile under the same name. A solid or "
                    + "gradient fill cannot be moved or transformed; a pattern fill can "
                    + "(move_layers, an offset, transform_layer): it is re-rendered over the "
                    + "canvas, its mask moves, a linked tile follows the layer and an unlinked "
                    + "one stays on the canvas origin, and a transform keeps its Scale and "
                    + "Angle. Align and distribute refuse every fill. To show it in part of the image, give "
                    + "it a mask (add_layer_mask, then paint or fill with target \"mask\"). "
                    + "Colors are authored sRGB at 8, 16 and 32 bits; in a CMYK document "
                    + "they are stored as ink (pass ink for exact C, M, Y, K, else the colour "
                    + "separates through the document's profile); a pattern fill's tile separates "
                    + "to ink at render, as a Pattern Overlay's does. Returns the new "
                    + "layer's index and name. NOTE: painting on the layer's own pixels "
                    + "drops the fill description and leaves plain pixels. "
                    + LayerInsertPosition.catalogSentence,
                [
                    "kind": [
                        "type": "string", "enum": ["solid", "gradient", "pattern"],
                        "description": "solid takes color; gradient takes gradient; pattern "
                            + "takes pattern.",
                    ],
                    "pattern": Self.patternFillProperty(
                        "Pattern only (default: checkerboard, angle 0, scale 1, linked, phase "
                            + "[0, 0])."),
                    "color": [
                        "type": "string",
                        "description": "Solid only: hex color, #RRGGBB or #RRGGBBAA.",
                    ],
                    "ink": Self.cmykInk("a solid fill's colour"),
                    "gradient": [
                        "type": "object",
                        "description": "Gradient only. stops is required: 2 to 32 colors, each at a position 0 to 1 with its own opacity and optional midpoints. style defaults to linear, angle to 90, scale to 1, reverse to false, smoothness to 1 and align_with_layer to false. The gradient spans the whole canvas; with align_with_layer true it spans the bounds of the layer's mask (none yet on a new layer: add_layer_mask, then edit_fill_layer re-renders it over the mask).",
                        "properties": [
                            "stops": [
                                "type": "array", "minItems": 2, "maxItems": 32,
                                "items": [
                                    "type": "object",
                                    "properties": [
                                        "position": ["type": "number", "minimum": 0, "maximum": 1],
                                        "color": [
                                            "type": "string",
                                            "description": "Hex color, #RRGGBB or #RRGGBBAA (or ink in a CMYK document).",
                                        ],
                                        "opacity": [
                                            "type": "number", "minimum": 0, "maximum": 1,
                                            "description": "Default 1.",
                                        ],
                                        "midpoint": Self.stopMidpointProperty("colour"),
                                        "opacity_midpoint": Self.stopMidpointProperty("opacity"),
                                        "ink": Self.cmykInk("the stop's colour"),
                                    ],
                                    "required": ["position"],
                                ],
                            ],
                            "style": ["type": "string", "enum": GradientFill.styles],
                            "angle": [
                                "type": "number",
                                "description": "Degrees; 0 runs left to right, 90 bottom to top.",
                            ],
                            "scale": ["type": "number", "minimum": 0.1, "maximum": 1.5],
                            "reverse": ["type": "boolean"],
                            "smoothness": [
                                "type": "number", "minimum": 0, "maximum": 1,
                                "description": "Smoothness as a fraction: 1 "
                                    + "(100 %) is the Classic ramp, 0 the linear one, between "
                                    + "them the two blended.",
                            ],
                            "align_with_layer": [
                                "type": "boolean",
                                "description": "Align with Layer: true spans the "
                                    + "gradient over the bounds of the layer mask's coverage "
                                    + "(the canvas without a mask); false over the canvas. A "
                                    + "later mask edit takes effect at the next re-render.",
                            ],
                        ],
                    ],
                    "name": [
                        "type": "string",
                        "description": "The layer's name (default Color Fill, Gradient Fill or "
                            + "Pattern Fill).",
                    ],
                    "document_id": docID,
                ].merging(LayerInsertPosition.catalogProperties) { current, _ in current },
                required: ["kind"]),
            tool(
                "edit_fill_layer",
                "Changes an existing FILL layer's color, gradient or pattern and re-renders it "
                    + "over the whole canvas (an aligned gradient over its mask's bounds, as "
                    + "they are now), as one undo step, the way double-clicking the "
                    + "layer in the app (or Layer > Layer Content Options...) reopens its "
                    + "dialog. Works only on layers get_document reports a \"fill\" object "
                    + "for; the kind (solid / gradient / pattern) is fixed at creation. An "
                    + "omitted argument, and an omitted key inside gradient or pattern, keeps "
                    + "the layer's current value; the mask and style ride along.",
                [
                    "layer": index,
                    "color": [
                        "type": "string",
                        "description": "Solid fill only: hex color, #RRGGBB or #RRGGBBAA "
                            + "(default: keep).",
                    ],
                    "ink": Self.cmykInk("a solid fill's colour"),
                    "pattern": Self.patternFillProperty(
                        "Pattern fill only: the keys to change (default: keep each)."),
                    "gradient": [
                        "type": "object",
                        "description": "Gradient fill only: the keys to change (default: keep each). stops replaces the whole ramp.",
                        "properties": [
                            "stops": [
                                "type": "array", "minItems": 2, "maxItems": 32,
                                "items": [
                                    "type": "object",
                                    "properties": [
                                        "position": ["type": "number", "minimum": 0, "maximum": 1],
                                        "color": [
                                            "type": "string",
                                            "description": "Hex color, #RRGGBB or #RRGGBBAA (or ink in a CMYK document).",
                                        ],
                                        "opacity": [
                                            "type": "number", "minimum": 0, "maximum": 1,
                                            "description": "Default 1.",
                                        ],
                                        "midpoint": Self.stopMidpointProperty("colour"),
                                        "opacity_midpoint": Self.stopMidpointProperty("opacity"),
                                        "ink": Self.cmykInk("the stop's colour"),
                                    ],
                                    "required": ["position"],
                                ],
                            ],
                            "style": ["type": "string", "enum": GradientFill.styles],
                            "angle": [
                                "type": "number",
                                "description": "Degrees; 0 runs left to right, 90 bottom to top.",
                            ],
                            "scale": ["type": "number", "minimum": 0.1, "maximum": 1.5],
                            "reverse": ["type": "boolean"],
                            "smoothness": [
                                "type": "number", "minimum": 0, "maximum": 1,
                                "description": "Smoothness as a fraction: 1 "
                                    + "(100 %) is the Classic ramp, 0 the linear one, between "
                                    + "them the two blended.",
                            ],
                            "align_with_layer": [
                                "type": "boolean",
                                "description": "Align with Layer: true spans the "
                                    + "gradient over the bounds of the layer mask's coverage "
                                    + "(the canvas without a mask); false over the canvas. A "
                                    + "later mask edit takes effect at the next re-render.",
                            ],
                        ],
                    ],
                    "document_id": docID,
                ]),
        ]
    }

    /// A pattern fill layer's `pattern` argument (AgentServer+FillLayers.swift's
    /// `fillPattern`): the Pattern Overlay's keys without its anchor. `lead` opens the description.
    private static func patternFillProperty(_ lead: String) -> [String: Any] {
        [
            "type": "object",
            "description": lead + " The tile repeats over the whole canvas. Linked "
                + "(link_with_layer true), its origin is the canvas origin + phase + the "
                + "layer's reference point (where it was made, moved by canvas edits); "
                + "unlinked, the canvas origin + phase. Unknown keys, anchor included, are "
                + "refused. Replies echo the tile as {preset} or {name, width, height} without "
                + "rgba. At 32 bits tile codes / 255 are linear values.",
            "properties": [
                "pattern": [
                    "type": "object",
                    "description": "{\"preset\": "
                        + RasterDocument.patternPresets().map(\.id).joined(separator: "|")
                        + "} or a custom tile {\"name\"?, \"width\", \"height\" 1..512, "
                        + "\"rgba\": standard base64 of width x height x 4 straight RGBA8 "
                        + "bytes}.",
                ],
                "angle": [
                    "type": "number",
                    "description": "The Angle in degrees; turns the tile about its origin.",
                ],
                "scale": ["type": "number", "minimum": 0.01, "maximum": 10,
                          "description": "1 = 100 %."],
                "link_with_layer": ["type": "boolean"],
                "phase": [
                    "type": "array", "items": ["type": "number"], "minItems": 2, "maxItems": 2,
                    "description": "The tile's offset [x, y] in canvas px.",
                ],
            ],
        ]
    }

    /// A gradient stop's `midpoint` or `opacity_midpoint`, `kind` "colour" or "opacity":
    /// the fill layers' and the `gradient` tool's stops share it.
    static func stopMidpointProperty(_ kind: String) -> [String: Any] {
        ["type": "number", "minimum": 0.05, "maximum": 0.95,
         "description": "The \(kind) midpoint toward the next stop as a fraction: where "
            + "the \(kind) is half way between this stop and the next (default 0.5, 50 %; "
            + "rounded to 0.01; the last stop's is ignored). Outside 0.05…0.95 is refused."]
    }

    /// A CMYK ink argument (#547): C, M, Y, K in percent, kept exactly, its alpha the colour
    /// argument's beside it, else 1. An RGB document refuses it.
    private static func cmykInk(_ what: String) -> [String: Any] {
        ["type": "array", "items": ["type": "number", "minimum": 0, "maximum": 100],
         "minItems": 4, "maxItems": 4,
         "description": "CMYK document only: \(what) as ink, C, M, Y, K in percent 0–100, stored "
            + "exactly (a colour argument instead separates through the document's profile; "
            + "its alpha still applies). Replies report ink beside the appearance hex. An RGB "
            + "document refuses it."]
    }

    /// `stroke_join` / `stroke_dash` / `stroke_align`, the shape
    /// options bar's Corner style, Dash pattern and Align stroke. `fallback`
    /// words what an omitted argument means for the tool.
    private static func shapeStrokeStyleProperties(_ fallback: String) -> [String: Any] {
        [
            "stroke_join": [
                "type": "string",
                "enum": ShapeStrokeJoin.allCases.map { $0.rawValue },
                "description": "How the stroke's corners meet (\(fallback) miter).",
            ],
            "stroke_dash": [
                "type": "string",
                "enum": ShapeStrokeDash.allCases.map { $0.rawValue },
                "description": "The stroke's dash pattern (\(fallback) none): dashed is 4 "
                    + "stroke widths on, 2 off; dotted is round dots one stroke width "
                    + "across, every 2 widths.",
            ],
            "stroke_align": [
                "type": "string",
                "enum": ShapeStrokeAlign.allCases.map { $0.rawValue },
                "description": "Which side of the path the stroke's width lies on "
                    + "(\(fallback) center). A line takes only center.",
            ],
        ]
    }
}
