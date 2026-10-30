import AppKit

// `refreshProofImage()` is the ONE writer: it clears the slot when proofing is off, drops a held
// proof whose size no longer matches `canvas.image` immediately, keeps a same-size one on screen
// while the next renders, and accepts a landing result only when its size still matches.
//
// A 32-bit float document takes the FIRST arm of `refreshProofImage()`, above the proof guard: it
// has no soft proof, so the slot is its view exposure's (`refreshViewExposureImage()`) —
// nil at 0 stops with no candidate tone map, proofing on or off — and `proofDisplay` is kept, so
// Redo back into 16 bits proofs again. A sheet's or gesture's live preview (`canvas.previewImage`)
// wins over the slot there too and is drawn at 0 stops under the document's own map: the
// unproofed-preview rule below, restated for exposure. The one preview that follows the exposure
// is the Tone Map sheet's, which is a candidate map in the slot and never a `previewImage`. The
// canvas returns to the exposed image when a preview clears.
// A failed proof (never a cancelled one) turns Proof
// Colors and Gamut Warning off, beeps and names the reason in `ProofDisplay.failure` — the status
// bar (`proofStatusText`) and `get_proof_setup`'s `failure`; the next toggle or setup clears it.

/// View ▸ Proof Setup… / Proof Colors (⌘Y) / Gamut Warning (⇧⌘Y).
///
/// The proof is pure VIEW state and is delivered through
/// `ImageCanvasView.proofImage`, a display-only slot drawn as
/// `previewImage ?? proofImage ?? image`. It NEVER reaches `canvas.image`:
/// that property's `didSet` clears the overlay, resets the selection and the
/// Quick Mask, cancels the subject session and cancels an in-progress
/// selection stroke, all of which are correct for a real document change and
/// destructive for a proof landing.
///
/// A sheet's live preview wins over the proof and is UNPROOFED — a recorded
/// decision: a preview is a picture of a pending edit, and proofing it would
/// mean re-running the whole chain on every slider tick. The same holds for a
/// float window's view exposure, which shares the slot: a preview that
/// writes `canvas.previewImage` is drawn at 0 stops under the document's own
/// tone map, and the exposed image returns when the preview clears.
extension EditorViewController {
    /// View ▸ Proof Colors.
    @objc func toggleProofColors(_ sender: Any?) {
        // Nothing to proof through yet: the default proof preset, or
        // else the setup, instead of toggling a state that would show nothing.
        guard proofDisplay.setup != nil || seedProofSetupFromDefault(sender) else { return }
        proofDisplay.failure = nil
        proofDisplay.proofColors.toggle()
        refreshProofImage()
        updateStatus()
    }

    /// View ▸ Gamut Warning. Turning it on turns proofing on, because a
    /// warning with no proof profile has nothing to warn about.
    /// On a CMYK document (#547) it flags the inks the proof profile cannot hold
    /// (`cmyk_proof`); a proof that preserves the numbers flags nothing.
    @objc func toggleGamutWarning(_ sender: Any?) {
        guard proofDisplay.setup != nil || seedProofSetupFromDefault(sender) else { return }
        proofDisplay.failure = nil
        proofDisplay.gamutWarning.toggle()
        if proofDisplay.gamutWarning { proofDisplay.proofColors = true }
        refreshProofImage()
        updateStatus()
    }

    /// View ▸ Proof Setup ▸ Custom…
    @objc func showProofSetup(_ sender: Any?) {
        showProofSetup(sender, notice: nil)
    }

    /// The sheet, with `notice` — why the default proof preset could not be
    /// used — as the first line of its note.
    func showProofSetup(_ sender: Any?, notice: String?) {
        guard document != nil,
              let sheet = ProofSetupSheetController(editor: self, notice: notice) else {
            NSSound.beep()
            return
        }
        presentAsSheet(sheet)
    }

    /// Applies a setup chosen in the sheet and redraws.
    func applyProofSetup(_ setup: ProofSetup, proofColors: Bool, gamutWarning: Bool) {
        proofDisplay.setup = setup
        proofDisplay.proofColors = proofColors
        proofDisplay.gamutWarning = gamutWarning
        proofDisplay.failure = nil
        refreshProofImage()
        updateStatus()
    }

    /// The proof segment of the status bar: nil at 32-bit float (which has no
    /// soft proof) and when neither proofing nor turned off by a failure.
    var proofStatusText: String? {
        guard document?.doc?.isFloat != true else { return nil }
        return proofDisplay.statusLine
    }

    /// The ONE place `canvas.proofImage` is written, with its float arm
    /// `refreshViewExposureImage()`.
    ///
    /// A 32-bit float document comes FIRST, above the proof guard: it has no
    /// soft proof, so the slot holds its view exposure or nothing, and
    /// the proof state is kept for a Redo back into 16 bits.
    ///
    /// Off: the slot is cleared immediately. On: any held proof whose size no
    /// longer matches `canvas.image` is dropped at once (it would be drawn
    /// stretched over a resized canvas), a SAME-size one stays on display
    /// while the new render runs, and a landing result is accepted only if it
    /// still matches — a stale proof is dropped rather than shown.
    ///
    /// Depth: 8 bits proof the RETAINED projection, rendering
    /// the document only when none is retained; 16 bits proof the
    /// RETAINED projection's display codes, and while that projection waits
    /// on a scheduled refresh no request is made (the refresh re-enters
    /// here). A failed proof turns itself off visibly (`proofDidFail`).
    func refreshProofImage() {
        if document?.doc?.isFloat == true {
            proofRenderer.cancel()
            refreshViewExposureImage()
            return
        }
        // Off a float document nothing in the slot is an exposure image: one
        // left from before a Convert or Redo out of float is dropped here, so
        // it is never drawn as this document's proof.
        viewExposureRenderer.cancel()
        if viewExposure.shown != nil { canvas.proofImage = nil }
        viewExposure.shown = nil
        endExtendedRange()
        guard proofDisplay.isActive, let setup = proofDisplay.setup, let document else {
            proofRenderer.cancel()
            canvas.proofImage = nil
            return
        }
        let canvasSize = canvas.image.map { NSSize(width: $0.width, height: $0.height) }
        if let held = canvas.proofImage,
           let canvasSize,
           NSSize(width: held.width, height: held.height) != canvasSize {
            canvas.proofImage = nil
        }
        guard let doc = document.doc else {
            canvas.proofImage = nil
            return
        }
        // #547 phase 5: the window's setup made valid for this document before every request (a
        // mode change must not fail a proof the user cannot fix); a CMYK document proofs its ink
        // through the core, Gamut Warning against the proof profile included.
        let reconciled = setup.reconciled(isCmykDocument: doc.isCmyk)
        if reconciled != setup { proofDisplay.setup = reconciled }
        let source: ProofSource
        if doc.isCmyk {
            source = .cmyk(doc)
        } else if doc.isHighDepth {
            guard let projection = document.retainedPrecisionProjection else {
                // A scheduled refresh posts `imageDocumentImageDidChange`,
                // which re-enters here (a render of the old projection is
                // cancelled meanwhile); anything else is a projection that
                // will not arrive, so the proof fails visibly.
                if document.precisionProjectionTimer != nil {
                    proofRenderer.cancel()
                    return
                }
                proofDidFail(document.projectionError ?? "The 16-bit display is not available.")
                return
            }
            source = .display(doc, projection)
        } else {
            source = .flat(doc, document.retainedByteProjection)
        }
        proofRenderer.request(
            source: source, setup: reconciled, gamutWarning: proofDisplay.gamutWarning
        ) { [weak self] outcome in
            guard let self else { return }
            switch outcome {
            case .failed(let reason):
                self.proofDidFail(reason)
            case .rendered(let result):
                // Landing size must still be the canvas's own size: a resize
                // or a crop that landed while the proof was rendering makes
                // it stale.
                let current = self.canvas.image.map { NSSize(width: $0.width, height: $0.height) }
                guard current == nil || current == result.pixelSize else { return }
                self.canvas.proofImage = result.image
            }
        }
    }

    /// A proof that could not be rendered turns proofing OFF and says why:
    /// the status bar, `get_proof_setup`'s `failure` and a beep. The slot is
    /// cleared through `refreshProofImage()`, which stays its one writer.
    private func proofDidFail(_ reason: String) {
        proofDisplay.proofColors = false
        proofDisplay.gamutWarning = false
        proofDisplay.failure = reason
        refreshProofImage()
        NSSound.beep()
        updateStatus()
    }

    /// Validation for the View items and the Proof Setup submenu's rows.
    func validateProofItem(_ item: NSValidatedUserInterfaceItem) -> Bool {
        guard let menuItem = item as? NSMenuItem else { return document != nil }
        switch menuItem.action {
        case #selector(toggleProofColors(_:)):
            menuItem.state = proofDisplay.proofColors ? .on : .off
        case #selector(toggleGamutWarning(_:)):
            menuItem.state = proofDisplay.gamutWarning ? .on : .off
        case #selector(applyWorkingProof(_:)):
            // View ▸ Proof Setup ▸ Working CMYK / Working <Plate> Plate (#547 phase 5).
            let plate = CmykProof.plate(tag: menuItem.tag)
            menuItem.state = proofDisplay.setup?.isWorking(plate: plate) == true ? .on : .off
            return document != nil && WorkingCMYK.installedData != nil
        case #selector(applyProofPresetFromMenu(_:)):
            // The checkmark: no I/O, the setup's digest against the preset's.
            let preset = menuItem.representedObject as? ProofPreset
            menuItem.state = preset?.matches(proofDisplay.setup) == true ? .on : .off
        case #selector(deleteProofPresetFile(_:)):
            // A library write: it needs no document.
            return true
        default:
            break
        }
        return document != nil
    }
}
