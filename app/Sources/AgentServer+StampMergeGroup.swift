import AppKit

/// `stamp_selected` and `merge_group` (#547 phase 4c) at 8, 16 and 32 bits
/// and on a CMYK document — the MCP twins of Layer ▸ Stamp Selected and of
/// ⌘E with one group selected (Layer ▸ Merge Group,
/// `EditorViewController+MergeStamp.swift`). Each is ONE core call
/// (`RasterDocument.stampingSelected` / `mergingGroup`), so one undo step;
/// a refusal is the core's sentence (typed) or a named one here (8 bits).
/// Recorded by the generic recorder with their literal arguments.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let stampMergeGroupHandlers: [String: ToolHandler] = [
        "stamp_selected": { $0.stampSelected },
        "merge_group": { $0.mergeGroup },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let stampMergeGroupToolFacts: [String: ToolFacts] = [
        // #547 phase 4c: `layer_stamp_selected` / `layer_merge` at every
        // typed depth and on CMYK (`precision_merge_selected`).
        "stamp_selected": [.typedTool, .floatTool, .cmykTool],
        "merge_group": [.typedTool, .floatTool, .cmykTool],
    ]

    /// Mirrors Layer ▸ Stamp Selected: `layers` (default the SELECTED
    /// layers; a group's own children are inside it) — one group, or two or
    /// more sharing one parent — composited into ONE new layer directly above
    /// the topmost, named `name` (default "<topmost> (merged)"); the
    /// originals are untouched. Replies `{ok, layer, name, members}`.
    func stampSelected(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let requested = a["layers"] != nil
            ? try layerIndices(a, document) : document.selectedLayerIndices
        let members = doc.layerTree.independentRoots(requested)
        guard let top = members.last else {
            throw ToolError(message: "Stamp Selected requires at least one layer")
        }
        let name = stringArg(a, "name") ?? ((doc.layerInfo(top)?.name ?? "Layer") + " (merged)")
        let landing = doc.insertionIndex(above: top)
        guard try editStructure(document, "Stamp Selected", {
            try $0.stampingSelected(members, name: name)
        }) else {
            throw ToolError(message: "Those layers cannot be stamped: Stamp Selected takes one "
                + "group, or two or more layers that share one parent (get_document reports each "
                + "layer's parent), and something of them must be visible.")
        }
        let created = min(landing, (document.doc?.layerCount ?? 1) - 1)
        retarget(document, .single(created))
        return try jsonResult(["ok": true, "layer": created, "name": name, "members": members])
    }

    /// Mirrors ⌘E with one group selected (Layer ▸ Merge Group): the visible
    /// group `layer` (default the active layer) replaced by ONE layer holding
    /// its contents and keeping its name, opacity, blend mode (Pass Through
    /// becomes Normal), mask and place. Replies `{ok, layer, name}`.
    func mergeGroup(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let index = try structuralLayerIndex(a, document)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard doc.layerIsGroup(index), doc.layerInfo(index)?.visible == true else {
            throw ToolError(message: "Merge Group requires a visible group; layer \(index) is not one")
        }
        try rejectLockedEdit(document, index, DR_EDIT_MERGE)
        let name = doc.layerInfo(index)?.name ?? ""
        let landing = doc.layerTree.subtree(of: index).lowerBound
        guard try editStructure(document, "Merge Group", { try $0.mergingGroup(index) }) else {
            throw ToolError(message: "Group \(index) cannot be merged: nothing inside it is visible.")
        }
        let merged = min(landing, (document.doc?.layerCount ?? 1) - 1)
        retarget(document, .single(merged))
        return try jsonResult(["ok": true, "layer": merged, "name": name])
    }
}
