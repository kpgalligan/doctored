import Foundation

/// The catalog entries for the path tools (handlers in
/// `AgentServer+Paths.swift`), in their own file behind an explicit
/// non-inlining boundary for the reason `AgentCatalog.swift`'s header gives.
extension AgentServer {
    @inline(never) func pathsCatalog() -> [[String: Any]] {
        let doc = Self.docIDProperty
        // No `type`: an anchor is a pair OR an object, and no other schema in
        // the catalog uses a union keyword, so the description carries it.
        let anchor: [String: Any] = [
            "description": "[x, y] for a corner, or an object {x, y, in, out, smooth}: x, y "
                + "(numbers, the anchor's point), in ([x, y], the handle of the segment "
                + "arriving, default the point), out ([x, y], the handle of the segment "
                + "leaving, default the point), smooth (boolean, an editing hint, default "
                + "false). Handles are absolute canvas coordinates, not offsets.",
        ]
        let subpaths: [String: Any] = [
            "type": "array",
            "description": "The shape, in absolute canvas pixels (y down from the top-left; it "
                + "may lie outside the canvas): [{closed, anchors}, …]. The segment from one "
                + "anchor to the next is the cubic Bézier (point, out, next in, next point), a "
                + "straight line when both handles sit on their points; a closed subpath also "
                + "joins its last anchor to its first. Coordinates are kept to four decimals. "
                + "At most 4096 subpaths and 32768 anchors, each coordinate within ±1000000.",
            "items": [
                "type": "object",
                "properties": [
                    "closed": ["type": "boolean", "description": "Default false."],
                    "anchors": ["type": "array", "items": anchor, "minItems": 1],
                ] as [String: Any],
                "required": ["anchors"],
            ] as [String: Any],
        ]
        let fillRule: [String: Any] = [
            "type": "string", "enum": ["nonzero", "evenodd"],
            "description": "With subpaths: how overlapping subpaths fill. nonzero (default) "
                + "fills everything a subpath winds around; evenodd makes a subpath inside "
                + "another a hole.",
        ]
        let path: [String: Any] = [
            "type": "string",
            "description": "A stored path by name: a saved path's name, or \"Work Path\" for "
                + "the document's unsaved work path (list_paths).",
        ]
        let shapeNote = "Takes path (a stored path's name) OR subpaths with an optional "
            + "fill_rule (a shape that is not stored), not both. "
        let blendNames = DrBlendMode.allBlendModes.map { $0.1 }
        return [
            tool(
                "list_paths",
                "Lists the document's paths: Bézier outlines kept with the document — undoable, "
                    + "saved in .doctored, never drawn into the picture until one is filled, stroked "
                    + "or made a selection. Each has name, kind (\"work\": the one unsaved "
                    + "scratch path, always first and always named \"Work Path\"; \"saved\"), "
                    + "fill_rule, anchors (a count) and subpaths (set_path's own shape, so it "
                    + "can be edited and passed back). max_paths is the most a document holds. "
                    + "Canvas resizes, crops, rotations and flips move paths with the picture; "
                    + "moving or transforming a layer does not. A PSD, PSB or layered TIFF "
                    + "export writes them and its import reads them back (the work path and "
                    + "each saved path by name), and so does a flat TIFF in every colour mode "
                    + "but Duotone; no other flat format carries them. The clipping path's row "
                    + "has clipping: true and clipping_flatness, its flatness in device pixels "
                    + "(0 for none; set_clipping_path). The Paths panel.",
                ["document_id": doc]),
            tool(
                "set_path",
                "Creates a path or replaces one whole — what the Pen tool commits after each "
                    + "click or drag. With neither path nor name it is the WORK PATH (created, "
                    + "or replaced: a document has one). path (or name alone) names a SAVED "
                    + "path: replaced when the document has one of that name, created "
                    + "otherwise. name beside path also renames it in the same step (a work "
                    + "path becomes a saved one). An empty subpaths array stores an empty "
                    + "path. A path already equal to the shape answers changed:false. ONE undo "
                    + "step. A document holds at most 256 paths and 65536 anchors over all of "
                    + "them. 8-, 16-bit and 32-bit float documents.",
                [
                    "subpaths": subpaths, "fill_rule": fillRule,
                    "path": [
                        "type": "string",
                        "description": "The path to create or replace: a saved path's name, or "
                            + "\"Work Path\" (the default).",
                    ],
                    "name": [
                        "type": "string",
                        "description": "Without path: the saved path to create or replace. "
                            + "With path: its new name. At most 255 bytes, unique, not "
                            + "\"Work Path\".",
                    ],
                    "document_id": doc,
                ], required: ["subpaths"]),
            tool(
                "rename_path",
                "Renames a path. Renaming the work path SAVES it: it becomes a saved path and "
                    + "the document has no work path until the next one is drawn or traced. A "
                    + "name another path has, an empty name and \"Work Path\" refuse. ONE undo "
                    + "step. Double-clicking a row in the Paths panel.",
                [
                    "path": path,
                    "name": ["type": "string", "description": "The new name, at most 255 bytes."],
                    "document_id": doc,
                ], required: ["path", "name"]),
            tool(
                "duplicate_path",
                "Copies a path as a new saved path named name, or \"<its name> copy\" "
                    + "(numbered until free) when name is omitted. ONE undo step. Paths panel "
                    + "▸ Duplicate Path.",
                [
                    "path": path,
                    "name": ["type": "string", "description": "The copy's name; must be free."],
                    "document_id": doc,
                ], required: ["path"]),
            tool(
                "delete_path",
                "Deletes a path. ONE undo step. Paths panel ▸ Delete Path.",
                ["path": path, "document_id": doc], required: ["path"]),
            tool(
                "set_clipping_path",
                "Makes a saved path the document's CLIPPING PATH — the one path a PSD, PSB or "
                    + "TIFF names (image resource 2999) for a layout program to clip the image "
                    + "to — or, with path null, clears it. A document has at most one; setting "
                    + "another moves the designation. The work path cannot be it (save it first "
                    + "with rename_path). Renaming keeps it, deleting the path clears it, a "
                    + "duplicate is not it. flatness sets the clipping path's flatness, which "
                    + "the file carries for the output device (the reply reads it back). "
                    + "Already so answers changed:false. ONE undo step. Paths panel ▸ Clipping "
                    + "Path, and ▸ Clipping Path Flatness….",
                [
                    "path": [
                        "type": ["string", "null"],
                        "description": "A saved path's name (list_paths), or null to clear the "
                            + "clipping path. Required.",
                    ],
                    "flatness": [
                        "type": "number",
                        "description": "Device pixels: 0 for none, or from 0.2 to 100. Omitted, "
                            + "the path keeps the flatness it has as the clipping path, and a "
                            + "path newly made it has none. A file's own value outside the range "
                            + "is kept as read (list_paths' clipping_flatness). Needs a path.",
                    ],
                    "document_id": doc,
                ], required: ["path"]),
            tool(
                "path_to_selection",
                "Makes the path's fill the selection: every subpath filled by the path's fill "
                    + "rule (an open subpath is closed by a straight line), with the same "
                    + "coverage a shape layer of that outline has. " + shapeNote
                    + "mode combines it with the current selection; feather softens it; "
                    + "anti_alias false gives a hard edge. A path that covers no canvas pixel "
                    + "leaves nothing selected (replace, intersect) and says so. ONE selection "
                    + "undo step. Answers get_selection's fields. Not in Quick Mask. 8-, "
                    + "16-bit and 32-bit float documents. Paths panel ▸ Make Selection….",
                [
                    "path": path, "subpaths": subpaths, "fill_rule": fillRule,
                    "mode": [
                        "type": "string", "enum": ["replace", "add", "subtract", "intersect"],
                        "description": "Default replace.",
                    ],
                    "feather": [
                        "type": "number", "minimum": 0, "maximum": 250,
                        "description": "Feather radius in pixels (default 0).",
                    ],
                    "anti_alias": [
                        "type": "boolean",
                        "description": "Soften the edge (default true); false selects each "
                            + "pixel the path covers at least half of, fully.",
                    ],
                    "document_id": doc,
                ]),
            tool(
                "selection_to_path",
                "Traces the selection's outline into the WORK PATH, replacing it: closed "
                    + "subpaths under the even-odd rule (a hole is a subpath inside another) "
                    + "that follow the selection's 50% contour within tolerance pixels — "
                    + "straight sides stay straight, corners stay corners, curves become "
                    + "smooth anchors. A larger tolerance gives fewer anchors; a part of the "
                    + "selection no larger than the tolerance is dropped. Refuses with nothing "
                    + "selected, in Quick Mask, and when the outline needs more anchors than a "
                    + "path holds (raise tolerance). The selection itself is unchanged. ONE "
                    + "undo step. The reply has path, anchors and subpaths. 8-, 16-bit and "
                    + "32-bit float documents. Select ▸ Make Work Path….",
                [
                    "tolerance": [
                        "type": "number", "minimum": 0.5, "maximum": 10,
                        "description": "How far, in pixels, the path may stray from the "
                            + "selection's edge (default 2).",
                    ],
                    "document_id": doc,
                ]),
            tool(
                "fill_path",
                "Fills the path with ONE colour through a blend mode at an opacity: the "
                    + "path's anti-aliased fill (its fill rule; an open subpath closed by a "
                    + "straight line), inside the selection when there is one. " + shapeNote
                    + "It is stroke_selection's commit with that coverage as the band, so the "
                    + "colour, color_space, rgba32, blend_mode, opacity, preserve_transparency, "
                    + "layer and target arguments, the layer growing to the filled pixels, the "
                    + "locks, the rasterizing of a text or shape layer and every refusal are "
                    + "stroke_selection's (Quick Mask refuses by name). A path that covers no "
                    + "pixel of the canvas (or of the selection) answers changed:false with a "
                    + "note. ONE undo step, \"Fill Path\". 8-, 16-bit and 32-bit float "
                    + "documents; the reply is stroke_selection's without width, location and "
                    + "outline. Paths panel ▸ Fill Path.",
                [
                    "path": path, "subpaths": subpaths, "fill_rule": fillRule,
                    "color": [
                        "type": "string",
                        "description": "Fill colour, #RRGGBB or #RRGGBBAA (its alpha scales "
                            + "the fill). Required unless rgba32 is passed — the swatches are "
                            + "not read.",
                    ],
                    "rgba32": AgentServer.linearInkProperty,
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
                            + "false). Layer target only.",
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
                    "document_id": doc,
                ]),
            tool(
                "stroke_path",
                "Runs the Brush or the Eraser along the path: every subpath is one stroke "
                    + "that follows its outline (a closed subpath returns to its first "
                    + "anchor; an open one is not closed; a one-anchor subpath is a dot), at "
                    + "one pressure. " + shapeNote
                    + "tool \"brush\" (default) is brush_stroke and \"eraser\" is "
                    + "eraser_stroke, and size, hardness, flow, spacing, angle, roundness, "
                    + "opacity, layer and target mean what they mean there — as do color, "
                    + "color_space, rgba32 and blend_mode, which the eraser refuses. The selection "
                    + "confines the stroke, and every refusal of the stroke tool applies "
                    + "(locks, groups, a RAW layer until it is rasterized). Nothing is painted "
                    + "unless every subpath fits one stroke: a subpath that flattens to more "
                    + "than 10,000 points or leaves ±100,000 px refuses the whole call, naming "
                    + "the subpath. ALL subpaths are ONE undo step, \"Stroke Path\". 8-, 16-bit "
                    + "and 32-bit float documents: a float layer is painted in linear light "
                    + "as brush_stroke paints it (rgba32 for a colour above white, the float "
                    + "blend modes) and erased as eraser_stroke erases it. The reply is the "
                    + "stroke's own, plus tool, subpaths, points and changed. Paths panel ▸ "
                    + "Stroke Path.",
                [
                    "path": path, "subpaths": subpaths, "fill_rule": fillRule,
                    "tool": [
                        "type": "string", "enum": ["brush", "eraser"],
                        "description": "Default brush.",
                    ],
                    "size": [
                        "type": "number",
                        "description": "Stroke width in px (1-512, default 16).",
                    ],
                    "hardness": [
                        "type": "number", "minimum": 0, "maximum": 100,
                        "description": "Edge hardness in percent (default 100 = crisp).",
                    ],
                    "flow": ["type": "number", "description": "brush_stroke's flow."],
                    "spacing": ["type": "number", "description": "brush_stroke's spacing."],
                    "angle": ["type": "number", "description": "brush_stroke's angle."],
                    "roundness": ["type": "number", "description": "brush_stroke's roundness."],
                    "opacity": ["type": "number", "minimum": 0, "maximum": 1],
                    "color": [
                        "type": "string",
                        "description": "Brush only: hex colour, #RRGGBB or #RRGGBBAA (default "
                            + "#000000).",
                    ],
                    "rgba32": AgentServer.linearInkProperty,
                    "color_space": [
                        "type": "string", "enum": ["srgb", "document", "display_p3"],
                        "description": "Brush only: how color is read, as on brush_stroke.",
                    ],
                    "blend_mode": [
                        "type": "string", "enum": blendNames,
                        "description": "Brush only: brush_stroke's blend mode (default Normal).",
                    ],
                    "layer": [
                        "type": "integer",
                        "description": "Layer index; default the active layer.",
                    ],
                    "target": [
                        "type": "string",
                        "description": "What the stroke paints, as on brush_stroke: layer "
                            + "(default), mask, red, green, blue, alpha or channel:<name>.",
                    ],
                    "document_id": doc,
                ]),
        ] + vectorMaskCatalog(path: path, subpaths: subpaths, fillRule: fillRule)
    }

    /// The four vector-mask tools, with `pathsCatalog`'s shape properties.
    private func vectorMaskCatalog(
        path: [String: Any], subpaths: [String: Any], fillRule: [String: Any]
    ) -> [[String: Any]] {
        let doc = Self.docIDProperty
        let layer: [String: Any] = [
            "type": "integer",
            "description": "Layer index (a group or an adjustment layer too); default the "
                + "active layer.",
        ]
        let model = "A VECTOR MASK is a layer mask whose coverage is a path's fill, kept as a "
            + "shape: the layer's ordinary mask (what get_document, render and every mask tool "
            + "see) is that shape's anti-aliased fill MULTIPLIED by the raster mask the layer "
            + "had or is later given. "
        return [
            tool(
                "get_vector_mask",
                "Reads a layer's vector mask. " + model
                    + "vector_mask says whether the layer has one; with one the reply has "
                    + "enabled, linked (it moves and transforms with the layer), reveal_all "
                    + "(the shape is empty, so it hides nothing), has_raster_mask and "
                    + "raster_mask_enabled (a raster mask is kept beside the shape), fill_rule, "
                    + "anchors (a count) and subpaths (set_path's own shape, in canvas pixels "
                    + "as of now — a layer move moves it). get_document's layers carry a "
                    + "vector_mask flag. The Paths panel's vector-mask row.",
                ["layer": layer, "document_id": doc]),
            tool(
                "set_vector_mask",
                "Gives a layer a vector mask, or replaces the shape of the one it has. " + model
                    + "Takes exactly one of path (a stored path's name; the mask keeps its own "
                    + "copy), subpaths with an optional fill_rule, or reveal_all: true (an "
                    + "empty shape that hides nothing yet). A layer that already has a mask "
                    + "KEEPS it as the raster half; with none, add_layer_mask adds one later, "
                    + "and the mask tools (stroke with target \"mask\", set_layer_mask_enabled, "
                    + "remove_layer_mask, set_mask_properties, transform_mask) then edit that "
                    + "raster half and leave the shape alone. remove_layer_mask with apply: "
                    + "true bakes both into the pixels and removes both. The shape follows a "
                    + "layer move, crop and canvas resize always, and — while there is no "
                    + "raster half — a linked layer transform, a 90° rotation, a flip and "
                    + "image_size, staying sharp; any other resampling (perspective, warp, a "
                    + "crop with an angle, an arbitrary rotate, or any of them with a raster "
                    + "half) leaves an ordinary raster mask and no vector mask, as do "
                    + "duplicating or merging the layer. The same shape again answers "
                    + "changed:false. Lock All refuses. The shapes count toward the "
                    + "document's 65536 anchors. ONE undo step. Saved in .doctored; a layered "
                    + "PSD, PSB or TIFF, in every mode, writes one with no raster half as its "
                    + "path (and reads it back); one beside a raster half, "
                    + "and a flattened export, get the baked mask. 8-, 16-bit and 32-bit "
                    + "float documents. Layer ▸ "
                    + "Vector Mask ▸ Current Path / Reveal All.",
                [
                    "layer": layer, "path": path, "subpaths": subpaths, "fill_rule": fillRule,
                    "reveal_all": [
                        "type": "boolean",
                        "description": "true: an empty vector mask, to be drawn with a later "
                            + "set_vector_mask.",
                    ],
                    "document_id": doc,
                ]),
            tool(
                "set_vector_mask_properties",
                "Enables or disables a layer's vector mask and links or unlinks it. Disabled, "
                    + "the shape hides nothing (the raster half still applies when there is "
                    + "one) and is kept. Unlinked, moving or transforming the layer leaves the "
                    + "mask where it is. Pass enabled, linked or both; a flag already set "
                    + "answers changed:false. Refuses on a layer with no vector mask and under "
                    + "Lock All. ONE undo step. Layer ▸ Vector Mask ▸ Enable / Link.",
                [
                    "layer": layer,
                    "enabled": ["type": "boolean"],
                    "linked": ["type": "boolean"],
                    "document_id": doc,
                ]),
            tool(
                "remove_vector_mask",
                "Deletes a layer's vector mask. The raster mask kept beside it, if any, is the "
                    + "layer's mask again (raster_mask_kept); otherwise the layer has no mask. "
                    + "Nothing is baked into the pixels — for that use remove_layer_mask with "
                    + "apply: true. Refuses on a layer with no vector mask and under Lock All. "
                    + "ONE undo step. Layer ▸ Vector Mask ▸ Delete.",
                ["layer": layer, "document_id": doc]),
        ]
    }
}
