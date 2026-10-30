import Foundation

/// The catalog entry for `copy_layers_to_document`, in its own file
/// behind an explicit non-inlining boundary for the reason
/// `AgentCatalog.swift`'s header gives.
extension AgentServer {
    @inline(never) func layerDragCatalog() -> [[String: Any]] {
        [
            tool(
                "copy_layers_to_document",
                "Copies layers from one open document into another — what dragging "
                    + "layers from one window's Layers panel (or its canvas, with the Move "
                    + "tool) onto another window does. `layers` of `document_id` (default: "
                    + "its selected layers), groups with everything inside them, land in "
                    + "`to_document_id` above its active layer and become its layer "
                    + "selection, as ONE undo step in the destination. The source document "
                    + "is not changed and the clipboard is not used. placement \"auto\" "
                    + "(default) lands them by Paste's rule (centred in the view, or on the "
                    + "selection); \"in_place\" keeps their canvas position. Depth and "
                    + "colour profile are converted to the destination's, as paste does "
                    + "for copied layers, and the reply is paste's: where the layers "
                    + "landed, their new indices and what the conversion changed. Unlike "
                    + "copy and paste of layers, a camera RAW "
                    + "layer lands as its current pixels, with no "
                    + "image source in "
                    + "the destination; one whose pixels are locked refuses by name. Refuses "
                    + "when both ids name the same document. Not recorded in Actions.",
                [
                    "document_id": [
                        "type": "integer",
                        "description": "The SOURCE document's id from list_documents; omit "
                            + "for the frontmost document.",
                    ],
                    "to_document_id": [
                        "type": "integer",
                        "description": "The DESTINATION document's id from list_documents.",
                    ],
                    "layers": [
                        "type": "array", "items": ["type": "integer"],
                        "description": "Source layer indices (default: the source's "
                            + "selected layers).",
                    ],
                    "placement": [
                        "type": "string", "enum": ["auto", "in_place"],
                        "description": "auto (default) or in_place.",
                    ],
                ], required: ["to_document_id"]),
        ]
    }
}
