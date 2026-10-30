import AppKit

/// Select > Select Subject: Vision finds the foreground subjects in the
/// flattened composite and their coverage becomes the selection. The
/// segmentation itself is SubjectSelection.swift; everything below is the
/// command around it.
extension EditorViewController {
    /// Selects every foreground subject at once, which is what the command's
    /// name promises and what the equivalent in other editors does. Vision
    /// separates the subjects individually and the count reaches the agent
    /// (`select_subject`, which can ask for one of them), so the instance
    /// information is not thrown away here — it simply has no click target
    /// in a menu command.
    ///
    /// Replaces the selection rather than combining with it: a menu item
    /// carries no Shift/Option modifier the way the marquee gestures do, and
    /// Select All next to it behaves the same way.
    @objc func selectSubject(_ sender: Any?) {
        guard let document, let doc = document.doc else {
            NSSound.beep()
            return
        }
        do {
            let capture = try SelectionCapture(document: document)
            guard let image = capture.source?.makeCGImage(in: doc.colorSpace) else { throw SubjectSelection.Failure.noComposite }
            _ = try RasterResources.shared.admit(scratch: SubjectTargeting.scratchBytes(capture))
            let task = try WorkerTask(scratchEstimate: SubjectTargeting.scratchBytes(capture)) { try SubjectSelection.mask(in: image) }
            let subjects = try BackgroundWork.wait(task, title: "Select Subject")
            try capture.requireCurrent(in: document)
            // A mask that is entirely zero builds no selection. Vision
            // having reported a subject while covering nothing would be a
            // contradiction, so it is reported as "no subject" rather than
            // left as a silent no-op.
            guard
                let selection = CanvasSelection(
                    shape: .mask(subjects.mask), canvasWidth: doc.width,
                    canvasHeight: doc.height)
            else {
                presentSubjectFailure(SubjectSelection.Failure.noSubject)
                return
            }
            // The menu command selects EVERY subject and carries no combine
            // modifier, so it records no `instance` and the replace mode.
            commitCanvasSelection(selection, actionName: "Select Subject",
                                  record: .selectSubject(instance: nil, mode: "replace"))
        } catch {
            presentSubjectFailure(error)
        }
    }

    /// Explains a refused segmentation instead of beeping at it: the user
    /// asked for something specific and nothing visible happened, so the
    /// reason is worth a sentence. A non-`Failure` error — `SelectionCapture`
    /// throws the core's render sentence — shows the system's own
    /// description rather than being swallowed.
    private func presentSubjectFailure(_ error: Error) {
        let alert = NSAlert()
        alert.messageText = (error as? SubjectSelection.Failure)?.message
            ?? error.localizedDescription
        alert.informativeText =
            "Subject detection looks for people, animals and other prominent "
            + "foreground objects in the flattened image. Nothing was selected, so the "
            + "current selection is unchanged."
        if let window = view.window {
            alert.beginSheetModal(for: window)
        } else {
            alert.runModal()
        }
    }
}
