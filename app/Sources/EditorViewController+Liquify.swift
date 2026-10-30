import AppKit

/// Filters ▸ Liquify…: the menu action, and the one commit the
/// workspace's Apply runs.
extension EditorViewController {
    /// Opens the Liquify workspace on the active raster layer, at 8, 16 and
    /// 32 bits; a target that is not a raster layer's
    /// pixels gets the shared scope refusal.
    @objc func showLiquify(_ sender: Any?) {
        guard let document else { NSSound.beep(); return }
        guard !refuseOperationScope(.pixelOnly) else { return }
        do {
            presentAsSheet(try LiquifySheetController(editor: self))
        } catch { document.presentError(error) }
    }

    /// Runs `strokes` (canvas pixels) on the captured layer at full size as
    /// one undo step named "Liquify", as a cancellable job: at 8 bits the byte
    /// filter `liquify` through `applyCapturedByteFilter`, at 16 and 32 bits
    /// the selected edit of the same name, as
    /// `PrecisionAdjustmentSession.commit(filter:)` runs a filter (a float
    /// layer's resample is clamped to its visible range, not to 0…1). Both
    /// subtract the layer's canvas offset from the points
    /// and write through the captured selection. True when the document took
    /// the edit; false for strokes that change no pixel, a cancel, a refusal
    /// (presented) or a declined rasterize prompt.
    func commitLiquify(_ strokes: [LiquifyStroke], byte: OperationCapture?,
                       precision: PrecisionEditorCapture?, record: [ActionStep]) -> Bool {
        guard let document else { NSSound.beep(); return false }
        if let precision {
            do {
                try precision.requireCurrent(document)
                guard document.confirmRasterize(layer: precision.layer) else { return false }
                try precision.requireCurrent(document)
                BackgroundWork.waitForAnonymousOccupant()
                guard let placement = precision.facade.layerPlacement(precision.layer) else {
                    throw DoctoredCoreError(message: "The target layer no longer exists.")
                }
                let request = try SelectedAdjustment.json([
                    "layer": precision.layer, "op": "liquify",
                    "params": LiquifyStroke.kernelParameters(strokes, origin: (placement.x, placement.y)),
                ])
                let edit = try SelectedAdjustment.edit(precision, request: request, target: .layer)
                let result = try edit.run(title: "Liquify")
                return try precision.publish(result, to: document, name: "Liquify", record: record)
            } catch {
                document.presentError(error)
                return false
            }
        }
        guard let byte, let placement = byte.placement else { NSSound.beep(); return false }
        document.applyCapturedByteFilter(byte, "Liquify", record: record, request: ByteFilterRequest(
            op: "liquify",
            params: LiquifyStroke.kernelParameters(strokes, origin: (placement.x, placement.y))))
        return document.doc !== byte.doc
    }
}
