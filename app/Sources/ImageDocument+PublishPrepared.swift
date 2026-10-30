import Foundation

// The byte `commitFloatingTransform` answers `.refused`, keeping the session, when `applyEdit` did
// not change the document, through `ImageDocument.publishPrepared` (this file: `applyEdit` inside
// `withUndoGroupDiscardingEmpty`, answering a `PublishOutcome` — `.published`, `.unchanged` or
// `.refused` — and clearing a refused `preparedEdit`), and so does its 16-bit branch through
// `PrecisionGeometryGesture.finishOutcome` → `ImageDocument.applyPrepared`; the byte Move Selected
// commits and `applyPrepared` publish through the same helper, so a refused publication leaves no
// undo step.

/// What `ImageDocument.publishPrepared` did.
enum PublishOutcome {
    /// `doc` is now the edit's document: one undo step.
    case published
    /// The edit's document already was `doc` before the call: a true no-op.
    case unchanged
    /// `applyEdit` refused: `doc` is untouched and no undo step was left.
    case refused
}

extension ImageDocument {
    /// Publishes an already prepared edit through `applyEdit` inside
    /// `withUndoGroupDiscardingEmpty`, so a publication `applyEdit` refuses
    /// leaves no empty undo step. The byte Free Transform and Move
    /// Selected commits and `applyPrepared` share it.
    ///
    /// The refusal is a backstop: every caller asks `admitsPublication` (or
    /// `preflightPublication`), the same guards `applyEdit` asks, with no
    /// run-loop turn between (the `DocumentJobs.finish` wording). On a
    /// refusal a `preparedEdit` for this edit is cleared; nothing is noted,
    /// because `applyEdit`'s beep already notes its sentence
    /// (`SessionCommitRefusal.beep`). With `captured`, the transform also
    /// publishes only over that exact document; a mismatch answers `.refused`.
    @discardableResult
    func publishPrepared(_ edit: PreparedDocumentEdit, name: String, record: [ActionStep],
                         changesCanvasGeometry: Bool = false,
                         resultingSelection: SelectionHistoryState?,
                         captured: RasterDocument? = nil) -> PublishOutcome {
        // `.unchanged` also drops a differing `resultingSelection`; no caller relies on it.
        if doc === edit.document { return .unchanged }
        withUndoGroupDiscardingEmpty {
            applyEdit(name, record: record, changesCanvasGeometry: changesCanvasGeometry,
                      resultingSelection: resultingSelection) { current in
                if let captured, current !== captured { return nil }
                return edit.document
            }
        }
        if doc === edit.document { return .published }
        if preparedEdit?.document === edit.document { preparedEdit = nil }
        return .refused
    }
}
