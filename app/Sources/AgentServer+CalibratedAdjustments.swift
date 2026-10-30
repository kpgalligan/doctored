import AppKit

extension AgentServer {
    /// Mirrors Image > Adjustments and the New/Options adjustment sheets.
    /// Resolve evaluation BEFORE dispatch and recording, so new recordings
    /// contain their exact semantics. An older Action with no evaluation key
    /// retains its original result. Re-editing an existing description also
    /// retains its evaluation unless the caller explicitly replaces it; an
    /// explicit `legacy_srgb` re-edit of a refused `document_v2` layer
    /// mirrors the Options sheet's Switch to Legacy sRGB Evaluation….
    func calibratedArguments(
        tool: String, arguments: [String: Any], document: ImageDocument?
    ) throws -> [String: Any] {
        guard ["apply_filter", "add_adjustment_layer", "edit_adjustment_layer"].contains(tool),
              let document = document, let doc = document.doc
        else { return arguments }
        let current: AdjustmentLayerPayload?
        if tool == "edit_adjustment_layer" {
            let index = try paintLayerIndex(arguments, document)
            current = doc.adjustmentPayload(index)
        } else {
            current = nil
        }
        let op = stringArg(arguments, tool == "apply_filter" ? "filter" : "op") ?? current?.op
        guard let op = op, CalibratedAdjustment.supports(op) else { return arguments }
        var params = try adjustmentParamsObject(arguments)
        let target: PaintTarget = tool == "apply_filter" ? try paintTarget(arguments, document) : .layer
        if params["evaluation"] == nil {
            if current?.op == op {
                params["evaluation"] = current?.string("evaluation", default: CalibratedAdjustment.legacy)
            } else {
                params["evaluation"] = ActionPlayer.isPlaying || target != .layer
                    ? CalibratedAdjustment.legacy : CalibratedAdjustment.document
            }
        }
        if params["evaluation"] as? String == CalibratedAdjustment.document {
            guard target == .layer else {
                throw ToolError(message: "document_v2 evaluation requires RGB layer pixels. "
                    + "Masks, individual planes and saved channels use legacy_srgb grayscale evaluation.")
            }
            if let refusal = doc.calibratedRefusal(op: op) { throw ToolError(message: refusal) }
        }
        var normalized = arguments
        normalized["params"] = params
        return normalized
    }
}
