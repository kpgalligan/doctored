import Foundation

extension AgentServer {
    @inline(never)
    func basicSelectionCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        let docID = schema.docID
        let index = schema.index
        let selectionMode = schema.selectionMode
        let targetVocabulary = schema.targetVocabulary
        let alphaWriteNote = schema.alphaWriteNote
        // The marquee and lasso tools' Anti-alias checkbox.
        let antiAlias: [String: Any] = [
            "type": "boolean",
            "description": "Soften the edge (default true); false selects each pixel whose "
                + "coverage is at least half, fully, for a hard 0/255 edge.",
        ]
        return [
            tool(
                "select_rect",
                "Selects a rectangle (canvas coordinates). Selections confine "
                    + "brush/eraser strokes, fill, and gradient, and define Crop.",
                [
                    "x": ["type": "integer"], "y": ["type": "integer"],
                    "width": ["type": "integer"], "height": ["type": "integer"],
                    "mode": selectionMode,
                    "feather": ["type": "number", "minimum": 0, "maximum": 250,
                                "description": "Feather the new shape before combining (pixels, default 0)."],
                    "document_id": docID,
                ], required: ["x", "y", "width", "height"]),
            tool(
                "select_ellipse",
                "Selects an ellipse inscribed in the given rectangle.",
                [
                    "x": ["type": "integer"], "y": ["type": "integer"],
                    "width": ["type": "integer"], "height": ["type": "integer"],
                    "anti_alias": antiAlias,
                    "mode": selectionMode,
                    "feather": ["type": "number", "minimum": 0, "maximum": 250,
                                "description": "Feather the new shape before combining (pixels, default 0)."],
                    "document_id": docID,
                ], required: ["x", "y", "width", "height"]),
            tool(
                "select_polygon",
                "Selects a polygon through the given vertices (at least 3, "
                    + "closed automatically).",
                [
                    "points": [
                        "type": "array",
                        "description": "[[x, y], …] polygon vertices in canvas px.",
                        "items": [
                            "type": "array", "items": ["type": "number"],
                            "minItems": 2, "maxItems": 2,
                        ],
                        "minItems": 3, "maxItems": 10_000,
                    ],
                    "anti_alias": antiAlias,
                    "mode": selectionMode,
                    "feather": ["type": "number", "minimum": 0, "maximum": 250,
                                "description": "Feather the new shape before combining (pixels, default 0)."],
                    "document_id": docID,
                ], required: ["points"]),
            tool(
                "select_magic_wand",
                "Selects the region of similar color around a seed point, sampled "
                    + "from the flattened composite (what you see in a render). "
                    + "tolerance (historical 0–255 units, default 32) bounds how far each of R, G, B, A, R−G, G−B and R−B may lie from the seed's; native mode accepts fractional thresholds and reads original u16 samples; "
                    + "contiguous (default true) limits to the connected region. "
                    + "sample_all_layers: false reads one layer alone instead (the wand's "
                    + "Sample: Current layer): its own pixels on a transparent canvas, "
                    + "ignoring its opacity, mask and blend mode. "
                    + "sample_size (the wand's Sample Size, default 1 = Point Sample) "
                    + "compares against the per-channel mean of that square about the seed "
                    + "instead, clipped to the canvas (3, 5, 11, 31, 51 or 101 by 101 Average).",
                [
                    "x": ["type": "integer"], "y": ["type": "integer"],
                    "tolerance": ["type": "number", "minimum": 0, "maximum": 255],
                    "contiguous": ["type": "boolean"],
                    "sample_size": ["type": "integer", "enum": SelectToolOptions.wandSampleSizes],
                    "sample_all_layers": [
                        "type": "boolean",
                        "description": "true (default) samples the composite; false samples "
                            + "`layer` alone.",
                    ],
                    "layer": [
                        "type": "integer",
                        "description": "With sample_all_layers: false, the layer to sample "
                            + "(default: the active layer).",
                    ],
                    "mode": selectionMode,
                    "feather": ["type": "number", "minimum": 0, "maximum": 250,
                                "description": "Feather the new shape before combining (pixels, default 0)."],
                    "document_id": docID,
                ], required: ["x", "y"]),
            tool(
                "select_subject",
                "Selects the prominent foreground subjects — people, animals, "
                    + "objects — that macOS's Vision segmentation finds in the flattened "
                    + "composite. This is the same model behind Preview's Copy Subject, "
                    + "so it needs no seed point and no tolerance, unlike "
                    + "select_magic_wand. Mirrors Select > Select Subject. Omit instance "
                    + "to select every subject at once; pass instance (1-based) for a "
                    + "single one. Every result reports \"instances\", the number of "
                    + "subjects found, so you can select each in turn and compare their "
                    + "bounds to tell them apart. In a 32-bit float document Vision "
                    + "reads the tone-mapped 8-bit display at view exposure 0, so a "
                    + "subject entirely above white under the Clip tone map is found "
                    + "poorly: set the tone map to Reinhard first.",
                [
                    "instance": [
                        "type": "integer", "minimum": 1,
                        "description": "Which single subject to select (1-based). "
                            + "Omit to select all of them.",
                    ],
                    "mode": selectionMode,
                    "feather": ["type": "number", "minimum": 0, "maximum": 250,
                                "description": "Feather the new shape before combining (pixels, default 0)."],
                    "document_id": docID,
                ]),
            tool(
                "select_all",
                "Selects the whole canvas. Mirrors Select > All. Prefer this over a "
                    + "full-canvas select_rect: it is canvas-RELATIVE, so a recorded "
                    + "action replays correctly on a differently sized document, where "
                    + "literal coordinates would not.",
                [
                    "mode": selectionMode,
                    "document_id": docID,
                ]),
            tool(
                "deselect", "Clears the selection.", ["document_id": docID]),
            tool("reselect", "Restores the previous selection as one undoable, file-clean change.", ["document_id": docID]),
            tool("get_selection", "Reads selection bounds and Quick Mask state. Samples retain legacy 0–255 coverage plus exact coverage16 (0–65535) and report authoritative storage.", [
                "points": ["type": "array", "maxItems": 4096,
                           "items": ["type": "object", "properties": ["x": ["type": "integer"], "y": ["type": "integer"]], "required": ["x", "y"]]],
                "document_id": docID]),
            tool("set_quick_mask", "Enters/exits Quick Mask. Mode changes and each coverage stroke are undoable without dirtying the saved document.", [
                "active": ["type": "boolean"], "document_id": docID], required: ["active"]),
            tool("quick_mask_stroke", "Applies one canonical Quick Mask coverage stroke to the transient selection. "
                 + "Supply exactly one encoding: coverage_rle is base64 of 6-byte runs (little-endian UInt32 count, premultiplied gray byte, alpha byte); coverage_rle16 uses 8-byte runs with little-endian u16 gray and alpha. Native selection values remain exact. "
                 + "Runs cover the entire canvas in top-first row order; gray ≤ alpha. Output is gray + round(old × (255-alpha)/255). "
                 + "Recorded UI strokes include actual pressure, opacity and airbrush output. Works with either marquee or Quick Mask display; dimensions must match exactly.", [
                "width": ["type": "integer"], "height": ["type": "integer"],
                "coverage_rle": ["type": "string"], "coverage_rle16": ["type": "string"], "document_id": docID],
                 required: ["width", "height"]),
            tool(
                "modify_selection",
                "Transforms the current selection (errors when nothing is selected, except invert, "
                    + "which is then a no-op success): "
                    + "invert selects the complement over the canvas; feather "
                    + "Gaussian-softens the selection edge by radius px, so later "
                    + "fills, gradients, and strokes fade out across it; grow/shrink "
                    + "(Select ▸ Modify ▸ Expand/Contract) move the selection edge "
                    + "outward/inward by radius px (corners round into arcs); border "
                    + "replaces the selection with a band "
                    + "width px wide straddling its edge; smooth rounds corners and "
                    + "evens out jagged edges without moving long straight ones. An "
                    + "empty result clears the selection. For the colour-based "
                    + "Select ▸ Grow/Similar use select_similar.",
                [
                    "operation": [
                        "type": "string",
                        "enum": ["invert", "feather", "grow", "shrink", "border", "smooth"],
                    ],
                    "radius": [
                        "type": "number",
                        "description": "Radius in px (0-250; required for feather, grow, "
                            + "shrink, and smooth).",
                    ],
                    "width": [
                        "type": "number",
                        "description": "Band width in px (0-250; required for border).",
                    ],
                    "document_id": docID,
                ], required: ["operation"]),
            tool(
                "fill",
                "Bucket fill: flood-fills the similar-color region around the seed "
                    + "point on a layer's own pixels with a color. Respects the active "
                    + "selection. tolerance as in select_magic_wand. blend_mode lays the "
                    + "color down through a blend mode, and sample_all_layers grows the "
                    + "region over the composite instead of the layer alone (the Fill "
                    + "tool's Mode and Sample all layers); both apply to a layer target "
                    + "only, at 8 and 16 bits.",
                [
                    "x": ["type": "integer"], "y": ["type": "integer"],
                    "color": [
                        "type": "string",
                        "description": "Hex color, #RRGGBB or #RRGGBBAA (default #000000).",
                    ],
                    "tolerance": ["type": "integer", "minimum": 0, "maximum": 255],
                    "contiguous": ["type": "boolean"],
                    "blend_mode": [
                        "type": "string",
                        "enum": DrBlendMode.allBlendModes.map { $0.1 },
                        "description": "How the color blends with the layer (default Normal).",
                    ],
                    "sample_all_layers": [
                        "type": "boolean",
                        "description": "Grow the region over the flattened composite (default "
                            + "false: over the layer's own pixels). The color still lands on "
                            + "`layer`.",
                    ],
                    "layer": index,
                    "target": [
                        "type": "string",
                        "description": "What the fill lands on: " + targetVocabulary
                            + " A mask, plane or channel "
                            + "target holds COVERAGE, not color, so the color enters as "
                            + "its gray (its luma) and its alpha is the strength."
                            + alphaWriteNote,
                    ],
                    "document_id": docID,
                ], required: ["x", "y"]),
            tool(
                "gradient",
                "Paints a gradient over a layer (the whole layer, or the active "
                    + "selection if one exists): two colors (start_color, end_color), or "
                    + "stops, 2 to 32 colors each at a position with its own opacity. "
                    + "Linear runs along (x0,y0)->(x1,y1); radial spreads from (x0,y0) "
                    + "with radius to (x1,y1); angle sweeps clockwise around (x0,y0) "
                    + "starting along the line; reflected is linear mirrored about "
                    + "(x0,y0); diamond spreads from (x0,y0) as a diamond reaching "
                    + "(x1,y1). end_color defaults to transparent (a fade-out). "
                    + "Give start_color or stops. blend_mode lays the ramp down through "
                    + "a blend mode (the Gradient tool's Mode), on a layer target only. "
                    + "8 and 16 bits.",
                [
                    "x0": ["type": "number"], "y0": ["type": "number"],
                    "x1": ["type": "number"], "y1": ["type": "number"],
                    "start_color": [
                        "type": "string",
                        "description": "Hex color, #RRGGBB or #RRGGBBAA.",
                    ],
                    "end_color": [
                        "type": "string",
                        "description": "Hex color; omit to fade to transparent.",
                    ],
                    "stops": [
                        "type": "array", "minItems": 2, "maxItems": 32,
                        "description": "The ramp's stops, replacing start_color and "
                            + "end_color, in order of position. Colors interpolate "
                            + "between neighbouring stops, half way at the left stop's "
                            + "midpoint.",
                        "items": [
                            "type": "object",
                            "properties": [
                                "position": [
                                    "type": "number", "minimum": 0, "maximum": 1,
                                    "description": "0 at (x0,y0), 1 at (x1,y1); never "
                                        + "below the stop before it.",
                                ],
                                "color": [
                                    "type": "string",
                                    "description": "Hex color, #RRGGBB or #RRGGBBAA.",
                                ],
                                "opacity": [
                                    "type": "number", "minimum": 0, "maximum": 1,
                                    "description": "Scales the color's alpha (default 1).",
                                ],
                                "midpoint": Self.stopMidpointProperty("colour"),
                                "opacity_midpoint": Self.stopMidpointProperty("opacity"),
                            ],
                            "required": ["position", "color"],
                        ],
                    ],
                    "shape": [
                        "type": "string",
                        "enum": ["linear", "radial", "angle", "reflected", "diamond"],
                    ],
                    "smoothness": [
                        "type": "number", "minimum": 0, "maximum": 1,
                        "description": "Smoothness as a fraction: 1 (100 %, the "
                            + "default) is the Classic ramp, 0 the linear one, between them "
                            + "the two blended. Diamond ignores it.",
                    ],
                    "blend_mode": [
                        "type": "string",
                        "enum": DrBlendMode.allBlendModes.map { $0.1 },
                        "description": "How the ramp's colors blend with the layer (default "
                            + "Normal). A mask, plane, channel or Quick Mask target holds "
                            + "coverage and refuses any other mode.",
                    ],
                    "layer": index,
                    "target": [
                        "type": "string",
                        "description": "What the ramp lands on: " + targetVocabulary
                            + " On a mask, plane or "
                            + "channel the colors enter as their grays — coverage, not "
                            + "color — with their alpha as the strength."
                            + alphaWriteNote
                            + " \"quick_mask\" paints the Quick Mask coverage instead "
                            + "(refused when Quick Mask is off); while Quick Mask is on, "
                            + "a call with no target paints it too.",
                    ],
                    "document_id": docID,
                ], required: ["x0", "y0", "x1", "y1"]),
            tool(
                "clear_selection",
                "Erases the active selection out of a layer: the selected pixels lose "
                    + "their color and become transparent, in proportion to the selection's "
                    + "coverage, so a feathered selection cuts a soft-edged hole. Only the "
                    + "one layer changes — whatever sits below it shows through. Errors "
                    + "when nothing is selected; make a selection with the select_* tools "
                    + "first.",
                [
                    "layer": index,
                    "document_id": docID,
                ]),
            // The clipboard — Edit ▸ Cut / Copy / Copy Merged / Paste /
            // Paste in Place (AgentServer+Clipboard.swift). At 16 bits and
            // float a copy writes the private typed payload beside the
            // public TIFF/PNG, and a paste reads only that payload.
            tool(
                "copy",
                "Copies a layer's raw pixels (default the active layer) inside the selection's "
                    + "bounds — the whole canvas without one — to the clipboard, with alpha "
                    + "multiplied by the selection's coverage (Edit ▸ Copy). At 16 bits and "
                    + "32-bit float the clipboard also carries a private typed payload that "
                    + "round-trips samples, alpha, profile, offset, coverage and PPI bit for bit; "
                    + "the public TIFF/PNG beside it is a rendering. At 8 bits the clipboard "
                    + "carries a byte-origin typed payload too (its codes exactly ×257, the "
                    + "document's profile, offset and PPI), so paste in_place and a 16-bit paste "
                    + "work from an 8-bit copy. With no selection, a text, shape or adjustment "
                    + "layer's copy also carries its editable record at every depth (an "
                    + "adjustment layer copies as transparent pixels plus its settings), which "
                    + "paste restores; a selection copy, a raster layer or a RAW layer "
                    + "copies pixels only. A group copies its own "
                    + "composite at every depth: its children blended together, without the "
                    + "group's own opacity, blend mode or mask. Reply: typed, depth, "
                    + "width, height, offset, public_types, typed_bytes, editable (\"text\", "
                    + "\"shape\", \"adjust\" or null). While recording, the call is "
                    + "recorded as a step that copies from the replaying document; no pixels "
                    + "are stored. " + LayerClipboard.copyCatalogSentence + " "
                    + HeldClipboardPayload.catalogSentence,
                [
                    "layer": index,
                    "mode": [
                        "type": "string", "enum": ["pixels", "layers", "auto"],
                        "description": "pixels (default) copies pixels as described; layers "
                            + "copies whole layers (Edit ▸ Copy with no selection); auto is "
                            + "pixels with a pixel selection and layers (the selected layers) "
                            + "without one, as Edit ▸ Copy decides.",
                    ],
                    "layers": layerList("Layers to copy in mode layers, instead of layer."),
                    "public_copy_path": [
                        "type": "string",
                        "description": "Absolute path ending in .tiff; receives the same TIFF "
                            + "bytes placed on the clipboard.",
                    ],
                    "document_id": docID,
                ]),
            tool(
                "copy_merged",
                "Copies the flattened composite inside the selection's bounds (the whole canvas "
                    + "without one) to the clipboard (Edit ▸ Copy Merged). Typed at 16 bits and "
                    + "float, as copy. While recording, the call is recorded as a step that "
                    + "copies from the replaying document; no pixels are stored.",
                [
                    "public_copy_path": [
                        "type": "string",
                        "description": "Absolute path ending in .tiff; receives the same TIFF "
                            + "bytes placed on the clipboard.",
                    ],
                    "document_id": docID,
                ]),
            tool(
                "cut",
                "Copies a layer's selected pixels to the clipboard exactly as copy does, then "
                    + "clears them out of the layer as ONE undo step (Edit ▸ Cut). Requires a "
                    + "selection. A transparency-locked layer keeps its pixels (the clipboard "
                    + "still receives the copy) and the call reports it. In a 32-bit float "
                    + "document the erase scales alpha by (1 − coverage) and keeps RGB.",
                [
                    "layer": index,
                    "public_copy_path": [
                        "type": "string",
                        "description": "Absolute path ending in .tiff; receives the same TIFF "
                            + "bytes placed on the clipboard.",
                    ],
                    "document_id": docID,
                ]),
            tool(
                "paste",
                "Pastes the clipboard as a new layer and selects it (Edit ▸ Paste, or Edit ▸ "
                    + "Paste Special ▸ Paste in Place with placement in_place, which needs a "
                    + "Doctored typed payload's offset). placement auto (the default) is Edit "
                    + "▸ Paste's rule (measured against Photoshop), read from the view named by "
                    + "view (the document's first view without it): with an active selection "
                    + "(not in Quick Mask) the layer is centred on the selection's bounds, with "
                    + "no mask (placement selection); otherwise it is centred on the visible "
                    + "part of the canvas (view) and never keeps its source position, however "
                    + "visible the copied area is. With no window the visible part is the "
                    + "whole canvas, so the last case is the canvas centre (centered). "
                    + "placement in_place keeps the copy's source coordinates when this canvas "
                    + "is the size of the one it was copied from, and otherwise keeps the "
                    + "copy's centre at the same fraction of the canvas, the corner floored "
                    + "(placement relative_in_place), whatever the view and any selection; a "
                    + "payload_path paste keeps its own coordinates. "
                    + "A pixel paste made with an active selection deselects it in the same "
                    + "undo step (reselect restores it). "
                    + "placement centered is always the canvas centre, and at puts the layer's "
                    + "top-left exactly at x, y. Paste Special ▸ Paste Into (placement "
                    + "into) and Paste Outside (outside) need an active selection and are "
                    + "refused in Quick Mask: the layer is centred on the selection's bounds "
                    + "and gets a CANVAS-sized, unlinked mask at (0, 0) — the selection's "
                    + "coverage (into), or its inverse with outside coverage 255 (outside) — so "
                    + "the pasted pixels can be moved or the mask painted anywhere on the "
                    + "canvas. The selection is consumed in the same undo step (reselect "
                    + "restores it) and the layer, not its mask, stays the edit target. The "
                    + "pasted pixels are converted into this document's depth "
                    + "and profile at every depth, with no refusal for either: a float copy is "
                    + "tone-mapped under its own map into 8 or 16 bits, a 16-bit or 8-bit copy "
                    + "and an image from another app are promoted into a 32-bit float document "
                    + "(decoded to linear light, then converted into its profile), an image "
                    + "from another app enters 16 bits exactly (×257) and an 8-bit document "
                    + "converts any typed payload or payload_path file through the core. An "
                    + "untagged image from another app keeps its numbers in this document's "
                    + "profile. profile \"preserve\" "
                    + "is the Paste Profile Mismatch alert's Don't Convert (the menu asks only "
                    + "with Image ▸ Mode ▸ Working Space ▸ Ask When Pasting on): the pixels "
                    + "keep their numbers and take this document's profile. Paste in Place of "
                    + "an image from another app, which has no source position, lands like "
                    + "placement auto. A payload carrying an editable record (a whole-layer copy "
                    + "of a text, shape or adjustment layer) restores it — the artwork "
                    + "re-rendered at this document's depth, the adjustment re-created from its "
                    + "settings — before any depth or profile rule; a 32-bit float document "
                    + "restores editable artwork too, its colours rewritten into linear light so "
                    + "it looks the same, and a record that cannot be restored pastes "
                    + "as pixels with its reason. Reply: layer, name, placement (where it "
                    + "landed: in_place, relative_in_place, selection, view, centered, into, "
                    + "outside or at), "
                    + "offset, depth, typed, "
                    + "origin_depth, converted, external, narrowed, profile (same, converted "
                    + "or preserved; null for a restored record), editable (null or {type, "
                    + "restored, reason?}), mask (null, or for into/outside {x, y, width, height, "
                    + "linked, outside_coverage, frame [x, y, w, h]}), ppi {payload, document, "
                    + "match}, color_conversion (null, or {from, to, from_profile, to_profile, "
                    + "engine, intent, black_point_compensation} when the pixels changed colour "
                    + "model — RGB ↔ CMYK — or CMYK profile: engine \"doctored\" is the core's "
                    + "relative colorimetric conversion with black-point compensation, "
                    + "\"platform\" a Gray, Lab or CMYK image CoreGraphics converted while "
                    + "decoding it); a PPI "
                    + "mismatch is named in the history row (\"Paste Layer (300 → 72 ppi)\"), "
                    + "and so is a change of colour model (\"Paste Layer (CMYK → RGB)\"). "
                    + "While recording, a paste is one of two kinds of step. By default it is "
                    + "live: it stores no pixels and no position "
                    + "(the requested placement, x and y only for at, name and mode only when "
                    + "passed), a replay pastes whatever the clipboard holds then — pixels or "
                    + "copied layers — and the step fails when the clipboard holds no image, so "
                    + "record the copy that feeds it too. With payload_path or record_payload "
                    + "it is a saved-payload step: the landed payload is written as a .drcb "
                    + "file, the step names it as payload_path (package_action carries it as "
                    + "an asset) and replays without the clipboard, and an auto paste or an "
                    + "in_place paste that landed relative_in_place is recorded as at with the "
                    + "landed offset. view is not recorded. "
                    + LayerClipboard.pasteCatalogSentence + " "
                    + LayerInsertPosition.catalogSentence,
                [
                    "mode": [
                        "type": "string", "enum": ["auto", "layers", "pixels"],
                        "description": "auto (default): the copied layers when the clipboard "
                            + "holds a layers copy and the placement is auto, centered or "
                            + "in_place with no payload_path, else pixels — but into and "
                            + "outside are refused while a layers copy is held; "
                            + "layers requires a layers copy; pixels ignores it.",
                    ],
                    "placement": [
                        "type": "string",
                        "enum": ["auto", "centered", "in_place", "into", "outside", "at"],
                        "description": "auto (default) — Edit ▸ Paste's rule: the "
                            + "selection's bounds, else the centre of the visible canvas; "
                            + "centered — the canvas centre; in_place — the offset the "
                            + "payload was copied from, or the same relative location on a "
                            + "canvas of another size; into / outside — centred on the "
                            + "selection's bounds and "
                            + "masked by the selection or its inverse; at — exactly at x, y. "
                            + "Pasted layers take auto, centered or in_place only.",
                    ],
                    "x": [
                        "type": "integer",
                        "description": "Placement at only (required with it): the pasted "
                            + "layer's left edge in canvas pixels.",
                    ],
                    "y": [
                        "type": "integer",
                        "description": "Placement at only (required with it): the pasted "
                            + "layer's top edge in canvas pixels.",
                    ],
                    "name": [
                        "type": "string",
                        "description": "Default: an editable record's own layer name, "
                            + "otherwise \"Pasted Layer\".",
                    ],
                    "view": [
                        "type": "integer",
                        "description": "Which view (window) placement auto reads its visible "
                            + "canvas from, as list_views reports it; omit it for view 1. "
                            + "Pixels and layers alike; not recorded.",
                    ],
                    "index": ["type": "integer", "description": "Anchor layer, as layer."],
                    "profile": [
                        "type": "string", "enum": ["convert", "preserve"],
                        "description": "convert (default): pixels or layers in another profile "
                            + "are converted into this document's, keeping their appearance; "
                            + "preserve: Don't Convert — they keep their numbers and take this "
                            + "document's profile.",
                    ],
                    "convert": [
                        "type": "boolean",
                        "description": "Accepted and ignored (older recordings carry it): every "
                            + "paste converts.",
                    ],
                    "external": [
                        "type": "string", "enum": ["promote", "refuse"],
                        "description": "promote (default) or refuse: refuse rejects an image "
                            + "from another app at 16 bits and 32-bit float instead of "
                            + "promoting it.",
                    ],
                    "payload_path": [
                        "type": "string",
                        "description": "Absolute path to a .drcb payload file, pasted instead "
                            + "of the clipboard: the saved-payload form of a paste (what a "
                            + "saved-payload step replays, and what records one).",
                    ],
                    "record_payload": [
                        "type": "boolean",
                        "description": "While recording, save the landed payload as a .drcb "
                            + "file and record the step with payload_path, so the step replays "
                            + "without the clipboard. Pixel pastes only. A Doctored agent "
                            + "feature. Default false.",
                    ],
                    "document_id": docID,
                ].merging(LayerInsertPosition.catalogProperties) { current, _ in current }),
            tool(
                "get_clipboard",
                "Reports what the clipboard holds: typed (a Doctored typed payload is present), "
                    + "mode (\"rgb\" or \"cmyk\", the typed payload's colour mode; null "
                    + "without one), "
                    + "info (its version, mode, storage, depth, origin_depth — 8 for an 8-bit copy, "
                    + "a CMYK copy's declared depth beside cmyk_depth — "
                    + "width, height, offset, ppi, profile_description, has_coverage, editable "
                    + "— null, or {type, name, anchor (artwork), record_bytes} for a copy "
                    + "carrying an editable record — and, for a float payload, tone_map), "
                    + "public_types, source (null, or the {width, height} of the canvas a "
                    + "Doctored copy was made on, which paste in_place reads) and "
                    + LayerClipboard.clipboardCatalogSentence + " "
                    + HeldClipboardPayload.catalogSentence,
                ["document_id": docID]),
            tool(
                "new_from_clipboard",
                "Creates a new document from the clipboard and opens it (File ▸ New from "
                    + "Clipboard): a Doctored typed payload makes a document at the payload's "
                    + "own depth (8 for an 8-bit copy, 16, or 32-bit float with its tone map), "
                    + "profile and PPI, sized to its content — the box of every pixel with "
                    + "alpha > 0 (a fully transparent payload keeps its whole window); an image "
                    + "from another app makes an 8-bit document at its own size. A copied text "
                    + "or shape layer opens as that editable layer (still editable when its "
                    + "font is installed); a copied adjustment layer opens as an 8-bit document "
                    + "with the adjustment above a transparent \"Background\", whatever the "
                    + "source depth (origin_depth 8); a record that cannot be restored opens as "
                    + "pixels, with editable.reason saying why. A held layers copy (copy mode "
                    + "layers) makes a document the size of the copied layers' content box at "
                    + "their depth (a float copy at 16 bits), profile and PPI, with a white "
                    + "Background under the copied layers, each kept by name. "
                    + "Not recorded in actions. "
                    + "Reply: typed, origin_depth, depth, width, height, ppi, "
                    + "profile_description, tone_map (float), document, editable (null, or "
                    + "{type, restored, reason?}), trimmed (the payload was cut to its content "
                    + "box), source_offset ([x, y]: where the new canvas's origin sat on the "
                    + "source canvas; null for an image from another app), layers (the copied "
                    + "layers' record count for a layers copy, else null).",
                [:]),
            // Channels — saved selections, per-plane arithmetic (the
            // Channels panel, Select > Save/Load Selection, Image > Apply
            // Image… / Calculations…).
            // Guides and the ruler origin — View ▸ New Guide… / Clear
            // Guides, dragging a guide out of a ruler strip, and the ruler
            // corner box. The view PREFERENCES beside them (rulers, the
            // grid, the snap toggles, the ruler unit) deliberately have no
            // tool: they live in UserDefaults and are app-wide, so a call
            // naming one document would change every other open window,
            // undo could not walk one back, a model computes its coordinates
            // exactly and never benefits from a snap, and a preference
            // writes nothing save_copy would carry.
        ]
    }
}
