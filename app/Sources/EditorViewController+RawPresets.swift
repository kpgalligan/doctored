import AppKit

extension EditorViewController {
    @objc func showRawPresets(_ sender: Any?) {
        guard let document else { return }
        guard commitPendingSessions() else { return }
        do {
            let capture = try ImageSourceCapture(document)
            let layer = document.activeLayerIndex
            guard let info = try capture.source.retainedSourceInfo(layer) else {
                throw DoctoredCoreError(message: "Choose a retained RAW layer.")
            }
            let descriptor = try RawSourceDescriptor.read(info)
            guard let selected = RawPresetManagerSheet.run(descriptor: descriptor) else { return }
            try capture.requireCurrent(document)
            _ = try RawSettingsSynchronization.apply(settings: selected.settings,
                targets: [.init(document: document, layer: layer)], title: "Apply RAW Preset")
        } catch { document.presentError(error) }
    }

    @objc func synchronizeRawSettings(_ sender: Any?) {
        guard let document else { return }
        guard commitPendingSessions() else { return }
        do {
            let sourceCapture = try ImageSourceCapture(document)
            let source = RawSettingsTarget(document: document, layer: document.activeLayerIndex)
            guard let info = try sourceCapture.source.retainedSourceInfo(source.layer) else {
                throw DoctoredCoreError(message: "Choose a retained RAW source layer.")
            }
            let descriptor = try RawSourceDescriptor.read(info)
            let targets = RawSettingsSynchronization.availableTargets().filter {
                $0.document !== document || $0.layer != source.layer
            }
            guard !targets.isEmpty, targets.count <= 128 else {
                throw DoctoredCoreError(message: "Open 1–128 receiving RAW layers before synchronizing settings.")
            }
            var captures: [ObjectIdentifier: ImageSourceCapture] = [:]
            for target in targets { captures[ObjectIdentifier(target.document)] = try ImageSourceCapture(target.document) }
            guard let (fields, withLocals, selected) = RawSyncSheet.run(targets: targets,
                capabilities: descriptor.recipe.capabilities, recipe: descriptor.recipe) else { return }
            try sourceCapture.requireCurrent(document)
            for target in selected { try captures[ObjectIdentifier(target.document)]?.requireCurrent(target.document) }
            let settings = fields.isEmpty
                ? RawDevelopSettings() : try RawSettingsSynchronization.settings(from: source, fields: fields)
            let locals = withLocals
                ? try RawLocalSyncPayload.capture(source: sourceCapture.source, layer: source.layer) : nil
            let result = try RawSettingsSynchronization.apply(settings: settings, locals: locals, targets: selected,
                                                              title: "Synchronize RAW Settings", validateSource: {
                guard NSDocumentController.shared.documents.contains(where: { $0 === document }) else {
                    throw DoctoredCoreError(message: "The RAW settings source document closed. No settings were applied.")
                }
                try sourceCapture.requireCurrent(document)
            })
            if let note = result.note { reportSkippedLocals(note) }
        } catch { document.presentError(error) }
    }

    /// The generated locals Masking skipped (a model not installed, nothing found), in one
    /// informational alert.
    private func reportSkippedLocals(_ note: String) {
        let alert = NSAlert()
        alert.alertStyle = .informational
        alert.messageText = "Some Local Adjustments Were Not Synchronized"
        alert.informativeText = note
        alert.runModal()
    }

    func validateRawPresets(_ action: Selector) -> Bool? {
        guard [#selector(showRawPresets(_:)), #selector(synchronizeRawSettings(_:))].contains(action) else { return nil }
        return document?.doc?.hasRetainedRawSource(document?.activeLayerIndex ?? -1) == true
    }
}
