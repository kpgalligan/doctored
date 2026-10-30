import AppKit

/// The status bar's "Over working limit" segment: shown while the
/// history entry an undo, redo or History navigation landed on is current
/// and that step's first projection was estimated over the working limit.
/// The sentence (`HistoryMemoryWarning.detail`) is its tooltip.
extension EditorViewController {
    var historyWarningStatus: (text: String, detail: String)? {
        guard let warning = document?.historyMemoryWarning else { return nil }
        return (warning.text, warning.detail)
    }

    @objc func historyDidChange(_ note: Notification) {
        updateStatus()
    }
}
