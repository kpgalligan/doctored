import AppKit

/// Window ▸ Styles: the editor's two responder-chain actions
/// the Styles panel sends (`StylesPanelController`). Mirrored for the agent
/// by `apply_style_preset` and `save_style_preset`
/// (AgentServer+StylePresets.swift).
extension EditorViewController {
    /// A swatch click: the preset applied to every selected layer that can
    /// hold a style (`StylePreset.ineligibility`; the rest are skipped, and a
    /// selection with none beeps), bottom first, as ONE undo step. Shift adds
    /// it to each layer's style (`StylePresetRequest.add`). The result is
    /// computed first, as Paste Layer Style does, so a preset that changes
    /// nothing leaves no step.
    @objc func applyStylePreset(_ sender: Any?) {
        guard let request = sender as? StylePresetRequest, let document = document,
              let doc = document.doc
        else {
            NSSound.beep()
            return
        }
        let eligible = document.selectedLayerIndices.filter {
            StylePreset.ineligibility(doc, layer: $0) == nil
        }
        guard !eligible.isEmpty, commitShapeEditSession() else {
            NSSound.beep()
            return
        }
        let updated: RasterDocument?
        do {
            updated = try request.preset.applied(to: doc, layers: eligible, add: request.add)
        } catch {
            document.presentError(error)
            return
        }
        guard let updated else { return }
        document.applyEdit(
            request.add ? "Add Style" : "Apply Style",
            record: .applyStylePreset(name: request.preset.name, group: request.preset.group,
                                      add: request.add)
        ) { $0 === doc ? updated : nil }
    }

    /// The Styles panel's New Style…: the dialog over the active
    /// layer (`NewStyleSheetController`).
    @objc func newStylePreset(_ sender: Any?) {
        guard let document = document, let doc = document.doc else {
            NSSound.beep()
            return
        }
        let layer = document.activeLayerIndex
        if let refusal = StylePreset.ineligibility(doc, layer: layer) {
            document.presentError(DoctoredCoreError(message: refusal))
            return
        }
        presentAsSheet(NewStyleSheetController(document: document, layer: layer))
    }
}
