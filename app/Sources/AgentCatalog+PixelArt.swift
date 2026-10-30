import Foundation

/// The catalog entries for `put_pixels` and `close_document`, in their own
/// file behind an explicit non-inlining boundary for the reason
/// `AgentCatalog.swift`'s header gives.
extension AgentServer {
    @inline(never) func pixelArtCatalog() -> [[String: Any]] {
        [
            tool(
                "put_pixels",
                "Writes exact pixels onto one raster layer — pixel art, sprites, icons — in ONE "
                    + "call and ONE undo step. REPLACE semantics: each written pixel takes its "
                    + "colour's four bytes verbatim, alpha included, with no blending, so an "
                    + "alpha-0 colour writes transparency and #RRGGBB80 writes exactly alpha "
                    + "128. Pass runs, buffer or both. runs: horizontal spans {x, y, length "
                    + "(default 1), color} starting at canvas (x, y). buffer: one rect {x, y, "
                    + "width, height, data} whose data is base64 of exactly width×height×4 "
                    + "straight RGBA8 bytes, row 0 at the top. The buffer is written first, then "
                    + "the runs in array order; the last write to a pixel wins. Coordinates are "
                    + "integer canvas pixels and everything must lie inside the canvas (refused "
                    + "by name otherwise). The selection is IGNORED — the coordinates are the "
                    + "confinement, as with transform_layer and move_layers. A write outside a "
                    + "trimmed or offset layer's pixel rect grows the layer to its union with "
                    + "the canvas in the same undo step (refused while its transparency is "
                    + "locked). 8-bit documents only. The reply gives pixels (distinct positions "
                    + "written), bounds and grew_layer; a write that changes no byte answers "
                    + "unchanged: true and adds no undo step. Use render with scale to see the "
                    + "result as crisp enlarged pixels.",
                [
                    "document_id": AgentServer.docIDProperty,
                    "layer": [
                        "type": "integer",
                        "description": "Raster layer index (default: the active layer; index is "
                            + "accepted too). A group or an adjustment layer is refused.",
                    ],
                    "runs": [
                        "type": "array",
                        "description": "Horizontal spans, written in array order after the "
                            + "buffer.",
                        "items": [
                            "type": "object",
                            "properties": [
                                "x": ["type": "integer"],
                                "y": ["type": "integer"],
                                "length": [
                                    "type": "integer",
                                    "description": "Pixels to the right of x, x included "
                                        + "(default 1).",
                                ],
                                "color": [
                                    "type": "string",
                                    "description": "#RGB, #RRGGBB or #RRGGBBAA (required).",
                                ],
                            ],
                            "required": ["x", "y", "color"],
                        ] as [String: Any],
                    ],
                    "buffer": [
                        "type": "object",
                        "description": "One rect of raw pixels, written before the runs.",
                        "properties": [
                            "x": ["type": "integer"], "y": ["type": "integer"],
                            "width": ["type": "integer"], "height": ["type": "integer"],
                            "data": [
                                "type": "string",
                                "description": "Base64 of exactly width×height×4 straight "
                                    + "(non-premultiplied) RGBA8 bytes, row-major, row 0 at "
                                    + "the top.",
                            ],
                        ],
                    ],
                    "color_space": [
                        "type": "string",
                        "enum": ["srgb", "document", "display_p3"],
                        "description": "The space of every colour here, run hex and buffer "
                            + "bytes alike: srgb (default) converts once into the document's "
                            + "space; document writes the bytes verbatim; display_p3 converts "
                            + "from Display P3.",
                    ],
                ]),
            tool(
                "close_document",
                "Closes one open document — File ▸ Close. document_id is REQUIRED (there is no "
                    + "frontmost default). A document with unsaved changes is refused by name "
                    + "unless discard_changes is true, which LOSES those changes and their "
                    + "recovery copy; save_copy writes a copy first but does not clear the "
                    + "unsaved-changes flag. An open canvas session (text entry, a reopened "
                    + "shape, a Free Transform) is committed first, and a document with a "
                    + "dialog open in its window is refused. The app keeps running after its "
                    + "last document closes. For external MCP clients only: it is not offered "
                    + "to the in-app Assistant panel, and it may not be an Action step. Reply: "
                    + "closed {id, title, path}, discarded_changes and open_documents (the ids "
                    + "still open).",
                [
                    "document_id": [
                        "type": "integer",
                        "description": "The document to close, as list_documents reports it. "
                            + "REQUIRED.",
                    ],
                    "discard_changes": [
                        "type": "boolean",
                        "description": "Default false. True closes a document with unsaved "
                            + "changes and loses them.",
                    ],
                ], required: ["document_id"]),
        ]
    }
}
