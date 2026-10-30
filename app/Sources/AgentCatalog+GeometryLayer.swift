import Foundation

extension AgentServer {
    @inline(never)
    func geometryLayerCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        let docID = schema.docID
        let index = schema.index
        return [
            tool(
                "rotate",
                "Rotates the whole document clockwise by ANY finite angle, normalized into "
                    + "(-180, 180]. What happens depends on the angle, and the core decides "
                    + "which it is. A whole number of full turns (0, 360, -720) moves "
                    + "nothing and answers {\"ok\": true, \"changed\": false} with a note — no "
                    + "undo step, no dirtying and no recorded Action step — at 8 bits, 16 bits "
                    + "and 32-bit float. A content-identical exact quarter turn has the same "
                    + "successful no-op and recording behavior. An "
                    + "EXACT quarter turn (90, 180, 270, -90, and 450 or -270, which "
                    + "normalize onto them) is a lossless permutation: re-editable text "
                    + "and shape layers stay re-editable because the op composes "
                    + "into their descriptions, the ppi axes swap, guides swap orientation "
                    + "with the picture, and it is available at all three depths. ANY OTHER "
                    + "angle is a destructive arbitrary rotation at EVERY depth: the canvas "
                    + "GROWS to the rotated bounding box (the reply reports the new canvas "
                    + "and the origin the old canvas's top-left landed at), every layer, "
                    + "every mask and every saved channel is resampled with `sampler`, and "
                    + "re-editable text and shape layers are re-rendered from their "
                    + "descriptions and stay re-editable. Guides are shifted by that origin "
                    + "and DROPPED if they leave the new canvas, the ruler origin is "
                    + "clamped, and the ppi is NOT transposed: 90 degrees and 89.999 degrees "
                    + "are deliberately different operations. The canvas may also SHRINK on "
                    + "an axis (100x10 at 80 degrees is 28x101). In a 32-bit float document "
                    + "the rotation resamples premultiplied extended-range samples: each "
                    + "layer's RGB is clamped to that layer's own visible range so ringing "
                    + "never leaves it, an output alpha below 2^-24 becomes transparent, and "
                    + "a float result past 67,108,864 pixels refuses before any work. A "
                    + "RAW layer turns with the rest at "
                    + "8 and 16 bits: it is re-rendered from its retained source at the new angle. "
                    + "AFTER a recorded arbitrary rotation, later Action steps in "
                    + "`relative` or `anchor` geometry mode are remapped per axis against "
                    + "the expanded canvas, exactly as after any canvas-changing step — but "
                    + "the recorded space is no longer an axis-aligned copy of the original "
                    + "picture, so on a DIFFERENT-SIZE target those rectangles do not follow "
                    + "the rotated content. "
                    + "THREE BEHAVIOUR CHANGES from before this release: an unreadable "
                    + "`degrees` (\"abc\") used to be read as 90 and rotate silently and now "
                    + "refuses by name; 90.7 used to truncate to 90 and now rotates by "
                    + "90.7; and 0, 45, 360, -270 and 450 used to be errors.",
                [
                    "degrees": [
                        "type": "number",
                        "description": "Any finite angle in degrees; POSITIVE IS CLOCKWISE. "
                            + "Normalized into (-180, 180]. It must be a number; an "
                            + "unreadable value refuses at every depth rather than rotating "
                            + "by 90 silently.",
                    ],
                    "sampler": [
                        "type": "string",
                        "enum": ["nearest", "bilinear", "bicubic", "catmull-rom", "lanczos"],
                        "description": "How a non-quarter rotation resamples (default "
                            + "bicubic, the same vocabulary as crop and transform_layer). "
                            + "Ignored on an identity or an exact quarter turn, which "
                            + "resample nothing.",
                    ],
                    "document_id": docID,
                ], required: ["degrees"]),
            tool(
                "flip",
                "Flips the whole document. Re-editable text and shape layers "
                    + "stay re-editable: the op composes into their descriptions. Available "
                    + "at 8 bits, at 16 bits and in a 32-bit float document: a flip is an "
                    + "exact permutation, so no sample is ever combined. If the complete "
                    + "stored result is identical, the reply has changed:false and no undo, "
                    + "dirty or recorded Action step is added.",
                [
                    "axis": ["type": "string", "enum": ["horizontal", "vertical"]],
                    "document_id": docID,
                ], required: ["axis"]),
            tool(
                "crop",
                "Crops the document to a rectangle (canvas coordinates, origin top-left). "
                    + "A nonzero angle straightens first — the Crop tool's straighten "
                    + "slider: every layer — and every alpha channel, so saved selections "
                    + "keep lining up — is rotated by −angle about the rect's center, "
                    + "then the canvas is cropped, as one undo step. Straightening "
                    + "resamples every layer's pixels; re-editable text and shape layers "
                    + "are re-rendered from their descriptions and stay re-editable. "
                    + "At angle 0 the crop only moves "
                    + "the canvas window and every description stays valid. A 16-bit "
                    + "document takes both: the straighten is ONE native op equal to "
                    + "rotating every layer, every mask and every channel and then "
                    + "cropping, in one undo step. A 32-bit float "
                    + "document takes both too: the zero-angle crop keeps every allocation "
                    + "and moves the window only, and the straighten resamples "
                    + "premultiplied extended-range samples, clamping each layer's RGB to "
                    + "that layer's own visible range so ringing never leaves it (an output "
                    + "alpha below 2^-24 becomes transparent). A zero-angle crop to the WHOLE canvas changes "
                    + "nothing and answers {\"ok\": true, \"changed\": false} with a note "
                    + "rather than an error, at every depth: no undo step and no dirtying. "
                    + "So does a non-zero angle too small to move any pixel of the whole "
                    + "canvas; over a smaller rectangle such an angle still crops. When "
                    + "straightening, take the box from fit_crop or detect_horizon's "
                    + "suggested_crop, or the corners show transparent wedges. Pixels outside "
                    + "the new canvas are kept (moving a layer reveals them) unless "
                    + "delete_cropped is true. A rectangle past any canvas edge (negative x or "
                    + "y, or beyond the far edge; at most 100 megapixels) EXTENDS the canvas, "
                    + "turned or not, as the Crop tool dragged past the canvas does: with "
                    + "delete_cropped true an opaque Background (the bottom layer: visible, "
                    + "Normal, 100%, unmasked, unstyled, canvas-sized and fully opaque) is "
                    + "filled with background_color in the new area, and every other layer's "
                    + "new area is transparent (with delete_cropped false nothing is filled). "
                    + "Such a reply adds \"extended\": true and \"planned_bytes\". "
                    + "fill (the Crop tool's Fill popup) chooses what that new area holds, "
                    + "only for a rectangle past the canvas: background (default, the "
                    + "Background fill above); content_aware runs Content-Aware Fill (default "
                    + "settings) on the bottom layer over the new area, grown by the "
                    + "sampler's reach on a turned crop so no pad-coloured seam is left, "
                    + "and needs that layer to be the opaque Background (delete_cropped is "
                    + "forced on; false is refused); generative_expand sends the picture to "
                    + "a cloud image model as generative_expand does (prompt, provider, "
                    + "variations, gated the same way) and lands the answer on a new "
                    + "Generative Layer above the active layer, masked to the new area. "
                    + "Either is one undo step with the crop and one waited job; its reply "
                    + "is the job's report with \"extended\": true, \"fill\" and "
                    + "\"planned_bytes\". A generative_expand crop is not recorded into "
                    + "Actions.",
                [
                    "x": ["type": "integer"], "y": ["type": "integer"],
                    "width": ["type": "integer"], "height": ["type": "integer"],
                    "angle": [
                        "type": "number", "minimum": -45, "maximum": 45,
                        "description": "Straighten angle in degrees (-45 to 45, default 0): "
                            + "the content rotates by −angle, so a positive angle turns the "
                            + "picture counter-clockwise on screen — the same sign as the "
                            + "app's Straighten field. It must be a number; an unreadable "
                            + "value refuses at every depth rather than cropping silently.",
                    ],
                    "sampler": [
                        "type": "string",
                        "enum": ["nearest", "bilinear", "bicubic", "catmull-rom", "lanczos"],
                        "description": "How the straighten resamples (default bicubic, the "
                            + "same vocabulary as transform_layer). Ignored at angle 0, "
                            + "which resamples nothing. The Crop tool straightens with "
                            + "whatever the Free Transform options bar is set to, so a "
                            + "recorded straighten carries that choice here.",
                    ],
                    "delete_cropped": [
                        "type": "boolean",
                        "description": "The Crop tool's Delete Cropped Pixels (default false): "
                            + "each plain raster layer is trimmed to the new canvas, and one "
                            + "wholly outside becomes empty. Masks, groups, adjustment layers "
                            + "and re-editable text, shape and placed layers keep their "
                            + "pixels.",
                    ],
                    "background_color": [
                        "type": "string",
                        "description": "The Background fill of a rectangle past the canvas, "
                            + "#RRGGBB (default #FFFFFF, the Crop tool's background swatch). "
                            + "Read only for such a rectangle.",
                    ],
                    "fill": [
                        "type": "string",
                        "enum": CropFill.allCases.map(\.rawValue),
                        "description": "What a rectangle past the canvas fills its new area "
                            + "with (default background). Refused for a rectangle inside "
                            + "the canvas.",
                    ],
                    "prompt": [
                        "type": "string", "maxLength": GenerativeFillSettings.maximumPromptLength,
                        "description": "fill generative_expand only: what to add in the new "
                            + "area; empty continues the picture.",
                    ],
                    "provider": [
                        "type": "string", "enum": CloudProviderID.allCases.map(\.rawValue),
                        "description": "fill generative_expand only: the cloud provider "
                            + "(default openai).",
                    ],
                    "variations": [
                        "type": "integer",
                        "minimum": GenerativeFillSettings.variationRange.lowerBound,
                        "maximum": GenerativeFillSettings.variationRange.upperBound,
                        "description": "fill generative_expand only: how many pictures to "
                            + "ask for (default 3).",
                    ],
                    "document_id": docID,
                ], required: ["x", "y", "width", "height"]),
            tool(
                "image_size",
                "Scales the whole document to a new size (max 100 megapixels). Re-editable "
                    + "text and shape layers stay re-editable: the scale composes "
                    + "into their descriptions, and text and shapes re-render crisply. Alpha "
                    + "channels are canvas-sized and are resampled too, so a document "
                    + "carrying many of them is refused past the format's total "
                    + "channel-pixel budget (900 million): the error names how many the new "
                    + "canvas would hold, and delete_channel is the way out. The Image Size "
                    + "sheet can present dimensions as percentages of the pixel size captured "
                    + "when the sheet opened; each scaled axis rounds to the nearest integer "
                    + "pixel, halfway away from zero. This tool and recorded Image Size "
                    + "steps deliberately take those resolved integer pixels, not a relative "
                    + "percentage, so replay always reproduces the accepted result.",
                [
                    "width": ["type": "integer"], "height": ["type": "integer"],
                    "filter": [
                        "type": "string",
                        "enum": ["nearest", "bilinear", "catmull-rom", "lanczos3"],
                        "description": "Resampling filter, default lanczos3.",
                    ],
                    "document_id": docID,
                ], required: ["width", "height"]),
            tool(
                "canvas_size",
                "Resizes the canvas WITHOUT scaling the pixels; the anchor pins the existing "
                    + "content. Layers keep their pixels and "
                    + "can extend outside the canvas; alpha channels are padded with 0. "
                    + "GROWING the canvas grows every channel with it, so the same total "
                    + "channel-pixel budget image_size names can refuse it — delete channels "
                    + "and retry. Available at 8 bits, at 16 bits and in a 32-bit float "
                    + "document: the operation reads no sample, so every layer, mask and "
                    + "guide simply moves by the anchor's offset. A RAW layer "
                    + "rides the canvas and keeps its source — no rasterize needed. "
                    + "A call that would change NOTHING — "
                    + "the same width and height, whatever the anchor — SUCCEEDS with "
                    + "changed: false and a note: it adds no undo step and does not dirty the "
                    + "document, at 8 bits, at 16 bits and at 32-bit float alike, so a "
                    + "recorded canvas_size step replays cleanly on a document that is already "
                    + "the target size and a batch normalizing canvas size across mixed inputs "
                    + "does not halt on the files that already match.",
                [
                    "width": ["type": "integer"], "height": ["type": "integer"],
                    "anchor": [
                        "type": "string",
                        "enum": [
                            "top-left", "top", "top-right", "left", "center", "right",
                            "bottom-left", "bottom", "bottom-right",
                        ],
                        "description": "Where the existing content is pinned; default center.",
                    ],
                    "document_id": docID,
                ], required: ["width", "height"]),
            // Layer structure: groups, locks, links, align/distribute and the
            // workflow commands (AgentServer+Groups.swift). Every one of them
            // takes an EXPLICIT set and reports the NEW indices back, because
            // a structural edit renumbers the stack.
            tool(
                "group_layers",
                "Wraps the given layers in a new GROUP and selects it. They must all sit "
                    + "in the same group already (get_document reports each layer's "
                    + "parent), and a group cannot be grouped with one of its own "
                    + "children. The new group is Pass Through at 100 %, so its children "
                    + "keep compositing against the layers below it exactly as before — "
                    + "grouping alone never changes the picture. The reply carries the "
                    + "group's index, its children's NEW indices, and two things the "
                    + "operation had to change: cleared_clip (the bottom layer's clipping "
                    + "mask was released, because the layer it clipped to stayed outside) "
                    + "and reordered (a non-contiguous set is gathered together, so layers "
                    + "left between them change their relative order).",
                [
                    "layers": layerList("The layers to group."),
                    "name": ["type": "string", "description": "Group name; default \"Group N\"."],
                    "document_id": docID,
                ], required: ["layers"]),
            tool(
                "ungroup_layers",
                "Dissolves a group: its children take its place at its level, in order, "
                    + "and the group entry disappears. A group's own mask, style, opacity, "
                    + "blend mode and clipped flag cannot be expressed on its children, so "
                    + "they are DISCARDED — the reply's discarded array names whichever of "
                    + "them was set, and undo restores the group. The reply also carries "
                    + "cleared_clip: the bottom layer of the group was clipped to nothing "
                    + "inside it, so its clipping mask is RELEASED rather than silently "
                    + "re-pointed at the layer below the group (the mirror of "
                    + "group_layers' own cleared_clip).",
                ["index": index, "document_id": docID]),
            tool(
                "set_layer_lock",
                "Sets a layer's locks, replacing whatever it had. transparency freezes the "
                    + "ALPHA channel — every pixel keeps its alpha exactly: a transparent "
                    + "pixel stays empty, and on a semi-transparent one paint mixes in at its "
                    + "own opacity × coverage, not boosted by the kept "
                    + "alpha, so the layer's shape cannot change and an eraser cannot punch "
                    + "a hole; pixels refuses "
                    + "every pixel edit (the layer's MASK can still be painted); position "
                    + "refuses moves, offsets and transforms; all is the three together "
                    + "and additionally freezes the mask. A transform is a POSITION edit, "
                    + "never a pixel one: a transparency-locked layer still transforms. "
                    + "A GROUP's locks cover its contents: every layer inside a locked "
                    + "group answers to the group's locks as well as its own, and a refusal "
                    + "names the group to unlock. "
                    + "Locks never block delete, duplicate, reorder, group, rename, "
                    + "opacity, blend mode, visibility or styles. Pass `layers` instead of "
                    + "`layer` to write the same set to several entries as ONE undo step — "
                    + "what the Layer > Lock menu does over a multi-selection; an entry "
                    + "that already carries exactly those locks is skipped. Answers at "
                    + "every depth: 16-bit and 32-bit float layers carry the same locks "
                    + "with the same meanings.",
                [
                    "layer": index,
                    "layers": [
                        "type": "array", "items": ["type": "integer"],
                        "description": "Several layers, exclusive with `layer`/`index`. In "
                            + "a recorded Action this is {\"$layers\": \"selected\"}.",
                    ],
                    "locks": [
                        "type": "array",
                        "items": [
                            "type": "string",
                            "enum": ["transparency", "pixels", "position", "all", "none"],
                        ],
                        "description": "The locks to set; [] or [\"none\"] unlocks.",
                    ],
                    "document_id": docID,
                ], required: ["locks"]),
            tool(
                "align_layers",
                "Aligns the given layers' CONTENT bounds — content_x/y/width/height in "
                    + "get_document, the box of actually opaque pixels, NOT the pixel "
                    + "buffer rect — to one edge of the selection's union (to: layers, the "
                    + "default) or of the canvas (to: canvas). A layer with nothing opaque "
                    + "in it is skipped. Linked layers and a group's children follow the "
                    + "move; a position lock anywhere in that expanded set refuses the "
                    + "whole call BY NAME, even on a layer this alignment would not have "
                    + "moved. A pixel selection is never the reference here. Answers at 8 "
                    + "bits, 16 bits and 32-bit float.",
                [
                    "layers": layerList("The layers to align."),
                    "edge": [
                        "type": "string",
                        "enum": [
                            "left", "horizontal_center", "right",
                            "top", "vertical_center", "bottom",
                        ],
                    ],
                    "to": [
                        "type": "string", "enum": ["layers", "canvas"],
                        "description": "What to align to; default layers (their union).",
                    ],
                    "document_id": docID,
                ], required: ["layers", "edge"]),
            tool(
                "distribute_layers",
                "Spaces the given layers evenly along one axis: the two outermost keep "
                    + "their positions and the GAPS between adjacent CONTENT bounds are "
                    + "made equal (distribute spacing, not distribute centers). "
                    + "Needs at least three layers with opaque pixels. A gap can come out "
                    + "negative when the content is wider than the span; that is "
                    + "arithmetic, not an error. Like align_layers it moves through the "
                    + "expanded set — linked layers and a group's children — and a "
                    + "position lock anywhere in it refuses the whole call by name. Answers "
                    + "at 8 bits, 16 bits and 32-bit float.",
                [
                    "layers": layerList("The layers to distribute.", minItems: 3),
                    "axis": ["type": "string", "enum": ["horizontal", "vertical"]],
                    "document_id": docID,
                ], required: ["layers", "axis"]),
            tool(
                "link_layers",
                "Links the given layers: from now on each of them moves and transforms "
                    + "with the others whatever the call targets. Linking is not a group — "
                    + "the stack is unchanged and nothing composites differently. "
                    + "get_document reports the link id as linked. Answers at every "
                    + "depth.",
                [
                    "layers": layerList("The layers to link.", minItems: 2),
                    "document_id": docID,
                ], required: ["layers"]),
            tool(
                "unlink_layers",
                "Clears the given layers' link. A link group left with a single member is "
                    + "cleared too, since a link of one is not a link. Answers at every "
                    + "depth.",
                [
                    "layers": layerList("The layers to unlink."),
                    "document_id": docID,
                ], required: ["layers"]),
            tool(
                "layer_via_copy",
                "Copies the current selection out of a layer into a NEW layer directly "
                    + "above it, at the same canvas position (Layer ▸ Layer Via Copy, "
                    + "\u{2318}J). With no selection the whole layer is copied. This op "
                    + "RASTERIZES: the new layer keeps neither a text/shape "
                    + "description nor a layer style, and a group or an adjustment layer is "
                    + "refused — use duplicate_layer, which keeps all of them. (The app's "
                    + "\u{2318}J routes those cases, and a multi-selection, to Duplicate "
                    + "Layer for exactly this reason: this tool takes ONE layer, so a set "
                    + "would silently lose all but the first.) Answers at every depth "
                    + "(a 16-bit reply adds storage: rgba16_unorm and a 32-bit float one "
                    + "storage: rgba32_float; a whole-layer copy keeps the pixels the layer "
                    + "holds outside the canvas at every depth). In a 32-bit float document "
                    + "the copy keeps each linear RGB value exactly, above white and below "
                    + "zero included, and weights alpha by the selection.",
                [
                    "layer": index,
                    "name": ["type": "string"],
                    "document_id": docID,
                ]),
            tool(
                "layer_via_cut",
                "Layer Via Copy that also CLEARS the selection from the source layer "
                    + "(\u{21e7}\u{2318}J). Same rasterizing rules, and the source layer's "
                    + "own locks apply to the clear half — a pixel-locked source refuses "
                    + "the whole call. On a text or shape layer the description "
                    + "is dropped and the reply says so. Answers at every depth; in a "
                    + "32-bit float document the cut lowers the source's alpha by the "
                    + "selection and leaves its RGB values as they are.",
                [
                    "layer": index,
                    "name": ["type": "string"],
                    "document_id": docID,
                ]),
            tool(
                "merge_visible",
                "Replaces every layer that CONTRIBUTES to the picture — visible, with every "
                    + "enclosing group visible — with one flat layer holding the current "
                    + "composite. Everything that does not contribute SURVIVES untouched, "
                    + "including a visible layer inside a hidden group. Needs at least two "
                    + "contributing raster layers. Answers at every depth; the flat layer "
                    + "is canvas-sized at every depth, like 8-bit, so anything a "
                    + "contributing layer held outside the canvas is discarded. "
                    + "It bakes the current composite, so a layer with knockout bakes "
                    + "EXACTLY — like flatten_image and stamp_visible.",
                ["document_id": docID]),
            tool(
                "stamp_visible",
                "Adds the current composite as a NEW layer and leaves every existing layer "
                    + "alone (\u{21e7}\u{2325}\u{2318}E). The new layer lands UNCLIPPED "
                    + "directly above the given layer's whole subtree, at that layer's own "
                    + "level. Answers at every depth, and it is canvas-sized at every "
                    + "depth, like 8-bit. In 16-bit and 32-bit float documents it needs one "
                    + "contributing RASTER layer; an adjustment layer does not count.",
                [
                    "above": index,
                    "name": ["type": "string", "description": "Layer name; default \"Stamp\"."],
                    "document_id": docID,
                ]),
            tool(
                "arrange_layer",
                "Moves a layer within its own group: to the front, one step forward, one "
                    + "step backward, or to the back AMONG ITS SIBLINGS. It never moves a "
                    + "layer into or out of a group — reorder_layer with a depth does that. "
                    + "The reply gives the layer's new index.",
                [
                    "layer": index,
                    "to": [
                        "type": "string", "enum": ["front", "forward", "backward", "back"],
                    ],
                    "document_id": docID,
                ], required: ["to"]),
            tool(
                "auto_select_layer",
                "Which layer a Move-tool click at (x, y) would activate: the topmost one "
                    + "whose own coverage there is at least half, skipping hidden layers "
                    + "and adjustment layers. With group: true the answer is that layer's "
                    + "top-level GROUP instead. Answers layer: null when nothing opaque is "
                    + "under the point — a miss, not an error. Activates the layer it found "
                    + "unless activate is false, replacing the selection or — with "
                    + "mode: \"add\" — adding to it and becoming the primary, which is "
                    + "what a \u{21e7}-click on the canvas does.",
                [
                    "x": ["type": "number"], "y": ["type": "number"],
                    "group": [
                        "type": "boolean",
                        "description": "Answer the top-level group instead of the layer.",
                    ],
                    "mode": [
                        "type": "string", "enum": ["replace", "add"],
                        "description": "replace (default) makes the hit layer the whole "
                            + "selection; add appends it and makes it primary.",
                    ],
                    "activate": [
                        "type": "boolean",
                        "description": "Make the found layer active (default true).",
                    ],
                    "document_id": docID,
                ], required: ["x", "y"]),
            tool(
                "set_selected_layers",
                "Selects several layers at once — what the app's \u{21e7}/\u{2318}-click "
                    + "multi-selection does. It only retargets the app's own set-aware "
                    + "commands and adds no undo step; the tools here always take their "
                    + "own explicit layers array, so a script never needs this except to "
                    + "leave the app in a particular state.",
                [
                    "layers": layerList("The layers to select."),
                    "primary": [
                        "type": "integer",
                        "description": "Which of them becomes the active layer; default "
                            + "the topmost.",
                    ],
                    "document_id": docID,
                ], required: ["layers"]),
        ]
    }
}
