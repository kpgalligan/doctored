import AppKit

/// Explicit AppKit autosave requests share the native recovery policy. AppKit's
/// default autosavingFileType is the imported type and its save machinery also
/// touches saved markers, so recovery deliberately bypasses those defaults.
extension ImageDocument {
    override func scheduleAutosaving() {
        RecoveryCoordinator.shared.documentDidChange(self)
    }

    override var autosavingFileType: String? { Self.nativeTypeIdentifier }

    override func autosave(withImplicitCancellability autosavingIsImplicitlyCancellable: Bool,
                           completionHandler: @escaping (Error?) -> Void) {
        do {
            try RecoveryCoordinator.shared.checkpoint(self, wait: false, completion: completionHandler)
        } catch { completionHandler(error) }
    }
}
