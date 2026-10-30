import AppKit

/// The status bar's "Recovery off" segment: shown while this
/// document's latest crash-recovery attempt failed, with the reason and
/// whether an earlier recovery copy is kept as its tooltip. The words come
/// from `RecoveryCoordinator.offNotice`, which the Recover Unsaved Work panel
/// lists too.
extension EditorViewController {
    var recoveryStatus: (text: String, detail: String)? {
        guard let document else { return nil }
        return RecoveryCoordinator.shared.offNotice(for: document)
    }

    @objc func recoveryDidChange(_ note: Notification) {
        updateStatus()
    }
}
