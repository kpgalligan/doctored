import AppKit

extension EditorViewController {
    /// Layer ▸ Merge Layers to HDR…: the selected raster layers of an
    /// 8- or 16-bit document, each one exposure of the same scene, merge into
    /// a NEW 32-bit float document (`MergeHDR`, MergeHDR.swift). This
    /// document is not edited: there is no `applyEdit`, no history step and no
    /// dirty change. The card shows each layer's estimated EV; a layer whose
    /// EV could not be estimated has to be given one.
    /// MCP twin `merge_layers_to_hdr` (AgentServer+MergeHDR.swift).
    @objc func mergeLayersToHDR(_ sender: Any?) {
        guard let document, document.doc != nil, commitPendingSessions(),
              let source = document.doc else {
            NSSound.beep()
            return
        }
        do {
            let prepared = try MergeHDR.prepare(
                layers: document.selectedLayerIndices, in: source)
            guard let options = MergeHDRSheetController.run(
                frames: prepared.frames, title: "Merge Layers to HDR", notice: prepared.notice)
            else { return }
            let outcome = try MergeHDR.merge(consume prepared, options: options)
            try ImageDocument.openMergedHDR(outcome, name: nil)
            MergeHDR.report(outcome)
        } catch is JobCancelled {
            SessionCommitRefusal.note(JobCancelled.message)
        } catch {
            document.presentError(error)
        }
    }

    /// Auto-Align's rule — two or more selected entries, each a raster layer
    /// — in a document that is not already 32-bit float. The finer rules (a
    /// text or shape layer) are `MergeHDR.prepare`'s and answer with a
    /// sentence naming the layer.
    var canMergeLayersToHDR: Bool {
        guard let doc = document?.doc else { return false }
        return canAutoAlignLayers && !doc.isFloat
    }
}
