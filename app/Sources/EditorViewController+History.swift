import AppKit

extension EditorViewController {
    @objc func showHistory(_ sender: Any?) {
        guard let document else { NSSound.beep(); return }
        document.showHistoryPanel()
    }
}
