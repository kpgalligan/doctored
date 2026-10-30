import Foundation

/// The catalog entry for `transform_again`, in its own file behind an
/// explicit non-inlining boundary for the reason `AgentCatalog.swift`'s header
/// gives.
extension AgentServer {
    @inline(never) func transformAgainCatalog() -> [[String: Any]] {
        [
            tool(
                "transform_again",
                "Repeats the last committed Free Transform — Layer ▸ Transform "
                    + "Again (⇧⌘T), or with `duplicate` Layer ▸ Transform Again with Copy "
                    + "(⌥⇧⌘T). The APPLICATION remembers ONE transform, shared by every "
                    + "document: the canvas-space matrix, pivot included, its sampler and the "
                    + "canvas size of the document it was committed in. It is set by every committed "
                    + "⌘T / ⌥⌘T in the app and by transform_layer, transform_layers, "
                    + "distort_layer and transform_selected_pixels (and by transform_again "
                    + "itself), only when the edit changed something; a parallelogram "
                    + "distort stores its affine, and a TRUE PERSPECTIVE is remembered as its "
                    + "corners over its source rectangle. Undo, redo, history and closing a "
                    + "document never change it, and it is never saved. It repeats only in a "
                    + "document whose canvas is the size it was committed on: "
                    + "an edit that changes the canvas size makes it unavailable until "
                    + "the size is back (undo, or sizing back), Rotate 180°, the flips and a "
                    + "quarter turn of a square canvas keep it, and it is never remapped. "
                    + "The matrix is applied VERBATIM in canvas "
                    + "space — no pivot is re-derived from the new target — so repeating a "
                    + "15° turn on the same content gives 30° about the same pivot, a 10 px "
                    + "move becomes 20 px, and a different layer turns about the same canvas "
                    + "point; a true perspective repeats as the same canvas-space perspective "
                    + "(the reply carries source_rect and corners in place of matrix). With "
                    + "one, a text, shape or artwork layer in a layer target needs "
                    + "rasterize_artwork (transform_layers' rule), and a target the map would "
                    + "fold past its horizon refuses in band, changing nothing. "
                    + "The target is ⌘T's: with a selection, the ACTIVE layer's "
                    + "selected pixels (transform_selected_pixels' rules: one pixel layer, "
                    + "the hole transparent, the selection transformed with them); without "
                    + "one, the selected layers with their groups' descendants and linked "
                    + "partners (transform_layers' rules). ONE undo step, \"Transform "
                    + "Again\", or \"Transform Again with Copy\" with `duplicate`: over a "
                    + "selection a transformed COPY of the pixels lands and no hole is left; "
                    + "without one the selected layers are duplicated, the copies transform "
                    + "and become the layer selection (the result lists `copies`). Refuses in "
                    + "band when no transform is remembered or it was committed on a canvas of "
                    + "another size (the refusal names both sizes), on a Position lock (layers) or "
                    + "a Pixels lock (selected pixels), for an empty selected area, a group, "
                    + "an adjustment layer, Quick Mask and several layers selected with a "
                    + "selection. A described layer "
                    + "whose pixels are resampled is rasterized and the result says so; with "
                    + "an affine memory, one text or shape layer at 8 bits "
                    + "composes into its description instead. `duplicate` with no selection "
                    + "applies the same rules to the COPIES (transform_layers' duplicate "
                    + "rules): one copied text or shape layer at 8 bits composes "
                    + "into its own description, a remembered true perspective over "
                    + "described copies needs rasterize_artwork, and rasterized_layers lists "
                    + "the copies whose description was dropped. Works at 8 and 16 bits and "
                    + "in a 32-bit float document, with or without a selection. With a selection, "
                    + "`target` \"mask\" or \"channel:<name>\" repeats onto the "
                    + "selected area of the active layer's mask or of an alpha channel "
                    + "(transform_selected_pixels' plane rules: the box is the selection's "
                    + "bounds, the vacated area takes `background`, default #000000) — also "
                    + "in a 32-bit float document; the memory itself stores no target, so "
                    + "each call names it. A mask or channel target with no selection "
                    + "refuses (transform_mask transforms a whole mask). With a selection, "
                    + "`target` \"red\", \"green\", \"blue\" or \"alpha\" repeats onto "
                    + "that colour channel of the active layer (transform_selected_pixels' "
                    + "colour-plane rules, `background`'s rounded 30/59/11 luma as the red/green/blue "
                    + "vacated value).",
                [
                    "duplicate": [
                        "type": "boolean",
                        "description": "Transform a COPY and leave the original (⌥⇧⌘T); "
                            + "default false.",
                    ],
                    "rasterize_artwork": [
                        "type": "boolean",
                        "description": "Authorize rasterizing text, shape or "
                            + "artwork layers (or their copies, with `duplicate`) when the "
                            + "remembered transform is a true perspective and the target is "
                            + "layers; default false.",
                    ],
                    "target": [
                        "type": "string",
                        "description": "With a selection: \"layer\" (default, the active "
                            + "layer's selected pixels), \"mask\", \"channel:<name>\" or "
                            + "\"red\", \"green\", \"blue\", \"alpha\".",
                    ],
                    "background": [
                        "type": "string",
                        "description": "A mask or channel target's vacated-area colour as a "
                            + "hex, read as coverage gray; for a red, green or blue target "
                            + "the background colour whose 0.30/0.59/0.11 luma fills the vacated "
                            + "area; default #000000.",
                    ],
                    "document_id": AgentServer.docIDProperty,
                ]),
        ]
    }
}
