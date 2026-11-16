import AppKit

/// Layer ▸ Warp Text… (designs/warp-text.md, WT-5): the action, its validation and the commit,
/// which runs `edit_text_layer` with `warp` through the agent's dispatch (`runPathStep`), so it
/// is one undo step and records its step. The sheet is WarpTextSheetController.swift; the value
/// and its rect are TextWarp.swift.
extension EditorViewController {
    @objc func showWarpText(_ sender: Any?) {
        // A text session commits first: the block it holds is what gets warped.
        guard commitPendingSessions(), let document, validateWarpTextItem() else {
            NSSound.beep()
            return
        }
        do {
            presentAsSheet(try WarpTextSheetController(
                editor: self, document: document, layer: document.activeLayerIndex))
        } catch { document.presentError(error) }
    }

    /// Enabled with no Quick Mask and the active layer a text layer that is not on a path.
    func validateWarpTextItem() -> Bool {
        guard let document, !document.quickMaskActive,
              let payload = document.doc?.textPayload(document.activeLayerIndex)
        else { return false }
        return payload.onPath == nil
    }

    /// Sets layer `layer`'s warp (nil or all zero removes it) as one recorded `edit_text_layer`
    /// step, "warp" the four-key object or null.
    func commitWarpText(_ warp: TextWarp?, layer: Int) {
        guard let document, let doc = document.doc else {
            NSSound.beep()
            return
        }
        let value: Any = warp.flatMap { $0.isIdentity ? nil : $0 }.map {
            ["style": $0.style, "bend": $0.bend, "horizontal": $0.horizontal,
             "vertical": $0.vertical] as [String: Any]
        } ?? NSNull()
        runPathStep(
            "edit_text_layer", ["layer": ActionArgs.layer(doc.layerRef(layer)), "warp": value],
            undoName: "Warp Text", note: "Layer ▸ Warp Text…")
    }
}
