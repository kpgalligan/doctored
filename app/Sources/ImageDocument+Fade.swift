import AppKit

// Edit ▸ Fade (v1, `designs/v1-shortcuts-fade.md`): the candidate a fadeable edit leaves, whether
// it still stands, the faded result, and the commit that REPLACES the faded step with one step
// named "Fade <step>". The step tables are `FadeStep.swift`; the dialog `FadeSheetController`.

extension ImageDocument {
    /// The last edit when Edit ▸ Fade can still blend it back: the document
    /// is still its result, the selection, the layer selection and the paint
    /// target (the layer, no Quick Mask) are as it left them, and nothing is
    /// mid-gesture or publishing. nil otherwise — any other edit, an undo, a
    /// selection or layer change makes Fade unavailable.
    var currentFadeCandidate: FadeCandidate? {
        guard let candidate = fadeCandidate, doc === candidate.after,
              selectionHistory == candidate.selection,
              layerSelection == candidate.layerSelection,
              planeEditTarget == .layer, !quickMaskActive,
              !isLiveEditing, !isPublishingEditingState else { return nil }
        return candidate
    }

    /// Called by `applyEdit` and `endLiveEdit` once the edit is published and
    /// its history registered: the edit's hint is its recorded step's
    /// (`FadeStep.hint(for:)`), else `fadeHint`; a fadeable edit becomes the
    /// candidate, anything else clears it. `fadeHint` is consumed either way.
    func noteFadeCandidate(name: String, before: RasterDocument, record: [ActionStep]) {
        let hint = FadeStep.hint(for: record) ?? fadeHint
        fadeHint = nil
        // A step that added or removed a layer (a stroke onto a new layer)
        // has no same-index layer before it to fade toward.
        guard let hint, let after = doc, after !== before, planeEditTarget == .layer,
              !quickMaskActive, after.layerCount == before.layerCount else {
            fadeCandidate = nil
            return
        }
        fadeCandidate = FadeCandidate(
            name: name, kind: hint.kind, layer: hint.layer ?? activeLayerIndex, before: before,
            after: after, selection: selectionHistory, layerSelection: layerSelection,
            historyID: history.currentID)
    }

    /// The faded step's result blended back with the state before it at
    /// `opacity` (0…1) in `mode`; nil when that changes nothing. A region
    /// step is weighed by its selection, a stroke by the pixels it changed.
    /// At 16 and 32 bits the work runs as a captured job (`control` nil) or
    /// on the caller's own `control` (a preview).
    func fadedDocument(_ candidate: FadeCandidate, mode: DrBlendMode, opacity: Double,
                       control: RasterJobControl?) throws -> RasterDocument? {
        let inputs = FadeInputs(candidate, selection: candidate.selection.selection)
        if let control { return try inputs.result(mode: mode, opacity: opacity, control: control) }
        guard candidate.after.isHighDepth else {
            return try inputs.result(mode: mode, opacity: opacity, control: nil)
        }
        let scratch = try inputs.scratch()
        return try PrecisionCapturedWork.run(scratchEstimate: scratch, title: "Fade") {
            try inputs.result(mode: mode, opacity: opacity, control: $0)
        }
    }

    /// Publishes `result` as ONE step named "Fade <step>" in place of the
    /// faded step: the step is undone first and the fade applied over the
    /// state before it, so the history keeps one entry where there was one.
    /// During an Action replay (history suspended, registration disabled), or
    /// whenever the faded step is no longer the current history entry, the
    /// fade is applied over the result as an edit of its own. True when the
    /// fade was published.
    @discardableResult
    func commitFade(_ candidate: FadeCandidate, result: RasterDocument,
                    record: [ActionStep]) -> Bool {
        let name = "Fade \(candidate.name)"
        guard doc === candidate.after else {
            NSSound.beep()
            return false
        }
        if let id = candidate.historyID, history.currentID == id, let manager = undoManager,
           manager.groupingLevel == 0, manager.isUndoRegistrationEnabled, history.suspended == 0 {
            guard stepHistory(redo: false) else { return false }
            withUndoGroupDiscardingEmpty {
                applyEdit(name, record: record) { $0 === candidate.before ? result : nil }
            }
            if doc === result { return true }
            // Refused: the faded step comes back as it was.
            _ = stepHistory(redo: true)
            return false
        }
        withUndoGroupDiscardingEmpty {
            applyEdit(name, record: record) { $0 === candidate.after ? result : nil }
        }
        return doc === result
    }
}

/// What a fade computes from, captured on main so a preview can run it on
/// `PreviewRenderer`'s queue without reaching into the document: the two
/// handles, the layer, the kind and the selection's coverage at both widths.
struct FadeInputs {
    let candidate: FadeCandidate
    let coverage8: [UInt8]?
    let coverage16: [UInt16]?

    init(_ candidate: FadeCandidate, selection: CanvasSelection?) {
        self.candidate = candidate
        let region = candidate.kind == .region
        if candidate.after.isHighDepth {
            coverage8 = nil
            coverage16 = region ? selection?.maskSamples16() : nil
        } else {
            coverage8 = region ? selection?.maskBytes() : nil
            coverage16 = nil
        }
    }

    /// The typed plan's `scratch_bytes`, for the job's admission.
    func scratch() throws -> UInt64 {
        guard let after = candidate.after.precision, let before = candidate.before.precision else {
            throw DoctoredCoreError(message: "Fade needs both states at the document's depth.")
        }
        let context = try PrecisionWorkContext(source: after)
        let plan = try after.fadePlan(layer: candidate.layer, before: before,
                                      coverage: coverage16 != nil, workingLimit: context.limit,
                                      otherRetained: context.otherRetained)
        return try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
    }

    /// The faded document, or nil when the fade changes nothing. `ownReservation`
    /// is a preview's own scratch lease, which the core's plan already charges.
    func result(mode: DrBlendMode, opacity: Double, control: RasterJobControl?,
                ownReservation: UInt64 = 0) throws -> RasterDocument? {
        let changedOnly = candidate.kind == .stroke
        guard let after = candidate.after.precision else {
            return candidate.after.faded(
                from: candidate.before, layer: candidate.layer, mode: mode, opacity: opacity,
                coverage: coverage8, changedOnly: changedOnly)
        }
        guard let before = candidate.before.precision else {
            throw DoctoredCoreError(message: "Fade needs both states at the document's depth.")
        }
        let context = try PrecisionWorkContext(source: after)
        guard context.otherRetained >= ownReservation else {
            throw DoctoredCoreError(message: "The fade preview reservation is missing.")
        }
        let faded = try after.fading(
            layer: candidate.layer, before: before, mode: mode, opacity: opacity,
            coverage: coverage16, changedOnly: changedOnly, workingLimit: context.limit,
            otherRetained: context.otherRetained - ownReservation, control: control)
        return try faded.map { try RasterDocument(precision: $0) }
    }
}
