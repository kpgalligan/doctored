import AppKit

/// `reorder_layers` — the MULTI-entry twin of `reorder_layer`, and the last
/// of the four layer-set gaps a recording used to fill with a visible
/// `__unrecorded` placeholder.
///
/// It mirrors the layers panel's drag of SEVERAL rows
/// (`LayersPanelViewController.tableView(_:acceptDrop:row:dropOperation:)`),
/// and it exists because that gesture could not be written as N
/// `reorder_layer` steps: each move is expressed in the numbering the
/// PREVIOUS one produced, so the second step's `from` and `to` depend on what
/// the first did. The panel and this handler therefore run the SAME
/// arithmetic — `LayerReorder.reorder`, which moved out of the panel for
/// exactly this — rather than two implementations that would agree only for
/// the easy cases.
///
/// `at` is the insertion index in the PRE-move numbering: the drop line the
/// panel drew, before anything is taken out. That is deliberately NOT
/// `reorder_layer`'s `to` (which numbers the stack with the moved subtree
/// already removed), because with several subtrees in flight there is no
/// single "already removed" stack to number against.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let layerSetsHandlers: [String: ToolHandler] = [
        "reorder_layers": { $0.reorderLayers },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let layerSetsToolFacts: [String: ToolFacts] = [
        // Phase 12B's batch and bundle tools read no document, so they are
        // in `documentFreeTools`. `reorder_layers` is a TYPED tool
        // since 12E-1 (`typedTools` below): the core owns the typed reorder.
        // Phase 12C: a colour LABEL is metadata with a typed implementation
        // of its own (the `layer_label` edit), and `find_layers` only reads —
        // neither touches pixel storage. `new_document` and the three
        // document-preset tools read no document, so they are in
        // `documentFreeTools`.
        // Phase 12E-1: the four pure STRUCTURE tools whose typed twins the
        // core now owns (`precision_structure_ops`). `group_layers` was
        // already here, restricted to "every top-level layer"; that
        // restriction is gone — a subset sharing one parent is what the core
        // takes at either depth.
        // Phase 12E-1: the pure STRUCTURE edits move no pixels — the core's
        // typed ops splice records and keep every plane pointer-equal — so a
        // float document answers them exactly as it answers `group_layers`.
        "reorder_layers": [.typedTool, .floatTool, .cmykTool],
    ]

    func reorderLayers(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let sources = try layerIndices(a, document)
        let count = doc.layerCount
        guard let at = intArg(a, "at") else {
            throw ToolError(
                message: "reorder_layers requires at — the insertion index in the CURRENT "
                    + "numbering (0 is the bottom, \(count) is above everything), which is "
                    + "the drop line the layers panel draws.")
        }
        guard at >= 0, at <= count else {
            throw ToolError(message: "at \(at) is out of range (0..\(count))")
        }
        // Omitted depth means the level the drop lands in, read off the entry
        // BELOW the line — the same default `reorder_layer` applies, and what
        // a drag onto a row means.
        // `count - 1` is -1 on an empty stack, so the lookup is only asked
        // when there IS an entry below the line; depth 0 is the only answer
        // an empty document can have.
        let depth = intArg(a, "depth")
            ?? (count > 0 ? doc.layerDepth(min(max(at - 1, 0), count - 1)) : 0)
        guard depth >= 0 else { throw ToolError(message: "depth must be 0 or more") }
        // Computed from THIS handle before the edit, so a drag taken back is
        // reported as the no-op it is rather than minting an undo step for a
        // document that did not change.
        guard let outcome = try LayerReorder.reorder(
            doc, sources: sources, drop: (at: at, depth: depth, into: nil))
        else {
            throw ToolError(
                message: "That reorder was refused. `at` may not land INSIDE one of the "
                    + "subtrees being moved, and the (at, depth) pair has to leave a "
                    + "well-formed tree: an entry at depth d needs an enclosing group, and "
                    + "the topmost entry has to sit at depth 0. get_document reports each "
                    + "layer's depth and parent.")
        }
        let sameDepth = sources.allSatisfy { doc.layerDepth($0) == depth }
        guard outcome.landed != sources || !sameDepth else {
            return try structureNoOp(
                ["layers": sources, "at": at, "depth": depth],
                why: "every entry is already at that index and depth.")
        }
        // The reorder was computed from this very handle a moment ago — which
        // is what let the no-op above be detected before an undo step
        // existed — so the closure has nothing left to do with its argument.
        // The panel's own `applyEdit` has exactly this shape.
        guard try editStructure(document, "Reorder Layers", { _ in outcome.document }) else {
            throw ToolError(message: "The reorder was refused by the document.")
        }
        // Keep the moved entries selected at their NEW indices, exactly as the
        // panel does, so a following step aimed at "the selection" means them.
        if let primary = outcome.landed.last {
            retarget(document, LayerSelection(primary: primary, others: Set(outcome.landed)))
        }
        return try jsonResult([
            "ok": true, "landed": outcome.landed, "depth": depth,
            "document": summary(document),
        ])
    }
}
