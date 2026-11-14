import AppKit

/// Select ▸ Focus Area…: the sheet, its validation and the commit, over `FocusArea.swift` (the
/// `select_focus_area` MCP tool shares `SelectionCapture.focusAreaShape`).
extension EditorViewController {
    @objc func showFocusArea(_ sender: Any?) {
        guard document != nil, !isTransforming else {
            NSSound.beep()
            return
        }
        presentAsSheet(FocusAreaSheetController { [weak self] parameters in
            self?.commitFocusArea(parameters)
        })
    }

    /// Enabled where Color Range is, on an RGB document.
    func validateFocusAreaItem() -> Bool {
        guard let doc = document?.doc, !isTransforming, document?.quickMaskActive == false else { return false }
        return FocusArea.refusal(doc) == nil
    }

    /// One selection step in the menu's Replace mode; the recorded step carries the range used.
    func commitFocusArea(_ p: FocusAreaParameters) {
        guard let document, let doc = document.doc else {
            NSSound.beep()
            return
        }
        do {
            if let reason = FocusArea.refusal(doc) { throw DoctoredCoreError(message: reason) }
            let capture = try SelectionCapture(document: document)
            let (shape, range) = try capture.focusAreaShape(p, in: document)
            try capture.commit(shape, in: document, name: FocusArea.actionName, mode: .replace,
                               record: FocusArea.record(range: range, softenEdge: p.softenEdge, mode: .replace))
        } catch is JobCancelled {
            return
        } catch {
            NSApp.presentError(error)
        }
        syncSelectionHistory()
    }
}
