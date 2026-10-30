import Foundation

/// The catalog entry for `transform_selected_pixels`, in its own file behind
/// an explicit non-inlining boundary for the reason `AgentCatalog.swift`'s
/// header gives.
extension AgentServer {
    @inline(never) func transformSelectedPixelsCatalog() -> [[String: Any]] {
        [
            tool(
                "transform_selected_pixels",
                "Free-transforms the SELECTED PIXELS of one layer — ⌘T with a selection "
                    + "active. The BOX is the float's opaque bounds — the bbox of the layer's "
                    + "pixels with alpha > 0 under the selection, as ⌘T opens it; an empty "
                    + "float refuses. The "
                    + "layer's pixels × the selection coverage float, resample once through "
                    + "the transform (premultiplied, with `sampler`) and composite back "
                    + "Normal source-over; the hole they leave is transparent (alpha "
                    + "a·(1−coverage), no Background fill), pixels that would land off the "
                    + "canvas are discarded, the layer mask stays put, and the selection is "
                    + "transformed with the same map to become the new selection — ONE undo "
                    + "step, \"Transform Selected Pixels\". Pass transform_layer's rotate / "
                    + "scale / scale_x / scale_y / translate_x / translate_y (around the "
                    + "BOX's center by default, or around / pivot_x / pivot_y), OR `corners` "
                    + "for a distort/perspective of the box — not both. Needs an active "
                    + "selection (transform_layer transforms whole layers and ignores the "
                    + "selection). One pixel layer only: with several layers selected in the "
                    + "panel, name one with `layer`; links are ignored. Groups, adjustment "
                    + "layers, Quick Mask and a retained source refuse by name; Lock Pixels / "
                    + "Lock All refuse, Lock Transparency and "
                    + "Lock Position allow it. A text or shape layer is "
                    + "rasterized and the result says so. An identity is an unchanged no-op. "
                    + "Returns the box, the layer's new bounds and the new selection bounds. "
                    + "`duplicate: true` is ⌥⌘T, Free Transform a Copy: no hole, the original "
                    + "stays and the transformed COPY lands source-over, the selection "
                    + "transforms with the copy — ONE undo step, \"Transform Copy\"; echoed in "
                    + "the result. Works at 8 and 16 bits and in a 32-bit float document "
                    + "(f32, nothing quantized; a filtered resample clamps each RGB component "
                    + "to the floated pixels' own visible range and drops alpha below 2^-24). "
                    + "`target` \"mask\" or "
                    + "\"channel:<name>\" transforms the selected area of the layer's "
                    + "mask or of an alpha channel instead — at 8, 16 and 32-bit float (masks "
                    + "and channels are 16-bit coverage there): the BOX is the selection's "
                    + "bounds, the plane's selected coverage floats (feathered edges carry "
                    + "through) and lands back through the same resample, and the vacated area "
                    + "takes `background` (a hex, default #000000) as coverage gray, "
                    + "the background-colour rule for a plane without transparency; "
                    + "`duplicate` leaves no hole. A mask's stored rect grows to hold what "
                    + "lands on the canvas (a group's mask is clipped instead), a channel stays "
                    + "canvas-sized, and the layer's pixels and any description stay. Lock All "
                    + "refuses a mask (Lock Pixels allows it); a channel has no lock; Quick "
                    + "Mask refuses. The reply names the target and carries mask_bounds or "
                    + "channel in place of layer_bounds. "
                    + "`target` \"red\", \"green\", \"blue\" or \"alpha\" transforms ONE "
                    + "colour channel of the layer's selected pixels instead, at 8, 16 and "
                    + "32-bit float: only that component lands (the other components and the "
                    + "alpha stay, the layer never grows), the red/green/blue vacated area takes "
                    + "`background`'s 0.30/0.59/0.11 luma rounded to an 8-bit code ("
                    + "default #000000), Alpha's is transparent; selected pixels with no stored "
                    + "alpha float nothing, so a selection over only transparent pixels refuses "
                    + "with \"The selection holds no pixels in this colour "
                    + "channel.\" Lock Pixels refuses, and Lock Transparency an Alpha "
                    + "target too; a text or shape layer is rasterized and the reply "
                    + "is the layer target's. The box is then the selection's bounds.",
                [
                    "layer": [
                        "type": "integer",
                        "description": "Layer index whose selected pixels transform; default "
                            + "the active layer.",
                    ],
                    "rotate": [
                        "type": "number",
                        "description": "Rotation in degrees around the pivot; POSITIVE IS "
                            + "CLOCKWISE on screen.",
                    ],
                    "scale": [
                        "type": "number",
                        "description": "Uniform scale multiplier (1 = unchanged, negative "
                            + "mirrors); magnitudes clamp to 0.001-100 and 0 is refused.",
                    ],
                    "scale_x": [
                        "type": "number",
                        "description": "Horizontal scale multiplier, overriding scale.",
                    ],
                    "scale_y": [
                        "type": "number",
                        "description": "Vertical scale multiplier, overriding scale.",
                    ],
                    "translate_x": [
                        "type": "number",
                        "description": "Move right by this many canvas px, after the rotation "
                            + "and scale.",
                    ],
                    "translate_y": [
                        "type": "number",
                        "description": "Move down by this many canvas px, after the rotation "
                            + "and scale.",
                    ],
                    "around": [
                        "type": "string",
                        "enum": ["center", "top_left"],
                        "description": "The pivot: \"center\" (default) or \"top_left\" of "
                            + "the BOX. pivot_x / pivot_y override it.",
                    ],
                    "pivot_x": [
                        "type": "number",
                        "description": "Explicit pivot x in canvas px.",
                    ],
                    "pivot_y": [
                        "type": "number",
                        "description": "Explicit pivot y in canvas px.",
                    ],
                    "corners": [
                        "type": "array",
                        "items": ["type": "array", "items": ["type": "number"]],
                        "description": "Four [x, y] canvas destinations of the BOX's top-left, "
                            + "top-right, bottom-right and bottom-left corners (a convex "
                            + "quad); exclusive with the matrix arguments.",
                    ],
                    "duplicate": [
                        "type": "boolean",
                        "description": "Transform a COPY and leave the original (⌥⌘T); "
                            + "default false. Echoed in the result.",
                    ],
                    "target": [
                        "type": "string",
                        "description": "What transforms: \"layer\" (default, the layer's "
                            + "pixels), \"mask\" (the layer's mask), \"channel:<name>\" "
                            + "(an alpha channel) or \"red\", \"green\", \"blue\", "
                            + "\"alpha\" (one colour channel of the layer).",
                    ],
                    "background": [
                        "type": "string",
                        "description": "A mask or channel target's vacated-area colour as a "
                            + "hex (#RRGGBB), read as coverage gray; for a red, green or "
                            + "blue target the background colour whose 0.30/0.59/0.11 luma fills "
                            + "the vacated area; default #000000. Refused with a layer or "
                            + "alpha target.",
                    ],
                    "sampler": [
                        "type": "string",
                        "enum": ["nearest", "bilinear", "bicubic", "lanczos"],
                        "description": "Resampling filter, default bicubic (Catmull-Rom). "
                            + "Whole-pixel moves are lossless whatever this says.",
                    ],
                    "document_id": AgentServer.docIDProperty,
                ]),
        ]
    }
}
