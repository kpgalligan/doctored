import AppKit

extension EditorViewController {
    @objc func refineSelection(_ sender: Any?) {
        guard let document else { NSSound.beep(); return }
        do { presentAsSheet(try RefineSelectionSheetController(document: document)) }
        catch { NSApp.presentError(error) }
    }
}
