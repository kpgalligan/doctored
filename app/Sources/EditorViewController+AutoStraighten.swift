import AppKit

/// The Crop tool's Auto button: Vision's horizon sets the
/// Straighten angle, and the box re-fits inside the rotated image like any
/// other angle change. No undo step of its own — the commit
/// records the resolved `crop` with the angle, as for a typed angle — and
/// no recordable "auto" Action step. MCP twin: `detect_horizon`.
extension EditorViewController {
    func autoStraightenCrop() {
        guard cropSession != nil, let document, let doc = document.doc else {
            NSSound.beep()
            return
        }
        do {
            let angle = try HorizonDetection.detect(document, title: "Auto Straighten")
            // The wait runs the main loop: the box may have gone, or the
            // document changed, meanwhile.
            guard self.document?.doc === doc, cropSession != nil else { return }
            guard let angle else {
                presentNoHorizon()
                return
            }
            // Absolute, not added: the detection reads the unrotated image.
            cropStraightenDegrees = angle
        } catch is JobCancelled {
            return
        } catch {
            document.presentError(error)
        }
    }

    /// `presentSubjectFailure`'s shape: the user asked for something and
    /// nothing visible happened, so the reason is worth a sentence.
    private func presentNoHorizon() {
        let alert = NSAlert()
        alert.messageText = "No horizon was found."
        alert.informativeText =
            "Auto straighten looks for a clear horizon tilted by about 1° to 10°. "
            + "The image may already be level — set the angle in the Straighten field instead."
        if let window = view.window {
            alert.beginSheetModal(for: window)
        } else {
            alert.runModal()
        }
    }
}
