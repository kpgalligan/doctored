import Foundation

/// The Phase 12A catalog entry for `move_layers`, in its own file behind an
/// explicit non-inlining boundary for the reason `AgentCatalog.swift`'s
/// header gives.
extension AgentServer {
    @inline(never) func moveLayersCatalog() -> [[String: Any]] {
        [
            tool(
                "move_layers",
                "Moves a SET of layers by a relative delta — the Move tool's drag and the "
                    + "arrow-key nudge. One moveLayers call and one undo step: a group's "
                    + "subtree and every linked partner follow, and a position lock on any "
                    + "of them refuses the whole set by name. dx is canvas pixels right, dy "
                    + "is canvas pixels down. dx = dy = 0 is reported as an unchanged no-op "
                    + "rather than an error, so a recorded drag that ended where it began "
                    + "replays cleanly. To place ONE layer at an ABSOLUTE position use "
                    + "set_layer_properties' offset_x/offset_y instead (that setter writes "
                    + "one entry and does not follow links). move_layers ignores the "
                    + "selection and moves whole layers; to move only the selected pixels "
                    + "use move_selected_pixels. duplicate: true is the Move tool's ⌥-drag and "
                    + "⌥+arrow with no selection: it duplicates `layers` (their subtrees; the "
                    + "copies are unlinked and land directly above their sources) and moves the "
                    + "COPIES, one \"Duplicate Layer\" undo step, and the copies become the layer "
                    + "selection; a zero delta still duplicates, in place. Duplicating is never "
                    + "lock-refused; moving the copies refuses by name when an original, an "
                    + "enclosing group or a group original's descendant carries a position lock "
                    + "(a link partner's lock never refuses). The reply adds `copies` (the copies' "
                    + "indices) and \"duplicate\": true, and `moved` lists the copies.",
                [
                    "layers": [
                        "type": "array", "items": ["type": "integer"],
                        "description": "Layer indices to move; duplicates are refused. In a "
                            + "recorded Action this is {\"$layers\": \"selected\"}.",
                    ],
                    "dx": [
                        "type": "integer",
                        "description": "Canvas pixels to move right (negative moves left).",
                    ],
                    "dy": [
                        "type": "integer",
                        "description": "Canvas pixels to move down (negative moves up).",
                    ],
                    "duplicate": [
                        "type": "boolean",
                        "description": "Default false. True duplicates the layers and moves the "
                            + "copies (the Move tool's ⌥-drag / ⌥+arrow with no selection).",
                    ],
                    "document_id": AgentServer.docIDProperty,
                ], required: ["layers", "dx", "dy"]),
        ]
    }
}
