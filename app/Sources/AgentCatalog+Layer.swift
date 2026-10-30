import Foundation

extension AgentServer {
    @inline(never)
    func layerCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        let docID = schema.docID
        let index = schema.index
        return [
            tool(
                "set_active_layer",
                "Selects the layer that untargeted edits apply to.",
                ["index": ["type": "integer"], "document_id": docID], required: ["index"]),
            tool(
                "new_layer",
                "Adds an empty transparent layer above the active layer and selects it. "
                    + LayerInsertPosition.catalogSentence,
                ["name": ["type": "string"], "document_id": docID]
                    .merging(LayerInsertPosition.catalogProperties) { current, _ in current }),
            tool(
                "duplicate_layer",
                "Duplicates a layer, or every layer in layers, in ONE step. A GROUP "
                    + "duplicates with everything inside it. The reply reports the new "
                    + "layer_count — every structural edit renumbers the stack, so re-read "
                    + "get_document before addressing layers again.",
                [
                    "index": index,
                    "layers": layerList("Layers to duplicate, instead of index."),
                    "document_id": docID,
                ]),
            tool(
                "duplicate_to_new_document",
                "Mirrors Layer > Duplicate to New Document…: copies a layer, or every layer "
                    + "in layers (a GROUP with everything inside it), into a NEW untitled "
                    + "document and opens it; the source is unchanged (no undo step, no "
                    + "dirty change). The new document keeps the source's depth (8, 16 or "
                    + "32-bit float), canvas size, ICC profile, resolution, evaluation "
                    + "policy and global light. Every copied layer keeps its pixels, offset, "
                    + "name, opacity, blend mode, visibility, mask, style, Blend If, label, "
                    + "locks and text/shape/adjustment description, under a fresh id; a "
                    + "clip survives only over a copied base, a link only when two copies "
                    + "share it. Alpha channels, guides, layer comps, color samplers and "
                    + "EXIF/XMP/IPTC metadata are not carried. name sets the new "
                    + "document's title (default: the untitled name). The reply's "
                    + "document is the NEW one; later calls on the source keep passing "
                    + "its document_id. In an Action, later steps stay on the run's own "
                    + "document; edits made in the duplicate while recording are recorded "
                    + "as a non-replayable placeholder; an MCP-recorded step replays its "
                    + "literal index/layers. Refused inside a Batch.",
                [
                    "index": index,
                    "layers": layerList("Layers to copy, instead of index."),
                    "name": ["type": "string"],
                    "document_id": docID,
                ]),
            tool(
                "delete_layer",
                "Deletes a layer, or every layer in layers, in ONE step (a group takes its "
                    + "children with it). Refused only when it would leave the document "
                    + "with no layers at all.",
                [
                    "index": index,
                    "layers": layerList("Layers to delete, instead of index."),
                    "document_id": docID,
                ]),
            tool(
                "merge_down",
                "Merges a layer into the previous one AMONG ITS SIBLINGS, so a merge "
                    + "inside a group never reaches out of it. With layers (two or more, "
                    + "all sharing one parent) it is Merge Layers: they become "
                    + "one raster layer at the lowest one's position. Masks, styles and "
                    + "adjustment descriptions are baked in and dropped. The merge REPLACES "
                    + "the lower layer's pixels, so its transparency or pixel lock refuses "
                    + "the call; the upper one's locks do not, since it is only removed. "
                    + "At every depth the merged layer keeps the union of the members' "
                    + "extents, so pixels a member holds outside the canvas are kept. "
                    + "In 16-bit and 32-bit float documents a masked adjustment layer "
                    + "also adjusts merged pixels beyond its mask by the mask's outside "
                    + "coverage, where an 8-bit merge leaves them unadjusted. "
                    + "A layer with knockout merges against its REAL backdrop: the hole is filled with the backdrop's own pixels, so the picture is kept wherever that backdrop is opaque. Where no backdrop can be captured — the knockout layer sits inside an isolated group, inside a pass-through group that is itself being merged, is clipped to a base outside the merge, or punches to the root of a document with no qualifying Background layer — the hole bakes as TRANSPARENCY, the merged layer shows whatever lies beneath it, and the reply carries a `note`.",
                [
                    "index": index,
                    "layers": layerList("Layers to merge together, instead of index.", minItems: 2),
                    "document_id": docID,
                ]),
            tool(
                "flatten_image",
                "Flattens all layers into one. The single layer left behind is named "
                    + "\"Background\": hidden layers are DISCARDED, every layer comp goes "
                    + "with the identities it bound, and the channels, guides and ruler "
                    + "origin are kept. Answers at every depth, and the flat layer is "
                    + "canvas-sized at every depth, like 8-bit.",
                ["document_id": docID]),
            tool(
                "reorder_layer",
                "Moves a layer (and, for a group, everything inside it) to a new stack "
                    + "position. Omit depth and it lands at the destination's own level, "
                    + "which is what dragging a row onto another means; give a depth to "
                    + "move it INTO or OUT OF a group. `to` numbers the stack with this "
                    + "layer's own subtree ALREADY taken out, so naming that subtree's own "
                    + "start at the same depth puts the layer back where it started and is "
                    + "refused as a no-op. A (to, depth) pair that would not make a "
                    + "well-formed tree is refused too.",
                [
                    "from": ["type": "integer"], "to": ["type": "integer"],
                    "depth": [
                        "type": "integer", "minimum": 0, "maximum": 10,
                        "description": "Nesting level at the destination: 0 is the top "
                            + "level, and one more than a group's own depth puts the layer "
                            + "inside that group.",
                    ],
                    "document_id": docID,
                ], required: ["from", "to"]),
            tool(
                "set_layer_properties",
                "Changes any of a layer's name, opacity (0-1), blend mode, visibility, or "
                    + "pixel offset in one undoable step. Pass Through applies to GROUPS "
                    + "only — it is a new group's default, and it is what lets an adjustment "
                    + "layer inside the group reach the layers below it; naming it for a "
                    + "raster layer is refused. A field already holding the value you pass "
                    + "is simply left alone rather than failing the call, so echoing a "
                    + "get_document row back is safe; when NOTHING in the call would "
                    + "change, the reply is changed:false and no undo step is added. "
                    + "offset_x/offset_y that differ from the reported offset are refused "
                    + "on an ADJUSTMENT layer (it has no position of its own; move_layers "
                    + "moves its linked mask) and on a GROUP that contains no pixels (it has "
                    + "no position to move), at every depth. open works at every depth "
                    + "and, like the Layers panel's disclosure triangle, adds no undo step: "
                    + "it is saved with the document (so it marks it edited) and changes no "
                    + "pixel; with other properties in the same call, those are the one undo "
                    + "step, and undoing it also restores the disclosure.",
                [
                    "index": index,
                    "name": ["type": "string"],
                    "opacity": ["type": "number", "minimum": 0, "maximum": 1],
                    // The GROUP vocabulary, which is the shared one plus Pass
                    // Through. `blendNames` itself must NOT be widened: it
                    // also feeds brush_stroke, clone_stamp and apply_image,
                    // where Pass Through has no meaning.
                    "blend_mode": ["type": "string", "enum": DrBlendMode.groupBlendNames],
                    "visible": ["type": "boolean"],
                    "offset_x": ["type": "integer"],
                    "offset_y": ["type": "integer"],
                    "open": [
                        "type": "boolean",
                        "description": "GROUPS only: whether the panel shows it expanded. "
                            + "Saved with the document; it changes no pixel and adds no undo step.",
                    ],
                    "document_id": docID,
                ]),
            tool(
                "transform_layer",
                "Rotates, scales and/or moves a layer's pixels in a single resample — the "
                    + "same pipeline as the app's Free Transform (⌘T). The rotation and the "
                    + "scales act around a pivot (by default the centre of the layer's CURRENT "
                    + "bounds — for a re-editable text or shape layer the exact "
                    + "centre of its description's box, which a turn leaves fixed, so rotate "
                    + "then rotate-back returns it exactly), then the translation is added; "
                    + "the layer is resampled once "
                    + "into the outward-rounded bounding box of its transformed corners, so "
                    + "its offset AND size both change, and it may end up extending past the "
                    + "canvas. The canvas is untouched, and a layer mask rides along, "
                    + "resampled identically. Two entries OTHER than the named layer do "
                    + "follow the same matrix, because a person dragging the box would "
                    + "expect them to: everything inside it when the layer is a GROUP, and "
                    + "every layer LINKED to it (link_layers). Nothing else moves. Pass at "
                    + "least one of rotate, "
                    + "scale, scale_x, scale_y, translate_x, translate_y; the result reports "
                    + "the layer's new bounds so you can verify placement. For whole-document "
                    + "geometry use rotate / flip / image_size instead. On a re-editable TEXT, "
                    + "SHAPE layer nothing rasterizes: the matrix composes into "
                    + "the layer's description and the layer re-renders through it (crisp "
                    + "vector edges; the result says rasterized: false and reports the new "
                    + "transform and origin). Only a layer whose source cannot be rendered "
                    + "right now (a text family not installed here) "
                    + "is resampled as pixels and drops its description, reported "
                    + "as before with the reason (undo restores it). A transform over a SET "
                    + "— a group, or a linked layer — always resamples instead, so every "
                    + "described layer it carries loses its description too; those are "
                    + "listed in rasterized_layers. It ignores any selection; to transform "
                    + "only the selected pixels use transform_selected_pixels.",
                [
                    "layer": index,
                    "rotate": [
                        "type": "number",
                        "description": "Rotation in degrees around the pivot. POSITIVE IS "
                            + "CLOCKWISE on screen (canvas y grows downward), matching the "
                            + "app's Angle field. Default 0.",
                    ],
                    "scale": [
                        "type": "number",
                        "description": "Uniform scale multiplier for both axes: 1 = unchanged "
                            + "(default), 0.5 = half size, 2 = double, negative mirrors. "
                            + "scale_x / scale_y override it per axis. Magnitudes are clamped "
                            + "to 0.001-100 and 0 is refused.",
                    ],
                    "scale_x": [
                        "type": "number",
                        "description": "Horizontal scale multiplier, overriding scale "
                            + "(1 = unchanged, negative mirrors left-right).",
                    ],
                    "scale_y": [
                        "type": "number",
                        "description": "Vertical scale multiplier, overriding scale "
                            + "(1 = unchanged, negative mirrors top-bottom).",
                    ],
                    "translate_x": [
                        "type": "number",
                        "description": "Move right by this many canvas px (negative = left), "
                            + "applied after the rotation and scale.",
                    ],
                    "translate_y": [
                        "type": "number",
                        "description": "Move down by this many canvas px (negative = up), "
                            + "applied after the rotation and scale.",
                    ],
                    "around": [
                        "type": "string",
                        "enum": ["center", "top_left"],
                        "description": "The pivot the rotation and scale turn/grow around: "
                            + "\"center\" (default) or \"top_left\" of the layer's current "
                            + "bounds; center is a described layer's exact centre, shared "
                            + "with the app's ⌘T session. pivot_x / pivot_y override it.",
                    ],
                    "pivot_x": [
                        "type": "number",
                        "description": "Explicit pivot x in canvas px; overrides around on "
                            + "this axis.",
                    ],
                    "pivot_y": [
                        "type": "number",
                        "description": "Explicit pivot y in canvas px; overrides around on "
                            + "this axis.",
                    ],
                    "sampler": [
                        "type": "string",
                        "enum": ["nearest", "bilinear", "bicubic", "lanczos"],
                        "description": "Resampling filter, default bicubic (Catmull-Rom). "
                            + "nearest keeps hard pixel edges (pixel art), lanczos is "
                            + "sharpest for big reductions. Whole-pixel moves and mirrors "
                            + "(scale -1) copy pixels losslessly whatever this says; every "
                            + "rotation is resampled, so for a lossless quarter turn of the "
                            + "WHOLE image use the rotate tool instead.",
                    ],
                    "document_id": docID,
                ]),
            tool(
                "distort_layer",
                "Maps the named layer, its group descendants and linked partners through "
                    + "one canvas perspective. corners are TL/TR/BR/BL destinations of the "
                    + "layer rect; for a group they are destinations of its content box. "
                    + "A parallelogram preserves editable text/shape artwork as affine "
                    + "geometry. True perspective rasterizes described artwork only when "
                    + "rasterize_artwork:true is supplied, and reports the affected layers. "
                    + "The canvas and unlinked masks remain in place.",
                [
                    "layer": index,
                    "corners": [
                        "type": "array",
                        "description": "Four [x, y] canvas points, the destinations of the "
                            + "layer rect's TL, TR, BR and BL corners in that order, e.g. "
                            + "[[0, 0], [80, 10], [75, 60], [5, 50]]. Pixels are y-down: "
                            + "the canvas origin is its top-left.",
                        "items": [
                            "type": "array",
                            "items": ["type": "number"],
                            "minItems": 2, "maxItems": 2,
                        ],
                        "minItems": 4, "maxItems": 4,
                    ],
                    "sampler": [
                        "type": "string",
                        "enum": ["nearest", "bilinear", "bicubic", "lanczos"],
                        "description": "Resampling filter, default bicubic (Catmull-Rom). "
                            + "Perspective compresses detail toward the quad's narrow side, "
                            + "where bicubic or lanczos hold up best.",
                    ],
                    "rasterize_artwork": ["type": "boolean",
                        "description": "Authorize true-perspective rasterization of described artwork."],
                    "document_id": docID,
                ], required: ["corners"]),
        ]
    }
}
