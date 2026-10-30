import AppKit

// `preflightPublication` runs `applyEdit`'s post-transform list: the entry guard,
// `publicationRefusal`, the depth guard, then the 16-bit display preparation or, at 8 bits and not
// carried, the admission of `ByteFilterAdmission.commitStageBytes` for the updated canvas beside
// its own retained bytes (no render).
//
// A beeped refusal throws `SilentEditRefusal`, and `admitsPublication` answers the way `applyEdit`
// does (beep or `presentError`, then false). `ImageDocument.presentError` beeps for a
// `SilentEditRefusal` instead of alerting, so a UI catch site presents a publication error
// through its document, never `NSApp.presentError`. `applyEdit` itself, `performGroupedEdit`,
// `applyPrepared`, `DocumentJobs.finish` and every prepared-edit publisher use
// `preflightPublication`.

/// A refusal applyEdit answers with a beep, not a sentence; `message` is for agent and job callers.
/// Presented through its document (`ImageDocument.presentError`), it beeps too.
struct SilentEditRefusal: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

extension ImageDocument {
    /// A UI catch site that presents a precision-publication error through
    /// its document answers a `SilentEditRefusal` the way `applyEdit` does:
    /// a beep and no alert. Everything else is presented as before.
    /// `willPresentError` is not reached for a beep, so the beep notes the
    /// refusal's message for the agent (`SessionCommitRefusal.beep`).
    @discardableResult
    override func presentError(_ error: Error) -> Bool {
        if let refusal = error as? SilentEditRefusal {
            SessionCommitRefusal.beep(refusal.message)
            return false
        }
        return super.presentError(error)
    }

    /// `applyEdit`'s publication guards for `updated`, in its order, asked
    /// BEFORE a caller opens an undo group: NSUndoManager keeps
    /// an empty explicit group as an Undo step, so a refusal answered inside
    /// the group would still leave one behind. In order: the entry guard
    /// (live edit, publishing, no document), `publicationRefusal` for
    /// `record` and `updated` (a `DoctoredCoreError`), the depth guard, then
    /// the 16-bit display preparation `applyEdit` would run, which leaves
    /// `preparedEdit` set for `updated` so `applyEdit` does not prepare
    /// again, or, at 8 bits, the admission of `updated`'s commit stage,
    /// `ByteFilterAdmission.commitStageBytes` (the n×4 result and
    /// written layer plus the first projection; a `DoctoredCoreError`, no
    /// render). The beeped refusals throw
    /// `SilentEditRefusal`. On any throw a `preparedEdit` for `updated` is
    /// cleared.
    func preflightPublication(_ updated: RasterDocument, record: [ActionStep],
                              allowWarpChange: Bool = false,
                              intentionalDepthConversion: Bool = false,
                              renderNeutral: Bool = false) throws {
        do {
            try checkPublication(updated, record: record, allowWarpChange: allowWarpChange,
                                 intentionalDepthConversion: intentionalDepthConversion,
                                 renderNeutral: renderNeutral)
        } catch {
            if preparedEdit?.document === updated { preparedEdit = nil }
            throw error
        }
    }

    /// `preflightPublication`, answered the way `applyEdit` answers: a
    /// `SilentEditRefusal` beeps, anything else is presented; then false.
    func admitsPublication(_ updated: RasterDocument, record: [ActionStep],
                           allowWarpChange: Bool = false,
                           intentionalDepthConversion: Bool = false,
                           renderNeutral: Bool = false) -> Bool {
        do {
            try preflightPublication(updated, record: record, allowWarpChange: allowWarpChange,
                                     intentionalDepthConversion: intentionalDepthConversion,
                                     renderNeutral: renderNeutral)
            return true
        } catch let refusal as SilentEditRefusal {
            SessionCommitRefusal.beep(refusal.message)
            return false
        } catch {
            presentError(error)
            return false
        }
    }

    private func checkPublication(_ updated: RasterDocument, record: [ActionStep],
                                  allowWarpChange: Bool, intentionalDepthConversion: Bool,
                                  renderNeutral: Bool) throws {
        guard !isLiveEditing, !isPublishingEditingState, let current = doc else {
            throw SilentEditRefusal(message: "Finish the current gesture or edit first. Nothing changed.")
        }
        if let reason = publicationRefusal(record: record, to: updated, allowWarpChange: allowWarpChange,
                                           intentionalDepthConversion: intentionalDepthConversion) {
            throw DoctoredCoreError(message: reason)
        }
        guard intentionalDepthConversion || (current.precision == nil) == (updated.precision == nil) else {
            throw SilentEditRefusal(message: "The edit was refused and nothing changed.")
        }
        // Prepare before changing the document or registering history. The
        // worker may service Undo on the old, fully committed state; that makes
        // this capture stale and is checked again before publication.
        if preparedEdit?.document !== updated {
            if renderNeutral, let carried = carriedProjection(from: current, to: updated) {
                preparedEdit = carried
            } else if updated.precision != nil {
                let capturedRevision = revision
                let capturedState = captureEditingState()
                let prepared = try BackgroundWork.wait(try WorkerTask { PreparedDocumentEdit(updated) },
                                                      title: "Preparing 16-bit Display")
                guard doc === current, revision == capturedRevision, !isLiveEditing,
                      isCurrentEditingState(capturedState) else {
                    throw SilentEditRefusal(message: "The document changed while this edit's display "
                        + "was being prepared. Nothing changed.")
                }
                preparedEdit = prepared
            } else {
                // 8-bit, not carried: admit the commit stage — the
                // n×4 result and written layer beside the projection
                // `refreshProjection` renders after publication — beside the
                // updated document's own retained bytes. Nothing renders here.
                let canvas = UInt64(updated.width) * UInt64(updated.height)
                _ = try RasterResources.shared.admit(
                    scratch: ByteFilterAdmission.commitStageBytes(pixels: canvas, in: updated),
                    own: updated.ownRetainedBytes())
            }
        }
    }
}
