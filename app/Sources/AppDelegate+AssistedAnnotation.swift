import AppKit

/// File ▸ Open Assisted Annotation Case… — the only UI entry point to the
/// internal Phase 9I experiment, and present only when it is enabled.
///
/// It lives on `AppDelegate` rather than the editor because no document is
/// involved: `EditorViewController.validateUserInterfaceItem` returns false at
/// its first line with nothing open, and an annotation session must be able to
/// start from an empty app.
extension AppDelegate {
    @objc func openAssistedAnnotationCase(_ sender: Any?) {
        guard AssistedAnnotation.isEnabled else {
            NSSound.beep()
            return
        }
        let panel = NSOpenPanel()
        panel.title = "Choose an Assisted Annotation Workspace"
        panel.prompt = "Open"
        panel.message = "Select the workspace directory that holds manifest.json and cases/."
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            try AssistedAnnotationWindowController.open(
                workspace: url, caseID: nil, synthetic: false)
        } catch {
            NSAlert(error: error).runModal()
        }
    }
}
