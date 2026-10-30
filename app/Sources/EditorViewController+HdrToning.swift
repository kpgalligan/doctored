import AppKit

// Image ▸ Adjustments ▸ HDR Toning…, and Image ▸ Mode ▸ 8/16 Bits/Channel on a
// 32-bit RGB document, which opens the same dialog at that depth
// (`EditorViewController+CmykProfile.swift`'s `convertToBits`). The flatten prompt comes before
// the dialog; the job and its one undo step are `HdrToningRun` (HdrToning.swift), the dialog
// `HdrToningSheetController`, the MCP twin `hdr_toning`.

extension EditorViewController {
    @objc func showHdrToning(_ sender: Any?) {
        presentHdrToning(bitDepth: nil)
    }

    /// Opens the dialog, `bitDepth` preselected for a 32-bit source (nil: 16). A CMYK document is
    /// refused by name; a document HDR Toning would flatten asks first, and No opens nothing.
    func presentHdrToning(bitDepth: Int?) {
        guard let document, let doc = document.doc else { NSSound.beep(); return }
        if doc.isCmyk {
            document.presentError(DoctoredCoreError(message: CmykDocument.refusal(tool: HdrToning.title)))
            return
        }
        if doc.isLab {
            document.presentError(DoctoredCoreError(message: LabAdjust.refusal(op: "hdr_toning")))
            return
        }
        if HdrToning.needsFlatten(doc), !HdrToning.confirmFlatten() { return }
        var settings = HdrToningSettings.defaults(for: doc)
        if let bitDepth, doc.isFloat { settings.bitDepth = bitDepth }
        presentAsSheet(HdrToningSheetController(editor: self, document: document, source: doc,
                                                settings: settings))
    }

    /// The dialog's OK: the job on the document's current handle, one undo step recorded as
    /// `hdr_toning`.
    func commitHdrToning(_ settings: HdrToningSettings, document: ImageDocument) {
        do {
            let run = try HdrToningRun(document, settings: settings)
            let prepared = try run.compute()
            try run.publish(prepared, to: document, record: [run.step()])
        } catch is JobCancelled {
            return
        } catch {
            document.presentError(error)
        }
    }

    /// Image ▸ Adjustments ▸ HDR Toning… at every depth (a CMYK document is refused by name on
    /// the click); a Lab document, a live edit and a running job disable it (the click's Lab
    /// refusal stays the backstop). nil for every other item.
    func validateHdrToningItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        guard item.action == #selector(showHdrToning(_:)) else { return nil }
        guard let document, let doc = document.doc, !doc.isLab else { return false }
        return !document.isLiveEditing && !BackgroundWork.occupied
    }
}
