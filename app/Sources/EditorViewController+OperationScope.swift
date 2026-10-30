import AppKit

extension EditorViewController {
    func supportsOperationScope(_ operation: OperationScope) -> Bool {
        guard let document = document, !document.quickMaskActive,
              let doc = document.doc else { return false }
        return OperationScope.refusalReason(
            operation, target: paintTarget, doc: doc,
            layerIndex: document.activeLayerIndex) == nil
    }

    /// The same specific refusal MCP reports; called before dialogs open and
    /// again by gesture paths that menu validation cannot protect.
    func refuseOperationScope(_ operation: OperationScope) -> Bool {
        guard let document = document, let doc = document.doc else {
            NSSound.beep()
            return true
        }
        let refusal = document.quickMaskActive ? OperationScope.quickMaskRefusal
            : OperationScope.refusalReason(
                operation, target: paintTarget, doc: doc, layerIndex: document.activeLayerIndex)
        guard let reason = refusal
        else { return false }
        NSSound.beep()
        let alert = NSAlert()
        alert.messageText = reason
        if let window = view.window { alert.beginSheetModal(for: window) }
        else { alert.runModal() }
        return true
    }

    /// Image ▸ Adjustments ▸ Invert with an 8-bit layer MASK targeted:
    /// the whole-canvas rule (`RasterDocument.invertingLayerMask`), not the
    /// stored-samples filter, so the area outside the mask's rectangle inverts
    /// too. false when the target is not a mask, so `applyInvert` carries on
    /// to the pixel filter; a 16/32-bit document never reaches here. The
    /// recorded step is the same `apply_filter` the pixel Invert records.
    func invertLayerMask() -> Bool {
        guard let document = document, paintTarget == .mask, !document.quickMaskActive
        else { return false }
        guard !refuseOperationScope(.filter) else { return true }
        guard let capture = document.captureOperation() else { NSSound.beep(); return true }
        document.applyCapturedRewrite(capture, "Invert", record: filterRecord("invert")) {
            capture.doc.invertingLayerMask(capture.layerIndex, selection: capture.selection)
        }
        return true
    }
}
