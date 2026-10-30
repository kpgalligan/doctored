import AppKit

extension EditorViewController {
    func commitCanvasSelection(_ selection: CanvasSelection?, actionName: String,
                               record: [ActionStep]) {
        document?.setCanvasSelection(selection, actionName: actionName, record: record)
        // A gesture can preview a different shape then commit unchanged coverage.
        // Restore the document display even when the command produced no undo step.
        syncSelectionHistory()
    }

    @objc func selectionHistoryDidChange(_ note: Notification) {
        guard (note.object as? ImageDocument) === document else { return }
        precisionGeometryDocumentDidChange()
        magneticLassoDocumentDidChange()
        syncSelectionHistory()
        updateOptionsBar()
    }

    func syncSelectionHistory() {
        guard let document else { return }
        canvas.displaySelectionHistory(document.selectionHistory)
        updateStatus()
    }

    @objc func reselect(_ sender: Any?) {
        guard document?.reselect(record: .step("reselect", note: "Select ▸ Reselect")) == true else {
            NSSound.beep()
            return
        }
    }

    func commitQuickMaskStroke(_ stroke: QuickMaskStroke) {
        do {
            try document?.applyQuickMaskStroke(stroke, record: stroke.actionSteps)
        } catch {
            document?.presentError(error)
            NSSound.beep()
        }
    }
}
