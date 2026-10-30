import AppKit

// Cut records the `cut` step at every depth, and Edit ▸ Copy records `copy {mode:
// "auto"}` and Copy Merged `copy_merged` (the frozen `copy(_:)` / `copyMerged(_:)` call
// `ActionRecorder.shared.record` once `copyLayers()` / `copyPrecision()` / `copyMergedPrecision()`
// / `copyToPasteboard` answers true; MCP `copy` / `copy_merged` are recorded verbatim, out of
// `nonRecordable`); float Cut is admitted.

/// Phase 12G-1: Edit ▸ Cut / Copy / Copy Merged at 16 bits and 32-bit float.
/// Each byte handler in `EditorViewController.swift` routes here on its first
/// line at depth, so a typed document never reaches a byte surrogate
/// (`layerCanvasImage`, `masked`, `cropped`). Phase 12G-2: Paste and Paste in
/// Place at EVERY depth go through `pastePlanned` — the plan
/// (`ClipboardBoundaries.swift`) is decided before any edit. A
/// paste converts into the document's depth and profile silently, and only
/// "Ask When Pasting" (`ColorSettings.askWhenPasting`) brings up the Paste
/// Profile Mismatch alert (`askPasteProfile`). Edit ▸ Paste requests `.auto`,
/// which lands by `PasteContext` (`PasteLanding.swift`) over this window's
/// view; Paste in Place reads the copy's `ClipboardSource`.
/// The payload model and the pasteboard type are
/// `ClipboardPayload.swift`'s. A Paste records the LIVE `paste`
/// step (no pixels, no position: the clipboard at replay), and Copy / Copy
/// Merged report whether the pasteboard was written so `copy(_:)` /
/// `copyMerged(_:)` can record their steps.
extension EditorViewController {
    /// The source canvas every copy from this window writes
    /// beside its item (`ClipboardSource`); nil without a document.
    var clipboardSource: ClipboardSource? {
        guard let document, let doc = document.doc else { return nil }
        return ClipboardSource(document: document, canvas: doc)
    }

    /// Writes one payload: the private RZCB type plus its public rendering,
    /// tagged with the document's profile, in ONE pasteboard item.
    private func writeTyped(_ payload: RasterClipboardPayload, from doc: RasterDocument,
                            space: CGColorSpace) throws -> Bool {
        let rendering = try doc.publicRendering(of: payload)
        guard let cgImage = rendering.makeCGImage(in: space) else { return false }
        let bytes = payload.bytes
        // #1 phase 4: a Grayscale copy's TIFF is its one-channel gray image.
        let tiff = try payload.pasteboardTIFF(from: doc, held: bytes.count)
        try Clipboard.writeItem(cgImage, typed: bytes, source: clipboardSource, tiff: tiff)
        return true
    }

    /// Edit ▸ Copy at depth: the ACTIVE layer's raw samples inside the
    /// selection's bounds (the whole canvas without one), coverage applied.
    /// Since 12G-3 a whole-layer Copy carries the layer's editable record,
    /// and an adjustment layer's (which has no samples) is a transparent
    /// payload carrying its meta. A group copies its raw projection.
    /// True only when the pasteboard was written.
    @discardableResult
    func copyPrecision() -> Bool {
        guard let document, let doc = document.doc else { NSSound.beep(); return false }
        do {
            let selection = document.canvasSelection
            guard let window = doc.clipboardWindow(selection) else {
                NSSound.beep()
                return false
            }
            let layer = document.activeLayerIndex
            // #1 phase 4c: a CMYK adjustment record has no ink to copy —
            // the core refuses it by name ("use Copy Merged"); since the
            // artwork batch a CMYK type or shape layer carries its record.
            let editable = selection == nil && !(doc.isCmyk && doc.layerIsAdjustment(layer))
            var payload = editable && doc.layerIsAdjustment(layer)
                ? try doc.transparentPayload()
                : try doc.clipboardCopy(layer: layer, window: window, selection: selection)
            if editable { payload = try doc.attachingEditable(payload, layer: layer) }
            guard try writeTyped(payload, from: doc, space: document.colorSpace) else {
                NSSound.beep()
                return false
            }
            return true
        } catch {
            presentPrecisionError(error)
            return false
        }
    }

    /// Edit ▸ Copy Merged at 8 bits: the document's composite
    /// (`byteComposite(of:)`). nil after a beep (no document) or after
    /// presenting the core's render sentence through the document.
    func mergedCopySource() -> RasterImage? {
        guard let document, let doc = document.doc else { NSSound.beep(); return nil }
        do {
            return try document.byteComposite(of: doc)
        } catch {
            document.presentError(error)
            return nil
        }
    }

    /// Edit ▸ Copy Merged at depth: the same window of the projection. True
    /// only when the pasteboard was written.
    @discardableResult
    func copyMergedPrecision() -> Bool {
        guard let document, let doc = document.doc else { NSSound.beep(); return false }
        do {
            let selection = document.canvasSelection
            guard let window = doc.clipboardWindow(selection) else {
                NSSound.beep()
                return false
            }
            let payload = try doc.clipboardCopyMerged(window: window, selection: selection)
            guard try writeTyped(payload, from: doc, space: document.colorSpace) else {
                NSSound.beep()
                return false
            }
            return true
        } catch {
            presentPrecisionError(error)
            return false
        }
    }

    /// Edit ▸ Cut at depth (16 bits and, since 12G-2, 32-bit float — whose
    /// erase is the core's own f32 alpha erase): Copy, then exactly Clear's
    /// erase, as one "Cut" step. The pasteboard is written before anything is published, so
    /// pixels are never removed without having been captured; a Cancel on
    /// the rasterize prompt degrades to Copy, exactly as the byte Cut does.
    /// A transparency lock makes the erase a byte-exact no-op: the pasteboard
    /// keeps the payload and the frozen-alpha alert explains the rest.
    func cutPrecision() {
        guard let document, document.doc != nil, document.canvasSelection != nil else {
            NSSound.beep()
            return
        }
        guard !refuseAdjustmentPixelEdit(), !refusePlaneTargetEdit(),
              !refuseGroupPixelEdit() else { return }
        let idx = document.activeLayerIndex
        guard !refuseLockedEdit(layer: idx, kind: DR_EDIT_PIXELS) else { return }
        guard document.confirmRasterize(layer: idx) else {
            copyPrecision()
            return
        }
        do {
            let capture = try PrecisionEditorCapture(document)
            guard let window = capture.facade.clipboardWindow(capture.selection) else {
                NSSound.beep()
                return
            }
            let (payload, next) = try capture.facade.clipboardCut(
                layer: capture.layer, window: window, selection: capture.selection,
                indexedBackground: backgroundColor)
            guard try writeTyped(payload, from: capture.facade, space: document.colorSpace) else {
                NSSound.beep()
                return
            }
            let changed = try capture.publish(next?.precision, to: document, name: "Cut",
                                              record: .cut(note: "Edit ▸ Cut"))
            if !changed { refuseFrozenAlpha(layer: idx) }
        } catch { presentPrecisionError(error) }
    }

    /// Phase 12G-2: the private typed payload an 8-bit Copy writes beside
    /// its public TIFF/PNG — a byte-origin RZCB of the copied pixels at
    /// `offset`, in this document's profile. nil (public types only, never a
    /// beep) when it cannot be built, and always nil at depth. A non-nil
    /// `layer` (a whole-layer Copy, 12G-3) attaches that layer's editable
    /// record.
    func typedBytes(forByteCopy image: RasterImage, offset: (x: Int, y: Int),
                    layer: Int?) -> Data? {
        guard let doc = document?.doc, !doc.isHighDepth else { return nil }
        do {
            let payload = try doc.bytePayload(image, offset: offset)
            guard let layer else { return payload.bytes }
            return try doc.attachingEditable(payload, layer: layer).bytes
        } catch { return nil }
    }

    /// Edit ▸ Paste (`.auto`) / Paste in Place at EVERY depth: resolve the
    /// plan, then `performPaste` — converting silently, or, with "Ask When
    /// Pasting" on and the clipboard's profile differing from the
    /// document's (`PastePlan.profileMismatch`), after the Paste Profile
    /// Mismatch alert. An editable record is never asked about (12G-3).
    func pastePlanned(placement: PastePlacement) {
        guard let document, let doc = document.doc else { NSSound.beep(); return }
        // Paste Into / Outside of copied layers refuses
        // (validation disables them; this backstops any other path).
        if placement.masksSelection, LayerClipboard.shared.current(on: Clipboard.pasteboard) != nil {
            NSSound.beep()
            return
        }
        // A held layer clipboard pastes as layers.
        if placement == .auto || placement == .centered || placement == .inPlace,
           let entry = LayerClipboard.shared.current(on: Clipboard.pasteboard) {
            pasteLayers(entry, placement: placement)
            return
        }
        let plan: PastePlan
        do {
            plan = try PastePlan.resolve(for: document, pasteboard: Clipboard.pasteboard,
                                         payloadFile: nil)
        } catch {
            presentPrecisionError(error)
            return
        }
        if case .refused(let reason) = plan {
            if doc.isHighDepth {
                presentPrecisionError(DoctoredCoreError(message: reason))
            } else {
                NSSound.beep()
            }
            return
        }
        if ColorSettings.askWhenPasting, let mismatch = PastePlan.profileMismatch(plan, in: doc) {
            askPasteProfile(
                message: "Paste Profile Mismatch",
                informative: "The clipboard's pixels are in “\(mismatch.source)” and this "
                    + "document is in “\(mismatch.destination)”. Convert keeps their "
                    + "appearance; Don't Convert keeps their numbers.",
                allowsPreserve: true) { [weak self] profile in
                self?.performPaste(plan, placement: placement, profile: profile)
            }
            return
        }
        performPaste(plan, placement: placement, profile: .convert)
    }

    /// The Paste Profile Mismatch alert — "Convert", "Don't
    /// Convert" (only when `allowsPreserve`) and "Cancel"; `proceed` runs
    /// with the choice and never on Cancel. Automated runs answer the first
    /// button, Convert (`AutomationAlerts.swift`).
    func askPasteProfile(message: String, informative: String, allowsPreserve: Bool,
                         then proceed: @escaping (PasteProfile) -> Void) {
        let alert = NSAlert()
        alert.messageText = message
        alert.informativeText = informative
        alert.addButton(withTitle: "Convert")
        if allowsPreserve { alert.addButton(withTitle: "Don't Convert") }
        alert.addButton(withTitle: "Cancel")
        let answer: (NSApplication.ModalResponse) -> Void = { response in
            if response == .alertFirstButtonReturn {
                proceed(.convert)
            } else if allowsPreserve, response == .alertSecondButtonReturn {
                proceed(.preserve)
            }
        }
        if let window = view.window {
            alert.beginSheetModal(for: window, completionHandler: answer)
        } else {
            answer(alert.runModal())
        }
    }

    /// The paste context for this window — the destination selection's
    /// frame (outside Quick Mask), this view's visible canvas rect and the
    /// copy's `origin` (the pasteboard's `ClipboardSource` for
    /// a pixel paste, nil for copied layers, whose clip has its own canvas).
    func pasteContext(selection: CanvasSelection?, quickMask: Bool,
                      in doc: RasterDocument, origin: ClipboardSource?) -> PasteContext {
        PasteContext(selection: selection, quickMask: quickMask, in: doc,
                     visible: viewState.visible, origin: origin)
    }

    /// Runs a resolved plan with its profile choice made — no alerts. The
    /// 8-bit transform publishes through `applyEdit`, the typed document
    /// through the capture; either records the LIVE `paste` step (the
    /// requested placement, no payload and no position, so a replay
    /// pastes the clipboard it finds) while a recording runs, and selects
    /// the landed layer. Paste Into /
    /// Outside read their `PasteMask` here, at publish time (after any
    /// mismatch alert), consume the selection in the same history row and leave the
    /// pasted LAYER as the edit target. The landing (`.auto` resolved
    /// against `pasteContext`) is decided here too; a `.selection` landing
    /// makes no mask. Every pixel paste made with a selection
    /// consumes it in the same row (Select ▸ Reselect restores it;
    /// `PasteKind`).
    func performPaste(_ plan: PastePlan, placement: PastePlacement, profile: PasteProfile) {
        guard let document, let doc = document.doc else { NSSound.beep(); return }
        func note(_ mask: PasteMask?) -> String {
            if placement == .inPlace { return "Edit ▸ Paste Special ▸ Paste in Place" }
            return mask.map { "Edit ▸ Paste Special ▸ \($0.historyName)" } ?? "Edit ▸ Paste"
        }
        func record(_ mask: PasteMask?) -> [ActionStep] {
            .paste(payloadPath: nil, placement: placement, offset: nil, name: nil,
                   profile: profile, note: note(mask))
        }
        // The copy's source canvas, for Paste in Place.
        let origin = ClipboardSource.read(Clipboard.pasteboard)
        // Paste Into / Outside consume through their mask; every other
        // pixel paste made with a selection consumes it too (`PasteKind`).
        func selectionAfter(_ state: SelectionHistoryState,
                            _ mask: PasteMask?) -> SelectionHistoryState? {
            mask?.consumed(state) ?? PasteKind(pixels: placement).resultingSelection(state)
        }
        func land(_ landing: Int, in current: RasterDocument, masked: Bool) {
            let landing = min(landing, current.layerCount - 1)
            if masked {
                document.landMaskedPaste(at: landing)
            } else {
                document.activeLayerIndex = landing
                activeLayerDidChange()
            }
        }
        do {
            if !doc.isHighDepth {
                let mask = placement.masksSelection
                    ? try PasteMask(placement: placement, selection: document.canvasSelection,
                                    quickMask: document.quickMaskActive, in: doc)
                    : nil
                let spot = try PasteWork.landing(
                    plan, request: placement, at: nil, mask: mask,
                    canvas: (doc.width, doc.height),
                    context: pasteContext(selection: document.canvasSelection,
                                          quickMask: document.quickMaskActive, in: doc,
                                          origin: origin))
                let (transform, outcome) = try PasteWork.byteTransform(
                    plan, document: document, anchor: document.activeLayerIndex,
                    landing: spot, name: nil, mask: mask, profile: profile)
                let before = document.doc
                let after = selectionAfter(document.selectionHistory, mask)
                document.applyEdit(outcome.historyName, record: record(mask),
                                   resultingSelection: after, transform)
                if document.doc !== before, let current = document.doc {
                    land(outcome.landing, in: current, masked: mask != nil)
                }
                return
            }
            let capture = try PrecisionEditorCapture(document)
            let mask = placement.masksSelection
                ? try PasteMask(placement: placement, selection: capture.selection,
                                quickMask: capture.quickMask, in: capture.facade)
                : nil
            let spot = try PasteWork.landing(
                plan, request: placement, at: nil, mask: mask,
                canvas: (capture.facade.width, capture.facade.height),
                context: pasteContext(selection: capture.selection,
                                      quickMask: capture.quickMask, in: capture.facade,
                                      origin: origin))
            let (pasted, outcome) = try PasteWork.typedDocument(
                plan, capture: capture, anchor: capture.layer, position: .above,
                landing: spot, name: nil, profile: profile, mask: mask)
            let changed = try capture.publish(
                pasted, to: document, name: outcome.historyName, record: record(mask),
                resultingSelection: selectionAfter(capture.selectionState, mask))
            if changed, let current = document.doc {
                land(outcome.landing, in: current, masked: mask != nil)
            }
        } catch { presentPrecisionError(error) }
    }

    /// Edit ▸ Paste in Place (⇧⌘V): a typed payload at the canvas offset it
    /// was copied from — at every depth, since an 8-bit Copy writes the
    /// private type too (12G-2) — or, into a canvas of another size than the
    /// copy's, at the same relative location
    /// (`PasteContext.inPlace`). An image from another app has no source
    /// position and lands like Paste (`PasteWork.landing`).
    @objc func pasteInPlace(_ sender: Any?) {
        pastePlanned(placement: .inPlace)
    }

    /// Edit ▸ Paste Special ▸ Paste Into (⌥⇧⌘V): the clipboard as a new
    /// layer centred on the selection's bounds, masked by the selection.
    @objc func pasteInto(_ sender: Any?) {
        pastePlanned(placement: .into)
    }

    /// Edit ▸ Paste Special ▸ Paste Outside: the same, masked by the
    /// selection's inverse (outside coverage 255).
    @objc func pasteOutside(_ sender: Any?) {
        pastePlanned(placement: .outside)
    }

    /// Paste's validation: a typed payload, a public image or a held layer
    /// clipboard, at every depth (a public image is promoted into
    /// 32-bit float). A TYPE query, never a read of the blob.
    func validatePaste() -> Bool {
        guard document?.doc != nil else { return false }
        if LayerClipboard.shared.isAvailable(on: Clipboard.pasteboard) { return true }
        return TypedClipboardPayload.isAvailable
            || Clipboard.pasteboard.canReadObject(forClasses: [NSImage.self], options: nil)
    }

    /// Paste in Place: Paste's rule at every depth — a typed payload lands
    /// at its offset, and a public image, which has none, lands like
    /// Paste.
    func validatePasteInPlace() -> Bool {
        validatePaste()
    }

    /// Paste Into / Outside: Paste's rule plus a selection to mask with,
    /// outside Quick Mask — and no held layer clipboard.
    func validatePasteInto() -> Bool {
        guard let document else { return false }
        return validatePaste() && document.canvasSelection != nil && !document.quickMaskActive
            && !LayerClipboard.shared.isAvailable(on: Clipboard.pasteboard)
    }
}
