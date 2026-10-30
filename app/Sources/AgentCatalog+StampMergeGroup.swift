import Foundation

/// `stamp_selected` and `merge_group`'s catalog entries (#1 phase 4c,
/// `AgentServer+StampMergeGroup.swift`). Behind an explicit non-inlining
/// boundary, for `AgentCatalog.swift`'s reason.
extension AgentServer {
    @inline(never) func stampMergeGroupCatalog() -> [[String: Any]] {
        let docID = Self.docIDProperty
        return [
            tool(
                "stamp_selected",
                "Layer ▸ Stamp Selected: the given layers "
                    + "— one group, or two or more layers that share one parent — composited "
                    + "into ONE new layer over their combined extent, Normal at 100 %, unclipped, "
                    + "directly above the topmost of them; every original layer is left exactly "
                    + "as it was. One undo step, at 8, 16 and 32 bits and in a CMYK document "
                    + "(composited in ink). Reply: ok, layer (the new layer's index), name, "
                    + "members (the layers stamped).",
                [
                    "layers": ["type": "array", "items": ["type": "integer"],
                               "description": "The layers to stamp; default the selected layers "
                                   + "(a selected group's children are inside it)."],
                    "name": ["type": "string",
                             "description": "The new layer's name; default \"<topmost layer> "
                                 + "(merged)\"."],
                    "document_id": docID,
                ]),
            tool(
                "merge_group",
                "Layer ▸ Merge Group (⌘E with one group selected): the visible group becomes ONE "
                    + "layer holding its contents, keeping the group's name, opacity, blend mode "
                    + "(Pass Through becomes Normal), mask, style, clipping, locks and place, so "
                    + "an ordinary (Normal or other non-Pass Through) group looks exactly as it "
                    + "did. One undo step, at 8, 16 and 32 bits and in a CMYK document. A hidden "
                    + "group, a layer that is not a group and a merge-locked group refuse by "
                    + "name. Reply: ok, layer, name. merge_down merges a layer into the one "
                    + "below it, as before.",
                [
                    "layer": ["type": "integer",
                              "description": "The group's index; default the active layer."],
                    "document_id": docID,
                ]),
        ]
    }
}
