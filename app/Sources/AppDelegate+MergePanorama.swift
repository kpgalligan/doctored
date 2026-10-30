import AppKit

extension AppDelegate {
    /// File ▸ Automate ▸ Merge Panorama…: the dialog, then
    /// `MergePanorama.run` and the new document (MergePanorama.swift). It lives
    /// here, not on `EditorViewController`, for `loadFilesIntoStack`'s reason
    /// — it has to work at the Welcome window, before any document exists.
    /// The files are taken in the dialog list's order. MCP twin `merge_panorama`
    /// (AgentServer+MergePanorama.swift).
    @objc func mergePanorama(_ sender: Any?) {
        guard let chosen = MergePanoramaSheetController.run() else { return }
        let options = chosen.options
        do {
            let outcome = try MergePanorama.run(chosen.urls, options: options)
            let made = try ImageDocument.openMergePanorama(outcome, name: nil)
            ActionRecorder.shared.noteCreatedDocument(made)
            ActionRecorder.shared.record(.mergePanorama(
                paths: chosen.urls.map(\.path), layout: options.layout.projectionName,
                blend: options.blend, vignette: options.vignette,
                distortion: options.distortion, fill: options.fill))
            guard let notice = outcome.notice else { return }
            let alert = NSAlert()
            alert.alertStyle = .informational
            alert.messageText = MergePanorama.title
            alert.informativeText = notice
            alert.runModal()
        } catch is JobCancelled {
            return
        } catch let error as CocoaError where error.code == .userCancelled {
            return
        } catch {
            let alert = NSAlert()
            alert.messageText = "Could Not Create the Panorama"
            alert.informativeText = error.localizedDescription
            alert.runModal()
        }
    }
}
