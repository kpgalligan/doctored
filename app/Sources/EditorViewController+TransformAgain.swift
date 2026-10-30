import AppKit

// Transform Again re-applies the stored matrix VERBATIM (no pivot re-derived: a repeat on the same
// content is M·M) with the stored sampler, to ⌘T's target with no session: over a selection through
// `publishSelectedPixelsTransform` (⌘T's selected-pixels commit, extracted), without one through
// `refuseUntransformable` (the explicit-value body of `refuseUntransformableSet`), the 8-bit
// described compose (`commitDescribedTransform(…, name:)`), `commitRasterTransformJob`, or at 16
// bits a fresh gesture's `.transformSet` / `.duplicateTransform`. ONE "Transform Again" /
// "Transform Again with Copy" step, recorded as `transform_again` (`"duplicate": true` only when
// true); replay uses the application's memory as the replay document's canvas size admits it.
//
// ⇧⌘T's described primary goes through `describedPrimaryGate`, and ⌥⇧⌘T takes the same gate over
// its copies after `replayRefusal` (at 16 bits `confirmTruePerspectiveArtwork` over the copy for a
// perspective memory), so a described copy stays editable; MCP `transform_again duplicate`
// composes one 8-bit copy under an affine memory (`composedCopy`) and otherwise follows
// `transform_layers duplicate` (`rasterize_artwork`, `rasterized_layers`).

/// Layer ▸ Transform Again (⇧⌘T) and Transform Again with Copy (⌥⇧⌘T):
/// the controller half. The model and the decisions live in
/// `TransformAgain.swift`. No session opens: the remembered matrix
/// (or perspective) is applied verbatim to ⌘T's own target — the active layer's selected pixels
/// with a selection, the expanded layer selection without one — through the
/// commit paths ⌘T and ⌥⌘T use, as ONE "Transform Again" / "Transform Again
/// with Copy" step. MCP `transform_again` is the twin. With a selection and
/// a mask, alpha channel or colour plane as the edit target
/// the repeat transforms that plane's selected area; the memory stores no
/// target.
extension EditorViewController {
    @objc func transformAgain(_ sender: Any?) {
        commitTransformAgain(duplicate: false)
    }

    @objc func transformAgainWithCopy(_ sender: Any?) {
        commitTransformAgain(duplicate: true)
    }

    func validateTransformAgainItem() -> Bool {
        transformSession == nil && document?.lastFreeTransform != nil
            && (document?.canvasSelection != nil || canBeginDuplicateTransform())
    }

    private func commitTransformAgain(duplicate: Bool) {
        // Never leave a text or shape session under the edit, as ⌘T does.
        guard canvas.commitTextSession(), commitShapeEditSession() else { return }
        guard transformSession == nil, let document, let again = document.lastFreeTransform,
              document.doc != nil else {
            NSSound.beep()
            return
        }
        let name = RepeatableTransform.actionName(duplicate: duplicate)
        let published: Bool
        if document.canvasSelection != nil {
            published = transformAgainSelectedPixels(
                document, again, duplicate: duplicate, name: name)
        } else if duplicate {
            published = transformAgainCopy(document, again, name: name)
        } else {
            published = transformAgainLayers(document, again, name: name)
        }
        if published { document.lastFreeTransform = again }
    }

    /// With a selection: ⌘T's selected-pixels refusals and capture, then
    /// its commit (with ⌥, a pixel copy). True when published.
    private func transformAgainSelectedPixels(
        _ document: ImageDocument, _ again: RepeatableTransform, duplicate: Bool, name: String
    ) -> Bool {
        guard let doc = document.doc, let selection = document.canvasSelection else { return false }
        let layer = document.activeLayerIndex
        // A non-layer target repeats onto that plane (a colour plane too),
        // before the layer's Pixels-lock alert and box.
        if document.planeEditTarget != .layer {
            return transformAgainPlane(
                document, again, target: document.planeEditTarget, layer: layer,
                selection: selection, duplicate: duplicate, name: name)
        }
        if refuseLockedEdit(layer: layer, kind: DR_EDIT_PIXELS) { return false }
        if let reason = TransformSelectedPixels.refusal(
            document, layer: layer, target: document.planeEditTarget) {
            document.presentError(DoctoredCoreError(message: reason))
            return false
        }
        guard TransformSelectedPixels.box(doc, layer: layer, selection: selection) != nil else {
            document.presentError(DoctoredCoreError(message: TransformSelectedPixels.emptyRefusal))
            return false
        }
        let described = document.layerDescribesSource(layer)
        if described, !document.confirmRasterize(layer: layer) { return false }
        // AFTER the prompt, whose modal loop services MCP: everything below
        // derives from this capture, and the publication refuses if it went
        // stale.
        guard self.document === document, transformSession == nil, let base = document.doc,
              document.activeLayerIndex == layer, let captured = document.canvasSelection,
              let box = TransformSelectedPixels.box(base, layer: layer, selection: captured)
        else {
            document.presentError(DoctoredCoreError(message: TransformSelectedPixels.staleRefusal))
            return false
        }
        let history = document.selectionHistory
        let coverage = captured.maskSamples16()
        do {
            var source = base
            if described, !base.isHighDepth {
                guard let dropped = base.withLayerMeta(layer, nil) else {
                    throw DoctoredCoreError(message: "The layer's description could not be dropped.")
                }
                source = dropped
            }
            let gesture = base.isHighDepth
                ? try PrecisionGeometryGesture(document, indexedBackground: backgroundColor) : nil
            defer { gesture?.cancel() }
            let floating = FloatingSelection(
                layer: layer, base: base, source: source, selection: history, coverage: coverage,
                described: described, budget: nil, duplicate: duplicate)
            let outcome = try publishSelectedPixelsTransform(
                document, floating, box: again.sourceRect(over: box), matrix: again.matrix,
                quad: again.quad, sampler: again.sampler, name: name,
                record: .transformAgain(duplicate: duplicate,
                                        rasterizeArtwork: again.perspective != nil),
                gesture: gesture)
            if outcome == .unchanged { NSSound.beep() }
            return outcome == .published
        } catch is JobCancelled {
            SessionCommitRefusal.note(JobCancelled.message)
            return false
        } catch {
            document.presentError(error)
            return false
        }
    }

    /// ⌘T's plane refusals and box (the selection's bounds), the
    /// background swatch as the vacated area, then ⌘T's selected-pixels
    /// commit with the plane target. A colour plane takes ⌘T's lock
    /// alert and rasterize prompt first. True when published.
    private func transformAgainPlane(
        _ document: ImageDocument, _ again: RepeatableTransform, target: PaintTarget,
        layer: Int, selection: CanvasSelection, duplicate: Bool, name: String
    ) -> Bool {
        if let kind = MoveSelectedPixels.planeLockKind(target),
           refuseLockedEdit(layer: layer, kind: kind) { return false }
        if let reason = TransformSelectedPixels.refusal(
            document, layer: layer, target: target, planes: true) {
            document.presentError(DoctoredCoreError(message: reason))
            return false
        }
        guard document.doc != nil, TransformSelectedPixels.planeBox(selection) != nil else {
            document.presentError(DoctoredCoreError(message: TransformSelectedPixels.emptyRefusal))
            return false
        }
        let described = target.isPlane && document.layerDescribesSource(layer)
        if described, !document.confirmRasterize(layer: layer) { return false }
        // AFTER the prompt, whose modal loop services MCP.
        guard self.document === document, transformSession == nil, let base = document.doc,
              document.activeLayerIndex == layer, let captured = document.canvasSelection,
              let box = TransformSelectedPixels.planeBox(captured) else {
            document.presentError(DoctoredCoreError(message: TransformSelectedPixels.staleRefusal))
            return false
        }
        let hex = ActionArgs.hex(backgroundColor)
        do {
            let background = try TransformSelectedPixels.planeBackground(
                hex: hex, target: target, in: document)
            var source = base
            if described, !base.isHighDepth {
                guard let dropped = base.withLayerMeta(layer, nil) else {
                    throw DoctoredCoreError(message: "The layer's description could not be dropped.")
                }
                source = dropped
            }
            let gesture = base.isHighDepth
                ? try PrecisionGeometryGesture(document, indexedBackground: backgroundColor) : nil
            defer { gesture?.cancel() }
            let floating = FloatingSelection(
                layer: layer, base: base, source: source, selection: document.selectionHistory,
                coverage: captured.maskSamples16(), described: described, budget: nil,
                duplicate: duplicate, target: target, background: background)
            let outcome = try publishSelectedPixelsTransform(
                document, floating, box: again.sourceRect(over: box), matrix: again.matrix,
                quad: again.quad, sampler: again.sampler, name: name,
                record: .transformAgain(duplicate: duplicate,
                                        rasterizeArtwork: again.perspective != nil,
                                        target: target.agentName(in: base),
                                        background: target == .plane(.alpha) ? nil : hex),
                gesture: gesture)
            if outcome == .unchanged { NSSound.beep() }
            return outcome == .published
        } catch is JobCancelled {
            SessionCommitRefusal.note(JobCancelled.message)
            return false
        } catch {
            document.presentError(error)
            return false
        }
    }

    /// ⇧⌘T with no selection: ⌘T's layer target and refusals, its described
    /// compose at 8 bits (an affine memory only), then the set commit. A
    /// perspective memory takes ⌘T's true-perspective prompts. True when
    /// published.
    private func transformAgainLayers(
        _ document: ImageDocument, _ again: RepeatableTransform, name: String
    ) -> Bool {
        let perspective = again.perspective
        return commitLayerSetTransform(
            document, matrix: again.matrix, quad: again.quad,
            sourceRect: perspective?.sourceRect, sampler: again.sampler,
            record: .transformAgain(duplicate: false, rasterizeArtwork: perspective != nil),
            name: name, refusal: { again.replayRefusal(source: $0, layers: $1) })
    }

    /// A sessionless transform of the layer selection, as ONE step called
    /// `name`: ⌘T's layer target and refusals, its described compose at 8
    /// bits (an affine `matrix` only), then the set commit. `quad` and
    /// `sourceRect` are a true perspective's corners and rectangle, nil for
    /// an affine; `refusal` is asked before any prompt. The body of ⇧⌘T's
    /// layer branch, shared with the Ruler's Straighten Layer. True
    /// when published.
    func commitLayerSetTransform(
        _ document: ImageDocument, matrix: CGAffineTransform, quad: [CGPoint]?,
        sourceRect: CGRect?, sampler: DrResizeFilter, record: [ActionStep], name: String,
        refusal: ((RasterDocument, [Int]) -> String?)? = nil
    ) -> Bool {
        guard !refuseOperationScope(.layerGeometry), let doc = document.doc else { return false }
        let set = doc.movingSet(document.selectedLayerIndices)
        guard let primary = set.first, !doc.isHighDepth || set.count == 1 else {
            NSSound.beep()
            return false
        }
        guard !refuseLockedEdit(layers: set, kind: DR_EDIT_POSITION) else { return false }
        if let reason = refusal?(doc, set) {
            document.presentError(DoctoredCoreError(message: reason))
            return false
        }
        let truePerspective = sourceRect != nil
        switch describedPrimaryGate(
            document, source: doc, base: doc, primary: primary, single: set.count == 1,
            matrix: matrix, quad: nil, truePerspective: truePerspective,
            sampler: sampler, record: record, name: name) {
        case .committed: return true
        case .refused: return false
        case .resample: break
        }
        if doc.isHighDepth, truePerspective,
           !confirmTruePerspectiveArtwork(doc, layers: set) { return false }
        guard !refuseUntransformable(
            layers: set, primary: primary, matrix: matrix, quad: quad,
            sourceRect: sourceRect ?? .zero, in: doc, duplicate: false)
        else { return false }
        if doc.isHighDepth {
            // Always the set request, so it equals MCP's `TransformWork`.
            return finishTransformAgain(
                document, .transformSet(layers: set, sourceRect: sourceRect ?? .zero,
                                        matrix: matrix, quad: quad, sampler: sampler),
                name: name, record: record)
        }
        return commitRasterTransformJob(
            document: document, source: doc, layers: set, quad: quad,
            matrix: matrix, sampler: sampler, record: record, name: name,
            sourceRect: sourceRect)
    }

    /// ⌥⇧⌘T with no selection: ⌥⌘T's open-time refusals, then ⇧⌘T's
    /// described rules over the copies and ⌥⌘T's commit over the
    /// duplicate; the copies are selected once published.
    private func transformAgainCopy(
        _ document: ImageDocument, _ again: RepeatableTransform, name: String
    ) -> Bool {
        guard !refuseOperationScope(.layerGeometry), let doc = document.doc else { return false }
        let originals = document.selectedLayerIndices
        let subtrees = MoveDuplicateLayers.subtrees(of: originals, in: doc)
        if doc.isHighDepth, subtrees.count != 1 {
            NSSound.beep()
            return false
        }
        guard !refuseLockedEdit(layers: subtrees, kind: DR_EDIT_POSITION) else { return false }
        let copy: DuplicateTransform
        do {
            guard let made = try DuplicateTransform(
                base: doc, originals: originals, primary: document.activeLayerIndex) else {
                document.presentError(DoctoredCoreError(message: "The copy could not be made."))
                return false
            }
            copy = made
        } catch {
            document.presentError(error)
            return false
        }
        let perspective = again.perspective
        let primary = copy.set.first ?? 0
        // Before any prompt, as `transformAgainLayers`.
        if let reason = again.replayRefusal(source: copy.source, layers: copy.set) {
            document.presentError(DoctoredCoreError(message: reason))
            return false
        }
        let record: [ActionStep] = .transformAgain(
            duplicate: true, rasterizeArtwork: perspective != nil)
        if doc.isHighDepth {
            if perspective != nil,
               !confirmTruePerspectiveArtwork(copy.source, layers: copy.set) { return false }
        } else {
            switch describedPrimaryGate(
                document, source: copy.source, base: copy.base, primary: primary,
                single: copy.set.count == 1, matrix: again.matrix, quad: nil,
                truePerspective: perspective != nil, sampler: again.sampler, record: record,
                name: name) {
            case .committed:
                setSelectedLayers(copy.selection)
                return true
            case .refused: return false
            case .resample: break
            }
        }
        guard !refuseUntransformable(
            layers: copy.set, primary: primary, matrix: again.matrix,
            quad: again.quad, sourceRect: perspective?.sourceRect ?? .zero, in: copy.source,
            duplicate: true) else { return false }
        let published: Bool
        if doc.isHighDepth {
            published = finishTransformAgain(
                document, .duplicateTransform(layers: copy.originals,
                                              sourceRect: perspective?.sourceRect ?? .zero,
                                              matrix: again.matrix, quad: again.quad,
                                              sampler: again.sampler),
                name: name, record: record)
        } else {
            published = commitRasterTransformJob(
                document: document, source: copy.source, layers: copy.set, quad: again.quad,
                matrix: again.matrix, sampler: again.sampler, record: record, name: name,
                base: copy.base, sourceRect: perspective?.sourceRect)
        }
        // After publication, never before.
        if published { setSelectedLayers(copy.selection) }
        return published
    }

    /// The 16-bit commit through a fresh typed gesture. True when published.
    private func finishTransformAgain(
        _ document: ImageDocument, _ request: PrecisionGeometryRequest, name: String,
        record: [ActionStep]
    ) -> Bool {
        do {
            let gesture = try PrecisionGeometryGesture(document, indexedBackground: backgroundColor)
            defer { gesture.cancel() }
            let outcome = try gesture.finishOutcome(request, name: name, record: record)
            if outcome == .unchanged { NSSound.beep() }
            return outcome == .published
        } catch is JobCancelled {
            SessionCommitRefusal.note(JobCancelled.message)
            return false
        } catch {
            document.presentError(error)
            return false
        }
    }
}
