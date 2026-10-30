import Foundation

/// The catalog entries for `straighten_layers` and `perspective_crop`,
/// `trim` and `reveal_canvas`, in their own file behind an explicit
/// non-inlining boundary for the reason `AgentCatalog.swift`'s header gives.
extension AgentServer {
    @inline(never) func canvasFramingCatalog() -> [[String: Any]] {
        let number: [String: Any] = ["type": "number"]
        let sampler: [String: Any] = [
            "type": "string", "enum": ["nearest", "bilinear", "bicubic", "lanczos"],
            "description": "Resampling filter (default bicubic).",
        ]
        return [
            tool(
                "straighten_layers",
                "Rotates layers so a line you name becomes level or plumb — the Ruler "
                    + "tool's Straighten Layer. (x0, y0)–(x1, y1) are the two ends of "
                    + "something in the picture that should be horizontal or vertical (a "
                    + "horizon, a wall edge), in canvas px. The layers turn about the canvas "
                    + "centre by the SMALLEST angle that lays the line along an axis (never "
                    + "more than 45°), so a nearly vertical line is made vertical. Applies to "
                    + "`layers` (default: the selected layers), with their groups' "
                    + "descendants and linked partners, as one undo step; the canvas size "
                    + "does not change (crop afterwards if the corners matter). The reply "
                    + "adds `rotation`, the degrees turned, counter-clockwise on screen. A "
                    + "line already along an axis changes nothing. Follows "
                    + "transform_layers' rules for locks and described layers. Works at 8 "
                    + "and 16 bits and in 32-bit float.",
                [
                    "x0": number, "y0": number, "x1": number, "y1": number,
                    "layers": [
                        "type": "array", "items": ["type": "integer"],
                        "description": "Layer indices to rotate (default: the selected layers).",
                    ],
                    "sampler": sampler,
                    "document_id": AgentServer.docIDProperty,
                ], required: ["x0", "y0", "x1", "y1"]),
            tool(
                "perspective_crop",
                "Crops the document to a quadrilateral and squares it up — the "
                    + "Perspective Crop tool. `corners` are four canvas points in the order "
                    + "top-left, top-right, bottom-right, bottom-left (the corners of a "
                    + "sign, a painting, a page photographed at an angle); the result is a "
                    + "width × height canvas showing that quadrilateral flat. Every layer, "
                    + "mask and alpha channel is resampled; anything the quadrilateral "
                    + "reaches beyond the old canvas comes out transparent; guides are "
                    + "cleared. width and height default to the longer of each pair of "
                    + "opposite edges. One undo step. The corners must form a convex "
                    + "quadrilateral. Text and shape layers are rasterized, "
                    + "which needs rasterize_artwork: true; a camera "
                    + "RAW layer refuses the crop until it is rasterized. The four corners "
                    + "of the whole canvas at its own size change nothing. Works at 8 and "
                    + "16 bits and in 32-bit float.",
                [
                    "corners": [
                        "type": "array", "minItems": 4, "maxItems": 4,
                        "items": ["type": "array", "items": number, "minItems": 2, "maxItems": 2],
                        "description": "[[x, y] × 4]: top-left, top-right, bottom-right, "
                            + "bottom-left, in canvas px.",
                    ],
                    "width": ["type": "integer", "minimum": 1,
                              "description": "Result width in px (default: automatic)."],
                    "height": ["type": "integer", "minimum": 1,
                               "description": "Result height in px (default: automatic)."],
                    "sampler": sampler,
                    "rasterize_artwork": [
                        "type": "boolean",
                        "description": "Authorize rasterizing text and shape "
                            + "layers; default false.",
                    ],
                    "document_id": AgentServer.docIDProperty,
                ], required: ["corners"]),
            tool(
                "trim",
                "Crops the canvas inward to remove an empty or flat border — Image ▸ "
                    + "Trim…. based_on says what counts as border: \"transparent\" (rows "
                    + "and columns whose every pixel is fully transparent), \"top_left\" or "
                    + "\"bottom_right\" (rows and columns whose every pixel exactly equals "
                    + "that corner pixel of the flattened image). top, bottom, left and "
                    + "right choose which edges may move (default: all four). Read from the "
                    + "flattened composite at the document's own depth; layers keep their "
                    + "pixels and nothing is resampled. One undo step; when no chosen edge "
                    + "has anything to trim, or everything would be trimmed away, nothing "
                    + "changes. The reply's `kept` is the rectangle of the old canvas that "
                    + "remains.",
                [
                    "based_on": [
                        "type": "string", "enum": TrimBasis.allCases.map(\.agentName),
                        "description": "What to trim away (default transparent).",
                    ],
                    "top": ["type": "boolean"], "bottom": ["type": "boolean"],
                    "left": ["type": "boolean"], "right": ["type": "boolean"],
                    "document_id": AgentServer.docIDProperty,
                ]),
            tool(
                "reveal_canvas",
                "Grows the canvas until it holds every layer's pixels — Image ▸ Reveal "
                    + "All. Layers that were moved, pasted or transformed partly off the "
                    + "canvas become fully visible; nothing is resampled and no layer "
                    + "moves relative to another. One undo step. The reply's `origin` is "
                    + "where the old canvas's top-left sits in the new one. Nothing changes "
                    + "when no layer reaches outside the canvas.",
                ["document_id": AgentServer.docIDProperty]),
        ]
    }
}
