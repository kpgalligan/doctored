import AppKit

/// Image ▸ Evaluation… — the per-document evaluation policy
/// (`DocumentEvaluation.swift`).
///
/// The sheet chooses; this file commits and validates, the
/// `EditorViewController+ColorManagement` shape: the switch re-runs on the
/// document's CURRENT handle inside the transform rather than on the handle
/// the sheet previewed, so an edit that slipped in while the dialog was open
/// survives.
///
/// A refusal — a profile the policy cannot decode, or an operator this phase
/// does not evaluate — is a SENTENCE, not a bare nil, so it is carried out of
/// the non-throwing transform and presented instead of beeping.
extension EditorViewController {
    @objc func showEvaluation(_ sender: Any?) {
        guard let sheet = EvaluationSheetController(editor: self) else {
            NSSound.beep()
            return
        }
        presentAsSheet(sheet)
    }

    /// Commits the policy as ONE undo step.
    func commitEvaluation(_ policy: EvaluationPolicy) {
        guard let document = document else {
            NSSound.beep()
            return
        }
        var refusal: Error?
        document.applyEdit("Evaluation", record: .setEvaluation(policy)) { source in
            do {
                return try source.withEvaluation(policy)
            } catch {
                refusal = error
                return nil
            }
        }
        if let refusal {
            document.presentError(refusal)
        }
    }

    /// Enablement for the menu item: any open document can be asked, because
    /// the sheet is where the refusal is explained. A document with no handle
    /// has nothing to ask.
    func validateEvaluationItem() -> Bool { document?.doc != nil }
}
