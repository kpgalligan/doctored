import AppKit

// At 16 bits and float both handlers (`align_layers`, `distribute_layers`) commit through one
// `PrecisionGeometryGesture` with the `.align` / `.distribute` request (`PrecisionGeometryWork` →
// the core's typed `align_set` / `distribute_set`, one typed edit), as `move_layers`' typed branch
// does; the byte path, its no-op why-texts and `movedFields` are shared.

/// `align_layers` / `distribute_layers` — mirror Layer ▸ Align / Distribute
/// and the Move tool's options-bar segments (`alignSelection` /
/// `distributeSelection`, EditorViewController+Arrange.swift). Moved out of
/// AgentServer+Groups.swift. Both answer at 16 bits and
/// 32-bit float through the core's typed `align_set` / `distribute_set`, in
/// ONE typed edit, exactly as `move_layers` routes its typed branch
/// (AgentServer+MoveLayers.swift).
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let alignDistributeHandlers: [String: ToolHandler] = [
        "align_layers": { $0.alignLayers },
        "distribute_layers": { $0.distributeLayers },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let alignDistributeToolFacts: [String: ToolFacts] = [
        // Align and Distribute answer at depth through the core's
        // typed `align_set` / `distribute_set`.
        // Align and Distribute are integer translations through
        // the core's typed `align_set` / `distribute_set`.
        "align_layers": [.commonTool, .floatTool, .cmykTool],
        "distribute_layers": [.commonTool, .floatTool, .cmykTool],
    ]

    // MARK: - Align and distribute

    /// Mirrors Layer ▸ Align. Acts on CONTENT bounds (`content_*` in
    /// get_document), never the pixel-buffer rect.
    func alignLayers(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let indices = try layerIndices(a, document)
        let edgeName = try requiredString(a, "edge")
        guard let edge = Self.alignEdges[edgeName] else {
            throw ToolError(
                message: "edge must be one of "
                    + Self.alignEdges.keys.sorted().joined(separator: ", ")
                    + " (got \"\(edgeName)\")")
        }
        let toCanvas = (stringArg(a, "to") ?? "layers") == "canvas"
        // Before the edit, so a position lock is reported as the lock it is
        // rather than as the "nothing would move" no-op below.
        // Align and Distribute never re-render, so every fill keeps its pin.
        try rejectPinnedFill(document, indices, refitting: false)
        try rejectLockedMove(document, indices)
        let changed: Bool
        if document.doc?.isHighDepth == true {
            changed = try typedAlignment(
                document, .align(layers: indices, edge: edgeName, toCanvas: toCanvas),
                name: "Align Layers")
        } else {
            changed = try editStructure(document, "Align Layers", {
                $0.alignLayers(indices, edge: edge, toCanvas: toCanvas)
            })
        }
        guard changed else {
            return try structureNoOp(
                ["layers": indices],
                why: "nothing would move — the layers are already aligned, or fewer than "
                    + "two INDEPENDENT boxes were named. A group and one of its own children "
                    + "are ONE box (the child is inside the subtree the group already "
                    + "moves), and an entry with no opaque pixels is no box at all; aligning "
                    + "a single box to the others is aligning it to itself. Pass "
                    + "to: \"canvas\" to align one box to the canvas instead.")
        }
        return try jsonResult(["ok": true, "moved": movedFields(document, indices)])
    }

    /// Mirrors Layer ▸ Distribute. Equalizes the GAPS between adjacent
    /// content boxes ("distribute spacing"), so it needs three.
    func distributeLayers(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let indices = try layerIndices(a, document, minimum: 3)
        let axis = try requiredString(a, "axis")
        guard axis == "horizontal" || axis == "vertical" else {
            throw ToolError(message: "axis must be horizontal or vertical (got \"\(axis)\")")
        }
        // As in alignLayers: the lock, not the no-op.
        // Align and Distribute never re-render, so every fill keeps its pin.
        try rejectPinnedFill(document, indices, refitting: false)
        try rejectLockedMove(document, indices)
        let changed: Bool
        if document.doc?.isHighDepth == true {
            changed = try typedAlignment(
                document, .distribute(layers: indices, vertical: axis == "vertical"),
                name: "Distribute Layers")
        } else {
            changed = try editStructure(document, "Distribute Layers", {
                $0.distributeLayers(indices, vertical: axis == "vertical")
            })
        }
        guard changed else {
            return try structureNoOp(
                ["layers": indices],
                why: "nothing would move — the spacing is already even, or fewer than three "
                    + "of those layers have any opaque pixels.")
        }
        return try jsonResult(["ok": true, "moved": movedFields(document, indices)])
    }

    /// The 16-bit / float commit: one typed gesture, as `move_layers`' typed
    /// branch makes it. True when published; false when nothing moved.
    private func typedAlignment(_ document: ImageDocument, _ request: PrecisionGeometryRequest,
                                name: String) throws -> Bool {
        do {
            let gesture = try PrecisionGeometryGesture(document)
            defer { gesture.cancel() }
            return try gesture.finish(request, name: name, record: .notACommand)
        } catch { throw ToolError(message: error.localizedDescription) }
    }

    private static let alignEdges: [String: DrAlign] = [
        "left": DR_ALIGN_LEFT, "horizontal_center": DR_ALIGN_CENTER_X,
        "right": DR_ALIGN_RIGHT, "top": DR_ALIGN_TOP,
        "vertical_center": DR_ALIGN_CENTER_Y, "bottom": DR_ALIGN_BOTTOM,
    ]

    /// Where the moved entries ended up — their offsets after the edit, so a
    /// caller can check the arithmetic without a second round trip.
    private func movedFields(_ document: ImageDocument, _ indices: [Int]) -> [[String: Any]] {
        guard let doc = document.doc else { return [] }
        return indices.compactMap { index in
            guard let info = doc.layerInfo(index) else { return nil }
            return ["index": index, "offset_x": info.offsetX, "offset_y": info.offsetY]
        }
    }
}
