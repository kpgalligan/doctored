import AppKit

/// Scalar fill and gradient use exactly the same captured source, placement
/// and selected result as their MCP twins. Mask edits preserve layer pixels
/// and descriptions; color-plane edits use the ordinary rasterization prompt.
extension ImageDocument {
    func applyPlaneScratchEdit(
        _ actionName: String, _ target: PaintTarget, _ op: PlaneScratchOp, mask: [UInt8]?,
        record: [ActionStep]
    ) {
        guard !quickMaskActive, let doc = doc, target.isCoverage,
              let capture = OperationCapture(
                doc: doc, layerIndex: activeLayerIndex, target: target, selection: mask)
        else { NSSound.beep(); return }
        applyCapturedRewrite(capture, actionName, record: record) { capture.applyingScratch(op) }
    }
}
