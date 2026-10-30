import AppKit

extension EditorViewController {
    func presentPrecisionError(_ error: Error) {
        guard !(error is JobCancelled) else { return }
        let alert = NSAlert()
        alert.messageText = "16-bit operation could not finish"
        alert.informativeText = (error as? DoctoredCoreError)?.message ?? error.localizedDescription
        if let window = view.window { alert.beginSheetModal(for: window) }
        else { alert.runModal() }
    }
}
