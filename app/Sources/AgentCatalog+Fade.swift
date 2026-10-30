import Foundation

/// v1's catalog entry for `fade` (Edit ▸ Fade…), in its own file behind an
/// explicit non-inlining boundary for the reason `AgentCatalog.swift`'s header
/// gives.
extension AgentServer {
    @inline(never) func fadeCatalog() -> [[String: Any]] {
        [
            tool(
                "fade",
                "Edit ▸ Fade… ▸ Apply: blends the result of the LAST edit — a filter or "
                    + "destructive adjustment (apply_filter, auto_tone/contrast/color, "
                    + "liquify), a fill (fill, fill_selection, gradient) or a paint or "
                    + "retouch stroke — with the image before it, at `opacity` in `mode`. "
                    + "It replaces that edit as ONE undo step named "
                    + "\"Fade <step>\". A filter, adjustment, fill_selection or gradient "
                    + "fades within the selection it ran under; a stroke or a Paint Bucket "
                    + "fill only where it changed the layer. Opacity 1 in Normal "
                    + "keeps the edit (changed:false), 0 restores the image before it. "
                    + "Refused when there is nothing to fade: any other change since that "
                    + "edit (a selection, a layer, an undo) or an edit on a mask, plane, "
                    + "channel or the Quick Mask. In a 32-bit float document only the modes "
                    + "it admits are allowed. The reply is {ok, changed, faded, name}.",
                [
                    "document_id": AgentServer.docIDProperty,
                    "opacity": [
                        "type": "number", "minimum": 0, "maximum": 1,
                        "description": "How much of the edit to keep, 0…1.",
                    ],
                    "mode": [
                        "type": "string",
                        "enum": DrBlendMode.allBlendModes.map { $0.1 },
                        "description": "How the edit's result blends over the image before "
                            + "it (default Normal).",
                    ],
                ], required: ["opacity"]),
        ]
    }
}
