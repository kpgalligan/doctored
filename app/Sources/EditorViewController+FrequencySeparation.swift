import AppKit

/// Filters ▸ Other ▸ Frequency Separation…: the sheet, the menu validation and the commit. The
/// split is `FrequencySeparation`'s, shared with the `frequency_separation` MCP tool.
extension EditorViewController {
    @objc func showFrequencySeparation(_ sender: Any?) {
        guard let document, let doc = document.doc else {
            NSSound.beep()
            return
        }
        let layer = document.activeLayerIndex
        if let reason = FrequencySeparation.refusal(doc, layer: layer, quickMask: document.quickMaskActive) {
            document.presentError(DoctoredCoreError(message: reason))
            return
        }
        let form = FrequencySeparation.form(of: doc)
        presentAsSheet(FrequencySeparationSheetController(form: form) { [weak self] radius in
            self?.commitFrequencySeparation(FrequencySeparationParameters(layer: layer, radius: radius))
        })
    }

    /// Enabled on a layer target whose active layer `FrequencySeparation.refusal` accepts (an RGB,
    /// Grayscale, Duotone or Lab Color document; the mode and float whitelists admit the item).
    func validateFrequencySeparationItem() -> Bool {
        guard let document, let doc = document.doc, paintTarget == .layer else { return false }
        return FrequencySeparation.refusal(doc, layer: document.activeLayerIndex,
                                           quickMask: canvas.quickMaskActive) == nil
    }

    /// One undo step at every depth: the 8-bit RGB low layer is blurred under admission first and
    /// the split published through `applyEdit`; a typed document's split (16 and 32 bits, and every
    /// colour mode) runs as one captured job.
    func commitFrequencySeparation(_ p: FrequencySeparationParameters) {
        guard let document, let doc = document.doc else {
            NSSound.beep()
            return
        }
        do {
            if let reason = FrequencySeparation.refusal(doc, layer: p.layer, quickMask: document.quickMaskActive) {
                throw DoctoredCoreError(message: reason)
            }
            let record: [ActionStep] = .frequencySeparation(p, in: doc)
            if doc.isHighDepth {
                let capture = try PrecisionEditorCapture(document)
                let result = try FrequencySeparation.nativeSplit(capture, p)
                try capture.publish(result, to: document, name: FrequencySeparation.actionName, record: record)
                return
            }
            let revision = document.revision
            let low = try FrequencySeparation.blurredLow(doc, p, waitForSlot: true)
            guard document.doc === doc, document.revision == revision else {
                throw DoctoredCoreError(message: "The document changed while the low layer was blurred. No result was applied.")
            }
            let result = try FrequencySeparation.split(doc, p, low: low)
            document.applyEdit(FrequencySeparation.actionName, record: record) { _ in result }
        } catch is JobCancelled {
            return
        } catch {
            document.presentError(error)
        }
    }
}
