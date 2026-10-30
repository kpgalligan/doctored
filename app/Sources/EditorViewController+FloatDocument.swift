import AppKit

/// Image ▸ Tone Map… — the 32-bit float document's SDR map
/// (`FloatDocument.swift`).
///
/// The sheet chooses; this file commits, in the shape
/// `EditorViewController+Evaluation` established: the change re-runs on the
/// document's CURRENT handle inside the transform rather than on the handle
/// the sheet captured, so an edit that slipped in while the dialog was open
/// survives, and the capture is only asked whether the document is still the
/// one the sheet opened on.
///
/// A refusal — an operator or a field the core does not admit — is a
/// SENTENCE, carried out of the non-throwing transform and presented instead
/// of beeping.
extension EditorViewController {
    @objc func showToneMap(_ sender: Any?) {
        guard let sheet = ToneMapSheetController(editor: self) else {
            NSSound.beep()
            return
        }
        presentAsSheet(sheet)
    }

    /// Commits the map as ONE undo step.
    func commitToneMap(_ map: ToneMap, capture: PrecisionEditorCapture) {
        guard let document = document else {
            NSSound.beep()
            return
        }
        do {
            try capture.requireCurrent(document)
        } catch {
            document.presentError(error)
            return
        }
        var refusal: Error?
        document.applyEdit("Tone Map", record: .setToneMap(map)) { source in
            do {
                return try source.withToneMap(map)
            } catch {
                refusal = error
                return nil
            }
        }
        if let refusal {
            document.presentError(refusal)
        }
    }

    /// Enablement for the menu item: only a 32-bit float document has a map.
    func validateToneMapItem() -> Bool { document?.doc?.isFloat == true }
}
