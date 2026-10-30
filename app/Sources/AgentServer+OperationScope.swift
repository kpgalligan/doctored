import AppKit

/// MCP mirrors of Filters, Image > Adjustments and the Fill/Gradient tools.
/// The immutable capture and target support policy are also used by the UI,
/// including its sheet previews. Automatic analysis reads the whole source;
/// every destructive result writes only through the current selection.
extension AgentServer {
    /// A gesture still owns its base snapshot. A foreign edit applied between
    /// ticks could otherwise be overwritten by its next tick or cancellation.
    func refuseLiveGestureMutation(tool: String, document: ImageDocument?) throws {
        guard document?.isLiveEditing == true else { return }
        let reads: Set<String> = [
            "get_image_source",
            "get_mask_properties", "inspect_subjects",
            "get_recovery_status",
            "list_history",
            "list_snapshots",
            "job_status", "cancel_job", "resource_usage",
            "list_documents", "get_document", "get_precision", "render", "sample_color", "sample_pixel",
            "histogram", "get_color_profile", "get_metadata", "list_channels", "list_guides",
            "list_paths", "get_vector_mask",
            "get_selection", "list_actions", "get_color_samplers",
            "detect_horizon", "fit_crop",
        ]
        guard !reads.contains(tool) else { return }
        throw ToolError(message: "Finish or cancel the active canvas gesture before running \(tool).")
    }

    /// `OperationScope.refusal`'s sentence, ending with the `set_layer_lock`
    /// hint naming the lock's owner when a layer lock refused. The UI
    /// keeps asking `refusalReason`, which never names an MCP tool.
    func scopeRefusal(
        _ operation: OperationScope, target: PaintTarget,
        doc: RasterDocument, layer: Int
    ) -> String? {
        guard let refusal = OperationScope.refusal(
            operation, target: target, doc: doc, layerIndex: layer)
        else { return nil }
        guard let owner = refusal.lockOwner else { return refusal.message }
        return refusal.message + Self.lockHint(owner: owner)
    }

    func operationCapture(
        _ document: ImageDocument, operation: OperationScope,
        target: PaintTarget, layer: Int
    ) throws -> OperationCapture {
        guard !document.quickMaskActive else {
            throw ToolError(message: OperationScope.quickMaskRefusal)
        }
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        if let reason = scopeRefusal(operation, target: target, doc: doc, layer: layer) {
            throw ToolError(message: reason)
        }
        guard let capture = OperationCapture(
            doc: doc, layerIndex: layer, target: target, selection: selectionMask(document))
        else { throw ToolError(message: "Could not read the requested edit target") }
        return capture
    }

    func applyScopedFilter(
        _ document: ImageDocument, target: PaintTarget, layer: Int, filter: String,
        _ op: (RasterImage) throws -> RasterImage?
    ) throws -> String {
        try applyScopedFilter(document, target: target, layer: layer, filter: filter,
                              capturing: { try op($0.source) })
    }

    /// The same, with the op handed the whole capture rather than its source:
    /// a framed filter reads the capture's selection bounds and target.
    func applyScopedFilter(
        _ document: ImageDocument, target: PaintTarget, layer: Int, filter: String,
        capturing op: (OperationCapture) throws -> RasterImage?
    ) throws -> String {
        let capture = try operationCapture(document, operation: .filter, target: target, layer: layer)
        let state = document.captureEditingState()
        // A throwing operator keeps its own refusal wording — a parameter out
        // of range or an over-budget kernel says so instead of becoming the
        // generic "check the parameters".
        guard let result = try op(capture) else {
            throw ToolError(message: "Apply \(filter) failed — check the parameters")
        }
        // The op may run as a modal job that services other calls, so
        // a selection (or other editing-state) change during it refuses here
        // rather than writing through the captured selection.
        guard document.isCurrentEditingState(state) else {
            throw ToolError(message: "The document changed before the edit could be applied")
        }
        return try commitScopedResult(
            document, capture: capture, updated: capture.writing(result, selection: capture.selection),
            actionName: "Apply \(filter)", extra: ["filter": filter])
    }

    /// `apply_filter filter:"invert"` on an 8-bit layer mask: mirrors
    /// Image ▸ Adjustments ▸ Invert with the mask targeted
    /// (`EditorViewController.invertLayerMask`), which inverts the mask over
    /// the whole canvas. nil for any other filter or target.
    func invertMaskFilter(
        _ document: ImageDocument, filter: String, layer: Int, target: PaintTarget
    ) throws -> String? {
        guard filter == "invert", target == .mask else { return nil }
        let capture = try operationCapture(document, operation: .filter, target: target, layer: layer)
        return try commitScopedResult(
            document, capture: capture,
            updated: capture.doc.invertingLayerMask(layer, selection: capture.selection),
            actionName: "Apply invert", extra: ["filter": "invert"])
    }

    /// A refused write after a valid source/result means no covered byte
    /// changed. Report it without dropping a description or adding history.
    func commitScopedResult(
        _ document: ImageDocument, capture: OperationCapture, updated: RasterDocument?,
        actionName: String, extra: [String: Any] = [:]
    ) throws -> String {
        let fields = extra.merging([
            "ok": true, "layer": capture.layerIndex,
            "target": capture.target.agentName(in: capture.doc),
            "changed": updated != nil,
        ]) { _, shared in shared }
        guard let updated = updated else {
            return try jsonResult(fields.merging([
                "note": "No covered pixels changed. No undo step was added.",
            ]) { _, shared in shared })
        }
        guard document.doc === capture.doc else {
            throw ToolError(message: "The document changed before the edit could be applied")
        }
        let dropped = try performPixelEdit(
            document, actionName,
            pixelLayer: pixelLayer(for: capture.target, layer: capture.layerIndex)
        ) { _ in updated }
        return try pixelEditResult(fields, layer: capture.layerIndex, rasterized: dropped)
    }
}
