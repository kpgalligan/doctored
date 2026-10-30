import AppKit

// `copy(_:)` (frozen, first statement) routes a no-selection ⌘C to `copyLayers()`.
//
// `pastePlanned` routes Paste / Paste in Place to `pasteLayers(_:placement:)` while a
// `LayerClipboard` entry is current.

/// Edit ▸ Copy of LAYERS (no pixel selection) and Edit ▸ Paste /
/// Paste in Place of a held layer clipboard. The model is
/// `LayerClipboard.swift`'s; `copy(_:)` and `pastePlanned` route here.
extension EditorViewController {
    /// Edit ▸ Copy with no pixel selection: the selected layers, subtrees
    /// included, into the layer clipboard, with today's flat image on the
    /// pasteboard beside it. True only when the pasteboard was written.
    @discardableResult
    func copyLayers() -> Bool {
        guard let document, let doc = document.doc else { NSSound.beep(); return false }
        do {
            _ = try LayerClipboard.copy(document.selectedLayerIndices, from: document)
            return true
        } catch {
            if doc.isHighDepth { presentPrecisionError(error) } else { document.presentError(error) }
            return false
        }
    }

    /// Edit ▸ Paste (`.auto`) / Paste in Place (`.inPlace`) of the held
    /// layer clipboard at every depth, 32-bit float included
    /// (the core promotes, and names any copied layer the float mode cannot
    /// hold): with "Ask When Pasting" on and the clip's profile differing,
    /// the Paste Profile Mismatch alert — without "Don't Convert",
    /// and saying why, when `LayerPastePlan.preserveRefusal` answers; then
    /// one undo step. `record` replaces the recorded `paste` step: a layer
    /// drag from another document (LayerDrag.swift) pastes an entry
    /// the clipboard never held, so it records its own placeholder.
    func pasteLayers(_ entry: LayerClipboard.Entry, placement: PastePlacement,
                     record: [ActionStep]? = nil) {
        guard let document, let doc = document.doc else { NSSound.beep(); return }
        if ColorSettings.askWhenPasting,
           let mismatch = LayerPastePlan.profileMismatch(entry, into: doc) {
            let refusal = LayerPastePlan.preserveRefusal(entry, into: doc)
            var informative = "The clipboard's layers are in “\(mismatch.source)” and this "
                + "document is in “\(mismatch.destination)”. Convert keeps their appearance; "
                + "Don't Convert keeps their numbers."
            if let refusal { informative += " " + refusal }
            askPasteProfile(message: "Paste Profile Mismatch", informative: informative,
                            allowsPreserve: refusal == nil) { [weak self] profile in
                self?.performLayerPaste(entry, placement: placement, profile: profile,
                                        record: record)
            }
            return
        }
        performLayerPaste(entry, placement: placement, profile: .convert, record: record)
    }

    /// Runs a resolved layers paste with its profile choice made: 8 bits
    /// through `applyEdit`, typed through the capture; then the pasted roots
    /// are selected, the topmost primary. The paste context is read
    /// here, at publish time, after any mismatch alert; the selection is
    /// kept (`PasteKind.layers`). Records the LIVE
    /// `paste` step, the same one a pixel paste records: a
    /// replay pastes whatever the clipboard holds then.
    private func performLayerPaste(_ entry: LayerClipboard.Entry, placement: PastePlacement,
                                   profile: PasteProfile, record override: [ActionStep]?) {
        guard let document, let doc = document.doc else { NSSound.beep(); return }
        let record: [ActionStep] = override ?? .paste(
            payloadPath: nil, placement: placement, offset: nil, name: nil, profile: profile,
            note: placement == .inPlace ? "Edit ▸ Paste Special ▸ Paste in Place" : "Edit ▸ Paste")
        do {
            let outcome: LayerPasteOutcome
            if !doc.isHighDepth {
                let (transform, work) = try LayerPasteWork.byteTransform(
                    entry, document: document, anchor: document.activeLayerIndex,
                    position: .above, placement: placement, profile: profile,
                    context: pasteContext(selection: document.canvasSelection,
                                          quickMask: document.quickMaskActive, in: doc,
                                          origin: nil))
                let before = document.doc
                document.applyEdit(work.historyName, record: record, transform)
                guard document.doc !== before else { return }
                outcome = work
            } else {
                let capture = try PrecisionEditorCapture(document)
                let (pasted, work) = try LayerPasteWork.typedDocument(
                    entry, capture: capture, anchor: capture.layer, position: .above,
                    placement: placement, profile: profile,
                    context: pasteContext(selection: capture.selection,
                                          quickMask: capture.quickMask, in: capture.facade,
                                          origin: nil))
                guard try capture.publish(pasted, to: document, name: work.historyName,
                                          record: record) else { return }
                outcome = work
            }
            document.setLayerSelection(LayerSelection(primary: outcome.topRoot,
                                                      others: Set(outcome.roots)))
            activeLayerDidChange()
        } catch { presentLayerPasteError(error, in: doc) }
    }

    private func presentLayerPasteError(_ error: Error, in doc: RasterDocument) {
        if doc.isHighDepth { presentPrecisionError(error) } else { document?.presentError(error) }
    }
}
