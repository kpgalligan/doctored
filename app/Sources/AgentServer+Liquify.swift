import AppKit

/// The agent mirror of Filters ▸ Liquify…: `liquify` runs a stroke list on one layer at
/// full size as one undo step, the commit the workspace's Apply runs
/// (`EditorViewController.commitLiquify`). The workspace's proxy preview has no mirror: a call
/// carries its whole stroke list. Replayed Actions reach the filter through this tool.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let liquifyHandlers: [String: ToolHandler] = [
        "liquify": { $0.liquify },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in
    /// AgentServer+Registry.swift. `.floatTool`: a 32-bit float layer takes
    /// the same strokes in linear light. No `.repeatable`: Repeat Last Filter does not take
    /// Liquify.
    static let liquifyToolFacts: [String: ToolFacts] = [
        "liquify": [.selectionSensitive, .typedTool, .rasterTool, .floatTool, .cmykTool],
    ]

    /// liquify — mirrors Filters ▸ Liquify… and its Apply: `strokes` (canvas
    /// pixels, amounts in percent) on `layer`'s own raster, written through
    /// the selection. At 8 bits the byte filter `liquify`, admitted and run as
    /// a cancellable job like `apply_filter`'s filters; at 16 and 32 bits the
    /// selected edit of the same name. Both subtract the layer's canvas offset from the
    /// points. Strokes that move nothing answer `changed: false` and add no
    /// undo step.
    func liquify(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseNullOrBooleanNumbers(a, ["layer"], in: "liquify")
        let allowed: Set<String> = ["strokes", "layer", "document_id"]
        if let unexpected = a.keys.sorted().first(where: { !allowed.contains($0) }) {
            throw ToolError(message: "Unknown liquify parameter: \(unexpected).")
        }
        let strokes: [LiquifyStroke]
        do { strokes = try LiquifyStroke.parse(a["strokes"]) }
        catch { throw ToolError(message: error.localizedDescription) }
        let index = try paintLayerIndex(a, document)
        if document.doc?.isHighDepth == true {
            return try precisionLiquify(document, strokes, layer: index)
        }
        let capture = try operationCapture(document, operation: .filter, target: .layer, layer: index)
        guard let placement = capture.placement else {
            throw ToolError(message: "Could not read the requested edit target")
        }
        do { try LiquifyStroke.validatePoints(strokes, origin: (placement.x, placement.y)) }
        catch { throw ToolError(message: error.localizedDescription) }
        let state = document.captureEditingState()
        let request = ByteFilterRequest(
            op: "liquify",
            params: LiquifyStroke.kernelParameters(strokes, origin: (placement.x, placement.y)))
        let result = try byteFiltered(capture.source, request, in: document)
        // The job services other calls while it runs, so a selection
        // (or other editing-state) change during it refuses here rather than
        // writing through the captured selection.
        guard document.isCurrentEditingState(state) else {
            throw ToolError(message: "The document changed before the edit could be applied")
        }
        return try commitScopedResult(
            document, capture: capture, updated: capture.writing(result, selection: capture.selection),
            actionName: "Liquify", extra: ["strokes": strokes.count])
    }

    /// The 16- and 32-bit twin: `selectedPrecisionFilter`'s capture, scope
    /// check and one central publication, with the typed selected request
    /// `liquify`; `storage` names the depth it ran at.
    private func precisionLiquify(_ document: ImageDocument, _ strokes: [LiquifyStroke],
                                  layer index: Int) throws -> String {
        let capture = try PrecisionEditorCapture(document)
        if let reason = scopeRefusal(.filter, target: .layer, doc: capture.facade, layer: index) {
            throw ToolError(message: reason)
        }
        guard let placement = capture.facade.layerPlacement(index) else {
            throw ToolError(message: "The target layer no longer exists.")
        }
        do { try LiquifyStroke.validatePoints(strokes, origin: (placement.x, placement.y)) }
        catch { throw ToolError(message: error.localizedDescription) }
        let request = try SelectedAdjustment.json([
            "layer": index, "op": "liquify",
            "params": LiquifyStroke.kernelParameters(strokes, origin: (placement.x, placement.y)),
        ])
        let edit = try SelectedAdjustment.edit(capture, request: request, target: .layer)
        let result = try edit.run(title: "Liquify")
        let changed = try capture.publish(result, to: document, name: "Liquify")
        return try jsonResult([
            "ok": true, "changed": changed, "layer": index, "target": "layer",
            "strokes": strokes.count,
            "storage": capture.facade.isFloat ? "rgba32_float"
                : capture.facade.isCmyk ? PrecisionDocumentDescriptor.cmykStorage : "rgba16_unorm",
            "write_scope": capture.selection == nil ? "whole_target" : "selection",
        ])
    }
}
