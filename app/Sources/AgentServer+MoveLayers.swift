import AppKit

/// `move_layers` — the RELATIVE twin of the Move tool's drag and of the
/// arrow-key nudge (MultiLayerEdit.moveDidUpdate / moveNudge), and the one
/// thing `set_layer_properties` cannot say: move a SET of layers by a delta.
///
/// It exists because a recorded multi-layer Move had no twin at all and
/// recorded a visible `__unrecorded` placeholder (Phase 12A). The single-layer
/// Move keeps its absolute `set_layer_properties` form unchanged; the
/// absolute/relative unification of that one is 12B's.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let moveLayersHandlers: [String: ToolHandler] = [
        "move_layers": { $0.moveLayers },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let moveLayersToolFacts: [String: ToolFacts] = [
        // Phase 12A: the relative multi-layer Move is `set_layer_properties`'
        // offset op through `RasterDocument.moveLayers`, which routes to the
        // typed transform at depth, so it answers at either depth.
        // Phase 12A: the relative multi-layer Move is the offset op
        // `set_layer_properties` already exposes, run through the typed
        // transform (`RasterDocument.moveLayers`).
        "move_layers": [.commonTool, .floatTool, .cmykTool],
    ]

    /// One `moveLayers` call, exactly as a drag tick and a nudge make it, so
    /// linked entries and group subtrees follow and a position lock refuses
    /// the whole set. One undo step.
    func moveLayers(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let indices = try layerIndices(a, document)
        guard let dx = intArg(a, "dx"), let dy = intArg(a, "dy") else {
            throw ToolError(
                message: "move_layers requires dx and dy — whole canvas pixels to move by "
                    + "(x right, y down). Use set_layer_properties' offset_x/offset_y to "
                    + "place ONE layer at an absolute position instead.")
        }
        guard abs(dx) <= 1_000_000, abs(dy) <= 1_000_000 else {
            throw ToolError(message: "dx and dy must be within ±1,000,000 canvas pixels.")
        }
        if boolArg(a, "duplicate") == true {
            return try moveLayersDuplicate(document, indices, dx: dx, dy: dy)
        }
        // Before the edit, so a position lock is reported as the lock it is
        // rather than as the "nothing would move" no-op below — alignLayers'
        // rule, over the same EXPANDED set the gesture moves.
        try rejectLockedMove(document, indices)
        // A zero delta is not an error: a set drag that ended where it began
        // still records move_layers (0, 0), and replaying it must answer the
        // no-op rather than fail the whole action.
        guard dx != 0 || dy != 0 else {
            return try structureNoOp(
                ["layers": indices, "dx": dx, "dy": dy],
                why: "dx and dy are both zero, so nothing would move.")
        }
        if document.doc?.isHighDepth == true {
            do {
                let gesture = try PrecisionGeometryGesture(document)
                defer { gesture.cancel() }
                let changed = try gesture.finish(.move(layers: indices, dx: dx, dy: dy),
                                                 name: "Move Layer", record: .notACommand)
                guard changed else {
                    return try structureNoOp(["layers": indices, "dx": dx, "dy": dy],
                        why: "the native move changed no layer.")
                }
                return try jsonResult([
                    "ok": true, "layers": indices, "dx": dx, "dy": dy,
                    "moved": movedOffsets(document, indices),
                ])
            } catch { throw ToolError(message: error.localizedDescription) }
        }
        guard try editStructure(document, "Move Layer", {
            try $0.moveLayers(indices, dx: dx, dy: dy)
        }) else {
            return try structureNoOp(
                ["layers": indices, "dx": dx, "dy": dy],
                why: "the move was refused — none of those entries has pixels to move, or "
                    + "the document could not apply the offset.")
        }
        return try jsonResult([
            "ok": true, "layers": indices, "dx": dx, "dy": dy,
            "moved": movedOffsets(document, indices),
        ])
    }

    /// `duplicate` — the Move tool's ⌥-drag and ⌥+arrow with no
    /// selection (EditorViewController+MoveDuplicate.swift): duplicate
    /// `indices`, move the COPIES, one "Duplicate Layer" step, the copies
    /// selected. A zero delta still duplicates, in place. Duplicating is never
    /// lock-refused; moving the copies answers to the originals' SUBTREE
    /// Position block (`rejectLockedOffset`), never a link partner's, because
    /// the copies are unlinked (`MoveDuplicateLayers.swift`).
    private func moveLayersDuplicate(_ document: ImageDocument, _ indices: [Int],
                                     dx: Int, dy: Int) throws -> String {
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        if dx != 0 || dy != 0 {
            for index in indices { try rejectLockedOffset(document, index) }
        }
        let copies = MoveDuplicateLayers.copies(of: indices, in: doc)
        let name = MoveDuplicateLayers.actionName
        let fields: [String: Any] = ["layers": indices, "dx": dx, "dy": dy, "duplicate": true]
        let why = "the copy could not be made."
        if doc.isHighDepth {
            let changed: Bool
            do {
                let gesture = try PrecisionGeometryGesture(document)
                defer { gesture.cancel() }
                changed = try gesture.finish(.duplicateMove(layers: indices, dx: dx, dy: dy),
                                             name: name, record: .notACommand)
            } catch { throw ToolError(message: error.localizedDescription) }
            guard changed else { return try structureNoOp(fields, why: why) }
        } else {
            guard try editStructure(document, name, {
                try $0.duplicatingAndMoving(indices, dx: dx, dy: dy)?.document
            }) else { return try structureNoOp(fields, why: why) }
        }
        retarget(document, MoveDuplicateLayers.selection(
            copies: copies, primary: indices.last ?? 0, originals: indices, in: doc))
        return try jsonResult(fields.merging([
            "ok": true, "copies": copies, "moved": movedOffsets(document, copies),
        ]) { _, new in new })
    }

    /// Where the named entries ended up, the shape `align_layers` reports.
    private func movedOffsets(_ document: ImageDocument, _ indices: [Int]) -> [[String: Any]] {
        guard let doc = document.doc else { return [] }
        return indices.compactMap { index in
            guard let info = doc.layerInfo(index) else { return nil }
            return ["index": index, "offset_x": info.offsetX, "offset_y": info.offsetY]
        }
    }
}
